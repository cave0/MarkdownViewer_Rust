#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod markdown;
mod page;
mod sanitize;
mod site;
#[cfg(windows)]
mod gui;

use std::path::PathBuf;

const HELP: &str = "mdview - fast dark-mode Markdown viewer

USAGE:
    mdview [OPTIONS] [FILE]

    With no FILE, a file picker opens. You can also drop a .md file onto the window.

OPTIONS:
    --trusted               Preserve raw HTML in the document (trusted documents only)
    --allow-remote-images   Allow http(s) images (blocked by default)
    --dump-html <OUT>       Write the rendered page to OUT ('-' = stdout) and exit
    -h, --help              Show this help
    -V, --version           Show version

KEYS:
    Ctrl+F find   Ctrl+B sidebar   Ctrl+Shift+N night colors   Ctrl+Shift+K black background
    Ctrl+O open   F11 fullscreen   Ctrl+P print / PDF          Alt+Left back";

struct Args {
    file: Option<PathBuf>,
    trusted: bool,
    allow_remote: bool,
    dump: Option<String>,
}

enum Parsed {
    Run(Args),
    Info(String),
    Bad(String),
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Parsed {
    let mut a = Args { file: None, trusted: false, allow_remote: false, dump: None };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Parsed::Info(HELP.into()),
            "-V" | "--version" => return Parsed::Info(format!("mdview {}", env!("CARGO_PKG_VERSION"))),
            "--trusted" => a.trusted = true,
            "--allow-remote-images" => a.allow_remote = true,
            "--dump-html" => match it.next() {
                Some(v) => a.dump = Some(v),
                None => return Parsed::Bad("--dump-html needs an output path".into()),
            },
            // flags of the old Python viewer: accepted and ignored
            "--no-katex" | "--no-mermaid" | "--browser" | "--no-open" | "--disable-gpu" => {}
            "--port" => {
                it.next();
            }
            s if s.starts_with("--") => return Parsed::Bad(format!("unknown option {s}\n\n{HELP}")),
            _ => {
                if a.file.is_some() {
                    return Parsed::Bad("only one file can be opened at a time".into());
                }
                a.file = Some(PathBuf::from(arg));
            }
        }
    }
    Parsed::Run(a)
}

pub fn report(title: &str, text: &str) {
    #[cfg(windows)]
    gui::message_box(title, text);
    #[cfg(not(windows))]
    eprintln!("{title}: {text}");
}


fn absolute_output_path(output: &std::path::Path) -> std::io::Result<PathBuf> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    if absolute.exists() {
        return std::fs::canonicalize(absolute);
    }
    let name = absolute.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no filename")
    })?;
    let parent = absolute.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    Ok(std::fs::canonicalize(parent)?.join(name))
}

fn dump_output_is_input(input: &std::path::Path, output: &std::path::Path) -> std::io::Result<bool> {
    Ok(std::fs::canonicalize(input)? == absolute_output_path(output)?)
}

fn companion_asset_dir(output: &std::path::Path) -> std::io::Result<PathBuf> {
    let name = output.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no filename")
    })?;
    Ok(output.with_file_name(format!("{}.assets", name.to_string_lossy())))
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Parsed::Run(a) => a,
        Parsed::Info(t) => return report("mdview", &t),
        Parsed::Bad(t) => {
            report("mdview", &t);
            std::process::exit(2);
        }
    };

    if let Some(out) = &args.dump {
        let Some(file) = &args.file else {
            report("mdview", "--dump-html needs a FILE");
            std::process::exit(2);
        };
        let site = match site::Site::new(file, args.trusted, args.allow_remote) {
            Ok(s) => s,
            Err(e) => {
                report("mdview", &format!("Cannot open {}: {e}", file.display()));
                std::process::exit(1);
            }
        };
        let url = site.url_for(&site.current()).unwrap_or_default();
        let r = site.serve(&url);
        if out != "-" {
            match dump_output_is_input(file, std::path::Path::new(out)) {
                Ok(true) => {
                    report("mdview", "Refusing to overwrite the input Markdown file with HTML output.");
                    std::process::exit(2);
                }
                Ok(false) => {}
                Err(e) => {
                    report("mdview", &format!("Cannot resolve HTML output path: {e}"));
                    std::process::exit(2);
                }
            }
        }
        let res = if out == "-" {
            use std::io::Write;
            std::io::stdout().write_all(&r.body)
        } else {
            (|| -> std::io::Result<()> {
                let output = absolute_output_path(std::path::Path::new(out))?;
                let assets = companion_asset_dir(&output)?;
                site::write_embedded_assets(&assets)?;
                let assets_url = site::file_url(&assets)?;
                let source_dir = file.parent().unwrap_or_else(|| std::path::Path::new("."));
                let source_url = site::file_url(source_dir)?;
                let mut html = String::from_utf8_lossy(&r.body).into_owned();
                html = html.replace("<head>", &format!("<head><base href=\"{source_url}\">"));
                // The page CSP blocks <base>; remove only that directive for the exported file.
                html = html.replace("base-uri 'none'; ", "");
                for asset in ["boot.js", "app.js", "katex/katex.min.css"] {
                    let old = format!("/_a/{asset}");
                    let new = format!("{assets_url}{asset}");
                    html = html.replace(&old, &new);
                }
                std::fs::write(output, html)
            })()
        };
        if let Err(e) = res {
            report("mdview", &format!("Cannot write {out}: {e}"));
            std::process::exit(1);
        }
        return;
    }

    #[cfg(windows)]
    gui::run(args.file, args.trusted, args.allow_remote);
    #[cfg(not(windows))]
    {
        let _ = args.file;
        eprintln!("mdview's window needs Windows (WebView2). Use --dump-html <OUT> to render to a file.");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mdview_dump_test_{}_{}", std::process::id(), nonce))
    }

    #[test]
    fn dump_output_refuses_to_overwrite_source() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("document.md");
        std::fs::write(&source, "# keep me").unwrap();

        assert!(dump_output_is_input(&source, &source).unwrap());
        assert!(dump_output_is_input(&source, &dir.join(".").join("document.md")).unwrap());
        assert!(!dump_output_is_input(&source, &dir.join("document.html")).unwrap());

        std::fs::remove_dir_all(dir).unwrap();
    }
}
