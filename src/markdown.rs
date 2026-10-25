//! Markdown -> HTML. One pass over pulldown-cmark events; no regex, no per-line allocations.

use crate::sanitize::Sanitizer;
use pulldown_cmark::{
    html, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, TextMergeWithOffset,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use pulldown_cmark_escape::{escape_href, escape_html};
use std::borrow::Cow;
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct RenderOpts {
    /// Safe Mode: sanitize raw HTML, block remote images.
    pub safe: bool,
    pub allow_remote_images: bool,
}

pub struct Heading {
    pub level: u8,
    pub title: String,
    pub id: String,
    /// "user" / "assistant" / "system" when this heading starts a chat turn
    pub role: Option<&'static str>,
}

pub struct Doc {
    pub body: String,
    pub toc: Vec<Heading>,
    pub words: usize,
    pub reading_min: usize,
    pub has_math: bool,
    /// number of chat turns detected (speaker headings / bold speaker labels)
    pub turns: usize,
    /// the (pre-processed) Markdown the `data-o` turn offsets refer to; only kept when turns > 0
    pub turn_source: Option<String>,
}

pub fn render(src: &str, opts: RenderOpts) -> Doc {
    let src = preprocess_math(src);
    let mut po = Options::empty();
    po.insert(Options::ENABLE_TABLES);
    po.insert(Options::ENABLE_FOOTNOTES);
    po.insert(Options::ENABLE_STRIKETHROUGH);
    po.insert(Options::ENABLE_TASKLISTS);
    po.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    po.insert(Options::ENABLE_MATH);
    po.insert(Options::ENABLE_DEFINITION_LIST);
    po.insert(Options::ENABLE_GFM);
    po.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

    let parser = TextMergeWithOffset::new(Parser::new_ext(&src, po).into_offset_iter());

    let mut events: Vec<Event> = Vec::with_capacity(src.len() / 24 + 16);
    let mut toc: Vec<Heading> = Vec::new();
    let mut slugs: HashMap<String, u32> = HashMap::new();
    let mut san = Sanitizer::new(opts.allow_remote_images);
    let mut has_math = false;

    struct Hd {
        idx: usize,
        level: u8,
        start: usize,
        id: Option<String>,
        classes: Vec<String>,
        text: String,
    }
    struct Img {
        src: String,
        title: String,
        alt: String,
        depth: u32,
    }
    #[derive(PartialEq)]
    enum PState {
        None,
        Fresh,
        InStrong,
        Done,
    }
    let mut heading: Option<Hd> = None;
    let mut image: Option<Img> = None;
    let mut code: Option<(String, String)> = None; // (lang, content)
    let mut in_meta = false;
    let mut link_depth = 0u32;
    // chat-turn detection
    let mut container_depth = 0u32;
    let mut p_state = PState::None;
    let mut p_idx = 0usize;
    let mut p_start = 0usize;
    let mut p_text = String::new();
    let mut turns = 0usize;
    let mut u16c = U16Counter::default();

    for (ev, range) in parser {
        if in_meta {
            if matches!(ev, Event::End(TagEnd::MetadataBlock(_))) {
                in_meta = false;
            }
            continue;
        }
        if let Some(img) = image.as_mut() {
            match ev {
                Event::Start(Tag::Image { .. }) => img.depth += 1,
                Event::End(TagEnd::Image) => {
                    if img.depth > 0 {
                        img.depth -= 1;
                    } else {
                        let img = image.take().unwrap();
                        events.push(Event::Html(CowStr::from(image_html(&img.src, &img.title, &img.alt, opts))));
                    }
                }
                Event::Text(t) | Event::Code(t) => img.alt.push_str(&t),
                Event::SoftBreak | Event::HardBreak => img.alt.push(' '),
                _ => {}
            }
            continue;
        }
        if let Some((_, buf)) = code.as_mut() {
            match ev {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, buf) = code.take().unwrap();
                    if lang == "math" {
                        has_math = true;
                        events.push(Event::Html(CowStr::from(math_html(&buf, true))));
                    } else {
                        events.push(Event::Html(CowStr::from(code_html(&lang, &buf))));
                    }
                }
                _ => {}
            }
            continue;
        }

        // first-strong detection for "**User:** ..." style paragraphs
        match p_state {
            PState::Fresh => {
                p_state = if matches!(ev, Event::Start(Tag::Strong)) { PState::InStrong } else { PState::Done };
                if p_state == PState::InStrong {
                    p_text.clear();
                    events.push(ev);
                    continue;
                }
            }
            PState::InStrong => match &ev {
                Event::Text(t) | Event::Code(t) => p_text.push_str(t),
                Event::End(TagEnd::Strong) => {
                    p_state = PState::Done;
                    if let Some(role) = role_of(&p_text) {
                        let o = u16c.at(&src, p_start);
                        events[p_idx] = Event::Html(CowStr::from(format!("<p data-role=\"{role}\" data-o=\"{o}\">")));
                        turns += 1;
                    }
                }
                _ => p_state = PState::Done,
            },
            _ => {}
        }

        match ev {
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_) | Tag::Table(_) | Tag::DefinitionList) => {
                container_depth += 1;
                events.push(ev);
            }
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition | TagEnd::Table | TagEnd::DefinitionList) => {
                container_depth = container_depth.saturating_sub(1);
                events.push(ev);
            }
            Event::Start(Tag::Paragraph) if container_depth == 0 && heading.is_none() => {
                events.push(Event::Html(CowStr::Borrowed("<p>")));
                p_idx = events.len() - 1;
                p_start = range.start;
                p_state = PState::Fresh;
            }
            Event::End(TagEnd::Paragraph) if p_state != PState::None => {
                p_state = PState::None;
                events.push(Event::Html(CowStr::Borrowed("</p>\n")));
            }
            Event::Start(Tag::Heading { level, id, classes, .. }) => {
                events.push(Event::Html(CowStr::Borrowed(""))); // patched at End
                heading = Some(Hd {
                    idx: events.len() - 1,
                    level: level as u8,
                    start: range.start,
                    id: id.map(|s| s.to_string()),
                    classes: classes.into_iter().map(|c| c.to_string()).collect(),
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Heading(l)) => {
                if let Some(h) = heading.take() {
                    let id = match h.id {
                        Some(id) if !id.is_empty() => id,
                        _ => slugify(&h.text, &mut slugs, h.idx),
                    };
                    let role = if container_depth == 0 { role_of(&h.text) } else { None };
                    let mut s = format!("<h{} id=\"", h.level);
                    escape_html(&mut s, &id).ok();
                    s.push('"');
                    if !h.classes.is_empty() {
                        s.push_str(" class=\"");
                        escape_html(&mut s, &h.classes.join(" ")).ok();
                        s.push('"');
                    }
                    if let Some(r) = role {
                        let o = u16c.at(&src, h.start);
                        s.push_str(&format!(" data-role=\"{r}\" data-o=\"{o}\""));
                        turns += 1;
                    }
                    s.push('>');
                    events[h.idx] = Event::Html(CowStr::from(s));
                    let title = if h.text.trim().is_empty() { format!("Section {}", toc.len() + 1) } else { h.text.trim().to_string() };
                    toc.push(Heading { level: h.level, title, id, role });
                }
                events.push(Event::End(TagEnd::Heading(l)));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").trim_matches(|c| c == '{' || c == '}' || c == '.').to_ascii_lowercase()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::Start(Tag::Image { dest_url, title, .. }) => {
                image = Some(Img { src: dest_url.to_string(), title: title.to_string(), alt: String::new(), depth: 0 });
            }
            Event::Start(Tag::Link { link_type, dest_url, title, .. }) => {
                link_depth += 1;
                events.push(Event::Html(CowStr::from(link_open(link_type, &dest_url, &title))));
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
                events.push(Event::Html(CowStr::Borrowed("</a>")));
            }
            Event::InlineMath(m) => {
                has_math = true;
                if let Some(h) = heading.as_mut() {
                    h.text.push_str(&m);
                }
                events.push(Event::Html(CowStr::from(math_html(&m, false))));
            }
            Event::DisplayMath(m) => {
                has_math = true;
                events.push(Event::Html(CowStr::from(math_html(&m, true))));
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                if opts.safe {
                    let mut out = String::with_capacity(h.len());
                    san.push(&h, &mut out);
                    if !out.is_empty() {
                        events.push(Event::Html(CowStr::from(out)));
                    }
                } else {
                    events.push(Event::Html(h));
                }
            }
            Event::Text(t) => {
                if let Some(h) = heading.as_mut() {
                    h.text.push_str(&t);
                    events.push(Event::Text(t));
                } else if link_depth == 0 && (t.contains("http://") || t.contains("https://")) {
                    autolink(&t, &mut events);
                } else {
                    events.push(Event::Text(t));
                }
            }
            Event::Code(t) => {
                if let Some(h) = heading.as_mut() {
                    h.text.push_str(&t);
                }
                events.push(Event::Code(t));
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(h) = heading.as_mut() {
                    h.text.push(' ');
                }
                events.push(ev);
            }
            other => events.push(other),
        }
    }

    let mut body = String::with_capacity(src.len() * 2 + 256);
    html::push_html(&mut body, events.into_iter());
    if opts.safe {
        san.finish(&mut body);
    }

    let words = count_words(&src);
    let turn_source = (turns > 0 && src.len() <= 4_000_000).then(|| src.into_owned());
    Doc { body, toc, words, reading_min: ((words as f64 / 200.0).round() as usize).max(1), has_math, turns, turn_source }
}

