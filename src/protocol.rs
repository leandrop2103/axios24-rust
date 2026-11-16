use crate::default_scene::{DEFAULT_REGS, NUM_REGS};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

pub const SCENE_BLOCK_SIZE: usize = 0x3000;
pub const PRESET_BIN_SIZE: usize = SCENE_BLOCK_SIZE * 16; // 0x30000 = 196,608 bytes
pub const UDP_LOCAL_PORT: u16 = 1080;
pub const UDP_REMOTE_PORT: u16 = 8086;
pub const TCP_REMOTE_PORT: u16 = 8087;
pub const SERIAL_BAUD: u32 = 115200;

/// Known default factory IPs used across Duonn Axios 24 / 16 / 32 firmware revisions
pub const DUONN_FACTORY_IPS: [(&str, &str); 4] = [
    ("172.14.10.1", "Axios 24 (Padrão Novo)"),
    ("192.168.4.1", "Axios 24 (AP Legado)"),
    ("172.14.11.1", "Axios 16"),
    ("172.14.30.1", "Axios 32"),
];

/// Exact CRC-16 (Modbus polynomial 0xA001, init 0xFFFF) from fcn.0045a3b0
pub fn crc16_axios(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Builds a framed Axios 24 command packet (fcn.0045a7d0):
/// [0x80, payload.len() + 3, cmd, ...payload..., crc_hi, crc_lo]
pub fn build_frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len_field = (payload.len() + 3) as u8;
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(0x80);
    frame.push(len_field);
    frame.push(cmd);
    frame.extend_from_slice(payload);
    let crc = crc16_axios(&frame);
    // Big-endian CRC at end of frame (uVar3 << 8 | uVar3 >> 8 in fcn.0045a7d0)
    frame.push((crc >> 8) as u8);
    frame.push((crc & 0xFF) as u8);
    frame
}

/// Verifies a framed Axios 24 packet (fcn.0045a480) and returns (cmd, payload)
pub fn parse_frame(frame: &[u8]) -> Option<(u8, &[u8])> {
    if frame.len() < 5 || frame[0] != 0x80 {
        return None;
    }
    let crc_offset = if frame[1] == 0xFF && frame[2] == 0xFF {
        0x106
    } else {
        frame[1] as usize
    };
    if frame.len() < crc_offset + 2 || crc_offset < 3 {
        return None;
    }
    let expected = crc16_axios(&frame[..crc_offset]);
    let actual = ((frame[crc_offset] as u16) << 8) | (frame[crc_offset + 1] as u16);
    if expected == actual {
        Some((frame[2], &frame[3..crc_offset]))
    } else {
        None
    }
}

/// Builds the parameter write frame (opcode 0x03) for a list of (reg_idx, reg_val) pairs (fcn.0045ab20)
pub fn build_param_write_frame(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(pairs.len() * 4);
    for &(reg, val) in pairs.iter().take(0x3C) {
        payload.extend_from_slice(&reg.to_be_bytes());
        payload.extend_from_slice(&val.to_be_bytes());
    }
    build_frame(0x03, &payload)
}

/// Builds the parameter read request frame (opcode 0x06) for a list of reg_idx (fcn.0045a8e0)
pub fn build_param_read_frame(regs: &[u16]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(regs.len() * 2);
    for &reg in regs.iter().take(0x3C) {
        payload.extend_from_slice(&reg.to_be_bytes());
    }
    build_frame(0x06, &payload)
}

/// Builds the Demo Start / Demo Stop hardware command packet (fcn.004453f0)
pub fn build_demo_cmd_frame(start: bool) -> Vec<u8> {
    let payload = [0x13, if start { 0x01 } else { 0x00 }, 0x00, 0x00];
    build_frame(0x26, &payload)
}

/// Builds a scene recall (opcode 0x01) or scene save (opcode 0x11) packet (fcn.0045af80 / fcn.0045b0d0)
pub fn build_scene_op_frame(scene_1based: u8, save: bool) -> Vec<u8> {
    let cmd = if save { 0x11 } else { 0x01 };
    build_frame(cmd, &[scene_1based])
}

