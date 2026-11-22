//! Windows system-tray icon. Runs on its own std::thread with a raw Win32
//! message loop (same pattern as narou.rs's web_tray.rs): menu actions are
//! delivered via MenuEvent, "終了" posts /api/shutdown to our own server.
use std::{io::Write, mem::zeroed, net::TcpStream};

use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

pub fn spawn(url: String, control_token: String, port: u16) {
    std::thread::Builder::new()
        .name("ulanzi-d100h-tray".to_string())
        .spawn(move || {
            if let Err(error) = run(url, control_token, port) {
                eprintln!("tray: {error}");
            }
        })
        .expect("spawn tray thread");
}

fn run(url: String, control_token: String, port: u16) -> Result<(), String> {
    let menu = Menu::new();
    let open_item = MenuItem::new("Web UI を開く", true, None);
    let exit_item = MenuItem::new("終了", true, None);
    menu.append_items(&[&open_item, &exit_item]).map_err(|e| e.to_string())?;

    let open_id = open_item.id().clone();
    let exit_id = exit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            // `start "" "url"` — the empty title keeps cmd from treating the
            // URL as the window title.
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", &url])
                .spawn();
        } else if event.id == exit_id && post_shutdown(port, &control_token) {
            unsafe { PostQuitMessage(0) };
        }
    }));

    let _tray_icon = TrayIconBuilder::new()
        .with_tooltip("ulanzi-d100h-soft")
        .with_icon(build_icon()?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(true)
        .build()
        .map_err(|e| e.to_string())?;

    unsafe {
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    Ok(())
}

/// POST /api/shutdown on the loopback server with the internal token.
fn post_shutdown(port: u16, token: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let request = format!(
        "POST /api/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-internal-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    // Drain the (tiny) response so the server sees a clean close.
    let mut buf = [0u8; 512];
    let mut reader = std::io::Read::take(&mut stream, 2048);
    std::io::Read::read(&mut reader, &mut buf).is_ok()
}

fn build_icon() -> Result<Icon, String> {
    const SIDE: u32 = 32;
    const FG: [u8; 4] = [0x5a, 0xc8, 0xfa, 0xff];
    const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

    let center = (SIDE as f32 - 1.0) / 2.0;
    let outer = 13.5f32;
    let inner = 10.5f32;
    let dot = 3.0f32;

    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let r = (dx * dx + dy * dy).sqrt();
            let pixel = if r <= dot || (r >= inner && r <= outer) { FG } else { TRANSPARENT };
            rgba.extend_from_slice(&pixel);
        }
    }
    Icon::from_rgba(rgba, SIDE, SIDE).map_err(|e| e.to_string())
}