/// Byte offset -> UTF-16 offset (what JavaScript string indices use), incrementally: queries must
/// come in increasing order, which document order guarantees.
#[derive(Default)]
struct U16Counter {
    byte: usize,
    units: usize,
}
impl U16Counter {
    fn at(&mut self, src: &str, byte: usize) -> usize {
        if byte > self.byte && byte <= src.len() && src.is_char_boundary(byte) {
            self.units += src[self.byte..byte].encode_utf16().count();
            self.byte = byte;
        }
        self.units
    }
}

/// Is this heading / bold label a chat speaker ("User", "You said:", "🤖 Assistant", "Claude (Opus)")?
fn role_of(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > 80 {
        return None;
    }
    let t = t.trim_start_matches(|c: char| !c.is_alphanumeric());
    let lower = t.to_lowercase();
    let first_len = lower.find(|c: char| !c.is_alphanumeric()).unwrap_or(lower.len());
    let (first, rest) = lower.split_at(first_len);
    let rest = rest.trim_start();
    let role = match first {
        "user" | "you" | "me" | "human" | "prompt" | "question" | "query" | "myself" => "user",
        "assistant" | "ai" | "claude" | "chatgpt" | "gpt" | "gemini" | "bard" | "grok" | "deepseek" | "copilot"
        | "llama" | "mistral" | "qwen" | "kimi" | "perplexity" | "model" | "bot" | "response" | "reply" | "answer"
        | "openai" | "anthropic" => "assistant",
        "system" | "developer" => "system",
        "q" | "a" if rest.starts_with(':') => {
            if first == "q" { "user" } else { "assistant" }
        }
        _ => return None,
    };
    if rest.is_empty() {
        return Some(role);
    }
    if rest.starts_with(|c: char| matches!(c, ':' | '(' | '[' | '-' | '\u{2013}' | '\u{2014}' | '|' | '\u{b7}' | '\u{2022}' | '@' | '/' | '#' | '*' | '>' | '.' | ',' | '\u{ff1a}') || c.is_ascii_digit()) {
        return Some(role);
    }
    let w = rest.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
    const OK: &[&str] = &[
        "said", "says", "wrote", "asked", "replied", "message", "input", "output", "turn", "prompt", "query", "question",
        "response", "reply", "answer", "pro", "flash", "opus", "sonnet", "haiku", "ultra", "plus", "turbo", "thinking",
        "mini", "max", "instant", "preview",
    ];
    OK.contains(&w).then_some(role)
}

