use crate::dsp::{
    hpf_lpf_stages, peq_band_biquad, BiquadCoeffs, FilterFamily, FilterSlope, PeqBandMode,
};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const MT_SAMPLE_RATE: usize = 48000;
pub const MT_MAX_CHANNELS: usize = 24;

#[derive(Clone)]
pub struct LoadedTrack {
    pub name: String,
    pub channel_idx: usize, // 0..23 -> CH1..CH24
    pub samples: Arc<Vec<f32>>,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Clone, Copy)]
pub struct ChannelDspParams {
    pub gain_db: f32, // 0..60
    pub fader_db: f32,
    pub muted: bool,
    pub solo: bool,
    pub pan: i32, // -100..100
    pub phase_invert: bool,
    pub to_main_lr: bool,
    pub gate_enabled: bool,
    pub gate_threshold_db: f32,
    pub comp_enabled: bool,
    pub comp_threshold_db: f32,
    pub comp_ratio: f32,
    pub comp_gain_db: f32,
    pub eq_enabled: bool,
    pub hpf_enabled: bool,
    pub hpf_freq: f32,
    pub hpf_family: FilterFamily,
    pub hpf_slope: FilterSlope,
    pub lpf_enabled: bool,
    pub lpf_freq: f32,
    pub lpf_family: FilterFamily,
    pub lpf_slope: FilterSlope,
    pub peq_count: usize,
    pub is_output: bool,
    pub peq: [(PeqBandMode, f32, f32, f32); 7],
}

impl Default for ChannelDspParams {
    fn default() -> Self {
        Self {
            gain_db: 20.0,
            fader_db: 0.0,
            muted: false,
            solo: false,
            pan: 0,
            phase_invert: false,
            to_main_lr: true,
            gate_enabled: false,
            gate_threshold_db: -40.0,
            comp_enabled: false,
            comp_threshold_db: -18.0,
            comp_ratio: 3.0,
            comp_gain_db: 0.0,
            eq_enabled: false,
            hpf_enabled: false,
            hpf_freq: 20.0,
            hpf_family: FilterFamily::Butterworth,
            hpf_slope: FilterSlope::Slope18dB,
            lpf_enabled: false,
            lpf_freq: 20000.0,
            lpf_family: FilterFamily::Butterworth,
            lpf_slope: FilterSlope::Slope12dB,
            peq_count: 4,
            is_output: false,
            peq: [
                (PeqBandMode::LowShelf, 80.0, 0.0, 1.0),
                (PeqBandMode::Peaking, 400.0, 0.0, 1.5),
                (PeqBandMode::Peaking, 2500.0, 0.0, 1.5),
                (PeqBandMode::HighShelf, 10000.0, 0.0, 1.0),
                (PeqBandMode::Peaking, 1000.0, 0.0, 1.5),
                (PeqBandMode::Peaking, 4000.0, 0.0, 1.5),
                (PeqBandMode::Peaking, 8000.0, 0.0, 1.5),
            ],
        }
    }
}

#[derive(Clone, Copy)]
pub struct ConsoleDspSnapshot {
    pub channels: [ChannelDspParams; MT_MAX_CHANNELS],
    pub main_l: ChannelDspParams,
    pub main_r: ChannelDspParams,
    pub main_l_db: f32,
    pub main_r_db: f32,
    pub main_l_muted: bool,
    pub main_r_muted: bool,
}

impl Default for ConsoleDspSnapshot {
    fn default() -> Self {
        Self {
            channels: [ChannelDspParams::default(); MT_MAX_CHANNELS],
            main_l: ChannelDspParams { is_output: true, peq_count: 7, ..ChannelDspParams::default() },
            main_r: ChannelDspParams { is_output: true, peq_count: 7, ..ChannelDspParams::default() },
            main_l_db: 0.0,
            main_r_db: 0.0,
            main_l_muted: false,
            main_r_muted: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, x: f64, c: &BiquadCoeffs) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

struct ChannelRuntime {
    stages: Vec<BiquadCoeffs>,
    states: Vec<BiquadState>,
    gate_env: f32,
    comp_env: f32,
    last_eq_key: u64,
}

impl Default for ChannelRuntime {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            states: Vec::new(),
            gate_env: 1.0,
            comp_env: 0.0,
            last_eq_key: u64::MAX,
        }
    }
}

