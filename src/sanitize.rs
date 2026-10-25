//! Streaming sanitizer for user-authored raw HTML (Safe Mode).
//!
//! pulldown-cmark hands raw HTML over in fragments (one inline tag at a time, or a whole
//! block), so this is a small tag-level filter with state that persists across fragments:
//! an allow-list of tags/attributes, "discard content" tags (script, style, ...), URL scheme
//! checks on decoded attribute values, and a stack that keeps end tags balanced so user HTML
//! can't close the viewer's own containers.
//!
//! This is the second line of defence: the page's Content-Security-Policy (`script-src
//! 'self'`, no inline script) already stops injected scripts and event handlers.

use pulldown_cmark_escape::{escape_html, escape_html_body_text};

const ALLOWED_TAGS: &[&str] = &[
    "a", "abbr", "b", "blockquote", "br", "caption", "cite", "code", "dd", "del", "details", "dfn",
    "div", "dl", "dt", "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "ins", "kbd",
    "li", "mark", "ol", "p", "pre", "q", "rp", "rt", "ruby", "s", "samp", "small", "span",
    "strong", "sub", "summary", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "u",
    "ul", "var", "wbr", "center", "figure", "figcaption", "picture", "source", "colgroup", "col",
    // MathML core
    "math", "mrow", "mi", "mo", "mn", "mfrac", "msup", "msub", "msubsup", "msqrt", "mroot",
    "mtable", "mtr", "mtd", "mtext", "mspace", "mover", "munder", "munderover", "semantics",
    "annotation",
];
const VOID_TAGS: &[&str] = &["br", "hr", "img", "wbr", "source", "col"];
const DISCARD_TAGS: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "applet", "form", "meta", "link", "base",
    "template", "noscript", "title", "textarea", "select", "button", "svg", "frame", "frameset",
];
const ALLOWED_ATTRS: &[&str] = &[
    "class", "id", "title", "alt", "width", "height", "align", "valign", "src", "href", "start",
    "reversed", "loading", "colspan", "rowspan", "open", "lang", "dir", "srcset", "display",
    "xmlns", "mathvariant", "columnalign", "rowspacing", "columnspacing", "span",
];

pub struct Sanitizer {
    allow_remote_images: bool,
    discard: Vec<&'static str>,
    open: Vec<&'static str>,
}

impl Sanitizer {
    pub fn new(allow_remote_images: bool) -> Self {
        Self { allow_remote_images, discard: Vec::new(), open: Vec::new() }
    }