fn count_words(s: &str) -> usize {
    let mut n = 0;
    let mut inw = false;
    for c in s.chars() {
        let w = c.is_alphanumeric() || c == '_';
        if w && !inw {
            n += 1;
        }
        inw = w;
    }
    n
}

pub fn slugify(title: &str, counts: &mut HashMap<String, u32>, fallback: usize) -> String {
    let mut slug = String::with_capacity(title.len());
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if (c.is_whitespace() || c == '_' || c == '-') && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        slug = format!("section-{fallback}");
    }
    let n = counts.entry(slug.clone()).or_insert(0);
    let out = if *n == 0 { slug } else { format!("{slug}-{n}") };
    *n += 1;
    out
}

fn is_remote(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("//")
}

/// Scheme of a URL as a browser would see it (None = relative / fragment).
fn scheme_of(url: &str) -> Option<String> {
    let norm: String = url.chars().filter(|c| !c.is_ascii_control() && !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let end = norm.find([':', '/', '?', '#'])?;
    if norm.as_bytes()[end] == b':' {
        Some(norm[..end].to_string())
    } else {
        None
    }
}

fn link_open(kind: LinkType, dest: &str, title: &str) -> String {
    let dest: Cow<str> = if kind == LinkType::Email && !dest.starts_with("mailto:") {
        Cow::Owned(format!("mailto:{dest}"))
    } else {
        Cow::Borrowed(dest)
    };
    let ok = match scheme_of(&dest) {
        None => true,
        Some(s) => matches!(s.as_str(), "http" | "https" | "mailto" | "tel"),
    };
    let mut s = String::from("<a");
    if ok {
        s.push_str(" href=\"");
        escape_href(&mut s, &dest).ok();
        s.push('"');
    }
    if !title.is_empty() {
        s.push_str(" title=\"");
        escape_html(&mut s, title).ok();
        s.push('"');
    }
    s.push('>');
    s
}

fn image_html(src: &str, title: &str, alt: &str, o: RenderOpts) -> String {
    let remote = is_remote(src);
    let scheme_ok = match scheme_of(src) {
        None => true,
        Some(s) => s == "http" || s == "https" || (s == "data" && src.trim_start().to_ascii_lowercase().starts_with("data:image/")),
    };
    if !scheme_ok || (remote && o.safe && !o.allow_remote_images) {
        let mut s = String::from("<span class=\"remote-image-blocked\" title=\"Image blocked in Safe Mode: ");
        escape_html(&mut s, src).ok();
        s.push_str("\">[Image Blocked: ");
        escape_html(&mut s, if alt.is_empty() { "image" } else { alt }).ok();
        s.push_str("]</span>");
        return s;
    }
    let mut s = String::from("<img src=\"");
    escape_href(&mut s, src).ok();
    s.push_str("\" alt=\"");
    escape_html(&mut s, alt).ok();
    s.push('"');
    if !title.is_empty() {
        s.push_str(" title=\"");
        escape_html(&mut s, title).ok();
        s.push('"');
    }
    s.push_str(" loading=\"lazy\" decoding=\"async\">");
    s
}

fn math_html(m: &str, display: bool) -> String {
    let (cls, tag) = if display { ("math-display", "span") } else { ("math-inline", "span") };
    let mut s = format!("<{tag} class=\"{cls}\">");
    escape_html(&mut s, m.trim()).ok();
    s.push_str("</span>");
    s
}

fn code_html(lang: &str, code: &str) -> String {
    if lang == "svg" && code.len() <= 1_000_000 && code.trim_start().to_ascii_lowercase().starts_with("<svg") {
        let mut s = String::with_capacity(code.len() * 4 + 512);
        s.push_str("<div class=\"svg-render\"><img alt=\"SVG image\" decoding=\"async\" src=\"data:image/svg+xml;charset=utf-8,");
        s.extend(utf8_percent_encode(code.trim(), NON_ALPHANUMERIC));
        s.push_str("\"></div><details class=\"svg-source\"><summary>SVG source</summary>");
        s.push_str(&plain_code_html("svg", code));
        s.push_str("</details>\n");
        return s;
    }
    plain_code_html(lang, code)
}

fn plain_code_html(lang: &str, code: &str) -> String {
    let mut s = String::with_capacity(code.len() + code.len() / 8 + 64);
    if lang == "mermaid" {
        s.push_str("<pre class=\"mermaid\">");
        escape_html(&mut s, code).ok();
        s.push_str("</pre>\n");
        return s;
    }
    let label = if lang.is_empty() { "text" } else { lang };
    s.push_str("<div class=\"code-block-container\"><div class=\"code-block-header\"><span class=\"code-lang-tag\">");
    escape_html(&mut s, label).ok();
    s.push_str("</span><button class=\"code-copy-btn\" type=\"button\">Copy</button></div><pre><code class=\"highlight");
    if !lang.is_empty() {
        s.push_str(" language-");
        // class names: keep to a conservative charset
        for c in lang.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '-' | '_')) {
            s.push(c);
        }
    }
    s.push_str("\">");
    escape_html(&mut s, code).ok();
    s.push_str("</code></pre></div>\n");
    s
}

