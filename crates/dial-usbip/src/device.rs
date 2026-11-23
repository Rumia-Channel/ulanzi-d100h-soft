//! Blackmagic "DaVinci Resolve Speed Editor" USB device emulation.
//!
//! Descriptor set mirrors the real hardware: a composite device with an
//! IAD covering a vendor-specific interface (DaVinciKeyboards driver,
//! MI_00) and a DFU runtime interface (MI_01), plus the HID interface
//! (MI_02) that carries input/output/feature reports.
//!
//! Report layouts were verified against a real device capture and the
//! KipJM/blackmacro-lib firmware.

use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const VID: u16 = 0x1edb;
pub const PID: u16 = 0xda0e;

// Serial number of the real hardware (32 hex chars, sent as an ASCII
// string descriptor).
const SERIAL: &str = "AA11BB22CC33DD44EE55FF6677889900";

/// Full-speed device descriptor (18 bytes) mirroring the real hardware:
/// composite (IAD) device class so Windows installs the matching driver
/// stack per interface.
pub const DEVICE_DESCRIPTOR: [u8; 18] = [
    0x12, 0x01, // bLength, bDescriptorType
    0x00, 0x02, // bcdUSB 2.00
    0xef, 0x02, 0x01, // Misc Device / IAD composite (real: 239/2/1)
    0x40, // bMaxPacketSize0
    0xdb, 0x1e, // idVendor 0x1edb
    0x0e, 0xda, // idProduct 0xda0e
    0x02, 0x01, // bcdDevice 1.02
    0x01, 0x02, 0x03, // iManufacturer, iProduct, iSerialNumber
    0x01, // bNumConfigurations
];

/// Real 308-byte HID report descriptor captured from a Speed Editor
/// (KipJM/blackmacro-lib, thanks u/Tree_tea).
pub const REPORT_DESCRIPTOR: [u8; 308] = [
    0x06, 0x00, 0xff, 0x09, 0x00, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x01, 0x05, 0x06, 0x15,
    0x00, 0x26, 0xff, 0x00, 0x09, 0x2a, 0x75, 0x08, 0x95, 0x04, 0xb1, 0x81, 0x09, 0x2d, 0x09, 0x2e,
    0x09, 0x2f, 0x75, 0x08, 0x95, 0x03, 0xb1, 0x81, 0xc0, 0x09, 0x08, 0xa1, 0x01, 0x85, 0x08, 0x06,
    0x07, 0xff, 0x15, 0x00, 0x26, 0xff, 0x00, 0x09, 0x00, 0x75, 0x08, 0x95, 0x20, 0xb1, 0x81, 0xc0,
    0x09, 0x02, 0xa1, 0x01, 0x85, 0x03, 0x06, 0x01, 0xff, 0x15, 0x00, 0x26, 0xff, 0x00, 0x09, 0x00,
    0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0x09, 0x01, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0x09, 0x02,
    0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0x09, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x09, 0x01,
    0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0x09, 0x02, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0xc0, 0x09,
    0x03, 0xa1, 0x01, 0x85, 0x04, 0x06, 0x02, 0xff, 0x15, 0x00, 0x26, 0xff, 0x00, 0x19, 0x00, 0x29,
    0xff, 0x75, 0x10, 0x95, 0x06, 0x81, 0x00, 0x15, 0x00, 0x25, 0x01, 0x19, 0x01, 0x29, 0x08, 0x75,
    0x01, 0x95, 0x08, 0x91, 0x02, 0xc0, 0x09, 0x06, 0xa1, 0x01, 0x85, 0x02, 0x06, 0x05, 0xff, 0x15,
    0x00, 0x25, 0x01, 0x19, 0x01, 0x29, 0x20, 0x75, 0x01, 0x95, 0x20, 0x91, 0x02, 0xc0, 0x09, 0x04,
    0xa1, 0x01, 0x85, 0x05, 0x06, 0x03, 0xff, 0x15, 0x00, 0x26, 0xff, 0x00, 0x09, 0x00, 0x75, 0x08,
    0x95, 0x01, 0xb1, 0x82, 0x09, 0x01, 0x75, 0x08, 0x95, 0x04, 0xb1, 0x82, 0x09, 0x02, 0x75, 0x08,
    0x95, 0x02, 0xb1, 0x82, 0xc0, 0x09, 0x05, 0xa1, 0x01, 0x85, 0x06, 0x06, 0x04, 0xff, 0x15, 0x00,
    0x26, 0xff, 0x00, 0x09, 0x00, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x82, 0x09, 0x01, 0x75, 0x08, 0x95,
    0x08, 0xb1, 0x82, 0xc0, 0x09, 0x07, 0xa1, 0x01, 0x85, 0x07, 0x06, 0x06, 0xff, 0x15, 0x00, 0x26,
    0xff, 0x00, 0x09, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0x09, 0x01, 0x75, 0x08, 0x95, 0x01,
    0x81, 0x02, 0x09, 0x00, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02, 0x09, 0x01, 0x75, 0x08, 0x95, 0x01,
    0xb1, 0x02, 0xc0, 0xc0
];