    /// Sanitize one fragment of raw HTML, appending the result to `out`.
    pub fn push(&mut self, input: &str, out: &mut String) {
        let b = input.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let Some(rel) = input[i..].find('<') else {
                self.text(&input[i..], out);
                return;
            };
            if rel > 0 {
                self.text(&input[i..i + rel], out);
            }
            i += rel;
            let rest = &input[i..];
            // comments, doctype, processing instructions, CDATA: dropped
            if rest.starts_with("<!--") {
                match rest[4..].find("-->") {
                    Some(e) => i += 4 + e + 3,
                    None => return,
                }
                continue;
            }
            if rest.starts_with("<!") || rest.starts_with("<?") {
                match rest.find('>') {
                    Some(e) => i += e + 1,
                    None => return,
                }
                continue;
            }
            let is_end = rest.as_bytes().get(1) == Some(&b'/');
            let name_start = if is_end { 2 } else { 1 };
            let name_len = rest[name_start..]
                .bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b':')
                .count();
            if name_len == 0 || !rest.as_bytes()[name_start].is_ascii_alphabetic() {
                // a literal '<' that does not start a tag
                self.text("&lt;", out);
                i += 1;
                continue;
            }
            let name = rest[name_start..name_start + name_len].to_ascii_lowercase();
            let (attrs, consumed) = match parse_attrs(&rest[name_start + name_len..]) {
                Some(v) => v,
                None => {
                    // unterminated tag: show the remainder as text instead of swallowing it
                    if self.discard.is_empty() {
                        escape_html_body_text(&mut *out, rest).ok();
                    }
                    return;
                }
            };
            let self_closing = rest[..name_start + name_len + consumed].ends_with("/>");
            i += name_start + name_len + consumed;
            if is_end {
                self.end_tag(&name, out);
            } else {
                self.start_tag(&name, attrs, self_closing, out);
            }
        }
    }

    /// Close anything the document left open (call once at the end of the document).
    pub fn finish(&mut self, out: &mut String) {
        while let Some(t) = self.open.pop() {
            out.push_str("</");
            out.push_str(t);
            out.push('>');
        }
        self.discard.clear();
    }

    fn text(&self, s: &str, out: &mut String) {
        if self.discard.is_empty() {
            out.push_str(s);
        }
    }

    fn end_tag(&mut self, name: &str, out: &mut String) {
        if let Some(top) = self.discard.last() {
            if *top == name {
                self.discard.pop();
            }
            return;
        }
        let Some(&canon) = ALLOWED_TAGS.iter().find(|t| **t == name) else { return };
        if VOID_TAGS.contains(&canon) {
            return;
        }
        if let Some(pos) = self.open.iter().rposition(|t| *t == canon) {
            while self.open.len() > pos {
                let t = self.open.pop().unwrap();
                out.push_str("</");
                out.push_str(t);
                out.push('>');
            }
        }
        // else: orphan end tag, dropped
    }

    fn start_tag(
        &mut self,
        name: &str,
        attrs: Vec<(String, Option<String>)>,
        self_closing: bool,
        out: &mut String,
    ) {
        if !self.discard.is_empty() {
            if let Some(&d) = DISCARD_TAGS.iter().find(|t| **t == name) {
                if !self_closing {
                    self.discard.push(d);
                }
            }
            return;
        }
        if let Some(&d) = DISCARD_TAGS.iter().find(|t| **t == name) {
            if !self_closing || name == "script" || name == "style" {
                self.discard.push(d);
            }
            return;
        }
        if name == "input" {
            // static checkboxes only (task lists written as raw HTML)
            let is_cb = attrs.iter().any(|(k, v)| {
                k == "type" && v.as_deref().map(|v| v.eq_ignore_ascii_case("checkbox")).unwrap_or(false)
            });
            if is_cb {
                let checked = attrs.iter().any(|(k, _)| k == "checked");
                out.push_str("<input type=\"checkbox\" disabled");
                if checked {
                    out.push_str(" checked");
                }
                out.push('>');
            }
            return;
        }
        let Some(&canon) = ALLOWED_TAGS.iter().find(|t| **t == name) else { return };

        out.push('<');
        out.push_str(canon);
        for (k, v) in &attrs {
            if !ALLOWED_ATTRS.contains(&k.as_str()) {
                continue;
            }
            let val = decode_entities(v.as_deref().unwrap_or(""));
            if matches!(k.as_str(), "href" | "src" | "srcset") {
                if !self.url_ok(canon, k, &val) {
                    continue;
                }
            }
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_html(&mut *out, &val).ok();
            out.push('"');
        }
        if canon == "a" {
            out.push_str(" rel=\"noopener noreferrer\"");
        }
        out.push('>');
        if !VOID_TAGS.contains(&canon) {
            self.open.push(canon);
            if self_closing {
                self.end_tag(canon, out);
            }
        }
    }

    fn url_ok(&self, tag: &str, attr: &str, val: &str) -> bool {
        // what the browser will see: entities decoded, ASCII whitespace/control chars ignored
        let norm: String = val
            .chars()
            .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if attr == "srcset" {
            return !norm.contains(':') || norm.starts_with("data:image/");
        }
        if norm.contains('&') && norm.find('&') < norm.find(['/', '?', '#']) {
            return false; // undecoded entity before the path starts: could hide a scheme
        }
        let scheme_end = norm.find(|c| matches!(c, ':' | '/' | '?' | '#'));
        let has_scheme = matches!(scheme_end, Some(p) if norm.as_bytes()[p] == b':');
        if !has_scheme {
            return true; // relative URL or fragment
        }
        let remote = norm.starts_with("http://") || norm.starts_with("https://");
        if tag == "img" || tag == "source" {
            if remote {
                return self.allow_remote_images;
            }
            return norm.starts_with("data:image/") && !norm.starts_with("data:image/svg");
        }
        remote || norm.starts_with("mailto:") || norm.starts_with("tel:")
    }
}

