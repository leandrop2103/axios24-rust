#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../default_scene.rs"]
mod default_scene;
#[path = "../protocol.rs"]
#[allow(dead_code)]
mod protocol;

use default_scene::DEFAULT_REGS;
use eframe::egui;
use protocol::{
    build_frame, build_kv_frame, decode_official_scn, detect_local_ipv4, parse_frame,
    FirmwareLayout, AX24E_NUM_REGS, TCP_REMOTE_PORT, UDP_REMOTE_PORT,
};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const EMU_DEV_NAME: &str = "AX24E-EMU";
const EMU_FW_VER: &str = "AX24E.25.11.01.001";
const EMU_SCENES_FILE: &str = "emulator_scenes.bin";

/// Exact register mapping function from fcn.00455d70 (matches src/model.rs & AX24E.25.11.01.001.exe)
fn map_reg(group: u8, ch: usize, param: u16) -> u16 {
    let ch = ch as u16;
    if param == 0xEC {
        return match group {
            1 => 0x57 + 0x3E * ch,
            2 => 0xB3B + 0x2D * ch,
            3 => 0x698 + 0x6D * ch,
            4 => 0xBAA + 9 * ch,
            5 => 0xAEB + 5 * ch + param,
            6 => 0xAFB + 3 * ch + param,
            _ => 0xC0F,
        };
    } else if param == 0xEB {
        return match group {
            1 => 0x4A + 0x3E * ch,
            2 => 0xB30 + 0x2D * ch,
            3 => 0x68E + 0x6D * ch,
            4 => 0xBA9 + 9 * ch,
            5 => 0xAEB + 5 * ch + param,
            6 => 0xAFB + 3 * ch + param,
            _ => 0xC0F,
        };
    } else if param == 0xEA {
        return match group {
            1 => 0x4C + 0x3E * ch,
            2 => 0xB2F + 0x2D * ch,
            3 => 0x68C + 0x6D * ch,
            4 => 0xBA8 + 9 * ch,
            5 => 0xAEB + 5 * ch + param,
            6 => 0xAFB + 3 * ch + param,
            _ => 0xC0F,
        };
    }
    match group {
        1 => ch * 0x3E + param + 0x40,
        2 => {
            if param == 0xD8 {
                ch + 0xBF5
            } else if (0xAB..0xD8).contains(&param) {
                ch * 0x2D + param + 0xA84
            } else {
                0xC0F
            }
        }
        3 => ch * 0x6D + param + 0x64E,
        4 => ch * 9 + param + 0xACE,
        5 => ch * 5 + param + 0xAEB,
        6 => ch * 3 + param + 0xAFB,
        _ => 0xC0F,
    }
}

fn decode_fader_db(raw: u16) -> f32 {
    let mag = (raw & 0x3FFF) as i32;
    if mag == 0 {
        -120.0
    } else {
        (((mag - 0x4B0) as f32) / 10.0).clamp(-120.0, 10.0)
    }
}

fn encode_fader_db(db: f32) -> u16 {
    if db <= -119.5 {
        0
    } else {
        (((db.clamp(-120.0, 10.0) * 10.0).round() as i32) + 0x4B0).max(0) as u16
    }
}

fn format_db(db: f32) -> String {
    if db <= -115.0 {
        "-∞".to_string()
    } else {
        format!("{:+.1}dB", db)
    }
}