fn eq_key(p: &ChannelDspParams) -> u64 {
    let mut h: u64 = if p.eq_enabled { 1 } else { 0 };
    h = h.wrapping_mul(131).wrapping_add(if p.hpf_enabled { 2 } else { 0 });
    h = h.wrapping_mul(131).wrapping_add((p.hpf_freq * 10.0) as u64);
    h = h.wrapping_mul(131).wrapping_add(p.hpf_family as u64);
    h = h.wrapping_mul(131).wrapping_add(p.hpf_slope as u64);
    h = h.wrapping_mul(131).wrapping_add(if p.lpf_enabled { 3 } else { 0 });
    h = h.wrapping_mul(131).wrapping_add((p.lpf_freq * 10.0) as u64);
    h = h.wrapping_mul(131).wrapping_add(p.lpf_family as u64);
    h = h.wrapping_mul(131).wrapping_add(p.lpf_slope as u64);
    for &(m, f, g, q) in p.peq.iter().take(p.peq_count.min(7)) {
        h = h.wrapping_mul(131).wrapping_add(m as u64);
        h = h.wrapping_mul(131).wrapping_add((f * 10.0) as u64);
        h = h.wrapping_mul(131).wrapping_add(((g + 20.0) * 100.0) as u64);
        h = h.wrapping_mul(131).wrapping_add((q * 100.0) as u64);
    }
    h
}

impl ChannelRuntime {
    fn sync_filters(&mut self, p: &ChannelDspParams) {
        let key = eq_key(p);
        if key == self.last_eq_key {
            return;
        }
        self.last_eq_key = key;
        self.stages.clear();
        if p.eq_enabled {
            if p.hpf_enabled && p.hpf_freq > 20.5 {
                self.stages
                    .extend(hpf_lpf_stages(p.hpf_freq, p.hpf_family, p.hpf_slope, false));
            }
            if p.lpf_enabled && p.lpf_freq < 19900.0 {
                self.stages
                    .extend(hpf_lpf_stages(p.lpf_freq, p.lpf_family, p.lpf_slope, true));
            }
            for &(mode, freq, gain, q) in p.peq.iter().take(p.peq_count.min(7)) {
                if gain.abs() > 0.05 || mode != PeqBandMode::Peaking {
                    self.stages.push(peq_band_biquad(mode, freq, gain, q));
                }
            }
        }
        self.states.resize(self.stages.len(), BiquadState::default());
    }

    #[inline]
    fn process_sample(&mut self, mut x: f32, p: &ChannelDspParams) -> f32 {
        if !p.is_output {
            // Preamp trim: +20 dB is unity for standard recorded WAV stems
            let gain_lin = 10.0_f32.powf(((p.gain_db - 20.0) * 0.5).clamp(-15.0, 24.0) / 20.0);
            x *= gain_lin;
        }
        if p.phase_invert {
            x = -x;
        }
        // Noise Gate
        if p.gate_enabled {
            let env_in = x.abs();
            let th_lin = 10.0_f32.powf(p.gate_threshold_db / 20.0);
            let target = if env_in >= th_lin { 1.0 } else { 0.02 };
            let coeff = if target > self.gate_env { 0.01 } else { 0.0005 };
            self.gate_env += (target - self.gate_env) * coeff;
            x *= self.gate_env;
        }
        // PEQ + HPF/LPF cascade
        if !self.stages.is_empty() {
            let mut xd = x as f64;
            for (c, st) in self.stages.iter().zip(self.states.iter_mut()) {
                xd = st.process(xd, c);
            }
            x = xd as f32;
        }
        // Compressor
        if p.comp_enabled {
            let abs_x = x.abs().max(1e-6);
            self.comp_env = self.comp_env * 0.995 + abs_x * 0.005;
            let level_db = 20.0 * self.comp_env.log10();
            let over = (level_db - p.comp_threshold_db).max(0.0);
            let gr_db = over * (1.0 - 1.0 / p.comp_ratio.max(1.0));
            let comp_lin = 10.0_f32.powf((p.comp_gain_db - gr_db) / 20.0);
            x *= comp_lin;
        }
        x
    }
}