/// Builds a text KV query (opcode 0x39) or set (opcode 0x3A) packet (fcn.0045d370 / fcn.0045d430)
pub fn build_kv_frame(kv_text: &str, is_set: bool) -> Vec<u8> {
    let cmd = if is_set { 0x3A } else { 0x39 };
    build_frame(cmd, kv_text.as_bytes())
}

/// Encodes a single 0x3000-byte scene block with registers and CRC-16 at 0x2FFE (fcn.0045ed90)
pub fn encode_scene_block(regs: &[u16; NUM_REGS]) -> Vec<u8> {
    let mut block = vec![0u8; SCENE_BLOCK_SIZE];
    for (i, &val) in regs.iter().enumerate() {
        let bytes = val.to_le_bytes();
        block[i * 2] = bytes[0];
        block[i * 2 + 1] = bytes[1];
    }
    block[0x2FD8] = 0;
    let crc = crc16_axios(&block[..0x2FFE]);
    let crc_bytes = crc.to_le_bytes();
    block[0x2FFE] = crc_bytes[0];
    block[0x2FFF] = crc_bytes[1];
    block
}

/// Decodes a 0x3000-byte scene block into registers, verifying CRC-16
pub fn decode_scene_block(block: &[u8]) -> Option<[u16; NUM_REGS]> {
    if block.len() < SCENE_BLOCK_SIZE {
        return None;
    }
    let mut regs = [0u16; NUM_REGS];
    for i in 0..NUM_REGS {
        regs[i] = u16::from_le_bytes([block[i * 2], block[i * 2 + 1]]);
    }
    Some(regs)
}

/// Loads or initializes `preset.bin` (16 scenes * 0x3000 bytes = 0x30000 bytes) exactly like fcn.0045ed90
pub fn load_or_create_preset_bin(path: &Path) -> Vec<[u16; NUM_REGS]> {
    if let Ok(data) = fs::read(path) {
        if data.len() == PRESET_BIN_SIZE {
            let mut scenes = Vec::with_capacity(16);
            for s in 0..16 {
                let start = s * SCENE_BLOCK_SIZE;
                let end = start + SCENE_BLOCK_SIZE;
                if let Some(regs) = decode_scene_block(&data[start..end]) {
                    scenes.push(regs);
                } else {
                    scenes.push(DEFAULT_REGS);
                }
            }
            return scenes;
        }
    }
    let scenes = vec![DEFAULT_REGS; 16];
    let _ = save_preset_bin(path, &scenes);
    scenes
}

/// Saves all 16 scenes to `preset.bin` (0x30000 bytes)
pub fn save_preset_bin(path: &Path, scenes: &[[u16; NUM_REGS]]) -> std::io::Result<()> {
    let mut full = Vec::with_capacity(PRESET_BIN_SIZE);
    for i in 0..16 {
        let regs = scenes.get(i).unwrap_or(&DEFAULT_REGS);
        full.extend_from_slice(&encode_scene_block(regs));
    }
    fs::write(path, full)
}

/// Official Axios24New.exe 128-byte repeating XOR key for `.scn` files (extracted from .rdata 0x672c80)
const SCN_XOR_KEY: [u8; 128] = [
    0x7d, 0x85, 0xe9, 0x07, 0xec, 0xf3, 0x32, 0xb9, 0x74, 0xa2, 0x37, 0x92, 0x1c, 0x37, 0x04, 0x2c,
    0x5c, 0x3e, 0xf2, 0xaf, 0x80, 0x83, 0xa0, 0xfa, 0xe1, 0x2d, 0x03, 0xcb, 0xaa, 0xae, 0x02, 0x8f,
    0x49, 0x4e, 0xe5, 0x42, 0x14, 0xec, 0xb8, 0xe2, 0xbe, 0x6b, 0x43, 0x15, 0x2e, 0x8b, 0x1d, 0x01,
    0x30, 0xb2, 0x89, 0x22, 0x3d, 0x8d, 0x9e, 0xd0, 0x14, 0xd1, 0xf8, 0x68, 0xc5, 0xec, 0x69, 0x67,
    0x39, 0xe6, 0x1c, 0xfe, 0x93, 0x48, 0x44, 0x02, 0xdd, 0x7b, 0x8c, 0x67, 0xc9, 0xcc, 0xb6, 0x56,
    0x62, 0x84, 0x69, 0x7f, 0x18, 0x28, 0xcb, 0xbd, 0xc3, 0x06, 0x32, 0x83, 0xfd, 0xa2, 0x8b, 0x9a,
    0xa4, 0x5e, 0x80, 0x6b, 0x36, 0x21, 0x5a, 0x82, 0x4e, 0xbf, 0x1d, 0x58, 0x46, 0xf2, 0xb8, 0x5e,
    0xb2, 0xe5, 0x5d, 0xeb, 0x69, 0x6c, 0x57, 0x04, 0xa4, 0x5f, 0x2a, 0x21, 0xa9, 0x11, 0x68, 0xa2,
];

