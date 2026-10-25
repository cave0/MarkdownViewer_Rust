//! Everything the webview can request, as a pure function of the URL path. No GUI types in
//! here, so it is unit-tested on any platform.

use crate::markdown::{self, RenderOpts};
use crate::page;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

pub const MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd"];
const IMG_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"),
    ("webp", "image/webp"), ("bmp", "image/bmp"), ("ico", "image/x-icon"), ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
];
const MAX_DOC_BYTES: u64 = 64 * 1024 * 1024;
const CACHE_ENTRIES: usize = 8;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const PATH_SEG: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>').add(b'?').add(b'`')
    .add(b'{').add(b'}').add(b'%').add(b'/').add(b'\\');

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Cow<'static, [u8]>,
    pub csp: Option<String>,
    pub cacheable: bool,
}

impl Response {
    fn text(status: u16, msg: &'static str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: Cow::Borrowed(msg.as_bytes()), csp: None, cacheable: false }
    }
}

type Sig = (Option<SystemTime>, u64);
struct CacheEntry {
    path: PathBuf,
    sig: Sig,
    page: Arc<Vec<u8>>,
}

pub struct Site {
    root: Mutex<PathBuf>,
    current: Mutex<PathBuf>,
    cache: Mutex<Vec<CacheEntry>>,
    pub opts: RenderOpts,
    pub trusted: bool,
}

pub fn is_md(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map_or(false, |e| MD_EXTS.iter().any(|m| e.eq_ignore_ascii_case(m)))
}

pub fn file_sig(p: &Path) -> Option<Sig> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok(), m.len()))
}

impl Site {
    pub fn new(file: &Path, trusted: bool, allow_remote_images: bool) -> std::io::Result<Self> {
        let file = std::fs::canonicalize(file)?;
        let root = file.parent().map(Path::to_path_buf).unwrap_or_else(|| file.clone());
        Ok(Self {
            root: Mutex::new(root),
            current: Mutex::new(file),
            cache: Mutex::new(Vec::new()),
            opts: RenderOpts { safe: !trusted, allow_remote_images },
            trusted,
        })
    }

    pub fn current(&self) -> PathBuf {
        self.current.lock().unwrap().clone()
    }

    /// Switch to another document; its folder becomes the new serving root.
    pub fn open(&self, file: &Path) -> std::io::Result<String> {
        let file = std::fs::canonicalize(file)?;
        let root = file.parent().map(Path::to_path_buf).unwrap_or_else(|| file.clone());
        *self.root.lock().unwrap() = root;
        *self.current.lock().unwrap() = file.clone();
        Ok(self.url_for(&file).unwrap_or_default())
    }

    /// URL path ("/dir/file%20name.md") for a file under the root.
    pub fn url_for(&self, file: &Path) -> Option<String> {
        let root = self.root.lock().unwrap().clone();
        let rel = file.strip_prefix(&root).ok()?;
        let mut s = String::new();
        for c in rel.components() {
            if let Component::Normal(n) = c {
                s.push('/');
                s.extend(utf8_percent_encode(&n.to_string_lossy(), PATH_SEG));
            }
        }
        Some(s)
    }

    pub fn serve(&self, url_path: &str) -> Response {
        if let Some(name) = url_path.strip_prefix("/_a/") {
            return match embedded::ASSETS.iter().find(|(n, _)| *n == name) {
                Some((n, bytes)) => Response {
                    status: 200,
                    content_type: asset_type(n),
                    body: Cow::Borrowed(bytes),
                    csp: None,
                    cacheable: true,
                },
                None => Response::text(404, "Not found"),
            };
        }
        if url_path == "/_files" {
            return Response {
                status: 200,
                content_type: "application/json; charset=utf-8",
                body: Cow::Owned(self.list_files().into_bytes()),
                csp: None,
                cacheable: false,
            };
        }
        let Some(path) = self.resolve(url_path) else { return Response::text(404, "Not found") };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if is_md(&path) {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.render_md(&path)));
            return match r {
                Ok(resp) => resp,
                Err(_) => html_response(500, page::error_page("Render error", "The renderer crashed on this document.").into_bytes()),
            };
        }
        if let Some((_, ct)) = IMG_TYPES.iter().find(|(e, _)| *e == ext) {
            return match std::fs::read(&path) {
                Ok(bytes) => Response {
                    status: 200,
                    content_type: ct,
                    body: Cow::Owned(bytes),
                    // an SVG opened directly (not via <img>) must not run anything
                    csp: Some("default-src 'none'; style-src 'unsafe-inline'; sandbox".into()),
                    cacheable: false,
                },
                Err(_) => Response::text(404, "Not found"),
            };
        }
        Response::text(404, "Not found")
    }

    /// JSON list of Markdown files under the root (3 levels deep, newest first) for the quick switcher.
    pub fn list_files(&self) -> String {
        const MAX_FILES: usize = 5000;
        const MAX_DEPTH: usize = 3;
        let root = self.root.lock().unwrap().clone();
        let mut found: Vec<(u64, String, String)> = Vec::new(); // (mtime, url, display)
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                if found.len() >= MAX_FILES {
                    break;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(ft) = e.file_type() else { continue };
                let path = e.path();
                if ft.is_dir() {
                    if depth < MAX_DEPTH && !name.starts_with('.') && name != "node_modules" && name != "target" {
                        stack.push((path, depth + 1));
                    }
                } else if ft.is_file() && is_md(&path) {
                    let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                    if let (Some(url), Ok(rel)) = (self.url_for(&path), path.strip_prefix(&root)) {
                        found.push((mtime, url, rel.to_string_lossy().replace('\\', "/")));
                    }
                }
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
        let mut out = String::from("[");
        for (i, (t, u, n)) in found.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{{\"t\":{t},\"p\":{},\"n\":{}}}", json_str(u), json_str(n)));
        }
        out.push(']');
        out
    }

    /// Map a URL path to a file strictly inside the root (no `..`, drive letters or symlink escapes).
    fn resolve(&self, url_path: &str) -> Option<PathBuf> {
        let decoded = percent_decode_str(url_path).decode_utf8_lossy();
        if decoded.contains('\0') {
            return None;
        }
        let rel = Path::new(decoded.trim_start_matches(['/', '\\']));
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return None;
        }
        let root = self.root.lock().unwrap().clone();
        let full = std::fs::canonicalize(root.join(rel)).ok()?;
        (full.starts_with(&root) && full.is_file()).then_some(full)
    }

    fn render_md(&self, path: &Path) -> Response {
        *self.current.lock().unwrap() = path.to_path_buf();
        let Some(sig) = file_sig(path) else { return Response::text(404, "Not found") };
        if let Some(e) = self.cache.lock().unwrap().iter().find(|e| e.path == path && e.sig == sig) {
            return html_response(200, e.page.to_vec());
        }
        if sig.1 > MAX_DOC_BYTES {
            let msg = format!("{} is {} MB; the limit is {} MB.", path.display(), sig.1 >> 20, MAX_DOC_BYTES >> 20);
            return html_response(200, page::error_page("File too large", &msg).into_bytes());
        }
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(e) => return html_response(200, page::error_page("Cannot read file", &e.to_string()).into_bytes()),
        };
        let text = page::decode_text(&raw);
        drop(raw);
        let doc = markdown::render(&text, self.opts);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let html = page::build(&doc, &name, &text, self.trusted, self.opts.allow_remote_images).into_bytes();

        let page = Arc::new(html);
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|e| e.path != path);
        let mut total: usize = cache.iter().map(|e| e.page.len()).sum();
        while !cache.is_empty() && (cache.len() >= CACHE_ENTRIES || total + page.len() > CACHE_BYTES) {
            total -= cache.remove(0).page.len();
        }
        if page.len() <= CACHE_BYTES {
            cache.push(CacheEntry { path: path.to_path_buf(), sig, page: page.clone() });
        }
        html_response(200, page.to_vec())
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn html_response(status: u16, body: Vec<u8>) -> Response {
    Response { status, content_type: "text/html; charset=utf-8", body: Cow::Owned(body), csp: None, cacheable: false }
}

fn asset_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (PathBuf, Site) {
        let dir = std::env::temp_dir().join(format!("mdview_test_{}_{}", std::process::id(), rand_id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.md"), "# Hello\n\nworld [b](sub/b.md)\n").unwrap();
        fs::write(dir.join("sub/b.md"), "# B\n").unwrap();
        fs::write(dir.join("pic.png"), [0x89, b'P', b'N', b'G']).unwrap();
        fs::write(dir.join("secret.txt"), "nope").unwrap();
        fs::write(dir.parent().unwrap().join(format!("outside_{}.md", std::process::id())), "# out").unwrap();
        let site = Site::new(&dir.join("a.md"), false, false).unwrap();
        (dir, site)
    }
    fn rand_id() -> u128 {
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn serves_markdown_images_assets() {
        let (_d, s) = setup();
        let r = s.serve("/a.md");
        assert_eq!(r.status, 200);
        let html = String::from_utf8(r.body.to_vec()).unwrap();
        assert!(html.contains("<h1 id=\"hello\">Hello</h1>"));
        assert!(html.contains("Content-Security-Policy"));
        assert_eq!(s.serve("/sub/b.md").status, 200);
        assert_eq!(s.serve("/pic.png").content_type, "image/png");
        assert_eq!(s.serve("/_a/app.js").status, 200);
        assert_eq!(s.serve("/_a/katex/fonts/KaTeX_Main-Regular.woff2").content_type, "font/woff2");
        assert_eq!(s.serve("/_a/nope.js").status, 404);
    }

    #[test]
    fn blocks_traversal_and_other_types() {
        let (d, s) = setup();
        let out = format!("/outside_{}.md", std::process::id());
        assert_eq!(s.serve(&format!("/..{out}")).status, 404);
        assert_eq!(s.serve(&format!("/%2e%2e{out}")).status, 404);
        assert_eq!(s.serve(&format!("/sub/..%2f..{out}")).status, 404);
        assert_eq!(s.serve("/secret.txt").status, 404);
        assert_eq!(s.serve("/C:/Windows/win.ini").status, 404);
        assert_eq!(s.serve("//etc/passwd").status, 404);
        assert_eq!(s.serve("/a%00.md").status, 404);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.parent().unwrap().join(format!("outside_{}.md", std::process::id())), d.join("link.md")).unwrap();
            assert_eq!(s.serve("/link.md").status, 404);
        }
    }

    #[test]
    fn cache_invalidates_on_change() {
        let (d, s) = setup();
        let a = String::from_utf8(s.serve("/a.md").body.to_vec()).unwrap();
        assert!(a.contains("Hello"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(d.join("a.md"), "# Changed content here\n").unwrap();
        let b = String::from_utf8(s.serve("/a.md").body.to_vec()).unwrap();
        assert!(b.contains("Changed content here"));
    }

    #[test]
    fn url_for_encodes_segments() {
        let (d, s) = setup();
        fs::write(d.join("sub/we ird#name.md"), "x").unwrap();
        let p = fs::canonicalize(d.join("sub/we ird#name.md")).unwrap();
        let u = s.url_for(&p).unwrap();
        assert_eq!(u, "/sub/we%20ird%23name.md");
        assert_eq!(s.serve(&u).status, 200);
    }

    #[test]
    fn lists_markdown_files_as_json() {
        let (d, s) = setup();
        // '"' is not a legal file-name character on Windows, so the quoted name is created on Unix only.
        #[cfg(unix)]
        fs::write(d.join("sub/we \"q\".md"), "x").unwrap();
        #[cfg(not(unix))]
        fs::write(d.join("sub/we q.md"), "x").unwrap();
        let j = String::from_utf8(s.serve("/_files").body.to_vec()).unwrap();
        assert!(j.starts_with('[') && j.ends_with(']'));
        assert!(j.contains("\"p\":\"/a.md\"") && j.contains("\"n\":\"sub/b.md\""), "{j}");
        #[cfg(unix)]
        assert!(j.contains("we \\\"q\\\".md"), "{j}");
        #[cfg(not(unix))]
        assert!(j.contains("we q.md"), "{j}");
        assert!(!j.contains("secret.txt") && !j.contains("pic.png"));
    }

    #[test]
    fn embedded_app_js_serializes_mermaid_renders() {
        // mermaid.run() ids are timestamp-based; concurrent runs collided and drew diagrams into each other.
        let (_d, s) = setup();
        let js = String::from_utf8(s.serve("/_a/app.js").body.to_vec()).unwrap();
        assert!(js.contains("mermaidQueue"));
        assert!(js.contains("decorateDiagram"));
    }

    #[test]
    fn json_str_escapes_quotes_backslashes_and_controls() {
        assert_eq!(json_str("we \"q\".md"), "\"we \\\"q\\\".md\"");
        assert_eq!(json_str("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_str("a\nb"), "\"a\\u000ab\"");
    }
}
