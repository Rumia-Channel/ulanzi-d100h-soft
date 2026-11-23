//! usbip-win2 server: listens on the attach port, exports the emulated
//! Speed Editor as a virtual USB device and shuttles URBs between the
//! host VHCI driver and `SpeedEditor`.
//!
//! Wire format follows usbip-win2's reduced protocol: after the
//! OP_REQ_IMPORT handshake, each packet is a 48-byte header
//! (command, seqnum, devid, direction, ep) plus a variable payload.

use std::collections::HashMap;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::device::{ControlReply, PID, SpeedEditor, VID};

const USBIP_VERSION: u16 = 0x0111;

// op_code_t
const OP_REQ_DEVLIST: u16 = 0x8005;
const OP_REP_DEVLIST: u16 = 0x0005;
const OP_REQ_IMPORT: u16 = 0x8003;
const OP_REP_IMPORT: u16 = 0x0003;

// usbip commands
const CMD_SUBMIT: u32 = 1;
const CMD_UNLINK: u32 = 2;
const RET_SUBMIT: u32 = 3;
const ST_OK: u32 = 0;
const ST_STALL: u32 = 0x01;
const ST_NODEV: u32 = 0x04;
const ST_DEV_BUSY: u32 = 0x06;

const DIR_OUT: u32 = 0;
const DIR_IN: u32 = 1;

const DEV_PATH_MAX: usize = 256;
const BUS_ID_SIZE: usize = 32;
const BUS_ID: &[u8] = b"1-1";

enum Urb {
    Submit { seqnum: u32, direction: u32, ep: u32, setup: u64, payload: Vec<u8> },
    Unlink { seqnum: u32, target: u32 },
}

pub struct Server {
    device: SpeedEditor,
}

impl Server {
    pub fn new(device: SpeedEditor) -> Self {
        Self { device }
    }

    /// Listen on `port` and serve attaches until the task is cancelled.
    pub async fn run(self, port: u16) -> io::Result<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        loop {
            let (socket, _peer) = listener.accept().await?;
            let device = self.device.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_connection(socket, device).await {
                    eprintln!("usbip: connection ended: {e}");
                }
            });
        }
    }
}

// usbip device-info (312 bytes, big-endian scalars)
fn encode_device_info() -> Vec<u8> {
    let mut v = Vec::with_capacity(312);
    let mut path = [0u8; DEV_PATH_MAX];
    let path_str = b"/sys/devices/virtual/d100h-speed-editor";
    path[..path_str.len()].copy_from_slice(path_str);
    v.extend_from_slice(&path);
    let mut busid = [0u8; BUS_ID_SIZE];
    busid[..BUS_ID.len()].copy_from_slice(BUS_ID);
    v.extend_from_slice(&busid);
    v.extend_from_slice(&(1u32).to_be_bytes()); // busnum
    v.extend_from_slice(&(1u32).to_be_bytes()); // devnum
    v.extend_from_slice(&(2u32).to_be_bytes()); // speed: full
    v.extend_from_slice(&VID.to_be_bytes());
    v.extend_from_slice(&PID.to_be_bytes());
    v.extend_from_slice(&0x0102u16.to_be_bytes()); // bcdDevice
    v.push(0xef); // bDeviceClass
    v.push(0x02); // bDeviceSubClass
    v.push(0x01); // bDeviceProtocol
    v.push(1); // bConfigurationValue
    v.push(1); // bNumConfigurations
    v.push(3); // bNumInterfaces
    debug_assert_eq!(v.len(), 312);
    v
}