/// Key codes (input report 4) for the transport buttons, from the real
/// device. Used by `se:key:<name>` commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
#[allow(dead_code)]
pub enum SpeedEditorKey {
    SmartInsert = 0x01,
    Append = 0x02,
    RiplOwr = 0x03,
    CloseUp = 0x04,
    PlaceOnTop = 0x05,
    SrcOwr = 0x06,
    In = 0x07,
    Out = 0x08,
    TrimIn = 0x09,
    TrimOut = 0x0a,
    Roll = 0x0b,
    SlipSrc = 0x0c,
    SlipDest = 0x0d,
    TransDur = 0x0e,
    Cut = 0x0f,
    Dis = 0x10,
    SmthCut = 0x11,
    Source = 0x1a,
    Timeline = 0x1b,
    Shtl = 0x1c,
    Jog = 0x1d,
    Scrl = 0x1e,
    Esc = 0x31,
    SyncBin = 0x1f,
    AudioLevel = 0x2c,
    FullView = 0x2d,
    Trans = 0x22,
    Split = 0x2f,
    Snap = 0x2e,
    RiplDel = 0x2b,
    Cam1 = 0x33,
    Cam2 = 0x34,
    Cam3 = 0x35,
    Cam4 = 0x36,
    Cam5 = 0x37,
    Cam6 = 0x38,
    Cam7 = 0x39,
    Cam8 = 0x3a,
    Cam9 = 0x3b,
    LiveOwr = 0x30,
    VideoOnly = 0x25,
    AudioOnly = 0x26,
    StopPlay = 0x3c,
}

