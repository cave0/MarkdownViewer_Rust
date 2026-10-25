# mdview (Rust)

Fast dark-mode Markdown viewer for Windows 10/11 x64. A port of the Python/pywebview viewer:
same look, same features, native start-up.

* Markdown: GFM tables, task lists, strikethrough, footnotes, definition lists, `> [!NOTE]` alerts,
  heading ids, front matter hidden, bare-URL autolinks, `$..$`, `$$..$$`, `\(..\)`, `\[..\]`
* KaTeX and Mermaid embedded (offline), loaded only when the document contains them
* Syntax highlighting (highlight.js, 190+ languages), applied lazily to code near the viewport
* Outline sidebar with scroll-spy, find (Ctrl+F), night colors, black background, raw source view, print/PDF
* Live reload (keeps scroll position), drag-and-drop a .md onto the window, Ctrl+O to open
* Safe Mode by default: raw HTML sanitized, remote images blocked, strict CSP (no inline script).
  `--trusted` keeps raw HTML (scripts still never run).

## Download
Every push builds `mdview.exe` in GitHub Actions (Actions tab -> latest run -> Artifacts).
Pushing a tag such as `v3.0.0` publishes it as a GitHub Release.

## Build (on Windows)
1. Install Rust (https://rustup.rs) and Visual Studio Build Tools ("Desktop development with C++").
2. `powershell -ExecutionPolicy Bypass -File build.ps1` -> `dist\mdview.exe` (single file),
   or simply `cargo build --release --locked` -> `target\release\mdview.exe`.
   CSS/JS under `assets/` is embedded at compile time: after editing it, rebuild to see the change.
3. Optional: `powershell -ExecutionPolicy Bypass -File associate.ps1` to open .md files with it.

## Features added in this version
* **Light theme** - `Ctrl+Shift+L` (or the sun/moon button); remembered, Mermaid follows it.
* **Wide diagrams** keep a readable size and scroll sideways; click a diagram for a full-size view.
  Hover a diagram for *SVG* / *Mermaid* copy buttons; hover display math for a *TeX* copy button.
* **Emoji shortcodes** (`:tada:`) and `==highlight==` in body text (never inside code, links or math).
* **Reading position** is remembered per file and restored next time you open it.

## Use
`mdview.exe README.md`   `mdview.exe` (file picker)   `mdview.exe --trusted doc.md`   `mdview.exe --help`

Needs the Microsoft Edge WebView2 Runtime (preinstalled on current Windows 10/11).
Settings (night colors, theme, sidebar) persist in `%LOCALAPPDATA%\mdview`.

## Development on other platforms
The window needs Windows, but the renderer is portable: `cargo test --locked` runs on Linux/macOS, and
`mdview --dump-html out.html file.md` renders a document to a standalone page anywhere.

## Tests
* `cargo test --locked` - Rust unit tests (markdown, sanitizer, file server, embedded-asset regression checks).
* `cd tests/ui && npm install && npx playwright install chromium && npm test` - headless-browser tests for
  `assets/app.js` and `style.css` (diagram rendering and id collisions, wide-diagram scrolling, math copy, emoji,
  light theme, reading position). Set `CHROME_PATH` to use an existing Chromium.
* CI runs all of the above, plus advisory `cargo fmt`, `clippy` and `cargo audit`.
