//! Foreground-app tracking on X11: watch `_NET_ACTIVE_WINDOW` on the root
//! window and resolve the active window's `_NET_WM_PID` to an exe name.
//! Wayland has no common equivalent; under XWayland only X clients (which
//! includes DaVinci Resolve) are reported. Without an X server no foreground
//! events are produced and only running/always rules can match.
use std::sync::{LazyLock, Mutex};

use tokio::sync::mpsc;
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window},
    },
    rust_connection::RustConnection,
};

use crate::list::process_name;

static CHANNEL: LazyLock<FocusChannel> = LazyLock::new(FocusChannel::new);

struct FocusChannel {
    tx: mpsc::UnboundedSender<String>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

impl FocusChannel {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let thread_tx = tx.clone();
        std::thread::Builder::new()
            .name("ulanzi-d100h-focus".to_string())
            .spawn(move || {
                if let Err(error) = watch(&thread_tx) {
                    eprintln!("focus: {error}");
                }
            })
            .expect("spawn focus thread");
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    }
}

struct Atoms {
    active_window: u32,
    wm_pid: u32,
}

fn connect() -> Result<(RustConnection, Window, Atoms), String> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| format!("X11 connect: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &[u8]| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom)
    };
    let atoms = Atoms {
        active_window: atom(b"_NET_ACTIVE_WINDOW")?,
        wm_pid: atom(b"_NET_WM_PID")?,
    };
    Ok((conn, root, atoms))
}

fn cardinal(conn: &RustConnection, window: Window, property: u32, ty: AtomEnum) -> Option<u32> {
    let reply = conn
        .get_property(false, window, property, ty, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}

fn active_process(conn: &RustConnection, root: Window, atoms: &Atoms) -> Option<String> {
    let window = cardinal(conn, root, atoms.active_window, AtomEnum::WINDOW).filter(|&w| w != 0)?;
    let pid = cardinal(conn, window, atoms.wm_pid, AtomEnum::CARDINAL)?;
    if pid == std::process::id() {
        return None;
    }
    process_name(pid)
}

fn watch(tx: &mpsc::UnboundedSender<String>) -> Result<(), String> {
    let (conn, root, atoms) = connect()?;
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    let mut last = String::new();
    loop {
        match conn
            .wait_for_event()
            .map_err(|e| format!("X11 connection lost: {e}"))?
        {
            Event::PropertyNotify(event) if event.atom == atoms.active_window => {
                // No active window (or an unresolvable one) is reported as ""
                // so rules fall back instead of keeping the last app's.
                let name = active_process(&conn, root, &atoms).unwrap_or_default();
                if name != last {
                    last.clone_from(&name);
                    let _ = tx.send(name);
                }
            }
            _ => {}
        }
    }
}

/// Exe name of the active window right now, if an X server is reachable.
pub fn foreground_process() -> Option<String> {
    let (conn, root, atoms) = connect().ok()?;
    active_process(&conn, root, &atoms)
}

/// Take the single focus-event receiver. The watcher thread starts on first
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
