//! Foreground-window tracking: poll once, then stream changes through a
//! SetWinEventHook on a dedicated thread that owns a Win32 message loop.
use std::sync::{LazyLock, Mutex};

use tokio::sync::mpsc;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    UI::{
        Accessibility::SetWinEventHook,
        WindowsAndMessaging::{
            DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW,
            GetWindowThreadProcessId, MSG, TranslateMessage, WINEVENT_OUTOFCONTEXT,
            WINEVENT_SKIPOWNPROCESS,
        },
    },
};
/// static. The receiver is handed out once by `focus_events()`; the hook
/// thread is spawned inside the LazyLock initializer on first access.
static CHANNEL: LazyLock<FocusChannel> = LazyLock::new(FocusChannel::new);

struct FocusChannel {
    tx: mpsc::UnboundedSender<String>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

impl FocusChannel {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("ulanzi-focus-hook".to_string())
            .spawn(hook_thread)
            .expect("spawn focus hook thread");
        Self { tx, rx: Mutex::new(Some(rx)) }
    }
}

/// Exe name (lowercase, no path) owning the current foreground window.
pub fn foreground_process() -> Option<String> {
    process_name_for_window(unsafe { GetForegroundWindow() })
}

/// Exe name for any window handle.
pub fn process_name_for_window(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<String> {
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(|name| name.to_lowercase())
    }
}

unsafe extern "system" fn on_foreground(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: windows_sys::Win32::Foundation::HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _event_time: u32,
) {
    if let Some(name) = process_name_for_window(hwnd) {
        let _ = CHANNEL.tx.send(name);
    }
}

fn hook_thread() {
    unsafe {
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if hook.is_null() {
            eprintln!("focus: SetWinEventHook failed");
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Take the single focus-event receiver. The hook thread starts on first
/// call; the current foreground exe is pushed immediately when resolvable.
/// Panics on a second call — the engine is the sole subscriber by design.
pub fn focus_events() -> mpsc::UnboundedReceiver<String> {
    if let Some(current) = foreground_process() {
        let _ = CHANNEL.tx.send(current);
    }
    CHANNEL
        .rx
        .lock()
        .expect("focus receiver mutex")
        .take()
        .expect("focus_events() may only be called once")
}