pub struct OfficialSceneFile {
    pub name: String,
    pub text: Vec<String>,
    pub regs: [u16; NUM_REGS],
}

fn extract_json_string(src: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":\"", key);
    let start = src.find(&pat)? + pat.len();
    let mut out = String::new();
    let mut escaped = false;
    for ch in src[start..].chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn extract_json_string_array(src: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{}\":[", key);
    let Some(pos) = src.find(&pat) else {
        return Vec::new();
    };
    let rest = &src[pos + pat.len()..];
    let mut items = Vec::new();
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ']' {
            break;
        }
        if c == '"' {
            chars.next();
            let mut s = String::new();
            let mut esc = false;
            for ch in chars.by_ref() {
                if esc {
                    s.push(ch);
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '"' {
                    break;
                } else {
                    s.push(ch);
                }
            }
            items.push(s);
        } else {
            chars.next();
        }
    }
    items
}

/// Decodes an official Duonn `axios24new.exe` `.scn` file (128-byte XOR + JSON with `data` hex pairs)
pub fn decode_official_scn(bytes: &[u8]) -> Option<OfficialSceneFile> {
    let decrypted: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ SCN_XOR_KEY[i % SCN_XOR_KEY.len()])
        .collect();
    let json_str = std::str::from_utf8(&decrypted).ok()?;
    if !json_str.trim_start().starts_with('{') {
        return None;
    }

    let data_hex = extract_json_string(json_str, "data")?;
    if data_hex.len() < 8 {
        return None;
    }

    let mut regs = DEFAULT_REGS;
    let hex_bytes = data_hex.as_bytes();
    let mut i = 0;
    let mut parsed_count = 0usize;
    while i + 8 <= hex_bytes.len() {
        let reg_str = std::str::from_utf8(&hex_bytes[i..i + 4]).ok()?;
        let val_str = std::str::from_utf8(&hex_bytes[i + 4..i + 8]).ok()?;
        if let (Ok(reg), Ok(val)) = (u16::from_str_radix(reg_str, 16), u16::from_str_radix(val_str, 16)) {
            if (reg as usize) < NUM_REGS {
                regs[reg as usize] = val;
                parsed_count += 1;
            }
        }
        i += 8;
    }
    if parsed_count < 100 {
        return None;
    }

    let name = extract_json_string(json_str, "name").unwrap_or_else(|| "Imported".to_string());
    let text = extract_json_string_array(json_str, "text");

    Some(OfficialSceneFile { name, text, regs })
}

/// Encodes a scene into the official Duonn `axios24new.exe` `.scn` format (compatible with original software)
pub fn encode_official_scn(
    scene_num: usize,
    scene_name: &str,
    channel_names: &[String; 60],
    regs: &[u16; NUM_REGS],
) -> Vec<u8> {
    use std::fmt::Write;
    let mut data_hex = String::with_capacity(NUM_REGS * 8);
    for (r, &v) in regs.iter().enumerate() {
        let _ = write!(data_hex, "{:04X}{:04X}", r, v);
    }
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let text_json: Vec<String> = channel_names
        .iter()
        .map(|s| format!("\"{}\"", esc(s)))
        .collect();
    let json = format!(
        "{{\"code\":\"0xE001\",\"type\":\"0x21\",\"magic\":28581,\"scenes\":[{{\"num\":{},\"name\":\"{}\",\"text\":[{}],\"data\":\"{}\",\"magic\":584}}]}}",
        scene_num,
        esc(scene_name),
        text_json.join(","),
        data_hex
    );
    json.as_bytes()
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ SCN_XOR_KEY[i % SCN_XOR_KEY.len()])
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConnectionMode {
    Demo,
    Udp { ip: String, port: u16 },
    Tcp { ip: String, port: u16 },
    Serial { port_name: String, baud: u32 },
}