/// Turn bare http(s):// URLs inside text into links (like GFM/linkify autolinks).
fn autolink<'a>(text: &CowStr<'a>, out: &mut Vec<Event<'a>>) {
    let t: &str = text;
    let mut rest = 0usize;
    let mut i = 0usize;
    let b = t.as_bytes();
    while i < t.len() {
        let at_scheme = t[i..].starts_with("http://") || t[i..].starts_with("https://");
        let boundary = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if !(at_scheme && boundary) {
            i += t[i..].chars().next().map_or(1, |c| c.len_utf8());
            continue;
        }
        let mut end = i + t[i..].find(|c: char| c.is_whitespace() || c == '<').unwrap_or(t.len() - i);
        // trailing punctuation is not part of the URL; ')' only if unbalanced
        loop {
            let u = &t[i..end];
            let Some(last) = u.chars().next_back() else { break };
            let trim = matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*')
                || (last == ')' && u.matches(')').count() > u.matches('(').count());
            if trim {
                end -= last.len_utf8();
            } else {
                break;
            }
        }
        let scheme_len = if t[i..].starts_with("https://") { 8 } else { 7 };
        if end - i <= scheme_len {
            i += scheme_len;
            continue;
        }
        if i > rest {
            out.push(Event::Text(CowStr::from(t[rest..i].to_string())));
        }
        let url = &t[i..end];
        out.push(Event::Html(CowStr::from(link_open(LinkType::Autolink, url, ""))));
        out.push(Event::Text(CowStr::from(url.to_string())));
        out.push(Event::Html(CowStr::Borrowed("</a>")));
        rest = end;
        i = end;
    }
    if rest < t.len() {
        out.push(Event::Text(CowStr::from(t[rest..].to_string())));
    }
}