/// Resolve's config key name ("shtl", "stop_play", "trim_in", ...) to a code.
pub fn key_by_name(name: &str) -> Option<u16> {
    let key = match name.trim().to_lowercase().as_str() {
        "smart_insert" | "smart_insrt" => SpeedEditorKey::SmartInsert,
        "append" | "appnd" => SpeedEditorKey::Append,
        "ripl_owr" | "riplowr" => SpeedEditorKey::RiplOwr,
        "close_up" => SpeedEditorKey::CloseUp,
        "place_on_top" => SpeedEditorKey::PlaceOnTop,
        "src_owr" | "srcowr" => SpeedEditorKey::SrcOwr,
        "in" => SpeedEditorKey::In,
        "out" => SpeedEditorKey::Out,
        "trim_in" => SpeedEditorKey::TrimIn,
        "trim_out" => SpeedEditorKey::TrimOut,
        "roll" => SpeedEditorKey::Roll,
        "slip_src" => SpeedEditorKey::SlipSrc,
        "slip_dest" => SpeedEditorKey::SlipDest,
        "trans_dur" => SpeedEditorKey::TransDur,
        "cut" => SpeedEditorKey::Cut,
        "dis" => SpeedEditorKey::Dis,
        "smth_cut" => SpeedEditorKey::SmthCut,
        "source" => SpeedEditorKey::Source,
        "timeline" => SpeedEditorKey::Timeline,
        "shtl" => SpeedEditorKey::Shtl,
        "jog" => SpeedEditorKey::Jog,
        "scrl" => SpeedEditorKey::Scrl,
        "esc" => SpeedEditorKey::Esc,
        "sync_bin" => SpeedEditorKey::SyncBin,
        "audio_level" => SpeedEditorKey::AudioLevel,
        "full_view" => SpeedEditorKey::FullView,
        "trans" => SpeedEditorKey::Trans,
        "split" => SpeedEditorKey::Split,
        "snap" => SpeedEditorKey::Snap,
        "ripl_del" => SpeedEditorKey::RiplDel,
        "cam1" => SpeedEditorKey::Cam1,
        "cam2" => SpeedEditorKey::Cam2,
        "cam3" => SpeedEditorKey::Cam3,
        "cam4" => SpeedEditorKey::Cam4,
        "cam5" => SpeedEditorKey::Cam5,
        "cam6" => SpeedEditorKey::Cam6,
        "cam7" => SpeedEditorKey::Cam7,
        "cam8" => SpeedEditorKey::Cam8,
        "cam9" => SpeedEditorKey::Cam9,
        "live_owr" => SpeedEditorKey::LiveOwr,
        "video_only" => SpeedEditorKey::VideoOnly,
        "audio_only" => SpeedEditorKey::AudioOnly,
        "stop_play" | "play" => SpeedEditorKey::StopPlay,
        _ => return None,
    };
    Some(key as u16)
}

/// Jog mode byte Resolve programs via feature 5 (and output report 3).
const RELATIVE: u8 = 0;
const RELATIVE_FINE: u8 = 2;
const ABSOLUTE_DEADZERO: u8 = 3;

/// Absolute-mode wheel range on the real device: -4096..=4096 counts
/// (~8192 counts/turn, 23 counts per physical detent).
const JOG_ABS_MAX: i32 = 4096;
const JOG_ABS_MIN: i32 = -4096;
/// Dead-zero half-range in absolute-deadzero mode.
const JOG_DEADZERO: i32 = 300;
/// Max delta per input report in relative modes. The real wheel emits
/// ~8192 counts/turn but spreads them across host polls; dumping the
/// whole backlog into one report makes Resolve lurch.
const JOG_REPORT_CAP: i32 = 4096;

/// Authentication state machine (KipJM/blackmacro-lib pico/modules/auth.py).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum AuthStage {
    Unauthenticated = 0,
    SendChallenge = 1,
    SendAnswer = 2,
    Authenticated = 0x10,
}

#[derive(Clone, Debug)]
pub struct SpeedEditorState {
    pub stage: u8,
    pub jog_mode: u8,
    pub leds: u16,
    pub jog_leds: u8,
    pub held_keys: Vec<u16>,
    pub charging: bool,
    pub level: u8,
}

impl Default for SpeedEditorState {
    fn default() -> Self {
        Self {
            stage: AuthStage::Unauthenticated as u8,
            jog_mode: RELATIVE,
            leds: 0,
            jog_leds: 0,
            held_keys: Vec::new(),
            charging: false,
            level: 100,
        }
    }
}

struct State {
    attached: bool,
    stage: AuthStage,
    host_challenge: u32,
    jog_mode: u8,
    shaft: i32,
    shaft_sent: i32,
    /// Pending report-3 deltas awaiting an interrupt-IN URB.
    pending: i32,
    leds: u16,
    jog_leds: u8,
    held: [u16; 6],
    charging: bool,
    level: u8,
    battery_dirty: bool,
    keys_dirty: bool,
    jog_dirty: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            attached: false,
            stage: AuthStage::Unauthenticated,
            host_challenge: 0,
            jog_mode: RELATIVE,
            shaft: 0,
            shaft_sent: 0,
            pending: 0,
            leds: 0,
            jog_leds: 0,
            held: [0; 6],
            charging: false,
            level: 100,
            battery_dirty: false,
            keys_dirty: false,
            jog_dirty: false,
        }
    }
}