/// Parse attributes up to and including the closing '>'. Returns None if the tag never closes.
fn parse_attrs(s: &str) -> Option<(Vec<(String, Option<String>)>, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'>' {
            return Some((attrs, i + 1));
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if ns == i {
            i += 1; // stray '=' etc.
            continue;
        }
        let name = s[ns..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = None;
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            if b[i] == b'"' || b[i] == b'\'' {
                let q = b[i];
                let vs = i + 1;
                let ve = b[vs..].iter().position(|c| *c == q)? + vs;
                val = Some(s[vs..ve].to_string());
                i = ve + 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                val = Some(s[vs..i].to_string());
            }
        }
        if !name.starts_with("on") {
            attrs.push((name, val));
        }
    }
}

/// Decode the entities a browser would decode in an attribute value (numeric + a few named).
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let end = rest[1..].find(|c: char| !c.is_ascii_alphanumeric() && c != '#').map(|e| e + 1);
        let (body, has_semi) = match end {
            Some(e) if rest.as_bytes().get(e) == Some(&b';') => (&rest[1..e], true),
            Some(e) => (&rest[1..e], false),
            None => (&rest[1..], false),
        };
        let ch = if let Some(h) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = body.strip_prefix('#') {
            d.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match body {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "colon" => Some(':'),
                "Tab" => Some('\t'),
                "NewLine" => Some('\n'),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            }
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[1 + body.len() + usize::from(has_semi)..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &str) -> String {
        let mut z = Sanitizer::new(false);
        let mut o = String::new();
        z.push(s, &mut o);
        z.finish(&mut o);
        o
    }

    #[test]
    fn strips_scripts_and_handlers() {
        assert_eq!(run("<script>alert(1)</script>ok"), "ok");
        assert_eq!(run("<img src=x onerror=alert(1)>"), "<img src=\"x\">");
        assert_eq!(run("<b onclick=\"x()\">hi</b>"), "<b>hi</b>");
    }

    #[test]
    fn blocks_js_urls_even_when_obfuscated() {
        assert!(!run("<a href=\"javascript:alert(1)\">x</a>").contains("href"));
        assert!(!run("<a href=\"jav&#x61;script:alert(1)\">x</a>").contains("href"));
        assert!(!run("<a href=\"java\tscript:alert(1)\">x</a>").contains("href"));
        assert!(!run("<a href=\"&#106;avascript&colon;alert(1)\">x</a>").contains("href"));
        assert!(run("<a href=\"https://a.b/?x=1&amp;y=2\">x</a>").contains("href=\"https://a.b/?x=1&amp;y=2\""));
        assert!(run("<a href=\"#sec\">x</a>").contains("href=\"#sec\""));
    }

    #[test]
    fn remote_images_blocked() {
        assert_eq!(run("<img src=\"https://t.example/p.gif\">"), "<img>");
        assert!(run("<img src=\"pic.png\">").contains("pic.png"));
    }

    #[test]
    fn unbalanced_end_tags_cannot_escape() {
        assert_eq!(run("</div></div>text"), "text");
        assert_eq!(run("<div><p>x"), "<div><p>x</p></div>");
    }

    #[test]
    fn discard_spans_fragments() {
        let mut z = Sanitizer::new(false);
        let mut o = String::new();
        z.push("<style>", &mut o);
        z.push("body{display:none}", &mut o);
        z.push("</style>after", &mut o);
        assert_eq!(o, "after");
    }

    #[test]
    fn stray_lt_and_unterminated() {
        assert_eq!(run("a < b"), "a &lt; b");
        assert_eq!(run("<a href=\"x"), "&lt;a href=&quot;x".replace("&quot;", "\"").as_str());
    }
}
