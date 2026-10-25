//! Windows front end: tao window + WebView2 (via wry), custom `mdview://` protocol, live reload.

use crate::site::{file_sig, is_md, Site, MD_EXTS};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Fullscreen, Icon, WindowBuilder};
use wry::http::Response;
use wry::{DragDropEvent, NewWindowResponse, WebContext, WebViewBuilder};

const ORIGIN: &str = "http://mdview.localhost";
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const POLL_MS: u64 = 400;

#[derive(Debug)]
enum UserEvent {
    Reload,
    Fullscreen,
    OpenDialog,
    Open(PathBuf),
    Title(String),
}

pub fn message_box(title: &str, text: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    // 0x10 = MB_ICONERROR for failures, 0x40 = MB_ICONINFORMATION for plain info
    let kind = if title == "mdview" && !text.starts_with("Cannot") { 0x40 } else { 0x10 };
    unsafe { MessageBoxW(core::ptr::null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), kind) };
}

fn pick_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Markdown file")
        .add_filter("Markdown", MD_EXTS)
        .add_filter("All files", &["*"])
        .pick_file()
}

fn app_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("mdview"))
}

fn data_dir() -> Option<PathBuf> {
    app_dir().map(|d| d.join("WebView2"))
}

/// Remembered window placement. Size is in logical pixels, position in physical (screen) pixels.
#[derive(Clone, Copy)]
struct WinState {
    w: f64,
    h: f64,
    maximized: bool,
    pos: Option<(i32, i32)>,
}

fn load_window_state() -> Option<WinState> {
    let txt = std::fs::read_to_string(app_dir()?.join("window.txt")).ok()?;
    let mut it = txt.split_whitespace();
    let w: f64 = it.next()?.parse().ok()?;
    let h: f64 = it.next()?.parse().ok()?;
    let maximized = it.next()? == "1";
    let pos = match (it.next().and_then(|v| v.parse().ok()), it.next().and_then(|v| v.parse().ok())) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    ((400.0..=10000.0).contains(&w) && (300.0..=10000.0).contains(&h)).then_some(WinState { w, h, maximized, pos })
}

fn save_window_state(st: &WinState) {
    if let Some(d) = app_dir() {
        let _ = std::fs::create_dir_all(&d);
        let mut line = format!("{} {} {}", st.w.round(), st.h.round(), u8::from(st.maximized));
        if let Some((x, y)) = st.pos {
            line.push_str(&format!(" {x} {y}"));
        }
        let _ = std::fs::write(d.join("window.txt"), line);
    }
}

/// Small procedural icon (blue rounded square with a white "M"), so no image asset is needed.
fn app_icon() -> Option<Icon> {
    const N: u32 = 32;
    let mut px = vec![0u8; (N * N * 4) as usize];
    let inside = |x: f32, y: f32| {
        let (cx, cy, r) = (x.clamp(5.0, N as f32 - 6.0), y.clamp(5.0, N as f32 - 6.0), 5.0);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r + 0.5
    };
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = [0u8; 4];
            if inside(fx, fy) {
                c = [31, 111, 235, 255];
                // "M": two stems and a V in the middle
                let stem = (x >= 7 && x <= 9 || x >= 22 && x <= 24) && (9..=23).contains(&y);
                let v = (9..=17).contains(&y) && {
                    let d = (y as i32 - 9) as f32;
                    (fx - (9.5 + d * 0.9)).abs() < 1.6 || (fx - (22.5 - d * 0.9)).abs() < 1.6
                };
                if stem || v {
                    c = [255, 255, 255, 255];
                }
            }
            let i = ((y * N + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&c);
        }
    }
    Icon::from_rgba(px, N, N).ok()
}

pub fn run(file: Option<PathBuf>, trusted: bool, allow_remote: bool) {
    let file = match file.or_else(pick_file) {
        Some(f) => f,
        None => return, // picker cancelled
    };
    let site = match Site::new(&file, trusted, allow_remote) {
        Ok(s) if s.current().is_file() => Arc::new(s),
        Ok(_) => return message_box("mdview", &format!("Cannot open {}: not a file", file.display())),
        Err(e) => return message_box("mdview", &format!("Cannot open {}: {e}", file.display())),
    };
    if let Err(e) = run_window(site) {
        message_box(
            "mdview - cannot start",
            &format!("{e}\n\nmdview needs the Microsoft Edge WebView2 Runtime (included with current Windows 10/11 and Edge).\nDownload: {WEBVIEW2_URL}"),
        );
        std::process::exit(1);
    }
}