async fn serve_connection(socket: TcpStream, device: SpeedEditor) -> io::Result<()> {
    let mut socket = socket;
    socket.set_nodelay(true).ok();

    // Handshake phase: server
    // listing requests until an import succeeds.
    loop {
        let mut op = [0u8; 8];
        socket.read_exact(&mut op).await?;
        let version = u16::from_be_bytes([op[0], op[1]]);
        let code = u16::from_be_bytes([op[2], op[3]]);
        if version != USBIP_VERSION {
            send_op_reply(&mut socket, code, ST_DEV_BUSY).await?;
            continue;
        }
        match code {
            OP_REQ_DEVLIST => {
                // reply header + ndev + device + one interface
                let mut out = Vec::with_capacity(320);
                out.extend_from_slice(&USBIP_VERSION.to_be_bytes());
                out.extend_from_slice(&OP_REP_DEVLIST.to_be_bytes());
                out.extend_from_slice(&ST_OK.to_be_bytes());
                out.extend_from_slice(&(1u32).to_be_bytes()); // ndev
                out.extend_from_slice(&encode_device_info());
                socket.write_all(&out).await?;
            }
            OP_REQ_IMPORT => {
                let mut busid = [0u8; BUS_ID_SIZE];
                socket.read_exact(&mut busid).await?;
                let mut out = Vec::with_capacity(320);
                out.extend_from_slice(&USBIP_VERSION.to_be_bytes());
                out.extend_from_slice(&OP_REP_IMPORT.to_be_bytes());
                out.extend_from_slice(&ST_OK.to_be_bytes());
                out.extend_from_slice(&encode_device_info());
                socket.write_all(&out).await?;
                eprintln!("usbip: 1-1 imported");
                let result = urb_loop(socket, device.clone()).await;
                device.detach();
                return result;
            }
            other => {
                send_op_reply(&mut socket, other, ST_DEV_BUSY).await?;
            }
        }
    }
}

async fn send_op_reply(socket: &mut TcpStream, code: u16, status: u32) -> io::Result<()> {
    let rep = match code {
        0x8003 => OP_REP_IMPORT,
        0x8005 => OP_REP_DEVLIST,
        _ => code & 0x7fff,
    };
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    out.extend_from_slice(&rep.to_be_bytes());
    out.extend_from_slice(&status.to_be_bytes());
    socket.write_all(&out).await
}

// URB header: 48 bytes. Fields after seqnum are big-endian except the
// setup packet and the payload, which keep wire order.
fn read_urb_header(buf: &[u8; 48]) -> (u32, u32, u32, u32, u32, u64) {
    let command = u32::from_be_bytes(buf[0..4].try_into().unwrap());
    let seqnum = u32::from_be_bytes(buf[4..8].try_into().unwrap());
    // buf[8..24] is devid/busid/secnum padding - ignored
    let direction = u32::from_be_bytes(buf[24..28].try_into().unwrap());
    let ep = u32::from_be_bytes(buf[28..32].try_into().unwrap());
    let transfer_buffer_length = u32::from_be_bytes(buf[36..40].try_into().unwrap());
    let setup = u64::from_le_bytes(buf[40..48].try_into().unwrap());
    (command, seqnum, direction, ep, transfer_buffer_length, setup)
}

fn urb_reply_header(seqnum: u32) -> [u8; 48] {
    let mut h = [0u8; 48];
    h[0..4].copy_from_slice(&RET_SUBMIT.to_be_bytes());
    h[4..8].copy_from_slice(&seqnum.to_be_bytes());
    h
}

async fn write_ret_submit(
    write: &mut tokio::net::tcp::OwnedWriteHalf,
    seqnum: u32,
    status: u32,
    payload: &[u8],
) -> io::Result<()> {
    let mut out = Vec::with_capacity(48 + payload.len());
    out.extend_from_slice(&urb_reply_header(seqnum));
    out[24..28].copy_from_slice(&DIR_IN.to_be_bytes());
    out[28..32].copy_from_slice(&0u32.to_be_bytes()); // ep
    out[32..36].copy_from_slice(&status.to_be_bytes());
    out[36..40].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    write.write_all(&out).await
}