fn needs_math_pass(s: &str) -> bool {
    s.contains("$$") || s.contains("\\(") || s.contains("\\[") || s.contains("\\begin{")
}

/// Make math robust before pulldown-cmark sees it (it has no block-level math, so a line that
/// is just `=`, `-`, `+`, `1.` or `>` inside `$$ ... $$` would otherwise be parsed as a setext
/// heading / list / quote and tear the equation apart).
///
/// * normalises CRLF/CR line endings
/// * multi-line `$$ ... $$` blocks (also inside lists and block quotes) become ONE line
/// * `\( .. \)` -> `$ .. $` and `\[ .. \]` -> `$$ .. $$`
///
/// Fenced code blocks and code spans are left alone.
pub fn preprocess_math(src: &str) -> Cow<'_, str> {
    let has_cr = src.contains('\r');
    if !has_cr && !needs_math_pass(src) {
        return Cow::Borrowed(src);
    }
    let src: Cow<str> = if has_cr { Cow::Owned(src.replace("\r\n", "\n").replace('\r', "\n")) } else { Cow::Borrowed(src) };
    if !needs_math_pass(&src) {
        return src;
    }
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut chunk = String::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start_matches(' ');
        let indent = line.len() - t.len();
        if let Some((ch, n)) = fence {
            out.push_str(line);
            let run = t.bytes().take_while(|b| *b == ch).count();
            if indent < 4 && run >= n && t[run..].trim().is_empty() {
                fence = None;
            }
            i += 1;
            continue;
        }
        if indent < 4 && (t.starts_with("```") || t.starts_with("~~~")) {
            let ch = t.as_bytes()[0];
            let run = t.bytes().take_while(|b| *b == ch).count();
            if !(ch == b'`' && t[run..].contains('`')) {
                convert_chunk(&chunk, &mut out);
                chunk.clear();
                fence = Some((ch, run));
                out.push_str(line);
                i += 1;
                continue;
            }
        }
        // bare LaTeX environment (\begin{align} ... \end{align}) outside $$: wrap as display math
        {
            let plen = line.bytes().take_while(|b| matches!(b, b' ' | b'\t' | b'>')).count();
            let rest = &line[plen..];
            if let Some(env) = bare_env(rest) {
                let close = format!("\\end{{{env}}}");
                if let Some(p) = rest.find(&close) {
                    // whole environment on one line
                    let end = p + close.len();
                    convert_chunk(&chunk, &mut out);
                    chunk.clear();
                    out.push_str(&line[..plen]);
                    out.push_str("$$");
                    out.push_str(&collapse_math(&[&rest[..end]]));
                    out.push_str("$$");
                    out.push_str(rest[end..].trim_end_matches('\n'));
                    out.push('\n');
                    i += 1;
                    continue;
                }
                if let Some((joined, after, used)) = take_block(&lines[i..], &line[..plen], rest, &close, true) {
                    convert_chunk(&chunk, &mut out);
                    chunk.clear();
                    out.push_str(&line[..plen]);
                    out.push_str("$$");
                    out.push_str(&joined);
                    out.push_str("$$");
                    out.push_str(after.trim_end_matches('\n'));
                    out.push('\n');
                    i += used;
                    continue;
                }
            }
        }
        // multi-line display math block?
        let plen = line.bytes().take_while(|b| matches!(b, b' ' | b'\t' | b'>')).count();
        let rest = &line[plen..];
        if rest.starts_with("$$") && !rest[2..].contains("$$") {
            if let Some((joined, after, used)) = take_block(&lines[i..], &line[..plen], &rest[2..], "$$", false) {
                convert_chunk(&chunk, &mut out);
                chunk.clear();
                out.push_str(&line[..plen]);
                out.push_str("$$");
                out.push_str(&joined);
                out.push_str("$$");
                out.push_str(after.trim_end_matches('\n'));
                out.push('\n');
                i += used;
                continue;
            }
        }
        chunk.push_str(line);
        i += 1;
    }
    convert_chunk(&chunk, &mut out);
    Cow::Owned(out)
}