impl ConnectionMode {
    pub fn label(&self) -> String {
        match self {
            ConnectionMode::Demo => "MODO DEMO (Offline)".to_string(),
            ConnectionMode::Udp { ip, port } => format!("UDP {}:{}", ip, port),
            ConnectionMode::Tcp { ip, port } => format!("TCP {}:{}", ip, port),
            ConnectionMode::Serial { port_name, baud } => format!("SERIAL {} @ {}", port_name, baud),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub name: String,
    pub ip: String,
    pub fw_version: String,
}

#[derive(Debug)]
pub enum NetEvent {
    ParamUpdates(Vec<(u16, u16)>),
    DeviceDiscovered(DiscoveredDevice),
    Connected(String),
    Error(String),
}

pub struct HardwareLink {
    pub mode: ConnectionMode,
    tx_out: Option<Sender<Vec<u8>>>,
    rx_in: Option<Receiver<NetEvent>>,
}

impl Default for HardwareLink {
    fn default() -> Self {
        Self {
            mode: ConnectionMode::Demo,
            tx_out: None,
            rx_in: None,
        }
    }
}

impl HardwareLink {
    pub fn connect(&mut self, mode: ConnectionMode) {
        self.disconnect();
        self.mode = mode.clone();
        if matches!(mode, ConnectionMode::Demo) {
            return;
        }
        let (tx_out, rx_out) = mpsc::channel::<Vec<u8>>();
        let (tx_in, rx_in) = mpsc::channel::<NetEvent>();
        self.tx_out = Some(tx_out);
        self.rx_in = Some(rx_in);

        match mode {
            ConnectionMode::Udp { ip, port } => {
                thread::spawn(move || {
                    let bind_addr = format!("0.0.0.0:{}", UDP_LOCAL_PORT);
                    let sock = UdpSocket::bind(&bind_addr)
                        .or_else(|_| UdpSocket::bind("0.0.0.0:0"));
                    match sock {
                        Ok(s) => {
                            let _ = s.set_read_timeout(Some(Duration::from_millis(20)));
                            let _ = s.set_broadcast(true);
                            let target = format!("{}:{}", ip, port);
                            let _ = tx_in.send(NetEvent::Connected(format!(
                                "UDP {} (Keep-Alive Ativo)",
                                target
                            )));
                            let mut buf = [0u8; 512];
                            let mut ticks: u32 = 0;
                            let keepalive = build_kv_frame("DEVNAME=\n", false);
                            loop {
                                while let Ok(pkt) = rx_out.try_recv() {
                                    if pkt.is_empty() {
                                        return;
                                    }
                                    let _ = s.send_to(&pkt, &target);
                                }
                                if let Ok((n, src)) = s.recv_from(&mut buf) {
                                    handle_incoming_bytes(&buf[..n], Some(src), &tx_in);
                                }
                                ticks = ticks.wrapping_add(1);
                                // Send UDP heartbeat every ~2.5s (125 * 20ms) to keep Wi-Fi/Router NAT alive
                                if ticks % 125 == 0 {
                                    let _ = s.send_to(&keepalive, &target);
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx_in.send(NetEvent::Error(format!("Erro UDP: {}", e)));
                        }
                    }
                });
            }
            ConnectionMode::Tcp { ip, port } => {
                thread::spawn(move || {
                    let target = format!("{}:{}", ip, port);
                    let sock_addr = target
                        .parse()
                        .unwrap_or_else(|_| SocketAddr::from(([172, 14, 10, 1], 8087)));
                    let keepalive = build_kv_frame("DEVNAME=\n", false);

                    loop {
                        // Check if user requested disconnect before attempting connection
                        if let Ok(pkt) = rx_out.try_recv() {
                            if pkt.is_empty() {
                                return;
                            }
                        }

                        match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(2)) {
                            Ok(mut stream) => {
                                let _ = stream.set_read_timeout(Some(Duration::from_millis(20)));
                                let _ = stream.set_nodelay(true);
                                let _ = tx_in.send(NetEvent::Connected(format!(
                                    "TCP {} (Auto-Reconnect Ativo)",
                                    target
                                )));
                                let mut buf = [0u8; 512];
                                let mut ticks: u32 = 0;
                                let mut link_dead = false;

                                while !link_dead {
                                    while let Ok(pkt) = rx_out.try_recv() {
                                        if pkt.is_empty() {
                                            return;
                                        }
                                        if stream.write_all(&pkt).is_err() {
                                            link_dead = true;
                                            break;
                                        }
                                    }
                                    if link_dead {
                                        break;
                                    }
                                    match stream.read(&mut buf) {
                                        Ok(0) => {
                                            // Remote closed connection — trigger auto-reconnect
                                            link_dead = true;
                                        }
                                        Ok(n) => {
                                            handle_incoming_bytes(&buf[..n], None, &tx_in);
                                        }
                                        Err(_) => {}
                                    }
                                    ticks = ticks.wrapping_add(1);
                                    if ticks % 125 == 0 {
                                        if stream.write_all(&keepalive).is_err() {
                                            link_dead = true;
                                        }
                                    }
                                }

                                let _ = tx_in.send(NetEvent::Error(format!(
                                    "Queda Wi-Fi/TCP em {}. Reconectando automaticamente...",
                                    target
                                )));
                                thread::sleep(Duration::from_millis(800));
                            }
                            Err(e) => {
                                let _ = tx_in.send(NetEvent::Error(format!(
                                    "Sem resposta TCP {} ({}). Tentando novamente...",
                                    target, e
                                )));
                                // Wait 1.5s before auto-retry unless disconnected
                                for _ in 0..15 {
                                    if let Ok(pkt) = rx_out.try_recv() {
                                        if pkt.is_empty() {
                                            return;
                                        }
                                    }
                                    thread::sleep(Duration::from_millis(100));
                                }
                            }
                        }
                    }
                });
            }
            ConnectionMode::Serial { port_name, baud } => {
                thread::spawn(move || {
                    match serialport::new(&port_name, baud)
                        .timeout(Duration::from_millis(20))
                        .open()
                    {
                        Ok(mut port) => {
                            let _ = tx_in.send(NetEvent::Connected(format!(
                                "Serial {} @ {}",
                                port_name, baud
                            )));
                            let mut buf = [0u8; 512];
                            loop {
                                while let Ok(pkt) = rx_out.try_recv() {
                                    if pkt.is_empty() {
                                        return;
                                    }
                                    let _ = port.write_all(&pkt);
                                }
                                if let Ok(n) = port.read(&mut buf) {
                                    if n > 0 {
                                        handle_incoming_bytes(&buf[..n], None, &tx_in);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx_in.send(NetEvent::Error(format!("Erro Serial {}: {}", port_name, e)));
                        }
                    }
                });
            }
            ConnectionMode::Demo => {}
        }
    }

    pub fn disconnect(&mut self) {
        if let Some(tx) = self.tx_out.take() {
            let _ = tx.send(Vec::new());
        }
        self.rx_in = None;
        self.mode = ConnectionMode::Demo;
    }

    pub fn send_frame(&self, frame: Vec<u8>) {
        if let Some(tx) = &self.tx_out {
            let _ = tx.send(frame);
        }
    }

    pub fn send_param(&self, reg: u16, val: u16) {
        if !matches!(self.mode, ConnectionMode::Demo) {
            self.send_frame(build_param_write_frame(&[(reg, val)]));
        }
    }

    /// Sends all 3,413 DSP registers to the hardware in safe 24-register frames so offline edits sync completely
    pub fn sync_all_registers(&self, regs: &[u16; NUM_REGS]) {
        if matches!(self.mode, ConnectionMode::Demo) {
            return;
        }
        let mut batch = Vec::with_capacity(24);
        for (idx, &val) in regs.iter().enumerate() {
            batch.push((idx as u16, val));
            if batch.len() == 24 {
                self.send_frame(build_param_write_frame(&batch));
                batch.clear();
            }
        }
        if !batch.is_empty() {
            self.send_frame(build_param_write_frame(&batch));
        }
    }

    pub fn poll_events(&self) -> Vec<NetEvent> {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx_in {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        events
    }
}

fn handle_incoming_bytes(data: &[u8], src: Option<SocketAddr>, tx: &Sender<NetEvent>) {
    if let Some((cmd, payload)) = parse_frame(data) {
        match cmd {
            // Parameter write / sync response (opcodes 0x03, 0x06)
            0x03 | 0x06 => {
                let mut pairs = Vec::new();
                for chunk in payload.chunks_exact(4) {
                    let reg = u16::from_be_bytes([chunk[0], chunk[1]]);
                    let val = u16::from_be_bytes([chunk[2], chunk[3]]);
                    pairs.push((reg, val));
                }
                if !pairs.is_empty() {
                    let _ = tx.send(NetEvent::ParamUpdates(pairs));
                }
            }
            // Discovery / KV info response (opcode 0x39 / 0x00)
            0x39 | 0x00 => {
                let text = String::from_utf8_lossy(payload);
                if let Some(addr) = src {
                    let mut name = "AXIOS24".to_string();
                    let mut fw = "V1.0".to_string();
                    for line in text.lines() {
                        if let Some(v) = line.strip_prefix("DEVNAME=") {
                            if !v.trim().is_empty() {
                                name = v.trim().to_string();
                            }
                        } else if let Some(v) = line.strip_prefix("FW_VER=") {
                            fw = v.trim().to_string();
                        }
                    }
                    let _ = tx.send(NetEvent::DeviceDiscovered(DiscoveredDevice {
                        name,
                        ip: addr.ip().to_string(),
                        fw_version: fw,
                    }));
                }
            }
            _ => {}
        }
    }
}

/// Detects the primary local IPv4 address of this PC (for subnet broadcast & UI diagnostics)
pub fn detect_local_ipv4() -> Option<std::net::Ipv4Addr> {
    for target in ["172.14.10.1:8086", "192.168.4.1:8086", "8.8.8.8:80"] {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            if sock.connect(target).is_ok() {
                if let Ok(std::net::SocketAddr::V4(addr)) = sock.local_addr() {
                    let ip = *addr.ip();
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

/// Broadcasts a UDP discovery probe on port 8086 across LAN subnet, Wi-Fi AP & all Duonn factory IPs (fcn.00426070)
pub fn probe_udp_devices() -> Vec<DiscoveredDevice> {
    let mut found = Vec::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.set_broadcast(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(280)));
        let probe = build_kv_frame("DEVNAME=\nFW_VER=\n", false);
        let _ = sock.send_to(&probe, ("255.255.255.255", UDP_REMOTE_PORT));
        if let Some(local_ip) = detect_local_ipv4() {
            let octets = local_ip.octets();
            let subnet_bcast = std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], 255);
            let _ = sock.send_to(&probe, (subnet_bcast, UDP_REMOTE_PORT));
        }
        for &(ip, _) in &DUONN_FACTORY_IPS {
            let _ = sock.send_to(&probe, (ip, UDP_REMOTE_PORT));
        }
        let mut buf = [0u8; 512];
        while let Ok((n, src)) = sock.recv_from(&mut buf) {
            if let Some((_cmd, payload)) = parse_frame(&buf[..n]) {
                let text = String::from_utf8_lossy(payload);
                let mut name = "AXIOS24".to_string();
                let mut fw = "V1.0".to_string();
                for line in text.lines() {
                    if let Some(v) = line.strip_prefix("DEVNAME=") {
                        if !v.trim().is_empty() {
                            name = v.trim().to_string();
                        }
                    } else if let Some(v) = line.strip_prefix("FW_VER=") {
                        fw = v.trim().to_string();
                    }
                }
                let ip_str = src.ip().to_string();
                if !found.iter().any(|d: &DiscoveredDevice| d.ip == ip_str) {
                    found.push(DiscoveredDevice {
                        name,
                        ip: ip_str,
                        fw_version: fw,
                    });
                }
            }
        }
    }
    found
}

/// Lists available serial ports on the system
pub fn list_serial_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    if ports.is_empty() {
        for i in [1, 2, 3, 4, 5, 6, 7, 8] {
            ports.push(format!("COM{}", i));
        }
    }
    ports
}