fn db_to_lin(db: f32) -> f32 {
    if db <= -115.0 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

fn lin_to_db(lin: f32) -> f32 {
    if lin <= 1e-6 {
        -90.0
    } else {
        (20.0 * lin.log10()).clamp(-90.0, 6.0)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PersistedScenes {
    active_scene: u8,
    scenes: Vec<Vec<u16>>,
    scene_names: Vec<String>,
    scene_ch_names: Vec<Vec<String>>,
    live_ch_names: Vec<String>,
}

#[derive(Clone)]
struct PacketLogEntry {
    ts: String,
    proto: String,
    #[allow(dead_code)]
    peer: String,
    cmd: u8,
    summary: String,
    hex: String,
}

struct EmulatorCore {
    regs: Vec<u16>,
    scenes: Vec<Vec<u16>>,
    scene_names: Vec<String>,
    scene_ch_names: Vec<Vec<String>>,
    live_ch_names: Vec<String>,
    active_scene: u8,
    media_source: u8,
    udp_clients: HashMap<SocketAddr, Instant>,
    tcp_clients: Vec<(SocketAddr, Arc<Mutex<TcpStream>>)>,
    udp_socket: Option<Arc<UdpSocket>>,
    udp_online: bool,
    tcp_online: bool,
    udp_error: Option<String>,
    tcp_error: Option<String>,
    serial_connected: bool,
    serial_status: String,
    rx_packets: u64,
    tx_packets: u64,
    logs: VecDeque<PacketLogEntry>,
    start_instant: Instant,
    loaded_scene_source: String,
    // Live Virtual Audio Signal & VU Meter state
    signal_gen_on: bool,
    input_vu_db: [f32; 25],
    aux_vu_db: [f32; 8],
    master_l_vu_db: f32,
    master_r_vu_db: f32,
    master_l_peak_db: f32,
    master_r_peak_db: f32,
    rta_bands: [u8; 31],
}

fn default_ch_names() -> Vec<String> {
    (0..60)
        .map(|ch| match ch {
            0..=23 => format!("CH{:02}", ch + 1),
            24 => "BT/DIGI".to_string(),
            25 => "DIGI R".to_string(),
            26 => "FX 1".to_string(),
            27 => "FX 2".to_string(),
            28..=35 => format!("AUX {}", ch - 27),
            36 => "MAIN L".to_string(),
            37 => "MAIN R".to_string(),
            38..=41 => format!("DCA {}", ch - 37),
            _ => format!("CH{}", ch + 1),
        })
        .collect()
}

impl EmulatorCore {
    fn new() -> Self {
        let mut regs = vec![0u16; AX24E_NUM_REGS];
        for (i, &v) in DEFAULT_REGS.iter().enumerate() {
            let dev = FirmwareLayout::Ax24e2511.to_device(i as u16) as usize;
            if dev < regs.len() {
                regs[dev] = v;
            }
        }
        let mut live_ch_names = default_ch_names();
        let mut loaded_scene_source = "Cena Padrão de Fábrica (AX24E)".to_string();

        // Load igreja.scn if present as default slot 1
        if let Ok(bytes) = std::fs::read("igreja.scn") {
            if let Some(scn) = decode_official_scn(&bytes) {
                for (i, &v) in scn.regs.iter().enumerate() {
                    let dev = FirmwareLayout::Ax24e2511.to_device(i as u16) as usize;
                    if dev < regs.len() {
                        regs[dev] = v;
                    }
                }
                for (idx, nm) in scn.text.iter().enumerate() {
                    if idx < live_ch_names.len() && !nm.trim().is_empty() {
                        live_ch_names[idx] = nm.trim().to_string();
                    }
                }
                loaded_scene_source = format!("igreja.scn ({})", scn.name);
            }
        }

        let mut scenes = vec![regs.clone(); 16];
        let mut scene_names: Vec<String> = (1..=16)
            .map(|i| {
                if i == 1 {
                    "Igreja Culto".to_string()
                } else {
                    format!("Cena {:02}", i)
                }
            })
            .collect();
        let mut scene_ch_names = vec![live_ch_names.clone(); 16];
        let mut active_scene = 1u8;

        // Reload persisted scenes saved previously by AX24E.exe / Rust / Android!
        if let Ok(raw) = std::fs::read(EMU_SCENES_FILE) {
            if let Some(p) = Self::decode_persisted(&raw) {
                if p.scenes.len() == 16 && p.scene_names.len() == 16 {
                    scenes = p.scenes;
                    scene_names = p.scene_names;
                    scene_ch_names = p.scene_ch_names;
                    live_ch_names = p.live_ch_names;
                    let def_names = default_ch_names();
                    if live_ch_names.get(25).map(|s| s.starts_with("AUX") || s == "FX 1").unwrap_or(false)
                        || live_ch_names.get(27).map(|s| s == "FX 3").unwrap_or(false)
                        || live_ch_names.get(36).map(|s| s.starts_with("BUS")).unwrap_or(false)
                    {
                        for idx in 25..42 {
                            live_ch_names[idx] = def_names[idx].clone();
                            for sc in &mut scene_ch_names {
                                sc[idx] = def_names[idx].clone();
                            }
                        }
                    }
                    active_scene = p.active_scene.clamp(1, 16);
                    regs = scenes[(active_scene - 1) as usize].clone();
                    loaded_scene_source =
                        format!("{} (Cena {:02}: {})", EMU_SCENES_FILE, active_scene, scene_names[(active_scene - 1) as usize]);
                }
            }
        }

        Self {
            regs,
            scenes,
            scene_names,
            scene_ch_names,
            live_ch_names,
            active_scene,
            media_source: 1,
            udp_clients: HashMap::new(),
            tcp_clients: Vec::new(),
            udp_socket: None,
            udp_online: false,
            tcp_online: false,
            udp_error: None,
            tcp_error: None,
            serial_connected: false,
            serial_status: "USB/COM Desligada".to_string(),
            rx_packets: 0,
            tx_packets: 0,
            logs: VecDeque::with_capacity(200),
            start_instant: Instant::now(),
            loaded_scene_source,
            signal_gen_on: false,
            input_vu_db: [-90.0; 25],
            aux_vu_db: [-90.0; 8],
            master_l_vu_db: -90.0,
            master_r_vu_db: -90.0,
            master_l_peak_db: -90.0,
            master_r_peak_db: -90.0,
            rta_bands: [0; 31],
        }
    }

    fn persist_scenes(&self) {
        // Simple deterministic binary format: magic "AXEM" + active_scene + 16*(regs + name + 60 ch names) + live_ch_names
        let mut out = Vec::with_capacity(16 * AX24E_NUM_REGS * 2 + 16384);
        out.extend_from_slice(b"AXEM1");
        out.push(self.active_scene);
        for s in 0..16 {
            let sc = &self.scenes[s];
            out.extend_from_slice(&(sc.len() as u32).to_le_bytes());
            for &v in sc {
                out.extend_from_slice(&v.to_le_bytes());
            }
            let nb = self.scene_names[s].as_bytes();
            out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            out.extend_from_slice(nb);
            for ch in 0..60 {
                let cb = self.scene_ch_names[s]
                    .get(ch)
                    .map(|x| x.as_bytes())
                    .unwrap_or(b"");
                out.extend_from_slice(&(cb.len() as u16).to_le_bytes());
                out.extend_from_slice(cb);
            }
        }
        for ch in 0..60 {
            let cb = self
                .live_ch_names
                .get(ch)
                .map(|x| x.as_bytes())
                .unwrap_or(b"");
            out.extend_from_slice(&(cb.len() as u16).to_le_bytes());
            out.extend_from_slice(cb);
        }
        let _ = std::fs::write(EMU_SCENES_FILE, out);
    }

    fn decode_persisted(data: &[u8]) -> Option<PersistedScenes> {
        if data.len() < 6 || &data[0..5] != b"AXEM1" {
            return None;
        }
        let mut pos = 5usize;
        let active_scene = *data.get(pos)?;
        pos += 1;
        let mut scenes = Vec::with_capacity(16);
        let mut scene_names = Vec::with_capacity(16);
        let mut scene_ch_names = Vec::with_capacity(16);
        for _ in 0..16 {
            let rlen = u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?) as usize;
            pos += 4;
            if rlen != AX24E_NUM_REGS {
                return None;
            }
            let mut sc = Vec::with_capacity(rlen);
            for _ in 0..rlen {
                sc.push(u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?));
                pos += 2;
            }
            scenes.push(sc);
            let nlen = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize;
            pos += 2;
            let nm = String::from_utf8_lossy(data.get(pos..pos + nlen)?).to_string();
            pos += nlen;
            scene_names.push(nm);
            let mut cnames = Vec::with_capacity(60);
            for _ in 0..60 {
                let clen = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize;
                pos += 2;
                let cn = String::from_utf8_lossy(data.get(pos..pos + clen)?).to_string();
                pos += clen;
                cnames.push(cn);
            }
            scene_ch_names.push(cnames);
        }
        let mut live_ch_names = Vec::with_capacity(60);
        for _ in 0..60 {
            let clen = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize;
            pos += 2;
            let cn = String::from_utf8_lossy(data.get(pos..pos + clen)?).to_string();
            pos += clen;
            live_ch_names.push(cn);
        }
        Some(PersistedScenes {
            active_scene,
            scenes,
            scene_names,
            scene_ch_names,
            live_ch_names,
        })
    }

    fn push_log(&mut self, proto: &str, peer: &str, cmd: u8, summary: String, raw: &[u8]) {
        let secs = self.start_instant.elapsed().as_secs_f32();
        let hex = raw
            .iter()
            .take(18)
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        if self.logs.len() >= 180 {
            self.logs.pop_front();
        }
        self.logs.push_back(PacketLogEntry {
            ts: format!("{:06.1}s", secs),
            proto: proto.to_string(),
            peer: peer.to_string(),
            cmd,
            summary,
            hex,
        });
    }

    fn get_legacy_reg(&self, legacy_reg: u16) -> u16 {
        let dev = FirmwareLayout::Ax24e2511.to_device(legacy_reg) as usize;
        self.regs.get(dev).copied().unwrap_or(0)
    }

    /// Exact AX24E.25.11.01.001 Mute polarity (@ 0x00466A2C / 0x00441976):
    /// param 0xEB == 0 -> MUTED (RED), param 0xEB != 0 (1) -> ACTIVE/UNMUTED (GREEN)
    fn is_input_muted(&self, ch: usize) -> bool {
        self.get_legacy_reg(map_reg(1, ch, 0xEB)) == 0
    }

    fn is_input_to_lr(&self, ch: usize) -> bool {
        let p15 = self.get_legacy_reg(map_reg(1, ch, 0x15)) & 0x3FFF;
        let p16 = self.get_legacy_reg(map_reg(1, ch, 0x16)) & 0x3FFF;
        p15 != 0 || p16 != 0
    }

    fn is_aux_muted(&self, aux: usize) -> bool {
        self.get_legacy_reg(map_reg(3, aux, 0xEB)) == 0
    }

    fn is_main_l_muted(&self) -> bool {
        self.get_legacy_reg(map_reg(3, 8, 0xEB)) == 0
    }

    fn is_main_r_muted(&self) -> bool {
        self.get_legacy_reg(map_reg(3, 9, 0xEB)) == 0
    }

    fn set_legacy_reg_and_broadcast(&mut self, legacy_reg: u16, val: u16, origin_peer: Option<&str>) {
        let dev = FirmwareLayout::Ax24e2511.to_device(legacy_reg);
        if (dev as usize) < self.regs.len() {
            self.regs[dev as usize] = val;
            let mut p = Vec::with_capacity(4);
            p.extend_from_slice(&dev.to_be_bytes());
            p.extend_from_slice(&val.to_be_bytes());
            let frame = build_frame(0x25, &p);
            self.broadcast_frame(&frame, origin_peer);
        }
    }

    fn toggle_input_mute(&mut self, ch: usize) {
        let reg = map_reg(1, ch, 0xEB);
        let next = if self.is_input_muted(ch) { 1 } else { 0 };
        self.set_legacy_reg_and_broadcast(reg, next, None);
    }

    fn toggle_aux_mute(&mut self, aux: usize) {
        let reg = map_reg(3, aux, 0xEB);
        let next = if self.is_aux_muted(aux) { 1 } else { 0 };
        self.set_legacy_reg_and_broadcast(reg, next, None);
    }

    fn toggle_main_mute(&mut self, right: bool) {
        let ch = if right { 9 } else { 8 };
        let reg = map_reg(3, ch, 0xEB);
        let currently_muted = if right {
            self.is_main_r_muted()
        } else {
            self.is_main_l_muted()
        };
        let next = if currently_muted { 1 } else { 0 };
        self.set_legacy_reg_and_broadcast(reg, next, None);
    }

    /// Real-time virtual DSP meter engine: computes live VU for all 24 inputs, 8 AUX buses, and Master L & R!
    fn update_virtual_vu(&mut self) {
        let t = self.start_instant.elapsed().as_secs_f32();
        let mut sum_l_sq = 0.0_f32;
        let mut sum_r_sq = 0.0_f32;
        let mut sum_aux_sq = [0.0_f32; 8];

        for ch in 0..25 {
            let muted = self.is_input_muted(ch);
            let fader_db = decode_fader_db(self.get_legacy_reg(map_reg(1, ch, 0xEA)));
            let gain_db = (self.get_legacy_reg(map_reg(1, ch, 0x08)) & 0xFF).min(60) as f32;

            // Realistic musical signal envelope per channel when virtual audio generator is ON
            let target_db = if !self.signal_gen_on || muted || fader_db <= -115.0 {
                -90.0
            } else {
                let f1 = 1.4 + (ch as f32 * 0.37) % 2.9;
                let f2 = 3.1 + (ch as f32 * 0.53) % 4.1;
                let pulse = (t * f1 + ch as f32 * 1.1).sin() * 0.5 + 0.5;
                let groove = (t * f2 + ch as f32 * 0.6).cos() * 0.5 + 0.5;
                let base_in = -28.0 + (gain_db * 0.28) + pulse * 14.0 + groove * 8.0;
                (base_in + fader_db).clamp(-90.0, 3.0)
            };

            // Smooth VU ballistics (fast attack, smooth decay)
            let prev = self.input_vu_db[ch];
            let next = if target_db > prev {
                prev + (target_db - prev) * 0.55
            } else {
                (prev - 2.4).max(target_db)
            };
            self.input_vu_db[ch] = next;

            // Mix active unmuted channels into Master L & R and AUX 1..8
            if !muted && next > -85.0 {
                let lin = db_to_lin(next);
                if self.is_input_to_lr(ch) {
                    // On Duonn Axios 24, Pan param 0x0B is 0..200 where 100 (0x0064) = CENTER!
                    let pan_raw = (self.get_legacy_reg(map_reg(1, ch, 0x0B)) & 0xFF) as f32;
                    let pan = ((pan_raw - 100.0).clamp(-100.0, 100.0)) / 100.0;
                    let l_gain = ((1.0 - pan) * 0.5).sqrt();
                    let r_gain = ((1.0 + pan) * 0.5).sqrt();
                    // Apply realistic mix bus headroom (-9 dB per channel sum) so normal mix sits in green/yellow
                    let bus_lin = lin * 0.35;
                    sum_l_sq += (bus_lin * l_gain).powi(2);
                    sum_r_sq += (bus_lin * r_gain).powi(2);
                }
                for aux in 0..8 {
                    let send_db =
                        decode_fader_db(self.get_legacy_reg(map_reg(1, ch, 0x0D + aux as u16)));
                    if send_db > -115.0 {
                        let aux_lin = lin * db_to_lin(send_db);
                        sum_aux_sq[aux] += aux_lin * aux_lin;
                    }
                }
            }
        }

        // Compute AUX 1..8 output VU
        for aux in 0..8 {
            let muted = self.is_aux_muted(aux);
            let aux_fader_db = decode_fader_db(self.get_legacy_reg(map_reg(3, aux, 0xEA)));
            let target = if !self.signal_gen_on || muted || aux_fader_db <= -115.0 {
                -90.0
            } else {
                // Also allow visible gentle bus activity if AUX is active and unmuted even when sends are low
                let mixed_lin = sum_aux_sq[aux].sqrt();
                let bus_db = if mixed_lin > 1e-5 {
                    lin_to_db(mixed_lin) + aux_fader_db
                } else {
                    -90.0
                };
                bus_db.clamp(-90.0, 6.0)
            };
            let prev = self.aux_vu_db[aux];
            self.aux_vu_db[aux] = if target > prev {
                prev + (target - prev) * 0.55
            } else {
                (prev - 2.4).max(target)
            };
        }

        // Compute Master L & R VU
        let main_l_fader = decode_fader_db(self.get_legacy_reg(map_reg(3, 8, 0xEA)));
        let main_r_fader = decode_fader_db(self.get_legacy_reg(map_reg(3, 9, 0xEA)));
        let target_l = if !self.signal_gen_on || self.is_main_l_muted() || main_l_fader <= -115.0 {
            -90.0
        } else {
            (lin_to_db(sum_l_sq.sqrt()) + main_l_fader).clamp(-90.0, 6.0)
        };
        let target_r = if !self.signal_gen_on || self.is_main_r_muted() || main_r_fader <= -115.0 {
            -90.0
        } else {
            (lin_to_db(sum_r_sq.sqrt()) + main_r_fader).clamp(-90.0, 6.0)
        };

        self.master_l_vu_db = if target_l > self.master_l_vu_db {
            self.master_l_vu_db + (target_l - self.master_l_vu_db) * 0.6
        } else {
            (self.master_l_vu_db - 2.2).max(target_l)
        };
        self.master_r_vu_db = if target_r > self.master_r_vu_db {
            self.master_r_vu_db + (target_r - self.master_r_vu_db) * 0.6
        } else {
            (self.master_r_vu_db - 2.2).max(target_r)
        };

        // Peak hold decay
        self.master_l_peak_db = (self.master_l_peak_db - 0.35).max(self.master_l_vu_db);
        self.master_r_peak_db = (self.master_r_peak_db - 0.35).max(self.master_r_vu_db);

        // Update 31-band RTA array for 0x46 polls from AX24E.25.11.01.001.exe
        let avg_master = ((self.master_l_vu_db + self.master_r_vu_db) * 0.5 + 60.0).clamp(0.0, 66.0);
        for b in 0..31 {
            let shape = ((b as f32 * 0.35 + t * 2.5).sin() * 0.35 + 0.65)
                * (1.0 - ((b as f32 - 12.0).abs() / 26.0).min(0.65));
            self.rta_bands[b] = ((avg_master * shape) * 1.4).clamp(0.0, 100.0) as u8;
        }
    }

    fn broadcast_frame(&mut self, frame: &[u8], exclude_peer: Option<&str>) {
        let now = Instant::now();
        self.udp_clients
            .retain(|_, last| now.duration_since(*last) < Duration::from_secs(30));
        if let Some(sock) = &self.udp_socket {
            for &addr in self.udp_clients.keys() {
                if exclude_peer == Some(addr.to_string().as_str()) {
                    continue;
                }
                let _ = sock.send_to(frame, addr);
                self.tx_packets += 1;
            }
        }
        let mut alive = Vec::new();
        for (addr, stream_arc) in self.tcp_clients.drain(..) {
            if exclude_peer == Some(addr.to_string().as_str()) {
                alive.push((addr, stream_arc));
                continue;
            }
            let ok = if let Ok(mut s) = stream_arc.lock() {
                s.write_all(frame).is_ok()
            } else {
                false
            };
            if ok {
                self.tx_packets += 1;
                alive.push((addr, stream_arc));
            }
        }
        self.tcp_clients = alive;
    }

    fn describe_register_write(dev_reg: u16, val: u16) -> String {
        if let Some(leg) = FirmwareLayout::Ax24e2511.from_device(dev_reg) {
            if (0x40..(0x40 + 25 * 0x3E)).contains(&leg) {
                let rel = leg - 0x40;
                let ch = (rel / 0x3E) as usize;
                let off = rel % 0x3E;
                let ch_lbl = if ch == 24 {
                    "BT/DIGI".to_string()
                } else {
                    format!("CH{:02}", ch + 1)
                };
                return match off {
                    0x00 => format!("{} HI-Z = {}", ch_lbl, if (val & 1) != 0 { "ON" } else { "OFF" }),
                    0x01 => format!("{} +48V = {}", ch_lbl, if val != 0 { "ON" } else { "OFF" }),
                    0x08 => format!("{} Gain = +{}dB", ch_lbl, val & 0xFF),
                    0x09 => format!("{} Fase = {}", ch_lbl, if val == 0 { "Ø INV" } else { "NORMAL" }),
                    0x0A => format!("{} Mute = {}", ch_lbl, if val == 0 { "MUTE" } else { "ATIVO" }),
                    0x0B => format!("{} Pan = {}", ch_lbl, (val as i32) - 100),
                    0x0C => format!("{} Fader = {}", ch_lbl, format_db(decode_fader_db(val))),
                    0x0D..=0x14 => format!(
                        "{} AUX{} = {} ({})",
                        ch_lbl,
                        off - 0x0C,
                        format_db(decode_fader_db(val)),
                        if (val & 0x8000) != 0 { "PRE" } else { "POS" }
                    ),
                    0x15 => format!("{} MAIN L = {}", ch_lbl, if (val & 0x3FFF) != 0 { "ON" } else { "OFF" }),
                    0x16 => format!("{} MAIN R = {}", ch_lbl, if (val & 0x3FFF) != 0 { "ON" } else { "OFF" }),
                    0x17 | 0x18 => format!(
                        "{} Solo = {}",
                        ch_lbl,
                        if (val & 0x3FFF) != 0 && (val & 0x4000) == 0 { "ON" } else { "OFF" }
                    ),
                    0x19 => format!("{} FX1 = {}", ch_lbl, format_db(decode_fader_db(val))),
                    0x1A => format!("{} FX2 = {}", ch_lbl, format_db(decode_fader_db(val))),
                    _ => format!("{} p=0x{:02X} v=0x{:04X}", ch_lbl, off, val),
                };
            }
            if (0x64E..(0x64E + 10 * 0x6D)).contains(&leg) {
                let rel = leg - 0x64E;
                let out = (rel / 0x6D) as usize;
                let off = rel % 0x6D;
                let lbl = match out {
                    8 => "MAIN L".to_string(),
                    9 => "MAIN R".to_string(),
                    _ => format!("AUX {}", out + 1),
                };
                return match off {
                    0x3E => format!("{} Fader = {}", lbl, format_db(decode_fader_db(val))),
                    0x3F => format!("{} Fase = {}", lbl, if val == 0 { "Ø INV" } else { "NORMAL" }),
                    0x40 => format!("{} Mute = {}", lbl, if val == 0 { "MUTE" } else { "ATIVO" }),
                    0x4A | 0x4B => format!(
                        "{} Solo = {}",
                        lbl,
                        if (val & 0x3FFF) != 0 && (val & 0x4000) == 0 { "ON" } else { "OFF" }
                    ),
                    _ => format!("{} p=0x{:02X} v=0x{:04X}", lbl, off, val),
                };
            }
            if leg == 0xA00 {
                return format!("MAIN L Solo={}", if (val & 0x3FFF) != 0 { "ON" } else { "OFF" });
            }
            if leg == 0xA6E {
                return format!("MAIN R Solo={}", if (val & 0x3FFF) != 0 { "ON" } else { "OFF" });
            }
        }
        format!("reg[0x{:04X}]=0x{:04X}", dev_reg, val)
    }

    /// Processes one valid Axios 24 frame — full AX24E.25.11.01.001 protocol including all Scene Save/Load/Edit opcodes!
    fn handle_frame(&mut self, proto: &str, peer: &str, raw_frame: &[u8]) -> Vec<Vec<u8>> {
        self.rx_packets += 1;
        let Some((cmd, payload)) = parse_frame(raw_frame) else {
            self.push_log(
                proto,
                peer,
                0xFF,
                format!("Pacote bruto ({}B)", raw_frame.len()),
                raw_frame,
            );
            return Vec::new();
        };

        let mut replies = Vec::new();
        match cmd {
            // 0x00: Official AX24E Binary Discovery & Handshake
            0x00 => {
                let mut p = vec![0x00u8, 0xE0, 0x01, 0x39, 0x01];
                p.extend_from_slice(EMU_DEV_NAME.as_bytes());
                p.push(0x00);
                replies.push(build_frame(0x00, &p));
                self.push_log(
                    proto,
                    peer,
                    cmd,
                    format!("DISCOVERY (0x00) -> {} [E001/39/01]", EMU_DEV_NAME),
                    raw_frame,
                );
            }
            // 0x01: Scene Recall [slot 1..16]
            0x01 => {
                let slot = payload.first().copied().unwrap_or(1).clamp(1, 16);
                let idx = (slot - 1) as usize;
                self.regs = self.scenes[idx].clone();
                self.live_ch_names = self.scene_ch_names[idx].clone();
                self.active_scene = slot;
                self.persist_scenes();
                replies.push(build_frame(0x01, &[slot]));
                self.push_log(
                    proto,
                    peer,
                    cmd,
                    format!("CARREGAR CENA {:02} ({})", slot, self.scene_names[idx]),
                    raw_frame,
                );
            }
            // 0x03: Parameter Write — update RAM, echo 0x03 ACK to sender, push 0x25 to others
            0x03 => {
                let mut pairs = Vec::new();
                let mut desc = Vec::new();
                let mut ack_payload = Vec::new();
                for chunk in payload.chunks_exact(4) {
                    let r = u16::from_be_bytes([chunk[0], chunk[1]]);
                    let v = u16::from_be_bytes([chunk[2], chunk[3]]);
                    if (r as usize) < self.regs.len() {
                        self.regs[r as usize] = v;
                    }
                    if desc.len() < 3 {
                        desc.push(Self::describe_register_write(r, v));
                    }
                    pairs.push((r, v));
                    ack_payload.extend_from_slice(&r.to_be_bytes());
                    ack_payload.extend_from_slice(&v.to_be_bytes());
                }
                if !pairs.is_empty() {
                    replies.push(build_frame(0x03, &ack_payload));
                    let push_frame = build_frame(0x25, &ack_payload);
                    self.broadcast_frame(&push_frame, Some(peer));
                    let extra = if pairs.len() > 3 {
                        format!(" (+{} regs)", pairs.len() - 3)
                    } else {
                        String::new()
                    };
                    self.push_log(
                        proto,
                        peer,
                        cmd,
                        format!("WRITE: {}{}", desc.join(", "), extra),
                        raw_frame,
                    );
                }
            }
            // 0x04: Get Scene Name [slot 1..16]
            0x04 => {
                let slot = payload.first().copied().unwrap_or(1).clamp(1, 16);
                let name = &self.scene_names[(slot - 1) as usize];
                let mut p = vec![slot];
                p.extend_from_slice(name.as_bytes());
                replies.push(build_frame(0x04, &p));
            }
            // 0x06: Parameter Read
            0x06 => {
                let mut out_payload = Vec::new();
                for chunk in payload.chunks_exact(2) {
                    let r = u16::from_be_bytes([chunk[0], chunk[1]]);
                    let v = self.regs.get(r as usize).copied().unwrap_or(0);
                    out_payload.extend_from_slice(&r.to_be_bytes());
                    out_payload.extend_from_slice(&v.to_be_bytes());
                }
                if !out_payload.is_empty() {
                    for chunk in out_payload.chunks(56 * 4) {
                        replies.push(build_frame(0x06, chunk));
                    }
                }
            }
            // 0x10: Save / Rename Scene Slot [slot 1..16, optional ascii name...]
            0x10 => {
                let slot = payload.first().copied().unwrap_or(self.active_scene).clamp(1, 16);
                let idx = (slot - 1) as usize;
                self.scenes[idx] = self.regs.clone();
                self.scene_ch_names[idx] = self.live_ch_names.clone();
                if payload.len() > 1 {
                    let nm = String::from_utf8_lossy(&payload[1..])
                        .trim_matches('\0')
                        .trim()
                        .to_string();
                    if !nm.is_empty() {
                        self.scene_names[idx] = nm;
                    }
                }
                self.active_scene = slot;
                self.persist_scenes();
                // Reply 0x10 ACK + updated 0x04 scene name!
                replies.push(build_frame(0x10, &[slot]));
                let mut name_p = vec![slot];
                name_p.extend_from_slice(self.scene_names[idx].as_bytes());
                replies.push(build_frame(0x04, &name_p));
                self.push_log(
                    proto,
                    peer,
                    cmd,
                    format!("SALVAR CENA {:02} [{}]", slot, self.scene_names[idx]),
                    raw_frame,
                );
            }
            // 0x11: Save Scene (Commit current mixer state to slot!)
            0x11 => {
                let slot = payload.first().copied().unwrap_or(self.active_scene).clamp(1, 16);
                let idx = (slot - 1) as usize;
                self.scenes[idx] = self.regs.clone();
                self.scene_ch_names[idx] = self.live_ch_names.clone();
                if payload.len() > 1 {
                    let nm = String::from_utf8_lossy(&payload[1..])
                        .trim_matches('\0')
                        .trim()
                        .to_string();
                    if !nm.is_empty() {
                        self.scene_names[idx] = nm;
                    }
                }
                self.active_scene = slot;
                self.persist_scenes();
                replies.push(build_frame(0x11, &[slot]));
                let mut name_p = vec![slot];
                name_p.extend_from_slice(self.scene_names[idx].as_bytes());
                replies.push(build_frame(0x04, &name_p));
                self.push_log(
                    proto,
                    peer,
                    cmd,
                    format!("SALVAR CENA (0x11) {:02} [{}]", slot, self.scene_names[idx]),
                    raw_frame,
                );
            }
            // 0x2E: Get Live Channel Name [ch]
            0x2E => {
                let ch = payload.first().copied().unwrap_or(0) as usize;
                let name = self
                    .live_ch_names
                    .get(ch)
                    .cloned()
                    .unwrap_or_else(|| format!("CH{:02}", ch + 1));
                let mut p = vec![ch as u8];
                p.extend_from_slice(name.as_bytes());
                replies.push(build_frame(0x2E, &p));
            }
            // 0x2F: Set Live Channel Name [ch, ascii...]
            0x2F => {
                if let Some(&ch_b) = payload.first() {
                    let ch = ch_b as usize;
                    let nm = String::from_utf8_lossy(&payload[1..])
                        .trim_matches('\0')
                        .trim()
                        .to_string();
                    if ch < self.live_ch_names.len() && !nm.is_empty() {
                        self.live_ch_names[ch] = nm.clone();
                        self.persist_scenes();
                    }
                    replies.push(build_frame(0x2F, &[ch_b]));
                    let mut bc = vec![ch_b];
                    bc.extend_from_slice(nm.as_bytes());
                    self.broadcast_frame(&build_frame(0x2E, &bc), Some(peer));
                    self.push_log(proto, peer, cmd, format!("RENOMEAR CH{} -> {}", ch + 1, nm), raw_frame);
                }
            }
            // 0x38: System/Status ping
            0x38 => {
                replies.push(build_frame(0x38, payload));
            }
            // 0x39: KV Query
            0x39 => {
                let req = String::from_utf8_lossy(payload).trim().replace('\n', " ");
                let kv = if req.contains("SCENEKEYLOCK") {
                    "SCENEKEYLOCK=0\n".to_string()
                } else {
                    format!(
                        "DEVNAME={}\nFW_VER={}\nFW_BUILD=1018\nSN=EMU20260001\nMODEL=AX24E\n",
                        EMU_DEV_NAME, EMU_FW_VER
                    )
                };
                replies.push(build_kv_frame(&kv, false));
                self.push_log(proto, peer, cmd, format!("KV [{}] -> {}", req, EMU_DEV_NAME), raw_frame);
            }
            // 0x3A: KV Set
            0x3A => {
                replies.push(build_frame(0x3A, payload));
            }
            // 0x46: Live RTA / VU telemetry poll [start, count]
            0x46 => {
                let start = payload.first().copied().unwrap_or(0).min(30) as usize;
                let count = payload.get(1).copied().unwrap_or(31).min((31 - start) as u8) as usize;
                let mut p = Vec::with_capacity(2 + count);
                p.push(start as u8);
                p.push(count as u8);
                p.extend_from_slice(&self.rta_bands[start..start + count]);
                replies.push(build_frame(0x46, &p));
            }
            // 0x5A: Read Scene Register Chunk [slot, count, start_hi, start_lo]
            0x5A => {
                if payload.len() >= 4 {
                    let slot = payload[0].clamp(1, 16);
                    let count = payload[1].min(60) as usize;
                    let start = u16::from_be_bytes([payload[2], payload[3]]) as usize;
                    let sc = &self.scenes[(slot - 1) as usize];
                    let mut p = Vec::with_capacity(4 + count * 2);
                    p.push(slot);
                    p.push(count as u8);
                    p.extend_from_slice(&(start as u16).to_be_bytes());
                    for i in 0..count {
                        let v = sc.get(start + i).copied().unwrap_or(0);
                        p.extend_from_slice(&v.to_be_bytes());
                    }
                    replies.push(build_frame(0x5A, &p));
                }
            }
            // 0x5B: Write Scene Register Chunk [slot, count, start_hi, start_lo, vals...]
            0x5B => {
                if payload.len() >= 4 {
                    let slot = payload[0].clamp(1, 16);
                    let count = payload[1] as usize;
                    let start = u16::from_be_bytes([payload[2], payload[3]]) as usize;
                    let sc = &mut self.scenes[(slot - 1) as usize];
                    for i in 0..count {
                        let off = 4 + i * 2;
                        if off + 2 <= payload.len() && start + i < sc.len() {
                            sc[start + i] = u16::from_be_bytes([payload[off], payload[off + 1]]);
                        }
                    }
                    // Official ACK @ 0x469D04 expects [slot, count, start_hi, start_lo]
                    replies.push(build_frame(0x5B, &payload[0..4]));
                }
            }
            // 0x5C: Commit Scene Write [slot]
            0x5C => {
                let slot = payload.first().copied().unwrap_or(1).clamp(1, 16);
                if slot == self.active_scene {
                    self.regs = self.scenes[(slot - 1) as usize].clone();
                }
                self.persist_scenes();
                replies.push(build_frame(0x5C, &[slot]));
                self.push_log(
                    proto,
                    peer,
                    cmd,
                    format!("GRAVACAO CENA {:02} CONFIRMADA (salvo em disco!)", slot),
                    raw_frame,
                );
            }
            // 0x5D: Read Scene Channel Name [slot, ch]
            0x5D => {
                if payload.len() >= 2 {
                    let slot = payload[0].clamp(1, 16);
                    let ch = payload[1] as usize;
                    let nm = self.scene_ch_names[(slot - 1) as usize]
                        .get(ch)
                        .cloned()
                        .unwrap_or_default();
                    let mut p = vec![slot, payload[1]];
                    p.extend_from_slice(nm.as_bytes());
                    replies.push(build_frame(0x5D, &p));
                }
            }
            // 0x5E: Write Scene Channel Name [slot, ch, ascii...]
            0x5E => {
                if payload.len() >= 2 {
                    let slot = payload[0].clamp(1, 16);
                    let ch = payload[1] as usize;
                    let nm = String::from_utf8_lossy(&payload[2..])
                        .trim_matches('\0')
                        .trim()
                        .to_string();
                    if ch < 60 {
                        self.scene_ch_names[(slot - 1) as usize][ch] = nm.clone();
                        if slot == self.active_scene && !nm.is_empty() {
                            self.live_ch_names[ch] = nm;
                        }
                    }
                    replies.push(build_frame(0x5E, &payload[0..2]));
                }
            }
            // 0x63: System / Global Config Register Write ACK
            0x63 => {
                replies.push(build_frame(0x63, payload));
            }
            // 0x6A: Read Scene Title [slot]
            0x6A => {
                let slot = payload.first().copied().unwrap_or(1).clamp(1, 16);
                let title = &self.scene_names[(slot - 1) as usize];
                let mut p = vec![slot];
                p.extend_from_slice(title.as_bytes());
                replies.push(build_frame(0x6A, &p));
            }
            // 0x6B: Write Scene Title [slot, ascii...]
            0x6B => {
                if let Some(&slot_b) = payload.first() {
                    let slot = slot_b.clamp(1, 16);
                    let idx = (slot - 1) as usize;
                    let title = String::from_utf8_lossy(&payload[1..])
                        .trim_matches('\0')
                        .trim()
                        .to_string();
                    if !title.is_empty() {
                        self.scene_names[idx] = title.clone();
                    }
                    self.persist_scenes();
                    replies.push(build_frame(0x6B, &[slot]));
                    let mut p4 = vec![slot];
                    p4.extend_from_slice(self.scene_names[idx].as_bytes());
                    replies.push(build_frame(0x04, &p4));
                    self.push_log(
                        proto,
                        peer,
                        cmd,
                        format!("TITULO CENA {:02} -> {}", slot, self.scene_names[idx]),
                        raw_frame,
                    );
                }
            }
            // 0x72: Media / Bluetooth Source
            0x72 => {
                let b2 = payload.get(2).copied().unwrap_or(0);
                let src_name = match b2 {
                    0xA8 | 0xC0 | 0xB5 => {
                        self.media_source = 0;
                        "USB"
                    }
                    0xC7 | 0xC1 => {
                        self.media_source = 1;
                        "Bluetooth"
                    }
                    0xC3 | 0xC2 => {
                        self.media_source = 2;
                        "Óptico/Coaxial"
                    }
                    _ => "Digital",
                };
                replies.push(build_frame(0x72, payload));
                self.push_log(proto, peer, cmd, format!("Fonte Digital -> {}", src_name), raw_frame);
            }
            other => {
                self.push_log(
                    proto,
                    peer,
                    other,
                    format!("Opcode 0x{:02X} ({}B)", other, payload.len()),
                    raw_frame,
                );
            }
        }
        replies
    }
}

fn drain_stream_frames(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    loop {
        let Some(pos) = buf.iter().position(|&b| b == 0x80) else {
            buf.clear();
            break;
        };
        if pos > 0 {
            buf.drain(0..pos);
        }
        if buf.len() < 5 {
            break;
        }
        let crc_off = if buf[1] == 0xFF && buf[2] == 0xFF {
            0x106
        } else {
            buf[1] as usize
        };
        let total = crc_off + 2;
        if crc_off < 3 || total > 512 {
            buf.remove(0);
            continue;
        }
        if buf.len() < total {
            break;
        }
        let candidate: Vec<u8> = buf[..total].to_vec();
        if parse_frame(&candidate).is_some() {
            buf.drain(0..total);
            frames.push(candidate);
        } else {
            buf.remove(0);
        }
    }
    frames
}

fn start_udp_server(core: Arc<Mutex<EmulatorCore>>) {
    thread::spawn(move || {
        match UdpSocket::bind(format!("0.0.0.0:{}", UDP_REMOTE_PORT)) {
            Ok(sock) => {
                let _ = sock.set_broadcast(true);
                let _ = sock.set_read_timeout(Some(Duration::from_millis(40)));
                let sock = Arc::new(sock);
                {
                    let mut c = core.lock().unwrap();
                    c.udp_socket = Some(Arc::clone(&sock));
                    c.udp_online = true;
                    c.udp_error = None;
                }
                let mut buf = [0u8; 2048];
                loop {
                    if let Ok((n, src)) = sock.recv_from(&mut buf) {
                        let peer = src.to_string();
                        let replies = {
                            let mut c = core.lock().unwrap();
                            c.udp_clients.insert(src, Instant::now());
                            c.handle_frame("UDP", &peer, &buf[..n])
                        };
                        for r in replies {
                            let _ = sock.send_to(&r, src);
                            let mut c = core.lock().unwrap();
                            c.tx_packets += 1;
                        }
                    }
                }
            }
            Err(e) => {
                let mut c = core.lock().unwrap();
                c.udp_online = false;
                c.udp_error = Some(format!("Erro UDP 8086: {}", e));
            }
        }
    });
}

fn start_tcp_server(core: Arc<Mutex<EmulatorCore>>) {
    thread::spawn(move || {
        match TcpListener::bind(format!("0.0.0.0:{}", TCP_REMOTE_PORT)) {
            Ok(listener) => {
                {
                    let mut c = core.lock().unwrap();
                    c.tcp_online = true;
                    c.tcp_error = None;
                }
                for incoming in listener.incoming().flatten() {
                    let Ok(peer_addr) = incoming.peer_addr() else {
                        continue;
                    };
                    let _ = incoming.set_nodelay(true);
                    let _ = incoming.set_read_timeout(Some(Duration::from_millis(40)));
                    let Ok(write_clone) = incoming.try_clone() else {
                        continue;
                    };
                    let write_arc = Arc::new(Mutex::new(write_clone));
                    {
                        let mut c = core.lock().unwrap();
                        c.tcp_clients.push((peer_addr, Arc::clone(&write_arc)));
                    }
                    let core_c = Arc::clone(&core);
                    thread::spawn(move || {
                        let mut stream = incoming;
                        let mut read_buf = [0u8; 2048];
                        let mut acc = Vec::new();
                        let peer = peer_addr.to_string();
                        loop {
                            match stream.read(&mut read_buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    acc.extend_from_slice(&read_buf[..n]);
                                    for frame in drain_stream_frames(&mut acc) {
                                        let replies = {
                                            let mut c = core_c.lock().unwrap();
                                            c.handle_frame("TCP", &peer, &frame)
                                        };
                                        for r in replies {
                                            if let Ok(mut ws) = write_arc.lock() {
                                                if ws.write_all(&r).is_ok() {
                                                    let mut c = core_c.lock().unwrap();
                                                    c.tx_packets += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(ref e)
                                    if e.kind() == std::io::ErrorKind::WouldBlock
                                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                                Err(_) => break,
                            }
                        }
                        let mut c = core_c.lock().unwrap();
                        c.tcp_clients.retain(|(a, _)| *a != peer_addr);
                    });
                }
            }
            Err(e) => {
                let mut c = core.lock().unwrap();
                c.tcp_online = false;
                c.tcp_error = Some(format!("Erro TCP 8087: {}", e));
            }
        }
    });
}

fn start_serial_bridge(core: Arc<Mutex<EmulatorCore>>, port_name: String) {
    thread::spawn(move || {
        match serialport::new(&port_name, 115200)
            .timeout(Duration::from_millis(40))
            .open()
        {
            Ok(mut port) => {
                {
                    let mut c = core.lock().unwrap();
                    c.serial_connected = true;
                    c.serial_status = format!("COM {} ON", port_name);
                }
                let mut buf = [0u8; 512];
                let mut acc = Vec::new();
                loop {
                    if !core.lock().unwrap().serial_connected {
                        break;
                    }
                    if let Ok(n) = port.read(&mut buf) {
                        if n > 0 {
                            acc.extend_from_slice(&buf[..n]);
                            for frame in drain_stream_frames(&mut acc) {
                                let replies = {
                                    let mut c = core.lock().unwrap();
                                    c.handle_frame("COM", &port_name, &frame)
                                };
                                for r in replies {
                                    let _ = port.write_all(&r);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let mut c = core.lock().unwrap();
                c.serial_connected = false;
                c.serial_status = format!("Erro {}: {}", port_name, e);
            }
        }
    });
}

/// Draws a glowing hardware circle ("bolinha") — Green when active, Red when muted — with VU ring & label
fn draw_channel_circle(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    code: &str,
    sub_label: &str,
    db_text: &str,
    muted: bool,
    vu_db: f32,
    extra_badge: Option<&str>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter();

    let bg_card = egui::Color32::from_rgb(18, 24, 36);
    let border_card = if resp.hovered() {
        egui::Color32::from_rgb(0, 210, 255)
    } else {
        egui::Color32::from_rgb(38, 50, 72)
    };
    p.rect(
        rect,
        8.0,
        bg_card,
        egui::Stroke::new(1.0_f32, border_card),
        egui::StrokeKind::Inside,
    );

    let vu_norm = ((vu_db + 60.0) / 60.0).clamp(0.0, 1.0);
    let circle_radius = (rect.width().min(rect.height() - 26.0) * 0.44).max(16.0);
    let center = egui::pos2(rect.center().x, rect.top() + 8.0 + circle_radius);

    // Outer live VU pulse halo when active
    if !muted && vu_norm > 0.05 {
        let halo_alpha = (vu_norm * 130.0) as u8;
        let halo_col = if vu_db > -3.0 {
            egui::Color32::from_rgba_unmultiplied(255, 65, 54, halo_alpha)
        } else if vu_db > -12.0 {
            egui::Color32::from_rgba_unmultiplied(255, 204, 0, halo_alpha)
        } else {
            egui::Color32::from_rgba_unmultiplied(46, 204, 113, halo_alpha)
        };
        p.circle_filled(center, circle_radius + 3.0 + vu_norm * 3.5, halo_col);
    }

    // Main LED circle: RED if muted, GREEN if active!
    let (fill, ring) = if muted {
        (
            egui::Color32::from_rgb(205, 42, 42),
            egui::Color32::from_rgb(255, 115, 115),
        )
    } else {
        (
            egui::Color32::from_rgb(32, 178, 92),
            egui::Color32::from_rgb(115, 255, 170),
        )
    };
    p.circle_filled(center, circle_radius, fill);
    p.circle_stroke(center, circle_radius, egui::Stroke::new(2.0_f32, ring));

    // Code + dB inside circle
    p.text(
        egui::pos2(center.x, center.y - circle_radius * 0.22),
        egui::Align2::CENTER_CENTER,
        code,
        egui::FontId::proportional((circle_radius * 0.46).clamp(11.0, 22.0)),
        egui::Color32::WHITE,
    );
    p.text(
        egui::pos2(center.x, center.y + circle_radius * 0.28),
        egui::Align2::CENTER_CENTER,
        if muted { "MUTE" } else { db_text },
        egui::FontId::proportional((circle_radius * 0.34).clamp(9.5, 15.0)),
        egui::Color32::from_rgb(240, 250, 255),
    );

    // Mini horizontal VU bar right below circle
    let bar_y = center.y + circle_radius + 5.0;
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, bar_y),
        egui::pos2(rect.right() - 8.0, (bar_y + 5.0).min(rect.bottom() - 16.0)),
    );
    p.rect_filled(bar_rect, 2.0, egui::Color32::from_rgb(30, 38, 52));
    if !muted && vu_norm > 0.01 {
        let lit_w = bar_rect.width() * vu_norm;
        let bar_col = if vu_db > -3.0 {
            egui::Color32::from_rgb(255, 59, 48)
        } else if vu_db > -12.0 {
            egui::Color32::from_rgb(255, 204, 0)
        } else {
            egui::Color32::from_rgb(46, 204, 113)
        };
        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(lit_w, bar_rect.height()));
        p.rect_filled(fill_rect, 2.0, bar_col);
    }

    // Channel name / badge at bottom
    let footer = if let Some(b) = extra_badge {
        format!("{} {}", sub_label, b)
    } else {
        sub_label.to_string()
    };
    p.text(
        egui::pos2(rect.center().x, rect.bottom() - 7.0),
        egui::Align2::CENTER_CENTER,
        footer,
        egui::FontId::proportional(10.5),
        egui::Color32::from_rgb(195, 212, 232),
    );

    resp
}

/// Draws a large segmented vertical LED VU Meter bar for Master L or Master R
fn draw_vertical_vu_meter(ui: &mut egui::Ui, label: &str, level_db: f32, peak_db: f32, height: f32) {
    let width = 34.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, egui::Color32::from_rgb(12, 16, 24));
    p.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(48, 62, 85)),
        egui::StrokeKind::Inside,
    );

    let segments = 24;
    let top_pad = 18.0;
    let bot_pad = 18.0;
    let usable_h = (rect.height() - top_pad - bot_pad).max(40.0);
    let seg_gap = 2.0;
    let seg_h = (usable_h - (segments as f32 - 1.0) * seg_gap) / segments as f32;

    let norm = ((level_db + 60.0) / 66.0).clamp(0.0, 1.0);
    let peak_norm = ((peak_db + 60.0) / 66.0).clamp(0.0, 1.0);
    let lit_count = (norm * segments as f32).round() as usize;
    let peak_seg = ((peak_norm * segments as f32).round() as usize).saturating_sub(1);

    p.text(
        egui::pos2(rect.center().x, rect.top() + 9.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        egui::Color32::from_rgb(0, 216, 255),
    );

    for i in 0..segments {
        let frac = (i + 1) as f32 / segments as f32;
        let y_bot = rect.bottom() - bot_pad - i as f32 * (seg_h + seg_gap);
        let seg_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 5.0, y_bot - seg_h),
            egui::pos2(rect.right() - 5.0, y_bot),
        );
        let is_lit = i < lit_count || (peak_db > -58.0 && i == peak_seg);
        let col = if frac > 0.85 {
            if is_lit {
                egui::Color32::from_rgb(255, 59, 48)
            } else {
                egui::Color32::from_rgb(52, 22, 22)
            }
        } else if frac > 0.65 {
            if is_lit {
                egui::Color32::from_rgb(255, 204, 0)
            } else {
                egui::Color32::from_rgb(54, 46, 18)
            }
        } else if is_lit {
            egui::Color32::from_rgb(46, 204, 113)
        } else {
            egui::Color32::from_rgb(20, 46, 32)
        };
        p.rect_filled(seg_rect, 2.0, col);
    }

    p.text(
        egui::pos2(rect.center().x, rect.bottom() - 9.0),
        egui::Align2::CENTER_CENTER,
        if level_db <= -85.0 {
            "-∞".to_string()
        } else {
            format!("{:+.0}", level_db)
        },
        egui::FontId::proportional(10.0),
        egui::Color32::LIGHT_GRAY,
    );
}

struct EmulatorApp {
    core: Arc<Mutex<EmulatorCore>>,
    local_ip: String,
    com_input: String,
    show_sniffer: bool,
}

impl EmulatorApp {
    fn new(core: Arc<Mutex<EmulatorCore>>) -> Self {
        let local_ip = detect_local_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let com_input = serialport::available_ports()
            .unwrap_or_default()
            .first()
            .map(|p| p.port_name.clone())
            .unwrap_or_else(|| "COM10".to_string());
        Self {
            core,
            local_ip,
            com_input,
            show_sniffer: false,
        }
    }
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(45));
        ctx.set_visuals(egui::Visuals::dark());

        // Advance real-time virtual VU simulation
        {
            let mut c = self.core.lock().unwrap();
            c.update_virtual_vu();
        }

        // TOP HEADER BAR
        egui::TopBottomPanel::top("emu_header").show(ctx, |ui| {
            ui.add_space(4.0);
            let (udp_on, tcp_on, n_udp, n_tcp, rx, tx, active_scn, scn_name, mut sig_on, ser_on, ser_st, loaded_src) = {
                let c = self.core.lock().unwrap();
                let sn = c.scene_names[(c.active_scene - 1) as usize].clone();
                (
                    c.udp_online,
                    c.tcp_online,
                    c.udp_clients.len(),
                    c.tcp_clients.len(),
                    c.rx_packets,
                    c.tx_packets,
                    c.active_scene,
                    sn,
                    c.signal_gen_on,
                    c.serial_connected,
                    c.serial_status.clone(),
                    c.loaded_scene_source.clone(),
                )
            };

            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("DUONN AXIOS 24 — EMULADOR VIRTUAL")
                        .strong()
                        .size(15.0)
                        .color(egui::Color32::from_rgb(0, 216, 255)),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("IP: {}", self.local_ip))
                        .strong()
                        .color(egui::Color32::from_rgb(46, 204, 113)),
                );
                ui.label(
                    egui::RichText::new("UDP:8086")
                        .strong()
                        .color(if udp_on { egui::Color32::GREEN } else { egui::Color32::RED }),
                );
                ui.label(
                    egui::RichText::new("TCP:8087")
                        .strong()
                        .color(if tcp_on { egui::Color32::GREEN } else { egui::Color32::RED }),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Cena {:02}: {}", active_scn, scn_name))
                        .strong()
                        .color(egui::Color32::from_rgb(241, 196, 15)),
                );
                ui.separator();
                ui.label(format!("Clientes: {} | RX:{} TX:{}", n_udp + n_tcp, rx, tx));

                let sig_btn_col = if sig_on {
                    egui::Color32::from_rgb(32, 160, 88)
                } else {
                    egui::Color32::from_rgb(90, 50, 50)
                };
                if ui
                    .add(
                        egui::Button::new(if sig_on { "VU Sinal: ON" } else { "VU Sinal: OFF" })
                            .fill(sig_btn_col),
                    )
                    .clicked()
                {
                    sig_on = !sig_on;
                    self.core.lock().unwrap().signal_gen_on = sig_on;
                }

                if ui.button(if self.show_sniffer { "Ocultar Log" } else { "Log / Sniffer" }).clicked() {
                    self.show_sniffer = !self.show_sniffer;
                }

                if ui.button("Liberar Firewall").clicked() {
                    let _ = std::process::Command::new("powershell")
                        .args([
                            "-NoProfile",
                            "-Command",
                            "Start-Process -FilePath 'd:\\src\\axios24\\liberar_firewall.bat' -Verb RunAs",
                        ])
                        .spawn();
                }

                ui.add(egui::TextEdit::singleline(&mut self.com_input).desired_width(54.0));
                if !ser_on {
                    if ui.button("COM").clicked() {
                        start_serial_bridge(Arc::clone(&self.core), self.com_input.clone());
                    }
                } else if ui.button("Fechar COM").clicked() {
                    let mut c = self.core.lock().unwrap();
                    c.serial_connected = false;
                    c.serial_status = "USB/COM Desligada".to_string();
                }
                ui.label(egui::RichText::new(format!("{} | {}", ser_st, loaded_src)).size(10.5).color(egui::Color32::GRAY));
            });
            ui.add_space(4.0);
        });

        // OPTIONAL SNIFFER WINDOW (floating so it doesn't shrink the main layout!)
        if self.show_sniffer {
            egui::Window::new("Monitor de Pacotes (Sniffer Axios 24)")
                .default_size([720.0, 260.0])
                .open(&mut self.show_sniffer)
                .show(ctx, |ui| {
                    if ui.button("Limpar Log").clicked() {
                        self.core.lock().unwrap().logs.clear();
                    }
                    ui.separator();
                    let logs: Vec<PacketLogEntry> =
                        self.core.lock().unwrap().logs.iter().cloned().collect();
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in logs {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "[{}] {:3} 0x{:02X}",
                                            entry.ts, entry.proto, entry.cmd
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(0, 200, 245)),
                                    );
                                    ui.label(
                                        egui::RichText::new(&entry.summary)
                                            .strong()
                                            .size(11.0)
                                            .color(egui::Color32::WHITE),
                                    );
                                    ui.label(
                                        egui::RichText::new(&entry.hex)
                                            .monospace()
                                            .size(10.0)
                                            .color(egui::Color32::GRAY),
                                    );
                                });
                            }
                        });
                });
        }

        // BOTTOM FOOTER: 1 SINGLE ROW WITH 8 AUX CIRCLES ("8 bolinhas mostrando os auxiliares")
        egui::TopBottomPanel::bottom("emu_aux_footer")
            .exact_height(124.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("SAÍDAS AUXILIARES (AUX 1 – AUX 8)  •  Verde = Ativo  |  Vermelho = Mudo (clique para alternar)")
                        .strong()
                        .size(11.5)
                        .color(egui::Color32::from_rgb(0, 205, 245)),
                );
                ui.add_space(4.0);

                let aux_snap: Vec<(usize, String, f32, bool, f32)> = {
                    let c = self.core.lock().unwrap();
                    (0..8)
                        .map(|aux| {
                            let fdb = decode_fader_db(c.get_legacy_reg(map_reg(3, aux, 0xEA)));
                            let muted = c.is_aux_muted(aux);
                            let name = c
                                .live_ch_names
                                .get(28 + aux)
                                .cloned()
                                .unwrap_or_else(|| format!("AUX {}", aux + 1));
                            (aux, name, fdb, muted, c.aux_vu_db[aux])
                        })
                        .collect()
                };

                let total_w = ui.available_width();
                let spacing = 6.0;
                let item_w = ((total_w - spacing * 7.0) / 8.0).max(60.0);
                let item_h = (ui.available_height() - 6.0).max(86.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    for (aux, name, fdb, muted, vu_db) in aux_snap {
                        let resp = draw_channel_circle(
                            ui,
                            egui::vec2(item_w, item_h),
                            &format!("AUX{}", aux + 1),
                            &name,
                            &format_db(fdb),
                            muted,
                            vu_db,
                            None,
                        );
                        if resp.clicked() {
                            self.core.lock().unwrap().toggle_aux_mute(aux);
                        }
                    }
                });
            });

        // MAIN CENTRAL PANEL: 70% LEFT (24 BOLINHAS INPUTS) + 30% RIGHT (2 BOLINHAS MAIORES MASTER L&R + VU)
        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            let gap = 10.0;
            let left_w = ((avail.x - gap) * 0.70).max(320.0);
            let right_w = ((avail.x - gap) * 0.30).max(200.0);
            let panel_h = avail.y;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;

                // ===== LEFT 70%: 24 INPUT CHANNELS (6 cols x 4 rows) =====
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, panel_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.label(
                            egui::RichText::new("CANAIS DE ENTRADA (CH01 – CH24)  •  Verde = Ativo  |  Vermelho = Mudo (clique = Mute, scroll = Fader)")
                                .strong()
                                .size(12.0)
                                .color(egui::Color32::from_rgb(190, 215, 240)),
                        );
                        ui.add_space(4.0);

                        let inputs_snap: Vec<(usize, String, f32, bool, bool, f32)> = {
                            let c = self.core.lock().unwrap();
                            (0..24)
                                .map(|ch| {
                                    let fdb = decode_fader_db(c.get_legacy_reg(map_reg(1, ch, 0xEA)));
                                    let muted = c.is_input_muted(ch);
                                    let lr = c.is_input_to_lr(ch);
                                    let name = c
                                        .live_ch_names
                                        .get(ch)
                                        .cloned()
                                        .unwrap_or_else(|| format!("CH{:02}", ch + 1));
                                    (ch, name, fdb, muted, lr, c.input_vu_db[ch])
                                })
                                .collect()
                        };

                        let cols = 6usize;
                        let rows = 4usize;
                        let grid_gap = 6.0;
                        let rem_h = (ui.available_height() - 4.0).max(200.0);
                        let cell_w = ((left_w - grid_gap * (cols as f32 - 1.0)) / cols as f32).max(50.0);
                        let cell_h = ((rem_h - grid_gap * (rows as f32 - 1.0)) / rows as f32).max(60.0);

                        for r in 0..rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = grid_gap;
                                for col in 0..cols {
                                    let idx = r * cols + col;
                                    let (ch, ref name, fdb, muted, lr, vu_db) = inputs_snap[idx];
                                    let badge = if lr { Some("•LR") } else { None };
                                    let resp = draw_channel_circle(
                                        ui,
                                        egui::vec2(cell_w, cell_h),
                                        &format!("{:02}", ch + 1),
                                        name,
                                        &format_db(fdb),
                                        muted,
                                        vu_db,
                                        badge,
                                    );
                                    if resp.clicked() {
                                        self.core.lock().unwrap().toggle_input_mute(ch);
                                    }
                                    if resp.hovered() {
                                        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
                                        if scroll_y.abs() > 0.1 {
                                            let next_db = (fdb + if scroll_y > 0.0 { 2.0 } else { -2.0 }).clamp(-120.0, 10.0);
                                            let mut c = self.core.lock().unwrap();
                                            c.set_legacy_reg_and_broadcast(
                                                map_reg(1, ch, 0xEA),
                                                encode_fader_db(next_db),
                                                None,
                                            );
                                        }
                                    }
                                }
                            });
                            if r + 1 < rows {
                                ui.add_space(grid_gap);
                            }
                        }
                    },
                );

                // ===== RIGHT 30%: MASTER L & R (2 BIG CIRCLES + LARGE STEREO VU) =====
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, panel_h),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        egui::Frame::group(ui.style())
                            .fill(egui::Color32::from_rgb(16, 22, 34))
                            .stroke(egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(0, 145, 195)))
                            .corner_radius(10.0)
                            .show(ui, |ui| {
                                ui.set_min_size(egui::vec2(right_w - 8.0, panel_h - 8.0));
                                ui.vertical_centered(|ui| {
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new("MASTER L & R")
                                            .strong()
                                            .size(15.0)
                                            .color(egui::Color32::from_rgb(0, 220, 255)),
                                    );
                                    ui.add_space(8.0);

                                    let (l_db, r_db, l_mut, r_mut, l_vu, r_vu, l_pk, r_pk) = {
                                        let c = self.core.lock().unwrap();
                                        (
                                            decode_fader_db(c.get_legacy_reg(map_reg(3, 8, 0xEA))),
                                            decode_fader_db(c.get_legacy_reg(map_reg(3, 9, 0xEA))),
                                            c.is_main_l_muted(),
                                            c.is_main_r_muted(),
                                            c.master_l_vu_db,
                                            c.master_r_vu_db,
                                            c.master_l_peak_db,
                                            c.master_r_peak_db,
                                        )
                                    };

                                    // 2 Larger Master Circles side-by-side
                                    let circle_card_w = ((right_w - 36.0) * 0.5).clamp(84.0, 150.0);
                                    let circle_card_h = (panel_h * 0.34).clamp(115.0, 175.0);
                                    ui.horizontal(|ui| {
                                        let total_pair = circle_card_w * 2.0 + 10.0;
                                        let side_pad = ((ui.available_width() - total_pair) * 0.5).max(0.0);
                                        ui.add_space(side_pad);
                                        let resp_l = draw_channel_circle(
                                            ui,
                                            egui::vec2(circle_card_w, circle_card_h),
                                            "MAIN L",
                                            if l_mut { "MUDO" } else { "ATIVO" },
                                            &format_db(l_db),
                                            l_mut,
                                            l_vu,
                                            None,
                                        );
                                        if resp_l.clicked() {
                                            self.core.lock().unwrap().toggle_main_mute(false);
                                        }
                                        ui.add_space(10.0);
                                        let resp_r = draw_channel_circle(
                                            ui,
                                            egui::vec2(circle_card_w, circle_card_h),
                                            "MAIN R",
                                            if r_mut { "MUDO" } else { "ATIVO" },
                                            &format_db(r_db),
                                            r_mut,
                                            r_vu,
                                            None,
                                        );
                                        if resp_r.clicked() {
                                            self.core.lock().unwrap().toggle_main_mute(true);
                                        }
                                    });

                                    ui.add_space(12.0);
                                    ui.label(
                                        egui::RichText::new("VU ESTÉREO AO VIVO (L / R)")
                                            .strong()
                                            .size(12.0)
                                            .color(egui::Color32::from_rgb(200, 220, 240)),
                                    );
                                    ui.add_space(6.0);

                                    // Large Stereo Vertical LED VU Meters (pixel-aligned L | dB Scale | R)
                                    let vu_h = (ui.available_height() - 18.0).clamp(130.0, 320.0);
                                    ui.horizontal(|ui| {
                                        let vu_pair_w = 34.0 * 2.0 + 40.0;
                                        let side_pad = ((ui.available_width() - vu_pair_w) * 0.5).max(0.0);
                                        ui.add_space(side_pad);
                                        draw_vertical_vu_meter(ui, "L", l_vu, l_pk, vu_h);
                                        // Center dB scale drawn with exact same height vu_h so L & R align perfectly!
                                        let (scale_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(32.0, vu_h),
                                            egui::Sense::hover(),
                                        );
                                        let sp = ui.painter();
                                        let ticks = [
                                            ("+6", 0.04_f32),
                                            ("0", 0.16_f32),
                                            ("-6", 0.28_f32),
                                            ("-12", 0.42_f32),
                                            ("-24", 0.62_f32),
                                            ("-48", 0.86_f32),
                                        ];
                                        let usable = (scale_rect.height() - 36.0).max(40.0);
                                        for (lbl, frac) in ticks {
                                            let y = scale_rect.top() + 18.0 + usable * frac;
                                            sp.text(
                                                egui::pos2(scale_rect.center().x, y),
                                                egui::Align2::CENTER_CENTER,
                                                lbl,
                                                egui::FontId::proportional(10.0),
                                                egui::Color32::GRAY,
                                            );
                                        }
                                        draw_vertical_vu_meter(ui, "R", r_vu, r_pk, vu_h);
                                    });
                                });
                            });
                    },
                );
            });
        });
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "roboto".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Roboto-Medium.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["roboto".to_owned()],
    );
    fonts.families.insert(
        egui::FontFamily::Monospace,
        vec!["roboto".to_owned()],
    );
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let core = Arc::new(Mutex::new(EmulatorCore::new()));
    start_udp_server(Arc::clone(&core));
    start_tcp_server(Arc::clone(&core));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Duonn Axios 24 — Emulador de Hardware Virtual (24 CH + Master L&R VU + 8 AUX)")
            .with_inner_size([1240.0, 740.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Duonn Axios 24 Virtual Emulator",
        options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(EmulatorApp::new(core)))
        }),
    )
}
