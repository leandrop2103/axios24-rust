use crate::dsp::norm_x_to_freq;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const FFT_SIZE: usize = 2048;
pub const SAMPLE_RATE: u32 = 48000;

/// Real-time FFT Spectrum Analysis result from live USB / Microphone audio input
#[derive(Debug, Clone)]
pub struct RtaSnapshot {
    pub bins_db: [f32; 64],
    pub peak_freq_hz: f32,
    pub peak_db: f32,
    pub input_rms_db: f32,
}

impl Default for RtaSnapshot {
    fn default() -> Self {
        Self {
            bins_db: [-85.0; 64],
            peak_freq_hz: 1000.0,
            peak_db: -85.0,
            input_rms_db: -90.0,
        }
    }
}

/// Computes an in-place Radix-2 Cooley-Tukey FFT on `re` and `im` slices of length `N` (power of 2)
pub fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    assert!(n.is_power_of_two() && n == im.len());

    // Bit-reversal permutation
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while (j & bit) != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Butterfly stages
    let mut len = 2usize;
    while len <= n {
        let half = len >> 1;
        let ang = -2.0 * PI / (len as f32);
        let wlen_re = ang.cos();
        let wlen_im = ang.sin();

        for i in (0..n).step_by(len) {
            let mut w_re = 1.0_f32;
            let mut w_im = 0.0_f32;
            for k in 0..half {
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + half] * w_re - im[i + k + half] * w_im;
                let v_im = re[i + k + half] * w_im + im[i + k + half] * w_re;

                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;

                let next_w_re = w_re * wlen_re - w_im * wlen_im;
                w_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_w_re;
            }
        }
        len <<= 1;
    }
}