/// Serve one imported device connection: read URBs, answer control
/// transfers synchronously, park interrupt-IN URBs until the device has
/// input to report.
async fn urb_loop(socket: TcpStream, device: SpeedEditor) -> io::Result<()> {
    let (mut read, mut write) = socket.into_split();
    let (urb_tx, mut urb_rx) = mpsc::unbounded_channel::<Urb>();

    let reader = tokio::spawn(async move {
        loop {
            let mut header = [0u8; 48];
            if read.read_exact(&mut header).await.is_err() {
                return;
            }
            let (command, seqnum, direction, ep, len, setup) = read_urb_header(&header);
            match command {
                CMD_SUBMIT => {
                    let mut payload = vec![0u8; len as usize];
                    if direction == DIR_OUT && len > 0 && read.read_exact(&mut payload).await.is_err() {
                        return;
                    }
                    if urb_tx.send(Urb::Submit { seqnum, direction, ep, setup, payload }).is_err() {
                        return;
                    }
                }
                CMD_UNLINK => {
                    let mut target = [0u8; 4];
                    if read.read_exact(&mut target).await.is_err() {
                        return;
                    }
                    let t = u32::from_be_bytes(target);
                    if urb_tx.send(Urb::Unlink { seqnum, target: t }).is_err() {
                        return;
                    }
                }
                _ => return,
            }
        }
    });

    // Parked interrupt-IN URBs: seqnum -> true
    let mut pending_in: HashMap<u32, bool> = HashMap::new();
    // Queue of parked IN seqnums in arrival order
    let mut in_queue: Vec<u32> = Vec::new();

    while let Some(urb) = urb_rx.recv().await {
        match urb {
            Urb::Submit { seqnum, direction, ep, setup, payload } => {
                if direction == DIR_IN && ep != 0 {
                    // Interrupt IN: park until input arrives.
                    pending_in.insert(seqnum, true);
                    in_queue.push(seqnum);
                } else if direction == DIR_OUT && ep == 0 {
                    // Control OUT with data stage already collected.
                    match device.control(setup, &payload) {
                        ControlReply::Data(d) => write_ret_submit(&mut write, seqnum, 0, &d).await?,
                        ControlReply::Stall => write_ret_submit(&mut write, seqnum, ST_STALL, &[]).await?,
                    }
                } else if direction == DIR_OUT && ep != 0 {
                    // Interrupt OUT payload = raw report bytes
                    if payload.len() >= 1 {
                        device.control_output(&payload);
                    }
                    write_ret_submit(&mut write, seqnum, 0, &[]).await?;
                } else {
                    // Control IN (ep 0, dir in)
                    match device.control(setup, &[]) {
                        ControlReply::Data(d) => write_ret_submit(&mut write, seqnum, 0, &d).await?,
                        ControlReply::Stall => write_ret_submit(&mut write, seqnum, ST_STALL, &[]).await?,
                    }
                }
            }
            Urb::Unlink { seqnum, target } => {
                if pending_in.remove(&target).is_some() {
                    in_queue.retain(|&s| s != target);
                    write_ret_submit(&mut write, seqnum, 0, &[]).await?;
                } else {
                    write_ret_submit(&mut write, seqnum, ST_NODEV, &[]).await?;
                }
            }
        }

        // Flush pending input into parked interrupt-IN URBs, oldest first
        loop {
            if in_queue.is_empty() {
                break;
            }
            let Some(report) = device.take_input_report() else {
                break;
            };
            let seq = in_queue.remove(0);
            pending_in.remove(&seq);
            write_ret_submit(&mut write, seq, 0, &report).await?;
        }

        // If URBs are parked but no input yet, wait briefly for more
        // without blocking the read side (reader keeps running).
        if !in_queue.is_empty() {
            tokio::select! {
                _ = device.wait_input() => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
            loop {
                if in_queue.is_empty() {
                    break;
                }
                let Some(report) = device.take_input_report() else {
                    break;
                };
                let seq = in_queue.remove(0);
                pending_in.remove(&seq);
                write_ret_submit(&mut write, seq, 0, &report).await?;
            }
        }
    }

    let _ = reader.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urb_header_roundtrip() {
        let mut buf = [0u8; 48];
        buf[0..4].copy_from_slice(&CMD_SUBMIT.to_be_bytes());
        buf[4..8].copy_from_slice(&7u32.to_be_bytes());
        buf[24..28].copy_from_slice(&DIR_IN.to_be_bytes());
        buf[28..32].copy_from_slice(&1u32.to_be_bytes());
        buf[36..40].copy_from_slice(&64u32.to_be_bytes());
        let (cmd, seq, dir, ep, len, _setup) = read_urb_header(&buf);
        assert_eq!(cmd, CMD_SUBMIT);
        assert_eq!(seq, 7);
        assert_eq!(dir, DIR_IN);
        assert_eq!(ep, 1);
        assert_eq!(len, 64);
    }
}