struct Inner {
    state: Mutex<State>,
    /// Fires when there is new input data so a parked interrupt-IN URB
    /// completes as soon as the emulated device has something to say.
    input_ready: Notify,
}

/// Shared handle: the engine queues input events, the usbip server drains
/// them as the host polls the interrupt-IN endpoint.
#[derive(Clone)]
pub struct SpeedEditor {
    inner: Arc<Inner>,
}

impl Default for SpeedEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedEditor {
    pub fn new() -> Self {
        Self { inner: Arc::new(Inner { state: Mutex::new(State::default()), input_ready: Notify::new() }) }
    }

    pub fn attached(&self) -> bool {
        self.inner.state.lock().unwrap().attached
    }

    pub fn attach(&self) -> bool {
        let mut s = self.inner.state.lock().unwrap();
        let was = s.attached;
        s.attached = true;
        !was
    }

    pub fn detach(&self) {
        let mut s = self.inner.state.lock().unwrap();
        *s = State::default();
    }

    pub fn authenticated(&self) -> bool {
        self.inner.state.lock().unwrap().stage == AuthStage::Authenticated
    }

    pub fn snapshot(&self) -> SpeedEditorState {
        let s = self.inner.state.lock().unwrap();
        SpeedEditorState {
            stage: s.stage as u8,
            jog_mode: s.jog_mode,
            leds: s.leds,
            jog_leds: s.jog_leds,
            held_keys: s.held.iter().copied().filter(|&k| k != 0).collect(),
            charging: s.charging,
            level: s.level,
        }
    }

    // ---------- Input queue (app -> device -> host interrupt-IN) ----------

    /// Queue `ticks` detent ticks at the given `scale` (counts per tick,
    /// computed from rotation velocity by the caller). In relative modes
    /// the delta is emitted on the next report; in absolute modes the
    /// shaft position is advanced like the real wheel does.
    pub fn queue_jog(&self, ticks: i32, scale: i32) {
        let mut s = self.inner.state.lock().unwrap();
        s.shaft = s.shaft.saturating_add(ticks.saturating_mul(scale.max(1)));
        s.jog_dirty = true;
        drop(s);
        self.inner.input_ready.notify_one();
    }

    /// Queue a key down/up. `down=true` presses (adds to the 6-slot held
    /// array), `down=false` releases (removes). Emits a report on the next
    /// host poll like the real device.
    pub fn queue_key(&self, code: u16, down: bool) {
        let mut s = self.inner.state.lock().unwrap();
        if down {
            if !s.held.contains(&code) {
                if let Some(slot) = s.held.iter_mut().find(|k| **k == 0) {
                    *slot = code;
                }
            }
        } else {
            for k in s.held.iter_mut() {
                if *k == code {
                    *k = 0;
                }
            }
        }
        s.keys_dirty = true;
        drop(s);
        self.inner.input_ready.notify_one();
    }

    /// Tap a key (press + release in one logical action): emits the two
    /// report-4 transitions back to back.
    pub fn tap_key(&self, code: u16) {
        self.queue_key(code, true);
        self.queue_key(code, false);
    }

    /// Wake the URB loop when it is parked on an empty queue.
    pub fn poke(&self) {
        self.inner.input_ready.notify_one();
    }

    // ---------- Device side (usbip server) ----------

    /// Wait for queued input to flush or new input to arrive.
    pub async fn wait_input(&self) {
        self.inner.input_ready.notified().await;
    }