/// Cleans a WAV filename into a short mixer channel label (e.g. "01_Kick_In.wav" -> "Kick In")
pub fn clean_track_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Track".to_string());
    let trimmed = stem
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '_' || c == '-' || c == ' ')
        .replace(['_', '-'], " ");
    let base = if trimmed.is_empty() { stem } else { trimmed };
    base.chars().take(12).collect()
}

/// Parses an uncompressed RIFF/WAVE file (16/24/32-bit PCM or 32-bit IEEE float, mono/stereo/multichannel)
/// and returns one mono `48 kHz` `Vec<f32>` per audio channel inside the file.
pub fn parse_wav_file(path: &Path) -> Result<Vec<Vec<f32>>, String> {
    let data = fs::read(path).map_err(|e| format!("Erro ao ler {}: {}", path.display(), e))?;
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(format!("{} não é um arquivo WAV RIFF válido.", path.display()));
    }
    let mut pos = 12usize;
    let mut audio_fmt = 1u16;
    let mut num_ch = 1u16;
    let mut sample_rate = 48000u32;
    let mut bits_per_sample = 16u16;
    let mut pcm_bytes: Option<&[u8]> = None;

    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(data.len());
        if id == b"fmt " && body_end - body_start >= 16 {
            let b = &data[body_start..body_end];
            audio_fmt = u16::from_le_bytes([b[0], b[1]]);
            num_ch = u16::from_le_bytes([b[2], b[3]]).max(1);
            sample_rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]).max(8000);
            bits_per_sample = u16::from_le_bytes([b[14], b[15]]);
            if audio_fmt == 0xFFFE && b.len() >= 26 {
                audio_fmt = u16::from_le_bytes([b[24], b[25]]);
            }
        } else if id == b"data" {
            pcm_bytes = Some(&data[body_start..body_end]);
            break;
        }
        pos = body_start + size + (size & 1);
    }

    let raw = pcm_bytes.ok_or_else(|| "Chunk 'data' não encontrado no WAV.".to_string())?;
    let bytes_per_sample = (bits_per_sample / 8).max(1) as usize;
    let frame_stride = bytes_per_sample * num_ch as usize;
    if frame_stride == 0 {
        return Err("Formato WAV inválido.".to_string());
    }
    let num_frames = raw.len() / frame_stride;
    let mut channels = vec![Vec::with_capacity(num_frames); num_ch as usize];

    for f in 0..num_frames {
        let f_off = f * frame_stride;
        for c in 0..num_ch as usize {
            let s_off = f_off + c * bytes_per_sample;
            let sample = match (audio_fmt, bits_per_sample) {
                (1, 16) => {
                    let v = i16::from_le_bytes([raw[s_off], raw[s_off + 1]]);
                    v as f32 / 32768.0
                }
                (1, 24) => {
                    let b0 = raw[s_off] as i32;
                    let b1 = (raw[s_off + 1] as i32) << 8;
                    let b2 = ((raw[s_off + 2] as i8) as i32) << 16;
                    (b0 | b1 | b2) as f32 / 8_388_608.0
                }
                (1, 32) => {
                    let v = i32::from_le_bytes([raw[s_off], raw[s_off + 1], raw[s_off + 2], raw[s_off + 3]]);
                    v as f32 / 2_147_483_648.0
                }
                (3, 32) => f32::from_le_bytes([raw[s_off], raw[s_off + 1], raw[s_off + 2], raw[s_off + 3]])
                    .clamp(-1.0, 1.0),
                _ => return Err(format!("Formato WAV não suportado (fmt={}, {} bits).", audio_fmt, bits_per_sample)),
            };
            channels[c].push(sample);
        }
    }

    if sample_rate as usize != MT_SAMPLE_RATE && num_frames > 1 {
        let ratio = sample_rate as f64 / MT_SAMPLE_RATE as f64;
        let out_len = ((num_frames as f64) / ratio).round().max(1.0) as usize;
        for ch in &mut channels {
            let mut resampled = Vec::with_capacity(out_len);
            for i in 0..out_len {
                let src_pos = (i as f64) * ratio;
                let idx = (src_pos.floor() as usize).min(ch.len() - 1);
                let next = (idx + 1).min(ch.len() - 1);
                let frac = (src_pos - idx as f64) as f32;
                resampled.push(ch[idx] * (1.0 - frac) + ch[next] * frac);
            }
            *ch = resampled;
        }
    }

    Ok(channels)
}