/// `lines[0]` opens a math block whose first line's text is `first`; the block ends at the first
/// line containing `close`. Returns the collapsed content, whatever follows `close` on its line,
/// and how many lines were consumed. `keep_close` keeps the closer in the content (for `\end{..}`).
fn take_block<'a>(lines: &[&'a str], prefix: &str, first: &str, close: &str, keep_close: bool) -> Option<(String, &'a str, usize)> {
    const MAX_LINES: usize = 200;
    let quoted = prefix.contains('>');
    let mut parts: Vec<&str> = vec![first];
    for (k, l) in lines.iter().enumerate().skip(1).take(MAX_LINES) {
        let body = if quoted { l.trim_start_matches([' ', '\t', '>']) } else { l.trim_start_matches([' ', '\t']) };
        if let Some(p) = body.find(close) {
            let end = if keep_close { p + close.len() } else { p };
            parts.push(&body[..end]);
            return Some((collapse_math(&parts), &body[p + close.len()..], k + 1));
        }
        parts.push(body);
    }
    None
}

/// Name of a math environment that starts this line (`\begin{align*}` -> "align*"), if it is one
/// that only makes sense in math mode.
fn bare_env(rest: &str) -> Option<&str> {
    const ENVS: &[&str] = &[
        "equation", "align", "gather", "alignat", "multline", "eqnarray", "aligned", "split", "cases", "matrix",
        "pmatrix", "bmatrix", "Bmatrix", "vmatrix", "Vmatrix",
    ];
    let r = rest.strip_prefix("\\begin{")?;
    let end = r.find('}')?;
    let name = &r[..end];
    let base = name.strip_suffix('*').unwrap_or(name);
    ENVS.contains(&base).then_some(name)
}

/// Join math lines with spaces (TeX treats newlines as spaces) and drop `%` comments, which
/// would otherwise swallow the rest of the joined line.
fn collapse_math(parts: &[&str]) -> String {
    let mut out = String::new();
    for p in parts {
        let mut end = p.len();
        let b = p.as_bytes();
        let mut j = 0;
        while j < b.len() {
            match b[j] {
                b'\\' => j += 2,
                b'%' => {
                    end = j;
                    break;
                }
                _ => j += 1,
            }
        }
        let piece = p[..end.min(p.len())].trim();
        if !piece.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(piece);
        }
    }
    out
}