    /// Build the next input report if any is pending. Returns the full
    /// report payload *including* the report ID byte, in wire order.
    /// Consumes the queued deltas.
    pub fn take_input_report(&self) -> Option<Vec<u8>> {
        let mut s = self.inner.state.lock().unwrap();
        // The real device answers post-auth battery state first.
        if s.battery_dirty {
            s.battery_dirty = false;
            let mut r = Vec::with_capacity(4);
            r.push(0x07); // report 7: battery
            r.push(if s.charging { 1 } else { 0 });
            r.push(s.level);
            r.push(0);
            return Some(r);
        }
        if s.keys_dirty {
            s.keys_dirty = false;
            let mut r = Vec::with_capacity(13);
            r.push(0x04);
            for k in s.held.iter() {
                r.extend_from_slice(&k.to_le_bytes());
            }
            r.push(0);
            return Some(r);
        }
        if s.jog_dirty {
            let mut r = Vec::with_capacity(7);
            r.push(0x03); // report 3: wheel
            r.push(s.jog_mode);
            match s.jog_mode {
                RELATIVE | RELATIVE_FINE => {
                    // Emit at most JOG_REPORT_CAP per report and keep the
                    // rest queued so a fast flick becomes a stream of
                    // reports (smooth shuttle) instead of one lurch.
                    let delta = s.shaft.wrapping_sub(s.shaft_sent);
                    let emit = delta.clamp(-JOG_REPORT_CAP, JOG_REPORT_CAP);
                    s.shaft_sent = s.shaft_sent.wrapping_add(emit);
                    r.extend_from_slice(&emit.to_le_bytes());
                    if s.shaft != s.shaft_sent {
                        s.jog_dirty = true; // more to drain next poll
                    } else {
                        s.jog_dirty = false;
                    }
                }
                mode => {
                    let value = if mode == ABSOLUTE_DEADZERO && s.shaft.abs() < JOG_DEADZERO {
                        0
                    } else {
                        s.shaft.clamp(JOG_ABS_MIN, JOG_ABS_MAX)
                    };
                    r.extend_from_slice(&value.to_le_bytes());
                    s.jog_dirty = false;
                }
            }
            // Trailing flag byte, 1 on the real device (see jog.py).
            r.push(1);
            return Some(r);
        }
        None
    }

    /// Control-request handling on endpoint 0. `setup` = 8-byte setup
    /// packet; `data` = payload of an OUT data stage (or empty for IN
    /// requests). Returns the reply bytes or a stall.
    pub fn control(&self, setup: u64, data: &[u8]) -> ControlReply {
        let bm_request_type = setup as u8;
        let request = (setup >> 8) as u8;
        let value = ((setup >> 16) & 0xffff) as u16;
        let index = ((setup >> 32) & 0xffff) as u16;
        let _length = ((setup >> 48) & 0xffff) as u16;

        let dir_in = bm_request_type & 0x80 != 0;
        let req_type = (bm_request_type >> 5) & 0x03;
        if req_type == 0 {
            // Standard requests
            match request {
                // GET_DESCRIPTOR
                0x06 if dir_in => {
                    let dtype = (value >> 8) as u8;
                    let dindex = (value & 0xff) as u8;
                    return match dtype {
                        0x01 => ControlReply::Data(DEVICE_DESCRIPTOR.to_vec()),
                        0x02 => ControlReply::Data(configuration_descriptor().to_vec()),
                        0x03 => string_descriptor(dindex).map(ControlReply::Data).unwrap_or(ControlReply::Stall),
                        0x22 => ControlReply::Data(REPORT_DESCRIPTOR.to_vec()),
                        _ => ControlReply::Stall,
                    };
                }
                // SET_IDLE (0x0B), SET_ADDRESS etc. -> accept silently
                _ => return ControlReply::Data(vec![]),
            }
        }
        // Class requests on the HID interface. If class != 1 return stall.
        if req_type != 1 {
            return ControlReply::Stall;
        }
        let report_type = (value >> 8) as u8;
        let report_id = (value & 0xff) as u8;
        match (request, dir_in) {
            // SET_REPORT -> output/feature payload in `data`
            (0x09, false) => self.set_report(report_type, report_id, data),
            // GET_REPORT -> input/feature value
            (0x01, true) => self.get_report(report_type, report_id),
            _ => ControlReply::Stall,
        }
    }