pub struct MultitrackEngine {
    pub tracks: Arc<Mutex<Vec<LoadedTrack>>>,
    pub snapshot: Arc<Mutex<ConsoleDspSnapshot>>,
    pub channel_meters_db: Arc<Mutex<[f32; MT_MAX_CHANNELS + 2]>>, // 0..24 + MAIN L/R
    pub playing: Arc<AtomicBool>,
    pub looping: Arc<AtomicBool>,
    pub cursor_samples: Arc<AtomicUsize>,
    pub total_samples: Arc<AtomicUsize>,
    pub status: String,
    thread_started: bool,
}

impl Default for MultitrackEngine {
    fn default() -> Self {
        Self {
            tracks: Arc::new(Mutex::new(Vec::new())),
            snapshot: Arc::new(Mutex::new(ConsoleDspSnapshot::default())),
            channel_meters_db: Arc::new(Mutex::new([-90.0; MT_MAX_CHANNELS + 2])),
            playing: Arc::new(AtomicBool::new(false)),
            looping: Arc::new(AtomicBool::new(true)),
            cursor_samples: Arc::new(AtomicUsize::new(0)),
            total_samples: Arc::new(AtomicUsize::new(0)),
            status: "Nenhum multitrack carregado".to_string(),
            thread_started: false,
        }
    }
}

impl MultitrackEngine {
    /// Loads one or many WAV files at once.
    /// - Multiple mono/stereo files: each file maps sequentially to CH1..CH24 (stereo files summed to mono per channel).
    /// - A single polyphonic multi-channel WAV (>2 channels): splits each embedded channel into CH1..CH24.
    pub fn load_wav_paths(&mut self, paths: &[std::path::PathBuf]) -> Result<usize, String> {
        self.playing.store(false, Ordering::Relaxed);
        self.cursor_samples.store(0, Ordering::Relaxed);
        let mut loaded = Vec::new();
        let mut max_len = 0usize;

        for path in paths {
            let ch_vecs = parse_wav_file(path)?;
            let base_name = clean_track_name(path);
            if ch_vecs.len() > 2 && paths.len() == 1 {
                for (sub_i, vec) in ch_vecs.into_iter().enumerate() {
                    if loaded.len() >= MT_MAX_CHANNELS {
                        break;
                    }
                    max_len = max_len.max(vec.len());
                    loaded.push(LoadedTrack {
                        name: format!("{} {}", base_name.chars().take(8).collect::<String>(), sub_i + 1),
                        channel_idx: loaded.len(),
                        samples: Arc::new(vec),
                        muted: false,
                        solo: false,
                    });
                }
            } else if !ch_vecs.is_empty() && loaded.len() < MT_MAX_CHANNELS {
                let mono = if ch_vecs.len() == 1 {
                    ch_vecs.into_iter().next().unwrap()
                } else {
                    let len = ch_vecs[0].len();
                    let mut sum = Vec::with_capacity(len);
                    for i in 0..len {
                        sum.push(0.5 * (ch_vecs[0][i] + ch_vecs[1][i]));
                    }
                    sum
                };
                max_len = max_len.max(mono.len());
                loaded.push(LoadedTrack {
                    name: base_name,
                    channel_idx: loaded.len(),
                    samples: Arc::new(mono),
                    muted: false,
                    solo: false,
                });
            }
        }

        let count = loaded.len();
        if count == 0 {
            return Err("Nenhuma faixa WAV encontrada.".to_string());
        }
        *self.tracks.lock().unwrap() = loaded;
        self.total_samples.store(max_len, Ordering::Relaxed);
        self.status = format!(
            "{} faixas carregadas ({:.1}s @ 48kHz)",
            count,
            max_len as f32 / MT_SAMPLE_RATE as f32
        );
        self.ensure_audio_thread();
        Ok(count)
    }

