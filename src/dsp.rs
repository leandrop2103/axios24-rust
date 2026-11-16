use std::f64::consts::PI;

pub const SAMPLE_RATE: f64 = 48000.0;

/// 31 ISO 1/3-octave center frequencies for the 31-Band Graphic EQ (GEQ)
pub const GEQ_FREQS: [f32; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0,
    500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0,
    8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

pub const GEQ_LABELS: [&str; 31] = [
    "20", "25", "31.5", "40", "50", "63", "80", "100", "125", "160", "200", "250", "315", "400",
    "500", "630", "800", "1k", "1.25k", "1.6k", "2k", "2.5k", "3.15k", "4k", "5k", "6.3k", "8k",
    "10k", "12.5k", "16k", "20k",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterFamily {
    Butterworth = 0,
    Bessel = 1,
    LinkwitzRiley = 2,
}

impl FilterFamily {
    pub fn label(self) -> &'static str {
        match self {
            FilterFamily::Butterworth => "Butterworth",
            FilterFamily::Bessel => "Bessel",
            FilterFamily::LinkwitzRiley => "Linkwitz-Riley",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            FilterFamily::Butterworth => "BW",
            FilterFamily::Bessel => "BS",
            FilterFamily::LinkwitzRiley => "LR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterSlope {
    Slope12dB = 0,
    Slope18dB = 1,
    Slope24dB = 2,
    Slope30dB = 3,
    Slope36dB = 4,
    Slope42dB = 5,
    Slope48dB = 6,
}

impl FilterSlope {
    pub fn label(self) -> &'static str {
        match self {
            FilterSlope::Slope12dB => "12dB",
            FilterSlope::Slope18dB => "18dB",
            FilterSlope::Slope24dB => "24dB",
            FilterSlope::Slope30dB => "30dB",
            FilterSlope::Slope36dB => "36dB",
            FilterSlope::Slope42dB => "42dB",
            FilterSlope::Slope48dB => "48dB",
        }
    }

    pub fn order(self) -> usize {
        match self {
            FilterSlope::Slope12dB => 2,
            FilterSlope::Slope18dB => 3,
            FilterSlope::Slope24dB => 4,
            FilterSlope::Slope30dB => 5,
            FilterSlope::Slope36dB => 6,
            FilterSlope::Slope42dB => 7,
            FilterSlope::Slope48dB => 8,
        }
    }
}

/// Exact HPF/LPF hardware filter ID lookup table from 0x525214 in axios24new.exe
pub const HPF_FILTER_IDS: [[u8; 3]; 7] = [
    [8, 10, 12],   // 12dB: BW, BS, LR
    [14, 16, 18],  // 18dB
    [20, 22, 24],  // 24dB
    [46, 48, 50],  // 30dB
    [52, 54, 56],  // 36dB
    [58, 60, 62],  // 42dB
    [26, 28, 30],  // 48dB
];

pub const LPF_FILTER_IDS: [[u8; 3]; 7] = [
    [9, 11, 13],   // 12dB: BW, BS, LR
    [15, 17, 19],  // 18dB
    [21, 23, 25],  // 24dB
    [47, 49, 51],  // 30dB
    [53, 55, 57],  // 36dB
    [59, 61, 63],  // 42dB
    [27, 29, 31],  // 48dB
];

pub fn decode_hpf_lpf_id(raw_id: u16, is_lpf: bool) -> (FilterFamily, FilterSlope) {
    let table = if is_lpf { &LPF_FILTER_IDS } else { &HPF_FILTER_IDS };
    let id = (raw_id & 0xff) as u8;
    for (slope_idx, row) in table.iter().enumerate() {
        for (fam_idx, &cell) in row.iter().enumerate() {
            if cell == id {
                let fam = match fam_idx {
                    0 => FilterFamily::Butterworth,
                    1 => FilterFamily::Bessel,
                    _ => FilterFamily::LinkwitzRiley,
                };
                let slope = match slope_idx {
                    0 => FilterSlope::Slope12dB,
                    1 => FilterSlope::Slope18dB,
                    2 => FilterSlope::Slope24dB,
                    3 => FilterSlope::Slope30dB,
                    4 => FilterSlope::Slope36dB,
                    5 => FilterSlope::Slope42dB,
                    _ => FilterSlope::Slope48dB,
                };
                return (fam, slope);
            }
        }
    }
    (FilterFamily::Butterworth, FilterSlope::Slope24dB)
}

pub fn encode_hpf_lpf_id(family: FilterFamily, slope: FilterSlope, is_lpf: bool) -> u16 {
    let table = if is_lpf { &LPF_FILTER_IDS } else { &HPF_FILTER_IDS };
    table[slope as usize][family as usize] as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PeqBandMode {
    Peaking = 0,
    LowShelf = 1,
    HighShelf = 2,
}

impl PeqBandMode {
    pub fn from_reg(val: u16) -> Self {
        // In axios24new.exe: 7 = Peaking, 6 = LowShelf, 5 = HighShelf (or 0/1/2 in UI widget)
        match val & 0xff {
            6 | 1 => PeqBandMode::LowShelf,
            5 | 2 => PeqBandMode::HighShelf,
            _ => PeqBandMode::Peaking,
        }
    }

    pub fn to_reg(self) -> u16 {
        match self {
            PeqBandMode::Peaking => 7,
            PeqBandMode::LowShelf => 6,
            PeqBandMode::HighShelf => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PeqBandMode::Peaking => "PEQ",
            PeqBandMode::LowShelf => "L-SHELF",
            PeqBandMode::HighShelf => "H-SHELF",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    /// Evaluates 20 * log10(|H(e^{jw})|) at frequency `freq` Hz using the exact formula from fcn.004072e0
    #[inline]
    pub fn eval_db(&self, freq: f64) -> f64 {
        let w = 2.0 * PI * freq / SAMPLE_RATE;
        let sin_w = w.sin();
        let cos_w = w.cos();
        // cos(2w) = cos^2(w) - sin^2(w), sin(2w) = 2*sin(w)*cos(w)
        let cos_2w = cos_w * cos_w - sin_w * sin_w;
        let sin_2w = 2.0 * sin_w * cos_w;

        let num_re = self.b0 * cos_2w + self.b1 * cos_w + self.b2;
        let num_im = self.b0 * sin_2w + self.b1 * sin_w;
        let den_re = cos_2w + self.a1 * cos_w + self.a2;
        let den_im = sin_2w + self.a1 * sin_w;

        let den_mag_sq = den_re * den_re + den_im * den_im;
        if den_mag_sq <= 1e-30 {
            return 0.0;
        }
        let re = (den_re * num_re + den_im * num_im) / den_mag_sq;
        let im = (den_re * num_im - den_im * num_re) / den_mag_sq;
        let mag = (re * re + im * im).sqrt().max(1e-9);
        20.0 * mag.log10()
    }
}

/// Computes the biquad coefficient stages for a single PEQ band (Peaking / LowShelf / HighShelf)
/// Exact port of fcn.004072e0 (band_type == 2)
pub fn peq_band_biquad(mode: PeqBandMode, freq: f32, gain_db: f32, q: f32) -> BiquadCoeffs {
    let f = (freq as f64).clamp(19.6, 20600.0);
    let g = (gain_db as f64).clamp(-15.0, 15.0);
    let q_val = (q as f64).clamp(0.4, 30.0);

    let w0 = 2.0 * PI * f / SAMPLE_RATE;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();

    match mode {
        PeqBandMode::Peaking => {
            let a = 10.0_f64.powf(g / 40.0);
            let alpha = sin_w0 / (2.0 * q_val);
            let a0 = 1.0 + alpha / a;
            let a1 = (-2.0 * cos_w0) / a0;
            let a2 = (1.0 - alpha / a) / a0;
            let b0 = (1.0 + alpha * a) / a0;
            let b1 = (-2.0 * cos_w0) / a0;
            let b2 = (1.0 - alpha * a) / a0;
            BiquadCoeffs { b0, b1, b2, a1, a2 }
        }
        PeqBandMode::LowShelf => {
            let a = 10.0_f64.powf(g / 40.0);
            let sqrt_a = a.sqrt();
            let alpha = sin_w0 * 0.7071067811865475; // sin(w0)/2 * sqrt(2) as in fcn.004072e0
            let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
            let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            let a1 = (-2.0 * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
            let a2 = ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
            let b0 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha)) / a0;
            let b1 = (2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
            let b2 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha)) / a0;
            BiquadCoeffs { b0, b1, b2, a1, a2 }
        }
        PeqBandMode::HighShelf => {
            let a = 10.0_f64.powf(g / 40.0);
            let sqrt_a = a.sqrt();
            let alpha = sin_w0 * 0.7071067811865475;
            let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
            let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            let a1 = (2.0 * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
            let a2 = ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
            let b0 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha)) / a0;
            let b1 = (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
            let b2 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha)) / a0;
            BiquadCoeffs { b0, b1, b2, a1, a2 }
        }
    }
}

fn second_order_hpf_lpf(freq: f64, q: f64, is_lpf: bool) -> BiquadCoeffs {
    let w0 = 2.0 * PI * freq / SAMPLE_RATE;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = (sin_w0 * 0.5) / q;
    let a0 = 1.0 + alpha;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    if is_lpf {
        let b1 = (1.0 - cos_w0) / a0;
        let b0 = b1 * 0.5;
        BiquadCoeffs { b0, b1, b2: b0, a1, a2 }
    } else {
        let b0 = ((1.0 + cos_w0) * 0.5) / a0;
        let b1 = -(1.0 + cos_w0) / a0;
        BiquadCoeffs { b0, b1, b2: b0, a1, a2 }
    }
}

fn first_order_hpf_lpf(freq: f64, is_lpf: bool) -> BiquadCoeffs {
    let w0 = 2.0 * PI * freq / SAMPLE_RATE;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let k = cos_w0 / (sin_w0 + 1.0);
    let a1 = -k;
    if is_lpf {
        let b0 = (1.0 - k) * 0.5;
        BiquadCoeffs { b0, b1: b0, b2: 0.0, a1, a2: 0.0 }
    } else {
        let b0 = (1.0 + k) * 0.5;
        BiquadCoeffs { b0, b1: -b0, b2: 0.0, a1, a2: 0.0 }
    }
}

/// Computes the exact cascade of biquads for HPF or LPF from fcn.004072e0
pub fn hpf_lpf_stages(
    freq: f32,
    family: FilterFamily,
    slope: FilterSlope,
    is_lpf: bool,
) -> Vec<BiquadCoeffs> {
    if !is_lpf && freq <= 19.65 {
        return Vec::new();
    }
    if is_lpf && freq >= 20000.0 {
        return Vec::new();
    }
    let f = (freq as f64).clamp(19.6, 20600.0);
    let mut stages = Vec::with_capacity(4);

    // Exact Q-factors per stage from fcn.004072e0 in axios24new.exe
    match (family, slope) {
        (FilterFamily::Butterworth, FilterSlope::Slope12dB) => {
            stages.push(second_order_hpf_lpf(f, 0.70710678, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope12dB) => {
            stages.push(second_order_hpf_lpf(f, 0.57735027, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope12dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope18dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.0, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope18dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.6910466, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope18dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.70710678, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope24dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5411961, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.3065630, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope24dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5219346, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.8055383, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope24dB) => {
            stages.push(second_order_hpf_lpf(f, 0.70710678, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.70710678, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope30dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.6180340, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.6180340, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope30dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5635356, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.9164774, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope30dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5411961, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.3065630, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope36dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5176381, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.70710678, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.9318517, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope36dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5103178, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.6111945, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.0233140, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope36dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.0, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.0, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope42dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 2.2469796, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.8019377, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5549581, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope42dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5773505, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5773505, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5773505, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope42dB) => {
            stages.push(first_order_hpf_lpf(f, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.0, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.3065645, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5411964, is_lpf));
        }
        (FilterFamily::Butterworth, FilterSlope::Slope48dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5097956, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.6013449, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.8999762, is_lpf));
            stages.push(second_order_hpf_lpf(f, 2.5629154, is_lpf));
        }
        (FilterFamily::Bessel, FilterSlope::Slope48dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5059911, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5596092, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.7108521, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.2256694, is_lpf));
        }
        (FilterFamily::LinkwitzRiley, FilterSlope::Slope48dB) => {
            stages.push(second_order_hpf_lpf(f, 0.5411961, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.3065630, is_lpf));
            stages.push(second_order_hpf_lpf(f, 0.5411961, is_lpf));
            stages.push(second_order_hpf_lpf(f, 1.3065630, is_lpf));
        }
    }
    stages
}

pub fn eval_stages_db(stages: &[BiquadCoeffs], freq: f64) -> f32 {
    let mut sum = 0.0;
    for s in stages {
        sum += s.eval_db(freq);
    }
    sum as f32
}

/// Converts frequency (20 Hz .. 20 kHz) to normalized X coordinate [0.0, 1.0] (logarithmic)
#[inline]
pub fn freq_to_norm_x(freq: f32) -> f32 {
    let f = freq.clamp(20.0, 20000.0);
    ((f / 20.0).log10() / 3.0).clamp(0.0, 1.0)
}

/// Converts normalized X coordinate [0.0, 1.0] to frequency (20 Hz .. 20 kHz)
#[inline]
pub fn norm_x_to_freq(norm_x: f32) -> f32 {
    let nx = norm_x.clamp(0.0, 1.0);
    20.0 * 10.0_f32.powf(nx * 3.0)
}

/// Computes Noise Gate output level (in dB, -80..0) for an input level `in_db` (-80..0)
#[inline]
pub fn gate_transfer_db(in_db: f32, threshold_db: f32, enabled: bool) -> f32 {
    if !enabled {
        return in_db;
    }
    if in_db >= threshold_db {
        in_db
    } else {
        // Steep downward expansion below threshold as in fcn.00419dc0
        (threshold_db + (in_db - threshold_db) * 8.0).max(-80.0)
    }
}

/// Computes Compressor output level (in dB, -80..0) for an input level `in_db` (-80..0)
/// Exact port of fcn.00419dc0
#[inline]
pub fn comp_transfer_db(
    in_db: f32,
    threshold_db: f32,
    ratio: f32,
    makeup_gain_db: f32,
    enabled: bool,
) -> f32 {
    if !enabled {
        return in_db;
    }
    let r = ratio.max(1.0);
    let compressed = if in_db <= threshold_db {
        in_db
    } else {
        threshold_db + (in_db - threshold_db) / r
    };
    (compressed + makeup_gain_db).clamp(-80.0, 0.0)
}