fn convert_chunk(text: &str, out: &mut String) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                // find a closing run of exactly the same length
                let mut j = i + run;
                let mut end = None;
                while j < b.len() {
                    if b[j] == b'`' {
                        let r = b[j..].iter().take_while(|c| **c == b'`').count();
                        if r == run {
                            end = Some(j + r);
                            break;
                        }
                        j += r;
                    } else {
                        j += 1;
                    }
                }
                let stop = end.unwrap_or(i + run);
                out.push_str(&text[i..stop]);
                i = stop;
            }
            b'\\' if i + 1 < b.len() => {
                let (open_len, close, wrap) = match b[i + 1] {
                    b'(' => (2, "\\)", "$"),
                    b'[' => (2, "\\]", "$$"),
                    _ => {
                        // escaped char (incl. "\\"): copy backslash + the next char whole
                        let n = text[i + 1..].chars().next().map_or(1, |c| c.len_utf8());
                        out.push_str(&text[i..i + 1 + n]);
                        i += 1 + n;
                        continue;
                    }
                };
                match text[i + open_len..].find(close) {
                    Some(e) => {
                        let inner = &text[i + open_len..i + open_len + e];
                        let lines: Vec<&str> = inner.lines().collect();
                        out.push_str(wrap);
                        out.push_str(&collapse_math(&lines));
                        out.push_str(wrap);
                        i += open_len + e + close.len();
                    }
                    None => {
                        out.push_str(&text[i..i + open_len]);
                        i += open_len;
                    }
                }
            }
            _ => {
                let n = text[i..].chars().next().map_or(1, |c| c.len_utf8());
                out.push_str(&text[i..i + n]);
                i += n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE: RenderOpts = RenderOpts { safe: true, allow_remote_images: false };
    const TRUSTED: RenderOpts = RenderOpts { safe: false, allow_remote_images: false };

    #[test]
    fn headings_get_unique_ids_and_toc() {
        let d = render("# A b\n## A b\n### `code` *em* x\n", SAFE);
        assert!(d.body.contains("<h1 id=\"a-b\">"));
        assert!(d.body.contains("<h2 id=\"a-b-1\">"));
        assert_eq!(d.toc[2].title, "code em x");
        assert_eq!(d.toc.len(), 3);
    }

    #[test]
    fn math_is_marked_for_katex() {
        let d = render("inline $a<b$ and\n\n$$\nx^2\n$$\n\nand \\(y\\) and \\[z\\]", SAFE);
        assert!(d.has_math);
        assert!(d.body.contains("<span class=\"math-inline\">a&lt;b</span>"));
        assert_eq!(d.body.matches("math-display").count(), 2);
        assert!(d.body.contains("<span class=\"math-inline\">y</span>"));
    }

    #[test]
    fn bracket_math_skips_code() {
        let s = "```\n\\(x\\)\n```\n`\\(y\\)` and \\(z\\)";
        let p = preprocess_math(s);
        assert!(p.contains("```\n\\(x\\)\n```"));
        assert!(p.contains("`\\(y\\)`"));
        assert!(p.contains("$z$"));
    }

    #[test]
    fn mermaid_and_code() {
        let d = render("```mermaid\ngraph TD; A-->B\n```\n\n```rust\nfn x<T>() {}\n```\n", SAFE);
        assert!(d.body.contains("<pre class=\"mermaid\">graph TD; A--&gt;B\n</pre>"));
        assert!(d.body.contains("language-rust\">fn x&lt;T&gt;() {}\n</code>"));
    }

    #[test]
    fn safe_mode_sanitizes_html_and_blocks_remote_images() {
        let d = render("<script>alert(1)</script>\n\n<b onclick=\"x\">hi</b> ![a](https://x.y/z.png) [k](javascript:alert(1))", SAFE);
        assert!(!d.body.contains("script"));
        assert!(!d.body.contains("onclick"));
        assert!(d.body.contains("remote-image-blocked"));
        assert!(!d.body.contains("javascript"));
        let t = render("<b>hi</b>", TRUSTED);
        assert!(t.body.contains("<b>hi</b>"));
    }

    #[test]
    fn local_images_and_alt_text() {
        let d = render("![a *b* `c`](pic.png \"T\")", SAFE);
        assert!(d.body.contains("<img src=\"pic.png\" alt=\"a b c\" title=\"T\" loading=\"lazy\""));
    }

    #[test]
    fn front_matter_hidden() {
        let d = render("---\ntitle: x\n---\n\n# Hi\n", SAFE);
        assert!(!d.body.contains("title: x"));
        assert!(d.body.contains("Hi"));
    }

    #[test]
    fn autolinks_bare_urls() {
        let d = render("see https://a.com/x_y_z. and (https://b.com/q).", SAFE);
        assert!(d.body.contains("<a href=\"https://a.com/x_y_z\">https://a.com/x_y_z</a>."));
        assert!(d.body.contains("<a href=\"https://b.com/q\">"));
    }

    #[test]
    fn tables_tasklists_footnotes_deflist() {
        let d = render("|a|b|\n|-|-|\n|1|2|\n\n- [x] done\n\ntext[^1]\n\n[^1]: note\n\nTerm\n: Def\n", SAFE);
        assert!(d.body.contains("<table>"));
        assert!(d.body.contains("type=\"checkbox\""));
        assert!(d.body.contains("footnote"));
        assert!(d.body.contains("<dl>"));
    }

    #[test]
    fn empty_and_weird_inputs_do_not_panic() {
        for s in ["", "\n", "<", "<a", "```", "$$", "\\(", "[", "![", "# ", "\u{0}", "\\", "`"] {
            let _ = render(s, SAFE);
            let _ = render(s, TRUSTED);
        }
    }

    #[test]
    fn multiline_display_math_survives_block_syntax() {
        // lines that are just `=`, `-`, `+`, `>`, `1.`, `#` must not become headings/lists/quotes
        let src = "$$\n\\tilde{\\mu}\n=\n\\mu^0\n-\nRT\n+\nzF\n1.\n# x\n$$\n\nafter";
        let d = render(src, SAFE);
        assert!(d.toc.is_empty(), "{}", d.body);
        assert!(!d.body.contains("<ul>") && !d.body.contains("<ol>") && !d.body.contains("<h"), "{}", d.body);
        assert_eq!(d.body.matches("math-display").count(), 1, "{}", d.body);
        assert!(d.body.contains("\\tilde{\\mu} = \\mu^0 - RT + zF 1. # x"), "{}", d.body);
        assert!(d.body.contains("<p>after</p>"));
    }

    #[test]
    fn display_math_crlf_and_user_example() {
        let src = "For an ion:\r\n\r\n$$ \\tilde{\\mu}\r\n=\r\n\\mu^0+RT\\ln a+zF\\phi. $$\r\n\r\nNext\r\n";
        let d = render(src, SAFE);
        assert!(d.toc.is_empty());
        assert!(d.body.contains("\\tilde{\\mu} = \\mu^0+RT\\ln a+zF\\phi."), "{}", d.body);
        assert!(!d.body.contains('\r'));
    }

    #[test]
    fn display_math_in_lists_and_quotes_and_comments() {
        let d = render("1. item:\n   $$\n   a\n   =\n   b % note\n   $$\n2. two\n", SAFE);
        assert!(d.body.contains("a = b</span>"), "{}", d.body);
        assert!(d.body.contains("<li>two</li>"), "{}", d.body);
        let q = render("> $$\n> x\n> -\n> y\n> $$\n", SAFE);
        assert!(q.body.contains("x - y"), "{}", q.body);
        assert!(!q.body.contains("<ul>"), "{}", q.body);
    }

    #[test]
    fn unclosed_display_math_does_not_eat_document() {
        let d = render("$$\nx\n\n# Heading\n\ntext\n", SAFE);
        assert_eq!(d.toc.len(), 1);
    }

    #[test]
    fn bracket_display_math_multiline() {
        let d = render("a \\[ x\n=\ny \\] b", SAFE);
        assert!(d.toc.is_empty());
        assert!(d.body.contains("x = y"), "{}", d.body);
    }

    #[test]
    fn detects_chat_turns_in_headings_and_bold_labels() {
        let src = "# Chat log\n\n## User\n\nhé 😀 hi\n\n## Assistant\n\nhello\n\n**You:** and?\n\n**Claude:**\nanswer\n";
        let d = render(src, SAFE);
        assert_eq!(d.turns, 4, "{}", d.body);
        assert!(d.body.contains("data-role=\"user\""));
        assert!(d.body.contains("<p data-role=\"user\""));
        assert!(d.body.contains("<p data-role=\"assistant\""));
        assert_eq!(d.toc.iter().filter(|h| h.role.is_some()).count(), 2);
        // offsets are UTF-16 indices into turn_source (what JavaScript slices with)
        let ts = d.turn_source.as_ref().unwrap();
        let want = ts[..ts.find("## Assistant").unwrap()].encode_utf16().count();
        assert!(d.body.contains(&format!("data-role=\"assistant\" data-o=\"{want}\"")), "{}", d.body);
        let first = ts.encode_utf16().take(want).collect::<Vec<_>>();
        assert!(String::from_utf16(&first).unwrap().contains("😀"));
    }

    #[test]
    fn role_heuristics_avoid_false_positives() {
        for ok in ["User", "You said:", "🤖 Assistant", "Claude (Opus 4)", "ChatGPT said:", "Human:", "Gemini Pro", "Q:", "System prompt", "Me"] {
            assert!(role_of(ok).is_some(), "{ok}");
        }
        for no in ["Model selection and trade-offs", "User guide", "Answering questions", "Q&A", "A new hope", "Assistants in healthcare", "You can do it", ""] {
            assert!(role_of(no).is_none(), "{no}");
        }
        let d = render("- **User:** inside a list\n\n> **You:** in a quote\n", SAFE);
        assert_eq!(d.turns, 0);
        assert!(d.turn_source.is_none());
    }

    #[test]
    fn math_and_svg_fences() {
        let d = render("```math\nx^2 + y^2\n```\n\n```svg\n<svg viewBox=\"0 0 10 10\"><circle r=\"4\"/></svg>\n```\n\n```svg\nnot really svg\n```\n", SAFE);
        assert!(d.has_math);
        assert!(d.body.contains("<span class=\"math-display\">x^2 + y^2</span>"), "{}", d.body);
        assert!(d.body.contains("<img alt=\"SVG image\""));
        assert!(d.body.contains("data:image/svg+xml;charset=utf-8,%3Csvg"));
        assert!(!d.body.contains("<circle"), "raw svg must never reach the DOM: {}", d.body);
        assert!(d.body.contains("not really svg"));
    }

    #[test]
    fn bare_latex_environments_become_display_math() {
        let d = render("Before\n\n\\begin{align}\na &= b \\\\\n-c &= d\n\\end{align}\n\nAfter\n\n\\begin{equation} x \\end{equation}\n", SAFE);
        assert!(d.toc.is_empty());
        assert_eq!(d.body.matches("math-display").count(), 2, "{}", d.body);
        assert!(d.body.contains("\\begin{align} a &amp;= b \\\\ -c &amp;= d \\end{align}"), "{}", d.body);
        assert!(d.body.contains("<p>After</p>"));
    }

    #[test]
    fn details_blocks_with_markdown_inside() {
        let d = render("<details>\n<summary>Thinking</summary>\n\nSome **bold** and `code`.\n\n</details>\n\nafter\n", SAFE);
        assert!(d.body.contains("<details>"));
        assert!(d.body.contains("<summary>Thinking</summary>"));
        assert!(d.body.contains("<strong>bold</strong>"));
        assert!(d.body.contains("</details>"));
        assert_eq!(d.body.matches("<details>").count(), d.body.matches("</details>").count());
    }

    #[test]
    fn github_alerts() {
        let d = render("> [!WARNING]\n> careful\n", SAFE);
        assert!(d.body.contains("markdown-alert-warning"), "{}", d.body);
    }
}