    pub fn play(&mut self) {
        if self.total_samples.load(Ordering::Relaxed) > 0 {
            self.ensure_audio_thread();
            self.playing.store(true, Ordering::Relaxed);
        }
    }

    pub fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
        self.cursor_samples.store(0, Ordering::Relaxed);
    }

    pub fn clear(&mut self) {
        self.stop();
        self.tracks.lock().unwrap().clear();
        self.total_samples.store(0, Ordering::Relaxed);
        *self.channel_meters_db.lock().unwrap() = [-90.0; MT_MAX_CHANNELS + 2];
        self.status = "Nenhum multitrack carregado".to_string();
    }

    fn ensure_audio_thread(&mut self) {
        if self.thread_started {
            return;
        }
        self.thread_started = true;
        let tracks = Arc::clone(&self.tracks);
        let snapshot = Arc::clone(&self.snapshot);
        let meters = Arc::clone(&self.channel_meters_db);
        let playing = Arc::clone(&self.playing);
        let looping = Arc::clone(&self.looping);
        let cursor = Arc::clone(&self.cursor_samples);
        let total = Arc::clone(&self.total_samples);

        thread::spawn(move || {
            run_playback_loop(tracks, snapshot, meters, playing, looping, cursor, total);
        });
    }
}

struct MixerEngineState {
    runtimes: Vec<ChannelRuntime>,
    rt_main_l: ChannelRuntime,
    rt_main_r: ChannelRuntime,
}

impl MixerEngineState {
    fn new() -> Self {
        Self {
            runtimes: (0..MT_MAX_CHANNELS).map(|_| ChannelRuntime::default()).collect(),
            rt_main_l: ChannelRuntime::default(),
            rt_main_r: ChannelRuntime::default(),
        }
    }