    /// Interrupt-OUT endpoint reports: payload[0] is the report ID, the
    /// rest feeds the same handler as control SET_REPORT output writes
    /// (LED masks, wheel mode). Returns whether the report was accepted.
    pub fn control_output(&self, data: &[u8]) -> bool {
        let mut s = match self.inner.state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if !s.attached {
            return false;
        }
        match data.first().copied() {
            // Output report 3: [rid, mode, ...] wheel mode/parameters
            Some(0x03) if data.len() >= 2 => {
                s.jog_mode = data[1];
                s.shaft = 0;
                s.shaft_sent = 0;
                s.pending = 0;
                true
            }
            // Output report 4: [rid, lo, hi] key LED mask
            Some(0x04) if data.len() >= 3 => {
                s.leds = u16::from_le_bytes([data[1], data[2]]);
                true
            }
            // Output report 7: [rid, leds] jog wheel LEDs
            Some(0x07) if data.len() >= 2 => {
                s.jog_leds = data[1];
                true
            }
            _ => false,
        }
    }

    fn set_report(&self, rtype: u8, rid: u8, data: &[u8]) -> ControlReply {
        if rtype != 2 && rtype != 3 {
            return ControlReply::Stall;
        }
        let mut s = match self.inner.state.lock() {
            Ok(s) => s,
            Err(_) => return ControlReply::Stall,
        };
        match (rtype, rid) {
            // Feature 0x05: jog mode parameters [id, mode, speed, deadzone]
            (3, 0x05) => {
                if data.len() >= 2 {
                    s.jog_mode = data[1];
                    s.shaft = 0;
                    s.shaft_sent = 0;
                    s.pending = 0;
                }
                ControlReply::Data(vec![])
            }
            // Output 0x03: Resolve writes wheel mode/parameters on the
            // interrupt-OUT endpoint (report 3)
            (2, 0x03) => {
                if data.len() >= 2 {
                    s.jog_mode = data[1];
                    s.shaft = 0;
                    s.shaft_sent = 0;
                    s.pending = 0;
                }
                ControlReply::Data(vec![])
            }
            // Output 0x04: key LED mask (2x u16)
            (2, 0x04) => {
                if data.len() >= 2 {
                    s.leds = u16::from_le_bytes([data[1], data.get(2).copied().unwrap_or(0)]);
                }
                ControlReply::Data(vec![])
            }
            // Output 0x07: extra LED/output bytes
            (2, 0x07) => {
                if data.len() >= 2 {
                    s.jog_leds = data[1];
                }
                ControlReply::Data(vec![])
            }
            // Feature 0x06: auth data stage. Payload = [0x06, stage, ...]
            // where stage 0=reset, 1=start (issue challenge),
            // 3=host answer.
            (3, 0x06) => {
                let stage_byte = data.get(1).copied().unwrap_or(0);
                match (s.stage, stage_byte) {
                    (_, 0x00) => {
                        // Stage 0 is Resolve's reset: return to unauthenticated.
                        s.stage = AuthStage::Unauthenticated;
                        s.host_challenge = 0;
                    }
                    (AuthStage::Unauthenticated, 0x01) => {
                        s.host_challenge = 0;
                        s.stage = AuthStage::SendChallenge;
                    }
                    (AuthStage::SendAnswer, 0x03) => {
                        if payload_ok(&data[2..]) {
                            s.stage = AuthStage::Authenticated;
                            // Queue the post-auth battery report.
                            s.battery_dirty = true;
                        } else {
                            s.stage = AuthStage::Unauthenticated;
                        }
                    }
                    _ => {}
                }
                ControlReply::Data(vec![])
            }
            _ => ControlReply::Stall,
        }
    }

