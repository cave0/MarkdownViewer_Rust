//! HTML shell around the rendered body: CSP, sidebar, toolbar, source view.

use crate::markdown::{Doc, Heading};
use pulldown_cmark_escape::{escape_html, escape_html_body_text};

const STYLE: &str = include_str!("../assets/style.css");
/// Raw source view is skipped above this size (it would double the page for little benefit).
const MAX_SOURCE_CHARS: usize = 2_000_000;

pub fn csp(allow_remote_images: bool) -> String {
    let img = if allow_remote_images { "'self' data: http: https:" } else { "'self' data:" };
    format!(
        "default-src 'none'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src {img}; \
         font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'"
    )
}

/// Collapse heading levels (which may skip, e.g. h1 -> h3) into gap-free nesting depths.
fn depths(toc: &[Heading]) -> Vec<usize> {
    let mut stack: Vec<u8> = Vec::new();
    toc.iter()
        .map(|h| {
            while stack.last().map_or(false, |l| *l > h.level) {
                stack.pop();
            }
            if stack.last().map_or(true, |l| *l < h.level) {
                stack.push(h.level);
            }
            stack.len()
        })
        .collect()
}

fn sidebar(toc: &[Heading], turns: usize) -> String {
    if toc.is_empty() {
        return String::new();
    }
    let mut s = String::with_capacity(toc.len() * 96 + 512);
    s.push_str(
        "<nav id=\"mdview-sidebar\" class=\"mdview-sidebar\" aria-label=\"Table of contents\">\
         <div class=\"sidebar-header\"><span>Contents</span>\
         <button id=\"sidebar-collapse-btn\" type=\"button\" title=\"Collapse (Ctrl+B)\" aria-label=\"Collapse sidebar\">&laquo;</button></div>",
    );
    if turns > 0 {
        s.push_str(
            "<div class=\"turn-actions\"><button id=\"turns-fold-all\" type=\"button\" title=\"Fold all turns\">&#9662; Fold all</button>\
             <button id=\"turns-unfold-all\" type=\"button\" title=\"Unfold all turns\">&#9656; Unfold all</button></div>",
        );
    }
    s.push_str("<div class=\"outline-filter\"><input id=\"outline-filter\" type=\"search\" placeholder=\"Filter headings ( / )\" autocomplete=\"off\" spellcheck=\"false\"></div>");
    let mut depth = 0usize;
    let mut first = true;
    for (h, d) in toc.iter().zip(depths(toc)) {
        while depth < d {
            s.push_str("<ul>");
            depth += 1;
            first = true;
        }
        while depth > d {
            s.push_str("</li></ul>");
            depth -= 1;
            first = false;
        }
        if !first {
            s.push_str("</li>");
        }
        first = false;
        s.push_str("<li><a");
        if let Some(r) = h.role {
            s.push_str(" data-role=\"");
            s.push_str(r);
            s.push('"');
        }
        s.push_str(" href=\"#");
        escape_html(&mut s, &h.id).ok();
        s.push_str("\" data-target=\"");
        escape_html(&mut s, &h.id).ok();
        s.push_str("\">");
        escape_html_body_text(&mut s, &h.title).ok();
        s.push_str("</a>");
    }
    s.push_str("</li>");
    for remaining in (1..=depth).rev() {
        s.push_str("</ul>");
        if remaining > 1 {
            s.push_str("</li>"); // close the parent <li> of each nested list
        }
    }
    s.push_str("</nav><button id=\"sidebar-reopen-btn\" type=\"button\" title=\"Show contents (Ctrl+B)\" aria-label=\"Show sidebar\">&#9776;</button>");
    s
}