    fn render_stereo_block(
        &mut self,
        out_stereo: &mut [f32],
        tracks: &Arc<Mutex<Vec<LoadedTrack>>>,
        snapshot: &Arc<Mutex<ConsoleDspSnapshot>>,
        meters: &Arc<Mutex<[f32; MT_MAX_CHANNELS + 2]>>,
        playing: &Arc<AtomicBool>,
        looping: &Arc<AtomicBool>,
        cursor: &Arc<AtomicUsize>,
        total: &Arc<AtomicUsize>,
    ) -> bool {
        let tr_guard = tracks.lock().unwrap().clone();
        let snap = *snapshot.lock().unwrap();
        let tot = total.load(Ordering::Relaxed);
        if !playing.load(Ordering::Relaxed) || tr_guard.is_empty() || tot == 0 {
            out_stereo.fill(0.0);
            if tr_guard.is_empty() || tot == 0 {
                playing.store(false, Ordering::Relaxed);
            }
            return false;
        }

        let any_solo = snap.channels.iter().any(|c| c.solo) || tr_guard.iter().any(|t| t.solo);
        for ch in 0..MT_MAX_CHANNELS {
            self.runtimes[ch].sync_filters(&snap.channels[ch]);
        }
        self.rt_main_l.sync_filters(&snap.main_l);
        self.rt_main_r.sync_filters(&snap.main_r);

        let main_l_lin = if snap.main_l_muted || snap.main_l_db <= -119.0 {
            0.0
        } else {
            10.0_f32.powf(snap.main_l_db / 20.0)
        };
        let main_r_lin = if snap.main_r_muted || snap.main_r_db <= -119.0 {
            0.0
        } else {
            10.0_f32.powf(snap.main_r_db / 20.0)
        };

        let mut ch_peaks = [0.0_f32; MT_MAX_CHANNELS];
        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;
        let mut pos = cursor.load(Ordering::Relaxed);

        for frame in out_stereo.chunks_exact_mut(2) {
            if pos >= tot {
                if looping.load(Ordering::Relaxed) {
                    pos = 0;
                } else {
                    playing.store(false, Ordering::Relaxed);
                    frame[0] = 0.0;
                    frame[1] = 0.0;
                    continue;
                }
            }

            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;
            for tr in &tr_guard {
                let ch = tr.channel_idx.min(MT_MAX_CHANNELS - 1);
                let p = &snap.channels[ch];
                let raw = *tr.samples.get(pos).unwrap_or(&0.0);
                let proc_s = self.runtimes[ch].process_sample(raw, p);

                let muted = tr.muted || p.muted || (any_solo && !p.solo && !tr.solo);
                let fader_lin = if muted || p.fader_db <= -119.0 {
                    0.0
                } else {
                    10.0_f32.powf(p.fader_db / 20.0)
                };
                let post = proc_s * fader_lin;
                ch_peaks[ch] = ch_peaks[ch].max(if muted { proc_s.abs() * 0.25 } else { post.abs() });

                if p.to_main_lr && !muted {
                    let pan_norm = ((p.pan as f32 / 100.0) + 1.0) * 0.5;
                    let pan_l = ((1.0 - pan_norm) * std::f32::consts::FRAC_PI_2).sin();
                    let pan_r = (pan_norm * std::f32::consts::FRAC_PI_2).sin();
                    sum_l += post * pan_l;
                    sum_r += post * pan_r;
                }
            }

            let proc_l = self.rt_main_l.process_sample(sum_l, &snap.main_l);
            let proc_r = self.rt_main_r.process_sample(sum_r, &snap.main_r);
            let out_l = (proc_l * main_l_lin).tanh();
            let out_r = (proc_r * main_r_lin).tanh();
            peak_l = peak_l.max(out_l.abs());
            peak_r = peak_r.max(out_r.abs());
            frame[0] = out_l;
            frame[1] = out_r;
            pos += 1;
        }

        cursor.store(pos, Ordering::Relaxed);
        let mut m = meters.lock().unwrap();
        for ch in 0..MT_MAX_CHANNELS {
            m[ch] = if ch_peaks[ch] > 1e-5 {
                (20.0 * ch_peaks[ch].log10()).clamp(-90.0, 3.0)
            } else {
                -90.0
            };
        }
        m[MT_MAX_CHANNELS] = if peak_l > 1e-5 { (20.0 * peak_l.log10()).clamp(-90.0, 3.0) } else { -90.0 };
        m[MT_MAX_CHANNELS + 1] = if peak_r > 1e-5 { (20.0 * peak_r.log10()).clamp(-90.0, 3.0) } else { -90.0 };
        true
    }
}

