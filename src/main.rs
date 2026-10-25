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
        let res = if out == "-" {
            use std::io::Write;
            std::io::stdout().write_all(&r.body)
        } else {
            std::fs::write(out, &r.body)
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