fn run_window(site: Arc<Site>) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let start_name = site.current().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let saved = load_window_state();
    let mut state = saved.unwrap_or(WinState { w: 1200.0, h: 800.0, maximized: false, pos: None });
    // Only restore the position if its title bar is still on a connected monitor
    // (a laptop undocked from an external screen would otherwise open off-screen).
    let on_screen = |(x, y): (i32, i32)| {
        event_loop.available_monitors().any(|m| {
            let (p, sz) = (m.position(), m.size());
            let (px, py) = (x + 100, y + 16);
            px >= p.x && px < p.x + sz.width as i32 && py >= p.y && py < p.y + sz.height as i32
        })
    };
    if let Some(pos) = state.pos {
        if !on_screen(pos) {
            state.pos = None;
        }
    }
    let mut wb = WindowBuilder::new()
        .with_title(format!("{start_name} \u{2014} mdview"))
        .with_window_icon(app_icon())
        .with_inner_size(LogicalSize::new(state.w, state.h))
        .with_maximized(state.maximized);
    if let Some((x, y)) = state.pos {
        wb = wb.with_position(PhysicalPosition::new(x, y));
    }
    let window = wb
        .with_visible(false) // shown once the (dark) webview exists, so there is no white flash
        .build(&event_loop)
        .map_err(|e| e.to_string())?;

    let mut ctx = WebContext::new(data_dir());
    let handler_site = site.clone();
    let ipc_proxy = proxy.clone();
    let title_proxy = proxy.clone();
    let drop_proxy = proxy.clone();

    let start_url = format!("{ORIGIN}{}", site.url_for(&site.current()).unwrap_or_else(|| "/".into()));
    let webview = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_background_color((13, 17, 23, 255))
        .with_custom_protocol("mdview".into(), move |_id, request| {
            let r = handler_site.serve(request.uri().path());
            let mut b = Response::builder()
                .status(r.status)
                .header("Content-Type", r.content_type)
                .header("X-Content-Type-Options", "nosniff")
                .header("Cache-Control", if r.cacheable { "max-age=3600" } else { "no-store" });
            if let Some(csp) = r.csp {
                b = b.header("Content-Security-Policy", csp);
            }
            b.body(r.body).unwrap_or_else(|_| Response::new(std::borrow::Cow::Borrowed(&b"error"[..])))
        })
        .with_navigation_handler(|url| {
            if url.starts_with(ORIGIN) || url == "about:blank" {
                return true;
            }
            open_external(&url);
            false
        })
        .with_new_window_req_handler(|url, _| {
            open_external(&url);
            NewWindowResponse::Deny
        })
        .with_document_title_changed_handler(move |t| {
            let _ = title_proxy.send_event(UserEvent::Title(t));
        })
        .with_ipc_handler(move |req| match req.body().as_str() {
            "fullscreen" => {
                let _ = ipc_proxy.send_event(UserEvent::Fullscreen);
            }
            "open" => {
                let _ = ipc_proxy.send_event(UserEvent::OpenDialog);
            }
            _ => {}
        })
        .with_drag_drop_handler(move |ev| {
            if let DragDropEvent::Drop { paths, .. } = ev {
                if let Some(p) = paths.into_iter().find(|p| is_md(p)) {
                    let _ = drop_proxy.send_event(UserEvent::Open(p));
                }
            }
            true // we handle drops ourselves; never let WebView2 navigate to a dropped file
        })
        .with_url(start_url)
        .build(&window)
        .map_err(|e| e.to_string())?;

    window.set_visible(true);
    window.set_focus();

    // Live reload: cheap stat() polling of whichever document is showing. Unlike OS change
    // notifications it survives editors that save via rename, network drives, and navigation.
    let poll_site = site.clone();
    let poll_proxy = proxy.clone();
    std::thread::spawn(move || {
        let mut seen: Option<(PathBuf, _)> = None;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_MS));
            let cur = poll_site.current();
            let Some(sig) = file_sig(&cur) else { continue }; // mid-save: file briefly missing
            if sig.1 == 0 {
                continue; // truncated mid-write; wait for the real content
            }
            match &seen {
                Some((p, s)) if *p == cur && *s != sig => {
                    if poll_proxy.send_event(UserEvent::Reload).is_err() {
                        return;
                    }
                }
                _ => {}
            }
            seen = Some((cur, sig));
        }
    });

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // Track the *restored* placement only: ignore maximized, full-screen and minimized
            // states (a minimized window reports (-32000, -32000)).
            Event::WindowEvent { event: WindowEvent::Resized(sz), .. } => {
                if sz.width > 0 && sz.height > 0 && !window.is_maximized() && window.fullscreen().is_none() {
                    let l = sz.to_logical::<f64>(window.scale_factor());
                    state.w = l.width;
                    state.h = l.height;
                }
            }
            Event::WindowEvent { event: WindowEvent::Moved(p), .. } => {
                if p.x > -30000 && p.y > -30000 && !window.is_maximized() && window.fullscreen().is_none() {
                    state.pos = Some((p.x, p.y));
                }
            }
            Event::UserEvent(UserEvent::Reload) => {
                let _ = webview.evaluate_script("window.__mdReload && window.__mdReload()");
            }
            Event::UserEvent(UserEvent::Fullscreen) => {
                let fs = if window.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) };
                window.set_fullscreen(fs);
            }
            Event::UserEvent(UserEvent::OpenDialog) => {
                if let Some(p) = pick_file() {
                    open_in(&site, &webview, p);
                }
            }
            Event::UserEvent(UserEvent::Open(p)) => open_in(&site, &webview, p),
            Event::UserEvent(UserEvent::Title(t)) => {
                if !t.is_empty() {
                    window.set_title(&format!("{t} \u{2014} mdview"));
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                state.maximized = window.is_maximized();
                save_window_state(&state);
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

fn open_in(site: &Site, webview: &wry::WebView, file: PathBuf) {
    match site.open(&file) {
        Ok(url) => {
            let _ = webview.load_url(&format!("{ORIGIN}{url}"));
        }
        Err(e) => message_box("mdview", &format!("Cannot open {}: {e}", file.display())),
    }
}

fn open_external(url: &str) {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        let _ = open::that_detached(url);
    }
}