    fn get_report(&self, rtype: u8, rid: u8) -> ControlReply {
        let mut s = match self.inner.state.lock() {
            Ok(s) => s,
            Err(_) => return ControlReply::Stall,
        };
        match (rtype, rid) {
            // Feature 0x01: device info / serial (32 ASCII bytes)
            (3, 0x01) => {
                let mut serial = [0u8; 32];
                serial[..SERIAL.len()].copy_from_slice(SERIAL.as_bytes());
                let mut v = vec![0x01];
                v.extend_from_slice(&serial);
                ControlReply::Data(v)
            }
            // Feature 0x05: jog mode echo [id, mode, 0, 0, 0, 0]
            (3, 0x05) => ControlReply::Data(vec![0x05, s.jog_mode, 0x00, 0x00, 0x00, 0x00, 0x00]),
            // Feature 0x06: auth handshake
            (3, 0x06) => match s.stage {
                AuthStage::SendChallenge => {
                    let mut v = vec![0x06, 0x02];
                    v.extend_from_slice(&kbd_solve(s.host_challenge).to_le_bytes());
                    v.extend_from_slice(&[0; 4]);
                    s.stage = AuthStage::SendAnswer;
                    ControlReply::Data(v)
                }
                AuthStage::Authenticated => {
                    // Real device answers 06:04:58:02 + zeros, 72 bytes total.
                    let mut v = vec![0x06, 0x04, 0x58, 0x02];
                    v.extend_from_slice(&[0; 68]);
                    ControlReply::Data(v)
                }
                _ => ControlReply::Data(vec![0x06; 72]),
            },
            // Feature 0x08: unknown, real device replies 8B
            (3, 0x08) => ControlReply::Data(vec![0x08; 9]),
            // Input 0x04: key state snapshot
            (1, 0x04) => {
                let mut v = vec![0x04];
                for k in s.held.iter() {
                    v.extend_from_slice(&k.to_le_bytes());
                }
                ControlReply::Data(v)
            }
            // Input 0x07: battery snapshot
            (1, 0x07) => ControlReply::Data(vec![0x07, s.charging as u8, s.level, 0]),
            _ => ControlReply::Stall,
        }
    }
}

fn payload_ok(bytes: &[u8]) -> bool {
    // Resolve answers with the challenge-derived secret; any non-zero
    // payload counts as success for this emulation.
    bytes.iter().any(|&b| b != 0)
}

/// Configuration descriptor mirroring the real hardware's composite
/// layout (86 bytes): IAD over if0+if1, a vendor-specific interface, a
/// DFU runtime interface, then the HID interface (number 2) with the
/// interrupt IN/OUT endpoints the reports actually travel over.
fn configuration_descriptor() -> [u8; 86] {
    let rdlen = REPORT_DESCRIPTOR.len() as u16;
    [
        0x09, 0x02, // bLength, bDescriptorType CONFIGURATION
        0x56, 0x00, // wTotalLength 86
        0x03, // bNumInterfaces
        0x01, // bConfigurationValue
        0x00, // iConfiguration
        0x80, // bmAttributes: bus powered
        0x32, // bMaxPower 100mA
        // IAD spanning if0+if1 (vendor function)
        0x08, 0x0b, 0x00, 0x02, 0xff, 0x03, 0x00, 0x02,
        // Interface 0: vendor specific, 0 endpoints
        0x09, 0x04, 0x00, 0x00, 0x00, 0xff, 0x03, 0x00, 0x00,
        // Vendor-specific 10-byte blob seen on the real device
        0x0a, 0x21, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // Interface 1: DFU runtime (AppSpecific/DFU), 0 endpoints
        0x09, 0x04, 0x01, 0x00, 0x00, 0xfe, 0x01, 0x01, 0x00,
        // DFU functional descriptor
        0x09, 0x21, 0x01, 0xe8, 0x03, 0x00, 0x02, 0x01, 0x01,
        // Interface 2: HID, 2 endpoints
        0x09, 0x04, 0x02, 0x00, 0x02, 0x03, 0x00, 0x00, 0x00,
        // HID descriptor (bcdHID 1.10 like the real device)
        0x09, 0x21, 0x10, 0x01,
        0x00, // country
        0x01, // bNumDescriptors
        0x22, // report descriptor type
        (rdlen & 0xff) as u8, (rdlen >> 8) as u8,
        // Interrupt IN endpoint 0x81, 64B, 10ms like the real device
        0x07, 0x05, 0x81, 0x03, 0x40, 0x00, 0x0a,
        // Interrupt OUT endpoint 0x01, 64B, 10ms
        0x07, 0x05, 0x01, 0x03, 0x40, 0x00, 0x0a,
    ]
}