pub fn build(doc: &Doc, title: &str, source: &str, trusted: bool, allow_remote_images: bool) -> String {
    let side = sidebar(&doc.toc, doc.turns);
    let mut h = String::with_capacity(doc.body.len() + STYLE.len() + source.len() + 4096);
    h.push_str("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">");
    h.push_str("<meta name=\"color-scheme\" content=\"dark\">");
    h.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    h.push_str(&csp(allow_remote_images));
    h.push_str("\"><title>");
    escape_html_body_text(&mut h, title).ok();
    h.push_str("</title>");
    if doc.has_math {
        h.push_str("<link rel=\"stylesheet\" href=\"/_a/katex/katex.min.css\">");
    }
    h.push_str("<style>");
    h.push_str(STYLE);
    h.push_str("</style></head><body");
    if !side.is_empty() {
        h.push_str(" class=\"has-sidebar\"");
    }
    h.push_str("><script src=\"/_a/boot.js\"></script>");
    h.push_str(concat!(
        "<button id=\"back-toggle-btn\" type=\"button\" title=\"Back\" aria-label=\"Go back\">&#8592;</button>",
        "<button id=\"theme-toggle-btn\" type=\"button\" title=\"Night colors (Ctrl+Shift+N)\" aria-label=\"Toggle night colors\">&#9789;</button>",
        "<button id=\"fullscreen-toggle-btn\" type=\"button\" title=\"Full screen (F11)\" aria-label=\"Toggle full screen\">&#9974;</button>",
        "<button id=\"bgtheme-toggle-btn\" type=\"button\" title=\"Background theme (Ctrl+Shift+K)\" aria-label=\"Cycle background theme\">&#9682;</button>",
        "<button id=\"print-toggle-btn\" type=\"button\" title=\"Print / Export to PDF\" aria-label=\"Print or export to PDF\">&#9113;</button>",
    ));
    let show_source = source.len() <= MAX_SOURCE_CHARS;
    if show_source {
        h.push_str("<button id=\"source-toggle-btn\" type=\"button\" title=\"View raw Markdown source\" aria-label=\"Toggle raw source view\">&lt;/&gt;</button>");
    }
    h.push_str(&side);
    h.push_str("<div class=\"mdview-content\" tabindex=\"-1\"><div class=\"markdown-container\">");
    if trusted {
        h.push_str("<div class=\"security-warning-banner\"><strong>Security Notice:</strong> Trusted Document Mode is active. User HTML sanitization is bypassed (raw HTML preserved). Scripts and network access remain blocked.</div>");
    }
    h.push_str(&doc.body);
    h.push_str("</div>");
    if show_source {
        h.push_str("<pre class=\"raw-source-view\">");
        escape_html_body_text(&mut h, source).ok();
        h.push_str("</pre>");
    }
    h.push_str("</div>");
    if let Some(ts) = &doc.turn_source {
        // RCDATA: entities are decoded, nothing executes; the first newline is dropped by the parser
        h.push_str("<textarea id=\"md-turns\" hidden>\n");
        escape_html_body_text(&mut h, ts).ok();
        h.push_str("</textarea>");
    }
    h.push_str(&format!(
        "<div id=\"stats-badge\">{} words &middot; {} min read</div>",
        doc.words, doc.reading_min
    ));
    h.push_str("<script src=\"/_a/app.js\" defer></script></body></html>");
    h
}

pub fn error_page(title: &str, msg: &str) -> String {
    let mut h = String::from(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><style>",
    );
    h.push_str("html,body{background:#0d1117;color:#c9d1d9;margin:0}.error-page{padding:40px;color:#ff7b72;font-family:Consolas,monospace;white-space:pre-wrap}h2{color:#f0f6fc;font-family:'Segoe UI',sans-serif}</style><title>");
    escape_html_body_text(&mut h, title).ok();
    h.push_str("</title></head><body><div class=\"error-page\"><h2>");
    escape_html_body_text(&mut h, title).ok();
    h.push_str("</h2>");
    escape_html_body_text(&mut h, msg).ok();
    h.push_str("</div></body></html>");
    h
}

/// Decode a text file the way the Python viewer did, plus UTF-16 (Notepad "Unicode" files).
pub fn decode_text(raw: &[u8]) -> String {
    if let Some(r) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(r).into_owned();
    }
    if raw.len() >= 2 && (raw[..2] == [0xFF, 0xFE] || raw[..2] == [0xFE, 0xFF]) {
        let le = raw[0] == 0xFF;
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HI: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&b) { HI[(b - 0x80) as usize] } else { b as char }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{render, RenderOpts};

    #[test]
    fn sidebar_nesting_is_balanced() {
        let d = render("# A\n### B\n#### C\n## D\n# E\n", RenderOpts { safe: true, allow_remote_images: false });
        let p = build(&d, "t", "", false, false);
        assert_eq!(p.matches("<ul>").count(), p.matches("</ul>").count());
        assert_eq!(p.matches("<li>").count(), p.matches("</li>").count());
    }

    #[test]
    fn decodes_encodings() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFhi"), "hi");
        assert_eq!(decode_text(b"caf\xE9 \x93q\x94"), "café “q”");
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
    }
}