/// Converts a 2048-sample mono PCM frame (-1.0 .. +1.0) at 48 kHz into a 64-band logarithmic RTA + exact dominant frequency
pub fn compute_rta_from_pcm(samples: &[f32], sample_rate: u32) -> RtaSnapshot {
    let n = FFT_SIZE;
    let mut re = vec![0.0_f32; n];
    let mut im = vec![0.0_f32; n];

    let mut sum_sq = 0.0_f32;
    for i in 0..n {
        let s = samples.get(i).copied().unwrap_or(0.0);
        sum_sq += s * s;
        // Hann window for low spectral leakage
        let w = 0.5 * (1.0 - (2.0 * PI * (i as f32) / ((n - 1) as f32)).cos());
        re[i] = s * w;
    }

    let rms = (sum_sq / (n as f32)).sqrt();
    let input_rms_db = if rms > 1e-6 {
        (20.0 * rms.log10()).clamp(-90.0, 6.0)
    } else {
        -90.0
    };

    fft_in_place(&mut re, &mut im);

    let half = n / 2;
    let bin_hz = (sample_rate as f32) / (n as f32);
    let norm_factor = 2.0 / (half as f32);

    // Find exact peak frequency using parabolic interpolation around the loudest FFT bin (60 Hz .. 16 kHz)
    let min_peak_bin = ((60.0 / bin_hz).round() as usize).max(2);
    let max_peak_bin = ((16000.0 / bin_hz).round() as usize).min(half - 2);
    let mut best_k = min_peak_bin;
    let mut best_mag = 0.0_f32;

    let mut mags = vec![0.0_f32; half];
    for k in 1..half {
        let m = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm_factor;
        mags[k] = m;
        if k >= min_peak_bin && k <= max_peak_bin && m > best_mag {
            best_mag = m;
            best_k = k;
        }
    }

    // Parabolic interpolation for sub-bin frequency accuracy
    let m_left = mags[best_k - 1].max(1e-9).ln();
    let m_center = mags[best_k].max(1e-9).ln();
    let m_right = mags[best_k + 1].max(1e-9).ln();
    let denom = 2.0 * (2.0 * m_center - m_left - m_right);
    let delta = if denom.abs() > 1e-6 {
        ((m_right - m_left) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let peak_freq_hz = (((best_k as f32) + delta) * bin_hz).clamp(20.0, 20000.0);
    let peak_db = if best_mag > 1e-6 {
        (20.0 * best_mag.log10()).clamp(-85.0, 6.0)
    } else {
        -85.0
    };

    // Map FFT bins into 64 logarithmic bands (20 Hz .. 20 kHz)
    let mut bins_db = [-85.0_f32; 64];
    for b in 0..64 {
        let nx0 = ((b as f32) - 0.5).max(0.0) / 63.0;
        let nx1 = ((b as f32) + 0.5).min(63.0) / 63.0;
        let f0 = norm_x_to_freq(nx0);
        let f1 = norm_x_to_freq(nx1);

        let k0 = ((f0 / bin_hz).floor() as usize).clamp(1, half - 1);
        let k1 = ((f1 / bin_hz).ceil() as usize).clamp(k0, half - 1);

        let mut band_peak = 0.0_f32;
        for k in k0..=k1 {
            if mags[k] > band_peak {
                band_peak = mags[k];
            }
        }
        bins_db[b] = if band_peak > 1e-6 {
            (20.0 * band_peak.log10()).clamp(-85.0, 6.0)
        } else {
            -85.0
        };
    }

    RtaSnapshot {
        bins_db,
        peak_freq_hz,
        peak_db,
        input_rms_db,
    }
}

// =============================================================================
// WINDOWS NATIVE AUDIO INPUT CAPTURE (WinMM waveIn — Zero External Dependencies)
// =============================================================================
#[cfg(target_os = "windows")]
mod winmm_capture {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;

    const WAVE_FORMAT_PCM: u16 = 1;
    const CALLBACK_NULL: u32 = 0;
    const WHDR_DONE: u32 = 0x00000001;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WaveInCapsW {
        w_mid: u16,
        w_pid: u16,
        v_driver_version: u32,
        sz_pname: [u16; 32],
        dw_formats: u32,
        w_channels: u16,
        w_reserved1: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
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
        lp_data: *mut i8,
        dw_buffer_length: u32,
        dw_bytes_recorded: u32,
        dw_user: usize,
        dw_flags: u32,
        dw_loops: u32,
        lp_next: *mut WaveHdr,
        reserved: usize,
    }

    #[link(name = "winmm")]
    extern "system" {
        fn waveInGetNumDevs() -> u32;
        fn waveInGetDevCapsW(u_device_id: usize, pwic: *mut WaveInCapsW, cbwic: u32) -> u32;
        fn waveInOpen(
            phwi: *mut *mut c_void,
            u_device_id: u32,
            pwfx: *const WaveFormatEx,
            dw_callback: usize,
            dw_instance: usize,
            fdw_open: u32,
        ) -> u32;
        fn waveInPrepareHeader(hwi: *mut c_void, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveInUnprepareHeader(hwi: *mut c_void, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveInAddBuffer(hwi: *mut c_void, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveInStart(hwi: *mut c_void) -> u32;
        fn waveInReset(hwi: *mut c_void) -> u32;
        fn waveInClose(hwi: *mut c_void) -> u32;
    }

    pub fn enumerate_devices() -> Vec<(u32, String)> {
        let mut list = Vec::new();
        unsafe {
            let count = waveInGetNumDevs();
            for id in 0..count {
                let mut caps = WaveInCapsW {
                    w_mid: 0,
                    w_pid: 0,
                    v_driver_version: 0,
                    sz_pname: [0; 32],
                    dw_formats: 0,
                    w_channels: 0,
                    w_reserved1: 0,
                };
                if waveInGetDevCapsW(
                    id as usize,
                    &mut caps,
                    size_of::<WaveInCapsW>() as u32,
                ) == 0
                {
                    let len = caps
                        .sz_pname
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(caps.sz_pname.len());
                    let name = String::from_utf16_lossy(&caps.sz_pname[..len]);
                    list.push((id, name));
                }
            }
        }
        list
    }

    pub fn run_capture_loop(
        device_id: u32,
        stop_flag: Arc<Mutex<bool>>,
        shared_snap: Arc<Mutex<RtaSnapshot>>,
    ) {
        unsafe {
            let wfx = WaveFormatEx {
                w_format_tag: WAVE_FORMAT_PCM,
                n_channels: 1,
                n_samples_per_sec: SAMPLE_RATE,
                n_avg_bytes_per_sec: SAMPLE_RATE * 2,
                n_block_align: 2,
                w_bits_per_sample: 16,
                cb_size: 0,
            };

            let mut hwi: *mut c_void = null_mut();
            if waveInOpen(&mut hwi, device_id, &wfx, 0, 0, CALLBACK_NULL) != 0 || hwi.is_null() {
                return;
            }

            const NUM_BUFS: usize = 3;
            let mut pcm_buffers: Vec<Vec<i16>> =
                (0..NUM_BUFS).map(|_| vec![0i16; FFT_SIZE]).collect();
            let mut headers: Vec<WaveHdr> = Vec::with_capacity(NUM_BUFS);

            for b in 0..NUM_BUFS {
                let mut hdr = WaveHdr {
                    lp_data: pcm_buffers[b].as_mut_ptr() as *mut i8,
                    dw_buffer_length: (FFT_SIZE * 2) as u32,
                    dw_bytes_recorded: 0,
                    dw_user: 0,
                    dw_flags: 0,
                    dw_loops: 0,
                    lp_next: null_mut(),
                    reserved: 0,
                };
                let _ = waveInPrepareHeader(hwi, &mut hdr, size_of::<WaveHdr>() as u32);
                headers.push(hdr);
            }

            for hdr in headers.iter_mut() {
                let _ = waveInAddBuffer(hwi, hdr, size_of::<WaveHdr>() as u32);
            }

            let _ = waveInStart(hwi);
            let mut float_buf = vec![0.0_f32; FFT_SIZE];

            loop {
                if *stop_flag.lock().unwrap() {
                    break;
                }
                for b in 0..NUM_BUFS {
                    if (headers[b].dw_flags & WHDR_DONE) != 0 {
                        let recorded_samples =
                            (headers[b].dw_bytes_recorded as usize / 2).min(FFT_SIZE);
                        if recorded_samples >= 512 {
                            for i in 0..recorded_samples {
                                float_buf[i] = (pcm_buffers[b][i] as f32) / 32768.0;
                            }
                            for i in recorded_samples..FFT_SIZE {
                                float_buf[i] = 0.0;
                            }
                            let snap = compute_rta_from_pcm(&float_buf, SAMPLE_RATE);
                            if let Ok(mut guard) = shared_snap.lock() {
                                *guard = snap;
                            }
                        }
                        headers[b].dw_bytes_recorded = 0;
                        headers[b].dw_flags &= !WHDR_DONE;
                        let _ = waveInAddBuffer(
                            hwi,
                            &mut headers[b],
                            size_of::<WaveHdr>() as u32,
                        );
                    }
                }
                thread::sleep(Duration::from_millis(15));
            }

            let _ = waveInReset(hwi);
            for hdr in headers.iter_mut() {
                let _ = waveInUnprepareHeader(hwi, hdr, size_of::<WaveHdr>() as u32);
            }
            let _ = waveInClose(hwi);
        }
    }
}

pub fn list_audio_input_devices() -> Vec<(u32, String)> {
    #[cfg(target_os = "windows")]
    {
        winmm_capture::enumerate_devices()
    }
    #[cfg(target_os = "linux")]
    {
        linux_capture::enumerate_devices()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux_capture {
    use super::{compute_rta_from_pcm, RtaSnapshot, FFT_SIZE};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    pub fn enumerate_devices() -> Vec<(u32, String)> {
        let host = cpal::default_host();
        let Ok(devs) = host.input_devices() else {
            return Vec::new();
        };
        devs.enumerate()
            .map(|(i, d)| (i as u32, d.name().unwrap_or_else(|_| format!("Input {}", i + 1))))
            .collect()
    }

    pub fn run_capture_loop(
        device_id: u32,
        stop_flag: Arc<Mutex<bool>>,
        shared_snap: Arc<Mutex<RtaSnapshot>>,
    ) {
        let host = cpal::default_host();
        let dev = host
            .input_devices()
            .ok()
            .and_then(|mut ds| ds.nth(device_id as usize))
            .or_else(|| host.default_input_device());
        let Some(device) = dev else {
            return;
        };
        let Ok(cfg) = device.default_input_config() else {
            return;
        };
        let sr = cfg.sample_rate().0;
        let ch = cfg.channels().max(1) as usize;
        let ring = Arc::new(Mutex::new(Vec::<f32>::with_capacity(FFT_SIZE * 2)));
        let ring_cb = Arc::clone(&ring);
        let snap_cb = Arc::clone(&shared_snap);

        let stream = device.build_input_stream(
            &cfg.into(),
            move |data: &[f32], _| {
                if let Ok(mut buf) = ring_cb.lock() {
                    for frame in data.chunks(ch) {
                        buf.push(frame[0]);
                    }
                    if buf.len() >= FFT_SIZE {
                        let start = buf.len() - FFT_SIZE;
                        let snap = compute_rta_from_pcm(&buf[start..], sr);
                        if let Ok(mut g) = snap_cb.lock() {
                            *g = snap;
                        }
                        buf.drain(..start);
                    }
                }
            },
            |_| {},
            None,
        );
        if let Ok(stream) = stream {
            let _ = stream.play();
            while !*stop_flag.lock().unwrap() {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub struct LiveAudioRtaEngine {
    pub active: bool,
    pub selected_device_id: u32,
    pub available_devices: Vec<(u32, String)>,
    stop_flag: Option<Arc<Mutex<bool>>>,
    shared_snap: Arc<Mutex<RtaSnapshot>>,
}

impl Default for LiveAudioRtaEngine {
    fn default() -> Self {
        let available_devices = list_audio_input_devices();
        let selected_device_id = available_devices.first().map(|d| d.0).unwrap_or(0);
        Self {
            active: false,
            selected_device_id,
            available_devices,
            stop_flag: None,
            shared_snap: Arc::new(Mutex::new(RtaSnapshot::default())),
        }
    }
}

impl LiveAudioRtaEngine {
    pub fn refresh_devices(&mut self) {
        self.available_devices = list_audio_input_devices();
        if !self
            .available_devices
            .iter()
            .any(|(id, _)| *id == self.selected_device_id)
        {
            self.selected_device_id = self.available_devices.first().map(|d| d.0).unwrap_or(0);
        }
    }

    pub fn start(&mut self, device_id: u32) {
        self.stop();
        self.selected_device_id = device_id;
        let stop_flag = Arc::new(Mutex::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let snap_clone = Arc::clone(&self.shared_snap);
        self.stop_flag = Some(stop_flag);
        self.active = true;

        #[cfg(target_os = "windows")]
        thread::spawn(move || {
            winmm_capture::run_capture_loop(device_id, stop_clone, snap_clone);
        });
        #[cfg(target_os = "linux")]
        thread::spawn(move || {
            linux_capture::run_capture_loop(device_id, stop_clone, snap_clone);
        });
    }

    pub fn stop(&mut self) {
        if let Some(flag) = self.stop_flag.take() {
            if let Ok(mut g) = flag.lock() {
                *g = true;
            }
        }
        self.active = false;
    }

    pub fn latest_snapshot(&self) -> RtaSnapshot {
        self.shared_snap
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}