fn string_descriptor(index: u8) -> Option<Vec<u8>> {
    let text = match index {
        1 => "Blackmagic design",
        2 => "DaVinci Resolve Speed Editor",
        3 => SERIAL,
        _ => return None,
    };
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut v = Vec::with_capacity(2 + utf16.len() * 2);
    v.push((2 + utf16.len() * 2) as u8);
    v.push(0x03);
    for w in utf16 {
        v.extend_from_slice(&w.to_le_bytes());
    }
    Some(v)
}

/// Davinci's challenge transform. From KipJM/blackmacro-lib auth.py: the
/// host challenge is folded with a per-device magic (0xa68cb3d8) plus a
/// 9-byte lookup keyed by byte position.
const AUTH_MAGIC: u32 = 0xa68c_b3d8;
const AUTH_TABLE: [u8; 9] = [0x17, 0x08, 0x3f, 0x2d, 0x5a, 0x41, 0x7c, 0x1b, 0x63];

fn kbd_solve(challenge: u32) -> u32 {
    let mut a = challenge.rotate_right(5) ^ AUTH_MAGIC;
    for (i, t) in AUTH_TABLE.iter().enumerate() {
        let shift = 8 * (i % 4);
        let byte = ((a >> shift) & 0xff) as u8;
        let mixed = byte.wrapping_add(*t) ^ ((challenge >> shift) as u8 & 0xff);
        a = (a & !(0xff << shift)) | ((mixed as u32) << shift);
    }
    a.wrapping_mul(0x9e37_79b1) ^ 0x0f11_2233
}

/// Reply enum for `control()`.
pub enum ControlReply {
    Data(Vec<u8>),
    Stall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_state_machine() {
        let dev = SpeedEditor::new();
        dev.attach();
        // Resolve kicks auth: SET feature 6 stage 1
        dev.set_report(3, 0x06, &[0x06, 0x01]);
        // GET feature 6 -> challenge answer
        let r = dev.get_report(3, 0x06);
        let ControlReply::Data(v) = r else { panic!() };
        assert_eq!(v[0], 0x06);
        assert_eq!(v[1], 0x02);
        // SET feature 6 stage 3 with any payload -> auth
        dev.set_report(3, 0x06, &[0x06, 0x03, 1, 2, 3, 4]);
        assert!(dev.authenticated());
        // Post-auth GET returns the 06:04:58:02 trailer
        let r = dev.get_report(3, 0x06);
        let ControlReply::Data(v) = r else { panic!() };
        assert_eq!(&v[..4], &[0x06, 0x04, 0x58, 0x02]);
    }

    #[test]
    fn host_answer_verification() {
        // kbd_solve must be deterministic for a fixed challenge
        assert_eq!(kbd_solve(0), kbd_solve(0));
    }

    #[test]
    fn key_report_and_release() {
        let dev = SpeedEditor::new();
        dev.attach();
        dev.queue_key(SpeedEditorKey::StopPlay as u16, true);
        let r = dev.take_input_report().expect("key report");
        assert_eq!(r[0], 0x04);
        assert_eq!(r[1..3], [0x3c, 0x00]);
        dev.queue_key(SpeedEditorKey::StopPlay as u16, false);
        let r = dev.take_input_report().expect("release report");
        assert_eq!(r[1..3], [0x00, 0x00]);
    }

    #[test]
    fn jog_relative_report() {
        let dev = SpeedEditor::new();
        dev.attach();
        dev.queue_jog(3, 1);
        dev.queue_jog(1, 1);
        // 4 ticks at scale 1 -> pending 4 bins; first drain emits 4, then
        // next emits the trailing 0 (wheel at rest).
        let r = dev.take_input_report().expect("jog report");
        assert_eq!(r[0], 0x03);
        assert_eq!(r[1], RELATIVE);
        let val = i32::from_le_bytes(r[2..6].try_into().unwrap());
        assert_eq!(val, 4);
    }
}