#[cfg(windows)]
fn run_playback_loop(
    tracks: Arc<Mutex<Vec<LoadedTrack>>>,
    snapshot: Arc<Mutex<ConsoleDspSnapshot>>,
    meters: Arc<Mutex<[f32; MT_MAX_CHANNELS + 2]>>,
    playing: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    cursor: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
) {
    #[repr(C)]
    struct WaveFormatEx {
        w_format_tag: u16,
        n_channels: u16,
        n_samples_per_sec: u32,
        n_avg_bytes_per_sec: u32,
        n_block_align: u16,
        w_bits_per_sample: u16,
        cb_size: u16,
    }
    #[repr(C)]
    struct WaveHdr {
        lp_data: *mut i16,
        dw_buffer_length: u32,
        dw_bytes_recorded: u32,
        dw_user: usize,
        dw_flags: u32,
        dw_loops: u32,
        lp_next: usize,
        reserved: usize,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn waveOutOpen(
            phwo: *mut usize,
            u_device_id: u32,
            pwfx: *const WaveFormatEx,
            dw_callback: usize,
            dw_instance: usize,
            fdw_open: u32,
        ) -> u32;
        fn waveOutPrepareHeader(hwo: usize, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveOutWrite(hwo: usize, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveOutReset(hwo: usize) -> u32;
        fn waveOutClose(hwo: usize) -> u32;
    }

    const FRAMES: usize = 1024;
    const NUM_BUFS: usize = 4;
    const WHDR_DONE: u32 = 0x0000_0001;

    let fmt = WaveFormatEx {
        w_format_tag: 1,
        n_channels: 2,
        n_samples_per_sec: MT_SAMPLE_RATE as u32,
        n_avg_bytes_per_sec: (MT_SAMPLE_RATE * 4) as u32,
        n_block_align: 4,
        w_bits_per_sample: 16,
        cb_size: 0,
    };

    let mut hwo: usize = 0;
    if unsafe { waveOutOpen(&mut hwo, u32::MAX, &fmt, 0, 0, 0) } != 0 || hwo == 0 {
        return;
    }

    let mut bufs: Vec<Vec<i16>> = (0..NUM_BUFS).map(|_| vec![0i16; FRAMES * 2]).collect();
    let mut hdrs: Vec<WaveHdr> = bufs
        .iter_mut()
        .map(|b| WaveHdr {
            lp_data: b.as_mut_ptr(),
            dw_buffer_length: (FRAMES * 4) as u32,
            dw_bytes_recorded: 0,
            dw_user: 0,
            dw_flags: WHDR_DONE,
            dw_loops: 0,
            lp_next: 0,
            reserved: 0,
        })
        .collect();

    for h in &mut hdrs {
        unsafe {
            waveOutPrepareHeader(hwo, h, std::mem::size_of::<WaveHdr>() as u32);
        }
        h.dw_flags |= WHDR_DONE;
    }

    let mut eng = MixerEngineState::new();
    let mut f32_buf = vec![0.0_f32; FRAMES * 2];
    let mut buf_idx = 0usize;

    loop {
        if !playing.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        if (hdrs[buf_idx].dw_flags & WHDR_DONE) == 0 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        if !eng.render_stereo_block(&mut f32_buf, &tracks, &snapshot, &meters, &playing, &looping, &cursor, &total) {
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        let out = &mut bufs[buf_idx];
        for (i, &s) in f32_buf.iter().enumerate() {
            out[i] = (s * 32767.0).clamp(-32767.0, 32767.0) as i16;
        }

        unsafe {
            hdrs[buf_idx].dw_flags &= !WHDR_DONE;
            waveOutWrite(hwo, &mut hdrs[buf_idx], std::mem::size_of::<WaveHdr>() as u32);
        }
        buf_idx = (buf_idx + 1) % NUM_BUFS;
    }

    #[allow(unreachable_code)]
    unsafe {
        waveOutReset(hwo);
        waveOutClose(hwo);
    }
}

#[cfg(target_os = "linux")]
fn run_playback_loop(
    tracks: Arc<Mutex<Vec<LoadedTrack>>>,
    snapshot: Arc<Mutex<ConsoleDspSnapshot>>,
    meters: Arc<Mutex<[f32; MT_MAX_CHANNELS + 2]>>,
    playing: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    cursor: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return;
    };
    let Ok(def_cfg) = device.default_output_config() else {
        return;
    };
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(MT_SAMPLE_RATE as u32),
        buffer_size: cpal::BufferSize::Default,
    };
    let _ = def_cfg;
    let mut eng = MixerEngineState::new();
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            eng.render_stereo_block(data, &tracks, &snapshot, &meters, &playing, &looping, &cursor, &total);
        },
        |_| {},
        None,
    );
    if let Ok(stream) = stream {
        let _ = stream.play();
        loop {
            thread::sleep(Duration::from_millis(200));
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn run_playback_loop(
    _tracks: Arc<Mutex<Vec<LoadedTrack>>>,
    _snapshot: Arc<Mutex<ConsoleDspSnapshot>>,
    _meters: Arc<Mutex<[f32; MT_MAX_CHANNELS + 2]>>,
    _playing: Arc<AtomicBool>,
    _looping: Arc<AtomicBool>,
    _cursor: Arc<AtomicUsize>,
    _total: Arc<AtomicUsize>,
) {
}
