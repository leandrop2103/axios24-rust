use crate::default_scene::{DEFAULT_REGS, NUM_REGS};
use crate::dsp::{
    decode_hpf_lpf_id, encode_hpf_lpf_id, FilterFamily, FilterSlope, PeqBandMode,
};
use crate::protocol::{
    build_demo_cmd_frame, build_kv_frame, build_media_source_frame, build_scene_op_frame,
    decode_official_scn, decode_scene_block, encode_official_scn, load_or_create_preset_bin,
    probe_udp_devices, save_preset_bin, DiscoveredDevice, FirmwareLayout, HardwareLink, NetEvent,
};
use std::fs;
use std::path::PathBuf;

/// Exact register mapping function from fcn.00455d70 in axios24new.exe
pub fn map_reg(group: u8, ch: usize, param: usize) -> usize {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewMode {
    Normal,
    Analog,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Normal => "Modo Normal",
            ViewMode::Analog => "Modo Analógico",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FaderLayer {
    Ch1_8,
    Ch9_16,
    Ch17_24,
    DigiFx,
    Aux1_8,
    Dca1_4,
    AllInputs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TopNavTab {
    Mixer,
    EditChannel,
    PeqGeq,
    Effects,
    MediaRecord,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SettingsSubTab {
    Scene,
    CopyPaste,
    MuteGroup,
    DcaGroup,
    PhantomGroup,
    Record,
    PhonesMonitor,
    MoreMeMatrix,
    SignalGen,
    Network,
    Midi,
    Shortcuts,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectedChannel {
    /// Group 1: 0..23 = CH1..24, 24 = DIGI (Stereo In / USB)
    Input(usize),
    /// Group 2: 0 = FX1, 1 = FX2
    Fx(usize),
    /// Group 3: 0..7 = AUX1..8, 8 = MAIN L, 9 = MAIN R
    Output(usize),
    /// Group 4: 0..3 = DCA1..4
    Dca(usize),
}

impl SelectedChannel {
    pub fn group_and_index(self) -> (u8, usize) {
        match self {
            SelectedChannel::Input(i) => (1, i),
            SelectedChannel::Fx(i) => (2, i),
            SelectedChannel::Output(i) => (3, i),
            SelectedChannel::Dca(i) => (4, i),
        }
    }

    pub fn flat_id(self) -> usize {
        // Matches fcn.00453000 / fcn.004528f0: 0..25 Input, 26..27 FX, 28..37 Output, 38..41 DCA
        match self {
            SelectedChannel::Input(i) => i.min(25),
            SelectedChannel::Fx(i) => 26 + i.min(1),
            SelectedChannel::Output(i) => 28 + i.min(9),
            SelectedChannel::Dca(i) => 38 + i.min(3),
        }
    }

    pub fn from_flat_id(id: usize) -> Self {
        match id {
            0..=25 => SelectedChannel::Input(id),
            26..=27 => SelectedChannel::Fx(id - 26),
            28..=37 => SelectedChannel::Output(id - 28),
            _ => SelectedChannel::Dca((id.saturating_sub(38)).min(3)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PeqBandData {
    pub mode: PeqBandMode,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct FxAlgoSpec {
    pub id_1based: u16,
    pub name: &'static str,
    pub param1_name: &'static str,
    pub param2_name: &'static str,
    pub param1_max: u16,
}

/// Exact 32 FX algorithm definitions from fcn.00453da0, fcn.00454490, fcn.00454710 & 0x525478
/// Slots 1..16 are for FX1, Slots 17..32 are for FX2
pub const FX_ALGORITHMS: [FxAlgoSpec; 32] = [
    // FX1 (1..16)
    FxAlgoSpec { id_1based: 1, name: "REVERB HALL", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 2, name: "REVERB ROOM", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 3, name: "REVERB PLATE", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 4, name: "REVERB VOCAL1", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 5, name: "REVERB VOCAL2", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 6, name: "VOCAL ECHO1", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 68 },
    FxAlgoSpec { id_1based: 7, name: "VOCAL ECHO2", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 68 },
    FxAlgoSpec { id_1based: 8, name: "DELAY1", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
    FxAlgoSpec { id_1based: 9, name: "DELAY2", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
    FxAlgoSpec { id_1based: 10, name: "MOD.DELAY", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
    FxAlgoSpec { id_1based: 11, name: "REVERB GATE", param1_name: "Echo Dly", param2_name: "Echo Dly", param1_max: 100 },
    FxAlgoSpec { id_1based: 12, name: "PITCH CHANGE", param1_name: "Slope", param2_name: "Depth", param1_max: 63 },
    FxAlgoSpec { id_1based: 13, name: "CHORUS", param1_name: "Freq", param2_name: "Depth", param1_max: 42 },
    FxAlgoSpec { id_1based: 14, name: "PHASER", param1_name: "Freq", param2_name: "Depth", param1_max: 63 },
    FxAlgoSpec { id_1based: 15, name: "FLANGE", param1_name: "Freq", param2_name: "Depth", param1_max: 63 },
    FxAlgoSpec { id_1based: 16, name: "TREMOLO", param1_name: "Freq", param2_name: "Depth", param1_max: 127 },
    // FX2 (17..32)
    FxAlgoSpec { id_1based: 17, name: "REVERB HALL", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 18, name: "REVERB ROOM", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 19, name: "REVERB PLATE", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 20, name: "REVERB VOCAL1", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 21, name: "REVERB VOCAL2", param1_name: "Rvb PreDly", param2_name: "Rvb Durat.", param1_max: 30 },
    FxAlgoSpec { id_1based: 22, name: "VOCAL ECHO1", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 68 },
    FxAlgoSpec { id_1based: 23, name: "VOCAL ECHO2", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 68 },
    FxAlgoSpec { id_1based: 24, name: "DELAY1", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
    FxAlgoSpec { id_1based: 25, name: "DELAY2", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
    FxAlgoSpec { id_1based: 26, name: "EARLY REF.", param1_name: "Rvb Dly", param2_name: "Echo Repeat", param1_max: 100 },
    FxAlgoSpec { id_1based: 27, name: "GATE REVERB", param1_name: "Rvb Dly", param2_name: "Echo Repeat", param1_max: 100 },
    FxAlgoSpec { id_1based: 28, name: "VOCAL DOUBLER", param1_name: "Freq", param2_name: "Rvb Ratio", param1_max: 42 },
    FxAlgoSpec { id_1based: 29, name: "FLANGE", param1_name: "Freq", param2_name: "Rvb Ratio", param1_max: 63 },
    FxAlgoSpec { id_1based: 30, name: "SYMPHONIC", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 42 },
    FxAlgoSpec { id_1based: 31, name: "DISTORTION", param1_name: "Freq", param2_name: "Depth", param1_max: 63 },
    FxAlgoSpec { id_1based: 32, name: "TAP DELAY", param1_name: "Echo Dly", param2_name: "Echo Repeat", param1_max: 135 },
];

pub fn format_fx_param1(algo_1based: u16, raw: u16) -> String {
    match algo_1based {
        1..=5 | 17..=21 => format!("{} ms", raw),
        11 | 26 | 27 => format!("{}.{} s", raw / 10, raw % 10),
        12 => format!("{}", raw),
        13..=16 | 28 | 29 | 31 => format!("{} Hz", raw),
        _ => format!("{} ms", raw * 5),
    }
}

pub fn format_fx_param2(algo_1based: u16, raw: u16) -> String {
    match algo_1based {
        1..=5 | 17..=21 => format!("{}.{} s", raw / 10, raw % 10),
        _ => format!("{} %", raw),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MediaSource {
    UsbMusic = 0,
    Bluetooth = 1,
    Optical = 2,
    Coaxial = 3,
}

impl MediaSource {
    pub fn label(self) -> &'static str {
        match self {
            MediaSource::UsbMusic => "USB Music",
            MediaSource::Bluetooth => "Bluetooth",
            MediaSource::Optical => "Optical",
            MediaSource::Coaxial => "Coaxial",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RepeatMode {
    RepeatAll,
    RepeatOne,
    Random,
}

impl RepeatMode {
    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::RepeatAll => "Repetir Tudo",
            RepeatMode::RepeatOne => "Repetir Uma",
            RepeatMode::Random => "Aleatório",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendsOnFaderTarget {
    MainLR,
    Aux(usize), // 0..8 -> AUX 1..8
    Fx(usize),  // 0..2 -> FX 1..2
}

impl SendsOnFaderTarget {
    pub fn label(&self) -> String {
        match self {
            SendsOnFaderTarget::MainLR => "MAIN L/R".to_string(),
            SendsOnFaderTarget::Aux(i) => format!("AUX {}", i + 1),
            SendsOnFaderTarget::Fx(i) => format!("FX {}", i + 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPreset {
    VocalMasculino,
    VocalFeminino,
    LocucaoPastor,
    BumboKick,
    CaixaSnare,
    TonsSurdo,
    OverheadsPratos,
    BaixoEletrico,
    ViolaoAco,
    GuitarraEletrica,
    TecladoPiano,
    FlatReset,
}

impl ChannelPreset {
    pub const ALL: [ChannelPreset; 12] = [
        ChannelPreset::VocalMasculino,
        ChannelPreset::VocalFeminino,
        ChannelPreset::LocucaoPastor,
        ChannelPreset::BumboKick,
        ChannelPreset::CaixaSnare,
        ChannelPreset::TonsSurdo,
        ChannelPreset::OverheadsPratos,
        ChannelPreset::BaixoEletrico,
        ChannelPreset::ViolaoAco,
        ChannelPreset::GuitarraEletrica,
        ChannelPreset::TecladoPiano,
        ChannelPreset::FlatReset,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ChannelPreset::VocalMasculino => "Voz Masculina (Lead)",
            ChannelPreset::VocalFeminino => "Voz Feminina (Lead)",
            ChannelPreset::LocucaoPastor => "Locução / Pastor (Clareza)",
            ChannelPreset::BumboKick => "Bumbo (Kick Punch)",
            ChannelPreset::CaixaSnare => "Caixa (Snare Snap)",
            ChannelPreset::TonsSurdo => "Tons / Surdo",
            ChannelPreset::OverheadsPratos => "Overheads / Pratos",
            ChannelPreset::BaixoEletrico => "Contrabaixo (Bass)",
            ChannelPreset::ViolaoAco => "Violão Aço / Nylon",
            ChannelPreset::GuitarraEletrica => "Guitarra Elétrica",
            ChannelPreset::TecladoPiano => "Teclado / Piano",
            ChannelPreset::FlatReset => "Flat / Resetar Canal",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaTrack {
    pub folder: String,
    pub title: String,
    pub duration_secs: u32,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerState {
    pub source: MediaSource,
    pub usb_inserted: bool,
    pub playing: bool,
    pub repeat_mode: RepeatMode,
    pub current_track: usize,
    pub position_secs: f32,
    pub tracks: Vec<MediaTrack>,
    pub recording: bool,
    pub rec_seconds: f32,
}

impl Default for MediaPlayerState {
    fn default() -> Self {
        Self {
            source: MediaSource::UsbMusic,
            usb_inserted: true,
            playing: false,
            repeat_mode: RepeatMode::RepeatAll,
            current_track: 0,
            position_secs: 42.0,
            tracks: vec![
                MediaTrack { folder: "ROOT".into(), title: "01 - Duonn Axios 24 Demo Track.wav".into(), duration_secs: 215 },
                MediaTrack { folder: "ROOT".into(), title: "02 - Acoustic Guitar Reference.wav".into(), duration_secs: 184 },
                MediaTrack { folder: "ROOT".into(), title: "03 - Full Band Soundcheck 48kHz.wav".into(), duration_secs: 248 },
                MediaTrack { folder: "LIVE_SHOW".into(), title: "04 - Vocal & Horns Stem.wav".into(), duration_secs: 196 },
                MediaTrack { folder: "LIVE_SHOW".into(), title: "05 - Pink Noise Calibration.wav".into(), duration_secs: 120 },
            ],
            recording: false,
            rec_seconds: 0.0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkConfig {
    pub wifi_mode_sta: bool,
    pub ap_ssid: String,
    pub ap_key: String,
    pub ap_ip: String,
    pub domain: String,
    pub sta_ssid: String,
    pub sta_key: String,
    pub wifi_dhcp: bool,
    pub wifi_ip: String,
    pub wifi_netmask: String,
    pub wifi_gateway: String,
    pub wifi_dns: String,
    pub eth_dhcp: bool,
    pub eth_ip: String,
    pub eth_netmask: String,
    pub eth_gateway: String,
    pub eth_dns: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            wifi_mode_sta: false,
            ap_ssid: "AXIOS24-AP".into(),
            ap_key: "12345678".into(),
            ap_ip: "192.168.4.1".into(),
            domain: "axios24.local".into(),
            sta_ssid: "Estudio_WiFi_5G".into(),
            sta_key: "".into(),
            wifi_dhcp: true,
            wifi_ip: "192.168.1.124".into(),
            wifi_netmask: "255.255.255.0".into(),
            wifi_gateway: "192.168.1.1".into(),
            wifi_dns: "8.8.8.8".into(),
            eth_dhcp: true,
            eth_ip: "192.168.1.200".into(),
            eth_netmask: "255.255.255.0".into(),
            eth_gateway: "192.168.1.1".into(),
            eth_dns: "8.8.8.8".into(),
        }
    }
}

pub struct Axios24State {
    /// Current active scene registers (0xD55 = 3413 u16 registers)
    pub regs: [u16; NUM_REGS],
    /// All 16 scenes stored in preset.bin
    pub scenes: Vec<[u16; NUM_REGS]>,
    pub preset_bin_path: PathBuf,
    pub active_scene_idx: usize,
    pub scene_names: [String; 16],
    pub channel_names: [String; 60],
    /// Custom strip color index per channel (0..32 palette) like DS Mixer
    pub channel_colors: [u8; 60],
    /// Custom free channel link group per input channel (0 = none, 1..=4 = Group A..D; links all params except Pan!)
    pub custom_link_groups: [u8; 24],
    /// 31-band Graphic EQ gains (-12.0 .. +12.0 dB) for each of the 10 Output channels (AUX1-8, MAIN L, MAIN R)
    pub output_geq_db: [[f32; 31]; 10],
    /// Whether GEQ is active on the output channel
    pub output_geq_enabled: [bool; 10],

    // UI Navigation & Modes
    pub view_mode: ViewMode,
    pub layer: FaderLayer,
    pub top_tab: TopNavTab,
    pub settings_tab: SettingsSubTab,
    pub selected_channel: SelectedChannel,
    pub selected_eq_band: usize, // 0 = HPF, 1..=N = PEQ band, N+1 = LPF
    pub show_rta_on_eq: bool,
    pub show_band_fills: bool,
    pub show_connect_modal: bool,
    pub dark_theme: bool,
    pub renaming_channel: Option<SelectedChannel>,
    pub rename_buffer: String,
    pub console_locked: bool,
    pub lock_password_input: String,
    pub stored_password: String,
    pub current_pwd_input: String,
    pub new_pwd_input: String,
    pub confirm_pwd_input: String,
    pub scene_key_lock: bool,
    pub language_idx: usize, // 0 = PT-BR, 1 = EN, 2 = ZH
    pub device_name: String,
    pub fw_version: String,

    // Copy/Paste clipboard
    pub clipboard_channel: Option<SelectedChannel>,
    pub status_toast: Option<(String, f64)>,

    // Pro Workflow & Forum Complaint Fixes (Sends on Fader, Safe Unplug, +48V Anti-Pop, Search & Custom Views)
    pub sends_on_fader: SendsOnFaderTarget,
    pub safe_unplugged_inputs: [bool; 25],
    pub saved_fader_before_unplug: [f32; 25],
    pub phantom_safe_mute: bool,
    pub channel_search_query: String,
    pub show_search_bar: bool,
    pub hide_unused_channels: bool,
    pub show_stream_deck_macros: bool,
    pub master_dim_active: bool,
    pub saved_master_lr_before_dim: (f32, f32),

    // Real-Time Audio FFT Analyzer (WinMM USB / Microphone Input) + Demo Simulator
    pub live_rta: crate::audio_rta::LiveAudioRtaEngine,
    pub rta_peak_freq_hz: f32,
    pub rta_peak_db: f32,
    pub demo_running: bool,
    pub sim_time: f64,
    pub channel_meters_db: [f32; 60],
    pub rta_bins_db: [f32; 64],
    pub media: MediaPlayerState,
    pub net_cfg: NetworkConfig,
    pub hw_link: HardwareLink,
    pub discovered_devices: Vec<DiscoveredDevice>,
    pub discovery_scanning: bool,
    pub discovery_rx: Option<std::sync::mpsc::Receiver<Vec<DiscoveredDevice>>>,
    pub local_pc_ip: Option<String>,
    pub connect_ip: String,
    pub connect_serial_port: String,
    pub available_serial_ports: Vec<String>,

    // Soundcraft Ui24R / Armer MixHub 24 Pro Features (Tap Tempo, AFS2, MoreMe, Matrix, Virtual Soundcheck)
    pub tap_tempo_bpm: f32,
    pub tap_last_time: f64,
    pub tap_subdivision: usize,
    pub virtual_soundcheck: bool,
    pub afs2_live_active: bool,
    pub afs2_last_run: f64,
    pub more_me_channel: usize,
    pub more_me_aux: usize,
    pub matrix_sends_db: [[f32; 4]; 2],
    pub matrix_muted: [bool; 2],

    // Forum Complaint Fixes: Recall Safe, Undo History, Comp GR Meter, Network Ping & USB MIDI
    pub recall_safe_channels: [bool; 42],
    pub undo_stack: Vec<([u16; NUM_REGS], String)>,
    pub comp_gr_db: [f32; 60],
    pub last_net_rx_time: f64,
    pub net_ping_ms: u32,
    pub net_link_lost: bool,
    pub midi: crate::midi::MidiControllerEngine,

    // Pro Operator Features (Auto-Mixer, DCA Spill, Clip Hold, Soundcheck Wizard, Prefs Persistence)
    pub automix_enabled: bool,
    pub automix_channels: [bool; 24],
    pub automix_atten_db: [f32; 24],
    pub dca_spill: Option<usize>,
    pub channel_clip_hold: [bool; 60],
    pub show_soundcheck_wizard: bool,
    pub soundcheck_step: usize,
    pub personal_monitor_locked: bool,
    /// Per-channel A/B compare snapshots for EQ + Gate + Compressor (indexed by flat_id 0..42)
    pub ab_slots: Vec<Option<(Vec<(usize, u16)>, Vec<(usize, u16)>, bool)>>,
    pub multitrack: crate::multitrack::MultitrackEngine,
    pub multitrack_auto_name: bool,

    // GitHub Auto-Update
    pub update_info: Option<crate::updater::ReleaseUpdateInfo>,
    pub update_status: String,
    pub update_checking: bool,
    pub update_downloading: bool,
    pub show_update_modal: bool,
    pub update_check_rx: Option<std::sync::mpsc::Receiver<Result<Option<crate::updater::ReleaseUpdateInfo>, String>>>,
    pub update_apply_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
}

impl Axios24State {
    pub fn new() -> Self {
        let preset_bin_path = PathBuf::from("preset.bin");
        let scenes = load_or_create_preset_bin(&preset_bin_path);
        let mut regs = scenes.first().copied().unwrap_or(DEFAULT_REGS);
        // Enable MAIN L & R stereo link by default if not explicitly set
        if regs[0xBCC] == 0 {
            regs[0xBCC] |= 1 << 4;
        }

        let scene_names = std::array::from_fn(|i| format!("Scene{}", i + 1));
        let mut channel_names = std::array::from_fn(|_| String::new());
        // Default channel names matching fcn.00452f50
        for i in 0..24 {
            channel_names[i] = format!("CH{}", i + 1);
        }
        channel_names[24] = "DIGI L".to_string();
        channel_names[25] = "DIGI R".to_string();
        channel_names[26] = "FX1".to_string();
        channel_names[27] = "FX2".to_string();
        for i in 0..8 {
            channel_names[28 + i] = format!("AUX{}", i + 1);
        }
        channel_names[36] = "LEFT".to_string();
        channel_names[37] = "RIGHT".to_string();
        for i in 0..4 {
            channel_names[38 + i] = format!("DCA{}", i + 1);
        }

        let channel_colors = std::array::from_fn(|i| match i {
            0..=7 => 0,   // Cyan/Blue
            8..=15 => 1,  // Emerald/Green
            16..=23 => 2, // Amber/Orange
            24..=25 => 3, // Purple
            26..=27 => 4, // Magenta
            28..=35 => 5, // Teal
            36..=37 => 6, // Cyan Master
            _ => 7,       // Gold DCA
        });

        let ports = crate::protocol::list_serial_ports();
        let default_port = ports.first().cloned().unwrap_or_else(|| "COM8".to_string());

        let local_pc_ip = crate::protocol::detect_local_ipv4().map(|ip| ip.to_string());
        let mut state = Self {
            regs,
            scenes,
            preset_bin_path,
            active_scene_idx: 0,
            scene_names,
            channel_names,
            channel_colors,
            custom_link_groups: [0; 24],
            output_geq_db: [[0.0; 31]; 10],
            output_geq_enabled: [true; 10],
            view_mode: ViewMode::Normal,
            layer: FaderLayer::Ch1_8,
            top_tab: TopNavTab::Mixer,
            settings_tab: SettingsSubTab::Scene,
            selected_channel: SelectedChannel::Input(0),
            selected_eq_band: 1,
            show_rta_on_eq: true,
            show_band_fills: true,
            show_connect_modal: true,
            dark_theme: true,
            renaming_channel: None,
            rename_buffer: String::new(),
            console_locked: false,
            lock_password_input: String::new(),
            stored_password: "1234".to_string(),
            current_pwd_input: String::new(),
            new_pwd_input: String::new(),
            confirm_pwd_input: String::new(),
            scene_key_lock: false,
            language_idx: 0,
            device_name: "AXIOS24".to_string(),
            fw_version: format!("v{} (Rust)", crate::updater::CURRENT_VERSION),
            clipboard_channel: None,
            status_toast: None,
            sends_on_fader: SendsOnFaderTarget::MainLR,
            safe_unplugged_inputs: [false; 25],
            saved_fader_before_unplug: [0.0; 25],
            phantom_safe_mute: true,
            channel_search_query: String::new(),
            show_search_bar: false,
            hide_unused_channels: false,
            show_stream_deck_macros: false,
            master_dim_active: false,
            saved_master_lr_before_dim: (0.0, 0.0),
            live_rta: crate::audio_rta::LiveAudioRtaEngine::default(),
            rta_peak_freq_hz: 1000.0,
            rta_peak_db: -85.0,
            demo_running: true,
            sim_time: 0.0,
            channel_meters_db: [-90.0; 60],
            rta_bins_db: [-85.0; 64],
            media: MediaPlayerState::default(),
            net_cfg: NetworkConfig::default(),
            hw_link: HardwareLink::default(),
            discovered_devices: Vec::new(),
            discovery_scanning: false,
            discovery_rx: None,
            local_pc_ip,
            connect_ip: "172.14.10.1".to_string(),
            connect_serial_port: default_port,
            available_serial_ports: ports,
            tap_tempo_bpm: 120.0,
            tap_last_time: 0.0,
            tap_subdivision: 0,
            virtual_soundcheck: false,
            afs2_live_active: false,
            afs2_last_run: 0.0,
            more_me_channel: 0,
            more_me_aux: 0,
            matrix_sends_db: [[0.0, -120.0, -120.0, -120.0], [0.0, -120.0, -120.0, -120.0]],
            matrix_muted: [false, false],
            recall_safe_channels: [false; 42],
            undo_stack: Vec::new(),
            comp_gr_db: [0.0; 60],
            last_net_rx_time: 0.0,
            net_ping_ms: 0,
            net_link_lost: false,
            midi: crate::midi::MidiControllerEngine::default(),
            automix_enabled: false,
            automix_channels: [false; 24],
            automix_atten_db: [0.0; 24],
            dca_spill: None,
            channel_clip_hold: [false; 60],
            show_soundcheck_wizard: false,
            soundcheck_step: 0,
            personal_monitor_locked: false,
            ab_slots: vec![None; 42],
            multitrack: crate::multitrack::MultitrackEngine::default(),
            multitrack_auto_name: true,
            update_info: None,
            update_status: format!("Versão atual: v{}", crate::updater::CURRENT_VERSION),
            update_checking: false,
            update_downloading: false,
            show_update_modal: false,
            update_check_rx: None,
            update_apply_rx: None,
        };
        state.load_user_prefs();
        state.refresh_discovery();
        state.check_for_updates();
        state
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), self.sim_time + 3.0));
    }

    pub fn get_reg(&self, reg: usize) -> u16 {
        if reg < NUM_REGS {
            self.regs[reg]
        } else {
            0
        }
    }

    pub fn set_reg(&mut self, reg: usize, val: u16) {
        if reg < NUM_REGS && self.regs[reg] != val {
            self.regs[reg] = val;
            self.hw_link.send_param(reg as u16, val);
        }
    }

    /// Sets a channel parameter and automatically propagates to stereo-linked partner & custom link group (except Pan 0x0B)
    pub fn set_group_param(&mut self, group: u8, ch: usize, param: usize, val: u16) {
        let reg = map_reg(group, ch, param);
        self.set_reg(reg, val);

        // Stereo link & Custom Channel Link propagation (NEVER touches Pan 0x0B)
        if group == 1 && ch < 24 && param != 0x0B {
            let stereo_partner = if self.is_input_stereo_linked(ch) {
                let p = ch ^ 1;
                self.set_reg(map_reg(1, p, param), val);
                Some(p)
            } else {
                None
            };
            let custom_grp = self.custom_link_groups[ch];
            if custom_grp != 0 {
                for other in 0..24 {
                    if other != ch
                        && Some(other) != stereo_partner
                        && self.custom_link_groups[other] == custom_grp
                    {
                        self.set_reg(map_reg(1, other, param), val);
                    }
                }
            }
        } else if group == 1 && ch == 24 {
            // Stereo digital input (ch 24 & 25)
            let partner_reg = map_reg(1, 25, param);
            self.set_reg(partner_reg, val);
        } else if group == 3 && ch < 10 && self.is_output_stereo_linked(ch) {
            let partner = ch ^ 1;
            let partner_reg = map_reg(3, partner, param);
            self.set_reg(partner_reg, val);
        }
    }

    pub fn channel_name(&self, ch: SelectedChannel) -> &str {
        &self.channel_names[ch.flat_id()]
    }

    pub fn set_channel_name(&mut self, ch: SelectedChannel, name: String) {
        let id = ch.flat_id();
        self.channel_names[id] = name.clone();
        if !matches!(self.hw_link.mode, crate::protocol::ConnectionMode::Demo) && id < 60 {
            let mut p = vec![id as u8];
            p.extend_from_slice(name.as_bytes());
            self.hw_link.send_frame(crate::protocol::build_frame(0x2F, &p));
        }
    }

    pub fn channel_color_idx(&self, ch: SelectedChannel) -> u8 {
        self.channel_colors[ch.flat_id()]
    }

    pub fn set_channel_color_idx(&mut self, ch: SelectedChannel, color_idx: u8) {
        let c = color_idx % 32;
        self.channel_colors[ch.flat_id()] = c;
        match ch {
            SelectedChannel::Input(i) if i < 24 => {
                if self.is_input_stereo_linked(i) {
                    let partner = SelectedChannel::Input(i ^ 1);
                    self.channel_colors[partner.flat_id()] = c;
                }
                let grp = self.custom_link_groups[i];
                if grp != 0 {
                    for other in 0..24 {
                        if self.custom_link_groups[other] == grp {
                            self.channel_colors[SelectedChannel::Input(other).flat_id()] = c;
                        }
                    }
                }
            }
            SelectedChannel::Output(i) if i < 10 && self.is_output_stereo_linked(i) => {
                let partner = SelectedChannel::Output(i ^ 1);
                self.channel_colors[partner.flat_id()] = c;
            }
            _ => {}
        }
    }

    // =========================================================================
    // STEREO LINK (0xBCB for Inputs, 0xBCC for Outputs - fcn.00452e00 / 00452e80)
    // =========================================================================
    pub fn is_input_stereo_linked(&self, ch: usize) -> bool {
        if ch >= 24 {
            return true;
        }
        let pair = ch / 2;
        ((self.get_reg(0xBCB) >> pair) & 1) != 0
    }

    pub fn set_input_stereo_linked(&mut self, ch: usize, linked: bool) {
        if ch >= 24 {
            return;
        }
        let pair = ch / 2;
        let cur = self.get_reg(0xBCB);
        let next = if linked {
            cur | (1 << pair)
        } else {
            cur & !(1 << pair)
        };
        self.set_reg(0xBCB, next);
    }

    pub fn is_output_stereo_linked(&self, ch: usize) -> bool {
        let pair = ch / 2;
        ((self.get_reg(0xBCC) >> pair) & 1) != 0
    }

    pub fn set_output_stereo_linked(&mut self, ch: usize, linked: bool) {
        let pair = ch / 2;
        let cur = self.get_reg(0xBCC);
        let next = if linked {
            cur | (1 << pair)
        } else {
            cur & !(1 << pair)
        };
        self.set_reg(0xBCC, next);
    }

    pub fn input_custom_link_group(&self, ch: usize) -> u8 {
        if ch < 24 {
            self.custom_link_groups[ch]
        } else {
            0
        }
    }

    pub fn set_input_custom_link_group(&mut self, ch: usize, grp: u8) {
        if ch < 24 {
            self.custom_link_groups[ch] = grp.min(4);
        }
    }

    pub fn custom_link_group_label(grp: u8) -> &'static str {
        match grp {
            1 => "LNK A",
            2 => "LNK B",
            3 => "LNK C",
            4 => "LNK D",
            _ => "—",
        }
    }

    /// Returns a short badge label if this channel is stereo-linked or in a custom link group
    pub fn channel_link_badge(&self, sel: SelectedChannel) -> Option<String> {
        match sel {
            SelectedChannel::Input(ch) if ch < 24 => {
                let st = self.is_input_stereo_linked(ch);
                let cg = self.input_custom_link_group(ch);
                match (st, cg) {
                    (true, 0) => Some("LNK".to_string()),
                    (false, g) if g > 0 => Some(Self::custom_link_group_label(g).to_string()),
                    (true, g) if g > 0 => Some(format!("LNK+{}", (b'A' + g - 1) as char)),
                    _ => None,
                }
            }
            SelectedChannel::Output(ch) if ch < 10 && self.is_output_stereo_linked(ch) => {
                Some("LNK".to_string())
            }
            _ => None,
        }
    }

    /// Windows-style Ctrl+Click channel linking: joins/toggles channels in a custom link group without touching Pan
    pub fn ctrl_link_channel(&mut self, clicked: SelectedChannel) {
        let sel = self.selected_channel;
        match (sel, clicked) {
            (SelectedChannel::Input(a), SelectedChannel::Input(b)) if a < 24 && b < 24 => {
                if a == b {
                    if self.custom_link_groups[a] != 0 {
                        self.custom_link_groups[a] = 0;
                        self.toast(format!("{} removido do link", self.channel_name(clicked)));
                    }
                    return;
                }
                let grp = if self.custom_link_groups[a] != 0 {
                    self.custom_link_groups[a]
                } else {
                    (1..=4u8)
                        .find(|&g| !self.custom_link_groups.contains(&g))
                        .unwrap_or(1)
                };

                self.custom_link_groups[a] = grp;
                if self.custom_link_groups[b] == grp {
                    self.custom_link_groups[b] = 0;
                    let remaining: Vec<usize> = (0..24)
                        .filter(|&i| self.custom_link_groups[i] == grp)
                        .collect();
                    if remaining.len() <= 1 {
                        for i in remaining {
                            self.custom_link_groups[i] = 0;
                        }
                        self.toast(format!(
                            "Link Grupo {} desfeito (menos de 2 canais)",
                            (b'A' + grp - 1) as char
                        ));
                    } else {
                        self.toast(format!(
                            "{} removido do Grupo {}",
                            self.channel_name(clicked),
                            (b'A' + grp - 1) as char
                        ));
                    }
                } else {
                    self.custom_link_groups[b] = grp;
                    let members: Vec<String> = (0..24)
                        .filter(|&i| self.custom_link_groups[i] == grp)
                        .map(|i| self.channel_name(SelectedChannel::Input(i)).to_string())
                        .collect();
                    self.toast(format!(
                        "Grupo {} (Sem Pan): {}",
                        (b'A' + grp - 1) as char,
                        members.join(" + ")
                    ));
                }
            }
            (SelectedChannel::Output(a), SelectedChannel::Output(b)) if a < 10 && b < 10 && (a ^ 1) == b => {
                let next = !self.is_output_stereo_linked(a);
                self.set_output_stereo_linked(a, next);
                self.toast(if next {
                    "Par de saída linkado!"
                } else {
                    "Par de saída deslinkado!"
                });
            }
            _ => {
                self.selected_channel = clicked;
            }
        }
    }

    // =========================================================================
    // INPUT CHANNEL PARAMETERS (Group 1: ch 0..25)
    // =========================================================================
    /// Preamp Gain (0 .. 60 dB, param 0x08, type 0x03)
    pub fn input_preamp_gain_db(&self, ch: usize) -> f32 {
        (self.get_reg(map_reg(1, ch, 0x08)) & 0xFF) as f32
    }

    pub fn set_input_preamp_gain_db(&mut self, ch: usize, db: f32) {
        let v = db.round().clamp(0.0, 60.0) as u16;
        self.set_group_param(1, ch, 0x08, v);
    }

    /// +48V Phantom Power (param 0x01 = 0x41 + 0x3E*ch, verified in AX24E.25.11.01.001 @ 0x0043CAEE / 0x0046A8FA)
    pub fn input_phantom_48v(&self, ch: usize) -> bool {
        if ch >= 24 {
            return false;
        }
        self.get_reg(map_reg(1, ch, 0x01)) != 0
    }

    pub fn set_input_phantom_48v(&mut self, ch: usize, on: bool) {
        if ch < 24 {
            if self.phantom_safe_mute && !self.channel_muted(SelectedChannel::Input(ch)) {
                self.set_channel_muted(SelectedChannel::Input(ch), true);
                self.toast(format!(
                    "Proteção Anti-Estalo: CH{} mutado automaticamente ao alternar +48V!",
                    ch + 1
                ));
            }
            self.set_reg(map_reg(1, ch, 0x01), if on { 1 } else { 0 });
        }
    }

    /// Input Phase Invert Ø (param 0x09 = 0x49 + 0x3E*ch, INVERTED in AX24E @ 0x0043967B: 0 = INVERTED/Ø, 1 = NORMAL)
    pub fn input_phase_invert(&self, ch: usize) -> bool {
        self.get_reg(map_reg(1, ch, 0x09)) == 0
    }

    pub fn set_input_phase_invert(&mut self, ch: usize, inv: bool) {
        self.set_group_param(1, ch, 0x09, if inv { 0 } else { 1 });
    }

    /// High-Z (HI-Z on CH1 and CH2, param 0x00 bit 0 = 0x40 + 0x3E*ch, verified in AX24E @ 0x0043CBDD / 0x004393B6)
    pub fn input_hiz(&self, ch: usize) -> bool {
        if ch >= 2 {
            return false;
        }
        (self.get_reg(map_reg(1, ch, 0x00)) & 1) != 0
    }

    pub fn set_input_hiz(&mut self, ch: usize, on: bool) {
        if ch < 2 {
            let reg = map_reg(1, ch, 0x00);
            let cur = self.get_reg(reg) & !1;
            let next = if on { cur | 1 } else { cur };
            self.set_group_param(1, ch, 0x00, next);
        }
    }

    /// Input Delay in ms (0.0 .. 10.0 ms, param 0x1C, type 0x04: scale=10)
    pub fn input_delay_ms(&self, ch: usize) -> f32 {
        (self.get_reg(map_reg(1, ch, 0x1C)) as f32) / 10.0
    }

    pub fn set_input_delay_ms(&mut self, ch: usize, ms: f32) {
        let v = (ms * 10.0).round().clamp(0.0, 100.0) as u16;
        self.set_group_param(1, ch, 0x1C, v);
    }

    /// Pan (-100 = L100 .. 0 = C .. +100 = R100, param 0x0B, type 0x02: stored as val + 100)
    pub fn input_pan(&self, ch: usize) -> i32 {
        let raw = self.get_reg(map_reg(1, ch, 0x0B)) as i32;
        (raw - 100).clamp(-100, 100)
    }

    pub fn set_input_pan(&mut self, ch: usize, pan: i32) {
        let v = (pan.clamp(-100, 100) + 100) as u16;
        self.set_group_param(1, ch, 0x0B, v);
    }

    // =========================================================================
    // NOISE GATE (Group 1: Enable=0x02, Threshold=0x03, Attack=0x05, Hold=0x06, Release=0x07)
    // =========================================================================
    pub fn input_gate_enabled(&self, ch: usize) -> bool {
        self.get_reg(map_reg(1, ch, 0x02)) != 0
    }

    pub fn set_input_gate_enabled(&mut self, ch: usize, en: bool) {
        self.set_group_param(1, ch, 0x02, if en { 1 } else { 0 });
    }

    /// Gate Threshold (-80.0 .. 0.0 dB, param 0x03, type 0x05: stored as -val)
    pub fn input_gate_threshold_db(&self, ch: usize) -> f32 {
        let raw = self.get_reg(map_reg(1, ch, 0x03)) as i16;
        let db = if raw <= 0 { raw as f32 } else { -(raw as f32) };
        db.clamp(-80.0, 0.0)
    }

    pub fn set_input_gate_threshold_db(&mut self, ch: usize, db: f32) {
        let clamped = db.round().clamp(-80.0, 0.0) as i16;
        let raw = (-clamped) as u16;
        self.set_group_param(1, ch, 0x03, raw);
    }

    /// Gate Attack (3 .. 200 ms, param 0x05, type 0x06)
    pub fn input_gate_attack_ms(&self, ch: usize) -> f32 {
        (self.get_reg(map_reg(1, ch, 0x05)) as f32).clamp(3.0, 200.0)
    }

    pub fn set_input_gate_attack_ms(&mut self, ch: usize, ms: f32) {
        let v = ms.round().clamp(3.0, 200.0) as u16;
        self.set_group_param(1, ch, 0x05, v);
    }

    /// Gate Hold index (0..=5 -> [2, 4, 6, 8, 16, 32] ms, param 0x06, type 0x08)
    pub fn input_gate_hold_ms(&self, ch: usize) -> f32 {
        match self.get_reg(map_reg(1, ch, 0x06)) {
            0 => 2.0,
            1 => 4.0,
            2 => 6.0,
            3 => 8.0,
            4 => 16.0,
            _ => 32.0,
        }
    }

    pub fn set_input_gate_hold_ms(&mut self, ch: usize, ms: f32) {
        let idx = if ms < 3.0 {
            0
        } else if ms < 5.0 {
            1
        } else if ms < 7.0 {
            2
        } else if ms < 12.0 {
            3
        } else if ms < 24.0 {
            4
        } else {
            5
        };
        self.set_group_param(1, ch, 0x06, idx);
    }

    /// Gate Release (0 .. 2000 ms, param 0x07, type 0x07)
    pub fn input_gate_release_ms(&self, ch: usize) -> f32 {
        (self.get_reg(map_reg(1, ch, 0x07)) as f32).clamp(0.0, 2000.0)
    }

    pub fn set_input_gate_release_ms(&mut self, ch: usize, ms: f32) {
        let v = ms.round().clamp(0.0, 2000.0) as u16;
        self.set_group_param(1, ch, 0x07, v);
    }

    // =========================================================================
    // COMPRESSOR (Group 1 Inputs & Group 3 Outputs)
    // =========================================================================
    fn comp_base(group: u8) -> (usize, usize, usize, usize, usize, usize) {
        if group == 3 {
            // Group 3 Output Compressor: Enable=0x44, Ratio=0x45, Attack=0x46, Release=0x47, Thresh=0x48, Gain=0x49
            (0x44, 0x48, 0x45, 0x46, 0x47, 0x49)
        } else {
            // Group 1 Input Compressor: Enable=0x1D, Ratio=0x1E, Attack=0x1F, Release=0x20, Thresh=0x21, Gain=0x23
            (0x1D, 0x21, 0x1E, 0x1F, 0x20, 0x23)
        }
    }

    pub fn comp_enabled(&self, group: u8, ch: usize) -> bool {
        let (en_p, _, _, _, _, _) = Self::comp_base(group);
        self.get_reg(map_reg(group, ch, en_p)) != 0
    }

    pub fn set_comp_enabled(&mut self, group: u8, ch: usize, en: bool) {
        let (en_p, _, _, _, _, _) = Self::comp_base(group);
        self.set_group_param(group, ch, en_p, if en { 1 } else { 0 });
    }

    /// Compressor Threshold (-80.0 .. 0.0 dB, type 0x0A: sign bit 0x8000 + mag*10)
    pub fn comp_threshold_db(&self, group: u8, ch: usize) -> f32 {
        let (_, th_p, _, _, _, _) = Self::comp_base(group);
        let raw = self.get_reg(map_reg(group, ch, th_p));
        let val = if (raw & 0x8000) != 0 {
            -((raw & 0x7FFF) as f32) / 10.0
        } else {
            -(raw as f32) / 10.0
        };
        val.clamp(-80.0, 0.0)
    }

    pub fn set_comp_threshold_db(&mut self, group: u8, ch: usize, db: f32) {
        let (_, th_p, _, _, _, _) = Self::comp_base(group);
        let mag = (-db.clamp(-80.0, 0.0) * 10.0).round() as u16;
        let raw = if mag > 0 { mag | 0x8000 } else { 0 };
        self.set_group_param(group, ch, th_p, raw);
    }

    /// Compressor Ratio (1.0 .. 20.0, type 0x0C: AX24E 25.11 uses scale 100 @ 0x004617A9/0x0046198E; Legacy uses scale 10)
    pub fn comp_ratio(&self, group: u8, ch: usize) -> f32 {
        let (_, _, rt_p, _, _, _) = Self::comp_base(group);
        let raw = self.get_reg(map_reg(group, ch, rt_p)) as f32;
        let ratio = if raw >= 50.0 {
            raw / 100.0
        } else {
            raw / 10.0
        };
        ratio.clamp(1.0, 20.0)
    }

    pub fn set_comp_ratio(&mut self, group: u8, ch: usize, ratio: f32) {
        let (_, _, rt_p, _, _, _) = Self::comp_base(group);
        let scale = match self.hw_link.firmware_layout {
            FirmwareLayout::Ax24e2511 => 100.0,
            FirmwareLayout::Legacy => 10.0,
        };
        let raw = (ratio.clamp(1.0, 20.0) * scale).round() as u16;
        self.set_group_param(group, ch, rt_p, raw);
    }

    /// Compressor Attack (3.0 .. 1000.0 ms, type 0x0B: scale 10)
    pub fn comp_attack_ms(&self, group: u8, ch: usize) -> f32 {
        let (_, _, _, at_p, _, _) = Self::comp_base(group);
        ((self.get_reg(map_reg(group, ch, at_p)) as f32) / 10.0).clamp(3.0, 1000.0)
    }

    pub fn set_comp_attack_ms(&mut self, group: u8, ch: usize, ms: f32) {
        let (_, _, _, at_p, _, _) = Self::comp_base(group);
        let raw = (ms.clamp(3.0, 1000.0) * 10.0).round() as u16;
        self.set_group_param(group, ch, at_p, raw);
    }

    /// Compressor Release (10.0 .. 5000.0 ms, type 0x0D: scale 10)
    pub fn comp_release_ms(&self, group: u8, ch: usize) -> f32 {
        let (_, _, _, _, rl_p, _) = Self::comp_base(group);
        ((self.get_reg(map_reg(group, ch, rl_p)) as f32) / 10.0).clamp(10.0, 5000.0)
    }

    pub fn set_comp_release_ms(&mut self, group: u8, ch: usize, ms: f32) {
        let (_, _, _, _, rl_p, _) = Self::comp_base(group);
        let raw = (ms.clamp(10.0, 5000.0) * 10.0).round() as u16;
        self.set_group_param(group, ch, rl_p, raw);
    }

    /// Compressor Make-Up Gain (0.0 .. 20.0 dB, type 0x0E: stored as val*10 + 0x4B0)
    pub fn comp_gain_db(&self, group: u8, ch: usize) -> f32 {
        let (_, _, _, _, _, gn_p) = Self::comp_base(group);
        let raw = (self.get_reg(map_reg(group, ch, gn_p)) & 0x3FFF) as i32;
        (((raw - 0x4B0) as f32) / 10.0).clamp(0.0, 20.0)
    }

    pub fn set_comp_gain_db(&mut self, group: u8, ch: usize, db: f32) {
        let (_, _, _, _, _, gn_p) = Self::comp_base(group);
        let raw = ((db.clamp(0.0, 20.0) * 10.0).round() as i32 + 0x4B0) as u16;
        self.set_group_param(group, ch, gn_p, raw);
    }

    // =========================================================================
    // PARAMETRIC EQ + HPF/LPF (Group 1 Inputs: 4 bands; Group 2 FX: 4 bands; Group 3 Outputs: 7 bands)
    // =========================================================================
    pub fn peq_band_count(group: u8) -> usize {
        match group {
            3 => 7, // Outputs (AUX1-8, MAIN L/R) have 7 PEQ bands in axios24new.exe
            _ => 4, // Inputs & FX have 4 PEQ bands (LOW, L-MID, H-MID, HIGH)
        }
    }

    fn eq_param_offsets(group: u8) -> (usize, usize, usize, usize, usize, usize) {
        // Returns (eq_en, hpf_en, hpf_type, hpf_freq, lpf_type, band0_base)
        match group {
            2 => (0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBF),
            3 => (0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x53),
            _ => (0x24, 0x25, 0x26, 0x27, 0x28, 0x2A),
        }
    }

    pub fn eq_enabled(&self, group: u8, ch: usize) -> bool {
        let (eq_en, _, _, _, _, _) = Self::eq_param_offsets(group);
        self.get_reg(map_reg(group, ch, eq_en)) != 0
    }

    pub fn set_eq_enabled(&mut self, group: u8, ch: usize, en: bool) {
        let (eq_en, _, _, _, _, _) = Self::eq_param_offsets(group);
        self.set_group_param(group, ch, eq_en, if en { 1 } else { 0 });
    }

    pub fn decode_freq_reg(raw: u16) -> f32 {
        if (raw & 0x8000) != 0 {
            ((raw & 0x7FFF) as f32) / 10.0
        } else {
            raw as f32
        }
    }

    pub fn encode_freq_reg(freq_hz: f32) -> u16 {
        let f = freq_hz.clamp(19.6, 20600.0);
        if f < 100.0 {
            ((f * 10.0).round() as u16) | 0x8000
        } else {
            f.round() as u16
        }
    }

    pub fn hpf_freq_hz(&self, group: u8, ch: usize) -> f32 {
        let (_, _, _, hpf_freq, _, _) = Self::eq_param_offsets(group);
        Self::decode_freq_reg(self.get_reg(map_reg(group, ch, hpf_freq))).clamp(19.6, 20000.0)
    }

    pub fn set_hpf_freq_hz(&mut self, group: u8, ch: usize, freq_hz: f32) {
        let (_, _, _, hpf_freq, _, _) = Self::eq_param_offsets(group);
        self.set_group_param(group, ch, hpf_freq, Self::encode_freq_reg(freq_hz.clamp(19.6, 20000.0)));
    }

    pub fn hpf_family_slope(&self, group: u8, ch: usize) -> (FilterFamily, FilterSlope) {
        let (_, _, hpf_type, _, _, _) = Self::eq_param_offsets(group);
        decode_hpf_lpf_id(self.get_reg(map_reg(group, ch, hpf_type)), false)
    }

    pub fn set_hpf_family_slope(&mut self, group: u8, ch: usize, fam: FilterFamily, slope: FilterSlope) {
        let (_, _, hpf_type, _, _, _) = Self::eq_param_offsets(group);
        let id = encode_hpf_lpf_id(fam, slope, false);
        self.set_group_param(group, ch, hpf_type, id);
    }

    pub fn lpf_freq_hz(&self, group: u8, ch: usize) -> f32 {
        let (_, _, _, _, lpf_type, _) = Self::eq_param_offsets(group);
        let lpf_freq = lpf_type + 1;
        Self::decode_freq_reg(self.get_reg(map_reg(group, ch, lpf_freq))).clamp(19.7, 20600.0)
    }

    pub fn set_lpf_freq_hz(&mut self, group: u8, ch: usize, freq_hz: f32) {
        let (_, _, _, _, lpf_type, _) = Self::eq_param_offsets(group);
        let lpf_freq = lpf_type + 1;
        self.set_group_param(group, ch, lpf_freq, Self::encode_freq_reg(freq_hz.clamp(19.7, 20600.0)));
    }

    pub fn lpf_family_slope(&self, group: u8, ch: usize) -> (FilterFamily, FilterSlope) {
        let (_, _, _, _, lpf_type, _) = Self::eq_param_offsets(group);
        decode_hpf_lpf_id(self.get_reg(map_reg(group, ch, lpf_type)), true)
    }

    pub fn set_lpf_family_slope(&mut self, group: u8, ch: usize, fam: FilterFamily, slope: FilterSlope) {
        let (_, _, _, _, lpf_type, _) = Self::eq_param_offsets(group);
        let id = encode_hpf_lpf_id(fam, slope, true);
        self.set_group_param(group, ch, lpf_type, id);
    }

    pub fn peq_band(&self, group: u8, ch: usize, band_idx: usize) -> PeqBandData {
        let (_, _, _, _, _, base) = Self::eq_param_offsets(group);
        let b_off = base + band_idx * 4;
        let mode = PeqBandMode::from_reg(self.get_reg(map_reg(group, ch, b_off)));
        let freq_hz = Self::decode_freq_reg(self.get_reg(map_reg(group, ch, b_off + 1))).clamp(20.0, 20000.0);
        let gain_raw = self.get_reg(map_reg(group, ch, b_off + 2)) as i32;
        let gain_db = (((gain_raw - 500) as f32) / 10.0).clamp(-12.0, 12.0);
        let q_raw = self.get_reg(map_reg(group, ch, b_off + 3)) as f32;
        let q = (q_raw / 100.0).clamp(0.60, 28.08);
        PeqBandData {
            mode,
            freq_hz,
            gain_db,
            q,
        }
    }

    pub fn set_peq_band(&mut self, group: u8, ch: usize, band_idx: usize, data: PeqBandData) {
        let (_, _, _, _, _, base) = Self::eq_param_offsets(group);
        let b_off = base + band_idx * 4;
        self.set_group_param(group, ch, b_off, data.mode.to_reg());
        self.set_group_param(group, ch, b_off + 1, Self::encode_freq_reg(data.freq_hz.clamp(20.0, 20000.0)));
        let gain_reg = ((data.gain_db.clamp(-12.0, 12.0) * 10.0).round() as i32 + 500).max(0) as u16;
        self.set_group_param(group, ch, b_off + 2, gain_reg);
        let q_reg = (data.q.clamp(0.60, 28.08) * 100.0).round() as u16;
        self.set_group_param(group, ch, b_off + 3, q_reg);
    }

    pub fn reset_peq(&mut self, group: u8, ch: usize) {
        self.set_hpf_freq_hz(group, ch, 19.6);
        self.set_lpf_freq_hz(group, ch, 20600.0);
        let default_freqs_4 = [100.0, 400.0, 1600.0, 6300.0];
        let default_freqs_7 = [63.0, 150.0, 400.0, 1000.0, 2500.0, 6300.0, 16000.0];
        let count = Self::peq_band_count(group);
        for b in 0..count {
            let f = if count == 7 {
                default_freqs_7[b]
            } else {
                default_freqs_4[b]
            };
            self.set_peq_band(
                group,
                ch,
                b,
                PeqBandData {
                    mode: PeqBandMode::Peaking,
                    freq_hz: f,
                    gain_db: 0.0,
                    q: 1.0,
                },
            );
        }
    }

    // =========================================================================
    // FADERS, MUTE, SOLO & BUS SENDS (fcn.004531b0, 004532c0, 00453590)
    // =========================================================================
    pub fn decode_fader_db(raw: u16) -> f32 {
        let mag = (raw & 0x3FFF) as i32;
        if mag == 0 {
            -120.0
        } else {
            (((mag - 0x4B0) as f32) / 10.0).clamp(-120.0, 10.0)
        }
    }

    pub fn encode_fader_db(db: f32, max_db: f32) -> u16 {
        if db <= -119.5 {
            0
        } else {
            (((db.clamp(-120.0, max_db) * 10.0).round() as i32) + 0x4B0).max(0) as u16
        }
    }

    pub fn channel_fader_db(&self, sel: SelectedChannel) -> f32 {
        let (group, ch) = sel.group_and_index();
        let reg = map_reg(group, ch, 0xEA);
        Self::decode_fader_db(self.get_reg(reg))
    }

    pub fn set_channel_fader_db(&mut self, sel: SelectedChannel, db: f32) {
        let (group, ch) = sel.group_and_index();
        let raw = Self::encode_fader_db(db, 10.0);
        self.set_group_param(group, ch, 0xEA, raw);
    }

    /// Channel Mute (verified in AX24E.25.11.01.001 @ 0x00466A2C / 0x004679FA / 0x00441976):
    /// - Groups 1 (Inputs), 2 (FX), 3 (AUX/MAIN L/R): param 0xEB with INVERTED polarity: 0 = MUTED, 1 = UNMUTED (ON)
    /// - Group 4 (DCA): param 0xD9 (reg 0xBA7/0xBCB + ch*9): 1 = MUTED (ON), 0 = UNMUTED (OFF) (@ 0x0042EF85 / 0x0042F160)
    pub fn channel_muted(&self, sel: SelectedChannel) -> bool {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            return self.get_reg(map_reg(4, ch, 0xD9)) != 0;
        }
        self.get_reg(map_reg(group, ch, 0xEB)) == 0
    }

    fn set_single_channel_muted(&mut self, sel: SelectedChannel, muted: bool) {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            self.set_reg(map_reg(4, ch, 0xD9), if muted { 1 } else { 0 });
            return;
        }
        let val = if muted { 0 } else { 1 };
        self.set_reg(map_reg(group, ch, 0xEB), val);
        if group == 1 && ch == 24 {
            // Stereo digital input (CH24 & CH25 = BT/DIGI L & R)
            self.set_reg(map_reg(1, 25, 0xEB), val);
        }
    }

    pub fn set_channel_muted(&mut self, sel: SelectedChannel, muted: bool) {
        self.set_single_channel_muted(sel, muted);
        let (group, ch) = sel.group_and_index();
        if group == 1 && ch < 24 {
            let stereo_partner = if self.is_input_stereo_linked(ch) {
                let p = ch ^ 1;
                self.set_single_channel_muted(SelectedChannel::Input(p), muted);
                Some(p)
            } else {
                None
            };
            let custom_grp = self.custom_link_groups[ch];
            if custom_grp != 0 {
                for other in 0..24 {
                    if other != ch
                        && Some(other) != stereo_partner
                        && self.custom_link_groups[other] == custom_grp
                    {
                        self.set_single_channel_muted(SelectedChannel::Input(other), muted);
                    }
                }
            }
        } else if group == 3 && ch < 10 && self.is_output_stereo_linked(ch) {
            self.set_single_channel_muted(SelectedChannel::Output(ch ^ 1), muted);
        }
    }

    pub fn set_output_geq_enabled(&mut self, out_idx: usize, en: bool) {
        let idx = out_idx.min(9);
        self.output_geq_enabled[idx] = en;
        if self.is_output_stereo_linked(idx) {
            self.output_geq_enabled[(idx ^ 1).min(9)] = en;
        }
    }

    pub fn set_output_geq_db(&mut self, out_idx: usize, band: usize, db: f32) {
        let idx = out_idx.min(9);
        let b = band.min(30);
        let clamped = (db.clamp(-12.0, 12.0) * 10.0).round() / 10.0;
        self.output_geq_db[idx][b] = clamped;
        if self.is_output_stereo_linked(idx) {
            self.output_geq_db[(idx ^ 1).min(9)][b] = clamped;
        }
    }

    pub fn reset_output_geq(&mut self, out_idx: usize) {
        let idx = out_idx.min(9);
        self.output_geq_db[idx] = [0.0; 31];
        if self.is_output_stereo_linked(idx) {
            self.output_geq_db[(idx ^ 1).min(9)] = [0.0; 31];
        }
    }

    /// Channel Solo / PFL (param 0xEC, verified in AX24E.25.11.01.001 @ 0x00461390 & 0x004613B0)
    /// Active when (val & 0x3FFF) != 0 && (val & 0x4000) == 0.
    /// MAIN L uses 0xA00 (+ 0 to 0xA01), MAIN R uses 0xA6E (+ 0 to 0xA6D), all others use reg & reg+1.
    pub fn channel_solo(&self, sel: SelectedChannel) -> bool {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            return false;
        }
        let reg = if group == 3 && ch == 8 {
            0xA00
        } else if group == 3 && ch == 9 {
            0xA6E
        } else {
            map_reg(group, ch, 0xEC)
        };
        let v = self.get_reg(reg);
        (v & 0x3FFF) != 0 && (v & 0x4000) == 0
    }

    pub fn set_channel_solo(&mut self, sel: SelectedChannel, solo: bool) {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            return;
        }
        let level = if solo { 0x04B0 } else { 0 };
        if group == 3 && ch == 8 {
            let next = (self.get_reg(0xA00) & 0xC000) | level;
            self.set_reg(0xA00, next);
            self.set_reg(0xA01, 0);
        } else if group == 3 && ch == 9 {
            let next = (self.get_reg(0xA6E) & 0xC000) | level;
            self.set_reg(0xA6E, next);
            self.set_reg(0xA6D, 0);
        } else {
            let reg = map_reg(group, ch, 0xEC);
            let next = (self.get_reg(reg) & 0xC000) | level;
            self.set_reg(reg, next);
            self.set_reg(reg + 1, next);
            if group == 1 && ch == 24 {
                let reg2 = map_reg(1, 25, 0xEC);
                let next2 = (self.get_reg(reg2) & 0xC000) | level;
                self.set_reg(reg2, next2);
                self.set_reg(reg2 + 1, next2);
            }
        }
    }

    /// Input -> AUX1..8 send level in dB (-120.0 .. 0.0 dB, params 0x0D .. 0x14)
    pub fn input_aux_send_db(&self, ch: usize, aux_idx: usize) -> f32 {
        let reg = map_reg(1, ch, 0x0D + aux_idx.min(7));
        Self::decode_fader_db(self.get_reg(reg))
    }

    pub fn set_input_aux_send_db(&mut self, ch: usize, aux_idx: usize, db: f32) {
        let param = 0x0D + aux_idx.min(7);
        let reg = map_reg(1, ch, param);
        let flags = self.get_reg(reg) & 0xC000;
        let raw = (Self::encode_fader_db(db, 0.0) & 0x3FFF) | flags;
        self.set_group_param(1, ch, param, raw);
    }

    /// Input -> AUX1..8 Pre/Post fader flag (verified in AX24E @ 0x004615C0..0x004615E8 & 0x0042A13D):
    /// Bit 15 (0x8000) = 1 -> PRE-FADER, Bit 15 = 0 -> POST-FADER
    pub fn input_aux_pre_fader(&self, ch: usize, aux_idx: usize) -> bool {
        let reg = map_reg(1, ch, 0x0D + aux_idx.min(7));
        ((self.get_reg(reg) >> 15) & 1) != 0
    }

    pub fn set_input_aux_pre_fader(&mut self, ch: usize, aux_idx: usize, pre: bool) {
        let param = 0x0D + aux_idx.min(7);
        let reg = map_reg(1, ch, param);
        let cur = self.get_reg(reg);
        let next = if pre { cur | 0x8000 } else { cur & !0x8000 };
        self.set_group_param(1, ch, param, next);
    }

    /// Input -> FX1..2 send level in dB (-120.0 .. 0.0 dB, params 0x19 .. 0x1A)
    pub fn input_fx_send_db(&self, ch: usize, fx_idx: usize) -> f32 {
        let reg = map_reg(1, ch, 0x19 + fx_idx.min(1));
        Self::decode_fader_db(self.get_reg(reg))
    }

    pub fn set_input_fx_send_db(&mut self, ch: usize, fx_idx: usize, db: f32) {
        let param = 0x19 + fx_idx.min(1);
        let flags = self.get_reg(map_reg(1, ch, param)) & 0xC000;
        let raw = (Self::encode_fader_db(db, 0.0) & 0x3FFF) | flags;
        self.set_group_param(1, ch, param, raw);
    }

    /// Input -> Main L/R bus assignment (params 0x15 & 0x16, verified in AX24E @ 0x004277BD / 0x004278C5 & igreja.scn)
    pub fn input_to_main_lr(&self, ch: usize) -> bool {
        let l = self.get_reg(map_reg(1, ch, 0x15)) & 0x3FFF;
        let r = self.get_reg(map_reg(1, ch, 0x16)) & 0x3FFF;
        l != 0 || r != 0
    }

    pub fn set_input_to_main_lr(&mut self, ch: usize, on: bool) {
        let fl_l = self.get_reg(map_reg(1, ch, 0x15)) & 0xE000;
        let fl_r = self.get_reg(map_reg(1, ch, 0x16)) & 0xE000;
        let level = if on { 0x04B0 } else { 0 };
        self.set_group_param(1, ch, 0x15, fl_l | level);
        self.set_group_param(1, ch, 0x16, fl_r | level);
    }

    // =========================================================================
    // OUTPUT CHANNEL PARAMETERS (Group 3: ch 0..7 = AUX1..8, 8 = MAIN L, 9 = MAIN R)
    // =========================================================================
    /// Output Phase Invert Ø (param 0x3F = 0x68D + 0x6D*ch, INVERTED in AX24E @ 0x00441B14: 0 = INVERTED/Ø, 1 = NORMAL)
    pub fn output_phase_invert(&self, ch: usize) -> bool {
        self.get_reg(map_reg(3, ch, 0x3F)) == 0
    }

    pub fn set_output_phase_invert(&mut self, ch: usize, inv: bool) {
        self.set_group_param(3, ch, 0x3F, if inv { 0 } else { 1 });
    }

    /// Output Delay in ms (0.0 .. 40.0 ms, param 0x43, type 0x23: scale=10)
    pub fn output_delay_ms(&self, ch: usize) -> f32 {
        ((self.get_reg(map_reg(3, ch, 0x43)) as f32) / 10.0).clamp(0.0, 40.0)
    }

    pub fn set_output_delay_ms(&mut self, ch: usize, ms: f32) {
        let raw = (ms.clamp(0.0, 40.0) * 10.0).round() as u16;
        self.set_group_param(3, ch, 0x43, raw);
    }

    // =========================================================================
    // FX PROCESSORS (Group 2: fx_idx 0 = FX1, 1 = FX2)
    // =========================================================================
    /// Active FX algorithm slot (1..16 for FX1, 17..32 for FX2, param 0xD3)
    pub fn fx_algo_id(&self, fx_idx: usize) -> u16 {
        let reg = map_reg(2, fx_idx.min(1), 0xD3);
        let raw = self.get_reg(reg);
        if fx_idx == 0 {
            raw.clamp(1, 16)
        } else {
            raw.clamp(17, 32)
        }
    }

    pub fn set_fx_algo_id(&mut self, fx_idx: usize, algo_1based: u16) {
        let clamped = if fx_idx == 0 {
            algo_1based.clamp(1, 16)
        } else {
            algo_1based.clamp(17, 32)
        };
        let reg = map_reg(2, fx_idx.min(1), 0xD3);
        self.set_reg(reg, clamped);
    }

    pub fn fx_param1(&self, fx_idx: usize) -> u16 {
        let reg = map_reg(2, fx_idx.min(1), 0xD4);
        self.get_reg(reg)
    }

    pub fn set_fx_param1(&mut self, fx_idx: usize, val: u16) {
        let algo = self.fx_algo_id(fx_idx);
        let max_v = FX_ALGORITHMS[(algo - 1) as usize].param1_max;
        let reg = map_reg(2, fx_idx.min(1), 0xD4);
        self.set_reg(reg, val.min(max_v));
    }

    pub fn fx_param2(&self, fx_idx: usize) -> u16 {
        let reg = map_reg(2, fx_idx.min(1), 0xD5);
        self.get_reg(reg).min(100)
    }

    pub fn set_fx_param2(&mut self, fx_idx: usize, val: u16) {
        let reg = map_reg(2, fx_idx.min(1), 0xD5);
        self.set_reg(reg, val.min(100));
    }

    pub fn fx_aux_send_db(&self, fx_idx: usize, aux_idx: usize) -> f32 {
        let reg = map_reg(2, fx_idx.min(1), 0xAD + aux_idx.min(7));
        Self::decode_fader_db(self.get_reg(reg))
    }

    pub fn set_fx_aux_send_db(&mut self, fx_idx: usize, aux_idx: usize, db: f32) {
        let raw = Self::encode_fader_db(db, 0.0);
        let reg = map_reg(2, fx_idx.min(1), 0xAD + aux_idx.min(7));
        self.set_reg(reg, raw);
    }

    // =========================================================================
    // DCA GROUPS, MUTE GROUPS, PHANTOM GROUPS (fcn.00452ad0, 00452c60, 00452cd0)
    // =========================================================================
    /// DCA group membership (dca_idx 0..3, flat_ch 0..37)
    pub fn is_in_dca_group(&self, dca_idx: usize, flat_ch: usize) -> bool {
        let base = map_reg(4, dca_idx.min(3), 0xDD);
        let word = self.get_reg(base + (flat_ch >> 4));
        ((word >> (flat_ch & 0xF)) & 1) != 0
    }

    pub fn set_in_dca_group(&mut self, dca_idx: usize, flat_ch: usize, member: bool) {
        let base = map_reg(4, dca_idx.min(3), 0xDD);
        let reg = base + (flat_ch >> 4);
        let cur = self.get_reg(reg);
        let next = if member {
            cur | (1 << (flat_ch & 0xF))
        } else {
            cur & !(1 << (flat_ch & 0xF))
        };
        self.set_reg(reg, next);
    }

    pub fn clear_dca_group(&mut self, dca_idx: usize) {
        for flat_ch in 0..38 {
            self.set_in_dca_group(dca_idx, flat_ch, false);
        }
        self.toast(format!("DCA {} limpo!", dca_idx + 1));
    }

    /// Soundcraft Ui24R Tap Tempo: calculates BPM & sets FX Delay time
    pub fn tap_tempo_hit(&mut self, fx_idx: usize) {
        let now = self.sim_time;
        let dt = now - self.tap_last_time;
        self.tap_last_time = now;
        if (0.25..=2.0).contains(&dt) {
            let bpm = (60.0 / dt as f32).clamp(40.0, 240.0);
            self.tap_tempo_bpm = (bpm * 10.0).round() / 10.0;
        }
        self.apply_tap_tempo_to_fx(fx_idx);
    }

    pub fn apply_tap_tempo_to_fx(&mut self, fx_idx: usize) {
        let mult = match self.tap_subdivision {
            1 => 0.5,   // 1/8
            2 => 0.75,  // 1/8 dotted (U2 style)
            3 => 0.25,  // 1/16
            _ => 1.0,   // 1/4
        };
        let delay_ms = (60_000.0 / self.tap_tempo_bpm.max(40.0)) * mult;
        let val = delay_ms.round().clamp(10.0, 1000.0) as u16;
        self.set_fx_param1(fx_idx, val);
        self.toast(format!(
            "Tap Tempo FX{}: {:.1} BPM ({:.0} ms)",
            fx_idx + 1,
            self.tap_tempo_bpm,
            delay_ms
        ));
    }

    /// Soundcraft AFS2 Live Feedback Hunter: scans RTA peak and attenuates feedback ring with narrow notch
    pub fn run_afs2_feedback_hunt(&mut self) {
        let peak_hz = self.rta_peak_freq_hz.clamp(60.0, 16000.0);
        let sel = self.selected_channel;
        let (grp, ch) = sel.group_and_index();
        if grp == 1 || grp == 3 {
            self.set_eq_enabled(grp, ch, true);
            let band = if grp == 1 { 3 } else { 6 };
            let mut b = self.peq_band(grp, ch, band);
            b.mode = PeqBandMode::Peaking;
            b.freq_hz = peak_hz;
            b.gain_db = -9.0;
            b.q = 18.0;
            self.set_peq_band(grp, ch, band, b);
            self.toast(format!("AFS2 Notch aplicado em {:.0} Hz (-9 dB, Q=18)", peak_hz));
        }
    }

    /// Armer MixHub 24 Virtual Soundcheck (toggles input source between Live XLR/P10 & USB Multitrack 16x16)
    pub fn toggle_virtual_soundcheck(&mut self) {
        self.virtual_soundcheck = !self.virtual_soundcheck;
        if self.virtual_soundcheck {
            self.set_input_to_main_lr(24, true);
            self.set_channel_muted(SelectedChannel::Input(24), false);
        }
        self.toast(if self.virtual_soundcheck {
            "Virtual Soundcheck ATIVO: Retorno USB 16x16 habilitado para passagem de som!"
        } else {
            "Virtual Soundcheck DESATIVADO: Entradas em modo ao vivo XLR/P10"
        });
    }

    /// Soundcraft MoreMe: adjusts all OTHER channels in the chosen AUX bus ("Band" level) while preserving the user's own channel
    pub fn adjust_more_me_band(&mut self, delta_db: f32) {
        let aux = self.more_me_aux.min(7);
        let my_ch = self.more_me_channel.min(24);
        for ch in 0..24 {
            if ch != my_ch {
                let cur = self.input_aux_send_db(ch, aux);
                if cur > -115.0 {
                    let next = (cur + delta_db).clamp(-120.0, 0.0);
                    self.set_input_aux_send_db(ch, aux, next);
                }
            }
        }
    }

    /// Mute group membership (mg_idx 0..3, flat_ch 0..41)
    pub fn is_in_mute_group(&self, mg_idx: usize, flat_ch: usize) -> bool {
        let base = map_reg(5, mg_idx.min(3), 0xE3);
        let word = self.get_reg(base + (flat_ch >> 4));
        ((word >> (flat_ch & 0xF)) & 1) != 0
    }

    pub fn set_in_mute_group(&mut self, mg_idx: usize, flat_ch: usize, member: bool) {
        let base = map_reg(5, mg_idx.min(3), 0xE3);
        let reg = base + (flat_ch >> 4);
        let cur = self.get_reg(reg);
        let next = if member {
            cur | (1 << (flat_ch & 0xF))
        } else {
            cur & !(1 << (flat_ch & 0xF))
        };
        self.set_reg(reg, next);
    }

    pub fn mute_group_active(&self, mg_idx: usize) -> bool {
        self.get_reg(map_reg(5, mg_idx.min(3), 0xE2)) != 0
    }

    pub fn set_mute_group_active(&mut self, mg_idx: usize, active: bool) {
        let mg = mg_idx.min(3);
        self.set_reg(map_reg(5, mg, 0xE2), if active { 1 } else { 0 });
        for flat_ch in 0..38 {
            if self.is_in_mute_group(mg, flat_ch) {
                let sel = match flat_ch {
                    0..=25 => SelectedChannel::Input(flat_ch),
                    26..=27 => SelectedChannel::Fx(flat_ch - 26),
                    _ => SelectedChannel::Output(flat_ch - 28),
                };
                self.set_channel_muted(sel, active);
            }
        }
    }

    /// Phantom group membership (pg_idx 0..3, input ch 0..23)
    pub fn is_in_phantom_group(&self, pg_idx: usize, ch: usize) -> bool {
        if ch >= 24 {
            return false;
        }
        let base = map_reg(6, pg_idx.min(3), 0xE8);
        let word = self.get_reg(base + (ch >> 4));
        ((word >> (ch & 0xF)) & 1) != 0
    }

    pub fn set_in_phantom_group(&mut self, pg_idx: usize, ch: usize, member: bool) {
        if ch >= 24 {
            return;
        }
        let base = map_reg(6, pg_idx.min(3), 0xE8);
        let reg = base + (ch >> 4);
        let cur = self.get_reg(reg);
        let next = if member {
            cur | (1 << (ch & 0xF))
        } else {
            cur & !(1 << (ch & 0xF))
        };
        self.set_reg(reg, next);
    }

    pub fn phantom_group_active(&self, pg_idx: usize) -> bool {
        self.get_reg(map_reg(6, pg_idx.min(3), 0xE7)) != 0
    }

    pub fn set_phantom_group_active(&mut self, pg_idx: usize, active: bool) {
        let pg = pg_idx.min(3);
        self.set_reg(map_reg(6, pg, 0xE7), if active { 1 } else { 0 });
        for ch in 0..24 {
            if self.is_in_phantom_group(pg, ch) {
                self.set_input_phantom_48v(ch, active);
            }
        }
    }

    // =========================================================================
    // SIGNAL GENERATOR & PHONES / MONITOR (0xACF, 0xAD1..0xADE - fcn.00450b60)
    // =========================================================================
    pub fn phones_level_db(&self) -> f32 {
        Self::decode_fader_db(self.get_reg(0xACF))
    }

    pub fn set_phones_level_db(&mut self, db: f32) {
        let raw = Self::encode_fader_db(db, 0.0);
        self.set_reg(0xACF, raw);
    }

    pub fn signal_gen_enabled(&self) -> bool {
        self.get_reg(0xAD1) != 0
    }

    pub fn set_signal_gen_enabled(&mut self, en: bool) {
        self.set_reg(0xAD1, if en { 1 } else { 0 });
    }

    /// Signal Generator Type: 1 = SINE WAVE, 2 = PINK NOISE, 3 = WHITE NOISE (0xAD2)
    pub fn signal_gen_type(&self) -> u16 {
        self.get_reg(0xAD2).clamp(1, 3)
    }

    pub fn set_signal_gen_type(&mut self, t: u16) {
        self.set_reg(0xAD2, t.clamp(1, 3));
    }

    pub fn signal_gen_freq_hz(&self) -> f32 {
        Self::decode_freq_reg(self.get_reg(0xAD3)).clamp(20.0, 20000.0)
    }

    pub fn set_signal_gen_freq_hz(&mut self, freq_hz: f32) {
        self.set_reg(0xAD3, Self::encode_freq_reg(freq_hz.clamp(20.0, 20000.0)));
    }

    pub fn signal_gen_level_db(&self) -> f32 {
        Self::decode_fader_db(self.get_reg(0xAD4))
    }

    pub fn set_signal_gen_level_db(&mut self, db: f32) {
        self.set_reg(0xAD4, Self::encode_fader_db(db, 0.0));
    }

    /// Signal Generator routing to AUX1..8 (0..7) and MAIN L/R (8..9) (regs 0xAD5..0xADE)
    pub fn signal_gen_dest(&self, out_idx: usize) -> bool {
        self.get_reg(0xAD5 + out_idx.min(9)) != 0
    }

    pub fn set_signal_gen_dest(&mut self, out_idx: usize, on: bool) {
        self.set_reg(0xAD5 + out_idx.min(9), if on { 1 } else { 0 });
    }

    // =========================================================================
    // SCENES & COPY/PASTE (preset.bin & .scn compatibility)
    // =========================================================================
    pub fn push_undo(&mut self, desc: impl Into<String>) {
        if self.undo_stack.len() >= 25 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((self.regs, desc.into()));
    }

    pub fn undo(&mut self) {
        if let Some((prev_regs, desc)) = self.undo_stack.pop() {
            self.regs = prev_regs;
            self.scenes[self.active_scene_idx] = prev_regs;
            let _ = save_preset_bin(&self.preset_bin_path, &self.scenes);
            self.sync_full_console_to_hardware();
            self.toast(format!("Desfeito: {}", desc));
        } else {
            self.toast("Nada para desfazer.");
        }
    }

    fn save_auto_backup(&self) {
        let backup_path = self.preset_bin_path.with_file_name("preset_backup.bin");
        let _ = save_preset_bin(&backup_path, &self.scenes);
    }

    pub fn recall_scene(&mut self, idx: usize) {
        if idx < 16 {
            self.push_undo(format!("Troca para Cena {}", idx + 1));
            self.save_auto_backup();
            let prev_regs = self.regs;
            self.active_scene_idx = idx;
            self.regs = self.scenes[idx];

            // Restore Recall Safe channels so protected mics/playback never change!
            for flat_id in 0..42 {
                if self.recall_safe_channels[flat_id] {
                    let sel = SelectedChannel::from_flat_id(flat_id);
                    let (grp, ch) = sel.group_and_index();
                    let param_range = match grp {
                        1 => 0..0x3E,
                        2 => 0xAB..0xD8,
                        3 => 0..0x6D,
                        _ => 0..9,
                    };
                    for p in param_range {
                        let r = map_reg(grp, ch, p);
                        if r < NUM_REGS {
                            self.regs[r] = prev_regs[r];
                        }
                    }
                }
            }

            self.hw_link.send_frame(build_scene_op_frame((idx + 1) as u8, false));
            self.hw_link.read_all_registers();
            let safe_cnt = self.recall_safe_channels.iter().filter(|&&s| s).count();
            if safe_cnt > 0 {
                self.toast(format!(
                    "Cena {} ({}) carregada ({} canal(is) em Recall Safe mantidos)!",
                    idx + 1,
                    self.scene_names[idx],
                    safe_cnt
                ));
            } else {
                self.toast(format!("Cena {} ({}) carregada!", idx + 1, self.scene_names[idx]));
            }
        }
    }

    pub fn save_scene(&mut self, idx: usize) {
        if idx < 16 {
            self.active_scene_idx = idx;
            self.scenes[idx] = self.regs;
            let _ = save_preset_bin(&self.preset_bin_path, &self.scenes);
            let slot = (idx + 1) as u8;
            let mut p10 = vec![slot];
            p10.extend_from_slice(self.scene_names[idx].as_bytes());
            self.hw_link.send_frame(crate::protocol::build_frame(0x10, &p10));
            self.hw_link.send_frame(build_scene_op_frame(slot, true));
            self.toast(format!("Cena {} ({}) salva na mesa e em preset.bin!", idx + 1, self.scene_names[idx]));
        }
    }

    pub fn reset_scene_to_default(&mut self, idx: usize) {
        if idx < 16 {
            self.push_undo(format!("Reset Cena {}", idx + 1));
            self.save_auto_backup();
            self.scenes[idx] = DEFAULT_REGS;
            if self.active_scene_idx == idx {
                self.regs = DEFAULT_REGS;
            }
            let _ = save_preset_bin(&self.preset_bin_path, &self.scenes);
            self.toast(format!("Cena {} restaurada para o padrão de fábrica!", idx + 1));
        }
    }

    pub fn export_scn_file(&mut self, path: &PathBuf) {
        let idx = self.active_scene_idx;
        let bytes = encode_official_scn(
            idx + 1,
            &self.scene_names[idx],
            &self.channel_names,
            &self.regs,
            self.hw_link.firmware_layout,
        );
        if fs::write(path, bytes).is_ok() {
            self.toast(format!("Cena exportada (.scn oficial): {}", path.display()));
        } else {
            self.toast("Erro ao salvar arquivo .scn");
        }
    }

    pub fn import_scn_file(&mut self, path: &PathBuf) {
        let Ok(bytes) = fs::read(path) else {
            self.toast(format!("Erro ao ler arquivo: {}", path.display()));
            return;
        };

        if let Some(scn) = decode_official_scn(&bytes) {
            self.push_undo(format!("Importar {}", scn.name));
            self.save_auto_backup();
            self.regs = scn.regs;
            let idx = self.active_scene_idx;
            self.scenes[idx] = scn.regs;
            if !scn.name.trim().is_empty() {
                self.scene_names[idx] = scn.name.clone();
            }
            for (i, t) in scn.text.iter().enumerate().take(60) {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    self.channel_names[i] = trimmed.to_string();
                } else if i < 24 {
                    self.channel_names[i] = format!("CH{}", i + 1);
                }
            }
            let _ = save_preset_bin(&self.preset_bin_path, &self.scenes);
            self.sync_full_console_to_hardware();
            self.toast(format!("Cena '{}' importada com sucesso!", scn.name));
        } else if let Some(regs) = decode_scene_block(&bytes) {
            self.regs = regs;
            self.scenes[self.active_scene_idx] = regs;
            let _ = save_preset_bin(&self.preset_bin_path, &self.scenes);
            self.sync_full_console_to_hardware();
            self.toast(format!("Cena importada (bloco): {}", path.display()));
        } else {
            self.toast("Erro: arquivo .scn inválido ou corrompido!");
        }
    }

    /// Channel Copy/Paste (fcn.00443170 / 00443390 / 004435e0)
    pub fn paste_channel_from(&mut self, src: SelectedChannel, dst: SelectedChannel) {
        self.push_undo(format!("Colar canal em {}", self.channel_name(dst)));
        match (src, dst) {
            (SelectedChannel::Input(s), SelectedChannel::Input(d)) if s < 24 && d < 24 => {
                for p in 0..0x3E {
                    let v = self.get_reg(map_reg(1, s, p));
                    self.set_group_param(1, d, p, v);
                }
                self.toast(format!("Copiado CH{} -> CH{}!", s + 1, d + 1));
            }
            (SelectedChannel::Output(s), SelectedChannel::Output(d)) => {
                for p in 0x3E..0x7C {
                    let v = self.get_reg(map_reg(3, s, p));
                    self.set_group_param(3, d, p, v);
                }
                self.output_geq_db[d] = self.output_geq_db[s];
                self.toast("Configurações de saída coladas!");
            }
            _ => {
                self.toast("Copie apenas entre canais do mesmo tipo (Entrada->Entrada ou Saída->Saída).");
            }
        }
    }

    // =========================================================================
    // DEMO MODE SIMULATION & HARDWARE SYNC
    // =========================================================================
    pub fn apply_dynamics_preset(&mut self, sel: SelectedChannel, preset_idx: usize) {
        self.push_undo("Preset Dinâmica".to_string());
        let (group, ch) = sel.group_and_index();
        let c_grp = if group == 3 { 3 } else { 1 };
        let c_ch = if group == 3 { ch } else { ch.min(25) };
        match preset_idx {
            0 => {
                // Voz Suave
                if c_grp == 1 {
                    self.set_input_gate_enabled(c_ch, true);
                    self.set_input_gate_threshold_db(c_ch, -48.0);
                    self.set_input_gate_attack_ms(c_ch, 5.0);
                    self.set_input_gate_release_ms(c_ch, 180.0);
                }
                self.set_comp_enabled(c_grp, c_ch, true);
                self.set_comp_threshold_db(c_grp, c_ch, -18.0);
                self.set_comp_ratio(c_grp, c_ch, 3.0);
                self.set_comp_attack_ms(c_grp, c_ch, 12.0);
                self.set_comp_release_ms(c_grp, c_ch, 140.0);
                self.toast("Dinâmica: Voz Suave aplicada!");
            }
            1 => {
                // Bumbo Seco
                if c_grp == 1 {
                    self.set_input_gate_enabled(c_ch, true);
                    self.set_input_gate_threshold_db(c_ch, -34.0);
                    self.set_input_gate_attack_ms(c_ch, 1.0);
                    self.set_input_gate_release_ms(c_ch, 90.0);
                }
                self.set_comp_enabled(c_grp, c_ch, true);
                self.set_comp_threshold_db(c_grp, c_ch, -16.0);
                self.set_comp_ratio(c_grp, c_ch, 4.5);
                self.set_comp_attack_ms(c_grp, c_ch, 15.0);
                self.set_comp_release_ms(c_grp, c_ch, 80.0);
                self.toast("Dinâmica: Bumbo Seco aplicada!");
            }
            2 => {
                // Locução / Pastor
                if c_grp == 1 {
                    self.set_input_gate_enabled(c_ch, true);
                    self.set_input_gate_threshold_db(c_ch, -52.0);
                    self.set_input_gate_attack_ms(c_ch, 8.0);
                    self.set_input_gate_release_ms(c_ch, 240.0);
                }
                self.set_comp_enabled(c_grp, c_ch, true);
                self.set_comp_threshold_db(c_grp, c_ch, -22.0);
                self.set_comp_ratio(c_grp, c_ch, 4.0);
                self.set_comp_attack_ms(c_grp, c_ch, 6.0);
                self.set_comp_release_ms(c_grp, c_ch, 160.0);
                self.toast("Dinâmica: Locução / Pastor aplicada!");
            }
            _ => {
                if c_grp == 1 {
                    self.set_input_gate_enabled(c_ch, false);
                }
                self.set_comp_enabled(c_grp, c_ch, false);
                self.toast("Dinâmica em Bypass!");
            }
        }
    }

    pub fn reconnect_last_hardware(&mut self) {
        let mode = self.hw_link.mode.clone();
        self.hw_link.connect(mode);
        self.last_net_rx_time = self.sim_time;
        self.net_link_lost = false;
        self.toast("Reconectando à mesa...");
    }

    pub fn clear_all_clips(&mut self) {
        self.channel_clip_hold = [false; 60];
    }

    /// Switches the physical mixer's Digital/Bluetooth input source (opcode 0x72)
    pub fn set_media_source(&mut self, src: MediaSource) {
        self.media.source = src;
        if !matches!(self.hw_link.mode, crate::protocol::ConnectionMode::Demo) {
            self.hw_link.send_frame(build_media_source_frame(src as u8));
        }
        self.toast(format!("Fonte Digital selecionada: {} (canal DIGI)", src.label()));
    }

    fn channel_eq_dyn_regs(sel: SelectedChannel) -> Vec<usize> {
        let (group, ch) = sel.group_and_index();
        let mut regs = Vec::new();
        match group {
            1 => {
                // Gate (0x02..=0x07), Comp (0x1D..=0x23), EQ + HPF/LPF (0x24..=0x3D)
                for p in 0x02..=0x07 {
                    regs.push(map_reg(1, ch, p));
                }
                for p in 0x1D..=0x23 {
                    regs.push(map_reg(1, ch, p));
                }
                for p in 0x24..=0x3D {
                    regs.push(map_reg(1, ch, p));
                }
            }
            2 => {
                // FX 4-band PEQ + HPF/LPF (0xB9..=0xD2)
                for p in 0xB9..=0xD2 {
                    regs.push(map_reg(2, ch, p));
                }
            }
            3 => {
                // Output Comp (0x44..=0x49), EQ + HPF/LPF + 7-band PEQ (0x4D..=0x73)
                for p in 0x44..=0x73 {
                    regs.push(map_reg(3, ch, p));
                }
            }
            _ => {}
        }
        regs
    }

    pub fn is_ab_b_active(&self, sel: SelectedChannel) -> bool {
        let id = sel.flat_id();
        id < self.ab_slots.len() && matches!(self.ab_slots[id], Some((_, _, true)))
    }

    pub fn toggle_ab_compare(&mut self, sel: SelectedChannel) {
        let id = sel.flat_id();
        if id >= self.ab_slots.len() {
            return;
        }
        let reg_ids = Self::channel_eq_dyn_regs(sel);
        if reg_ids.is_empty() {
            return;
        }
        let current: Vec<(usize, u16)> = reg_ids.iter().map(|&r| (r, self.get_reg(r))).collect();
        match self.ab_slots[id].take() {
            None => {
                // First press: store current as A and B, switch to B so user can tweak B and compare against A
                self.ab_slots[id] = Some((current.clone(), current, true));
                self.toast(format!("Comparação A/B: Editando B em {} (A salvo)", self.channel_name(sel)));
            }
            Some((mut slot_a, mut slot_b, is_b)) => {
                if is_b {
                    slot_b = current;
                    for &(r, v) in &slot_a {
                        self.set_reg(r, v);
                    }
                    self.ab_slots[id] = Some((slot_a, slot_b, false));
                    self.toast(format!("A/B: Ouvindo [A] (Original) em {}", self.channel_name(sel)));
                } else {
                    slot_a = current;
                    for &(r, v) in &slot_b {
                        self.set_reg(r, v);
                    }
                    self.ab_slots[id] = Some((slot_a, slot_b, true));
                    self.toast(format!("A/B: Ouvindo [B] (Editado) em {}", self.channel_name(sel)));
                }
            }
        }
    }

    pub fn copy_a_to_b(&mut self, sel: SelectedChannel) {
        let id = sel.flat_id();
        if id >= self.ab_slots.len() {
            return;
        }
        let reg_ids = Self::channel_eq_dyn_regs(sel);
        if reg_ids.is_empty() {
            return;
        }
        let current: Vec<(usize, u16)> = reg_ids.iter().map(|&r| (r, self.get_reg(r))).collect();
        self.ab_slots[id] = Some((current.clone(), current, false));
        self.toast(format!("A/B sincronizado em {} (A = B)", self.channel_name(sel)));
    }

    pub fn is_hpf_active(&self, sel: SelectedChannel) -> bool {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            return false;
        }
        let (_, hpf_en, _, _, _, _) = Self::eq_param_offsets(group);
        self.eq_enabled(group, ch)
            && self.get_reg(map_reg(group, ch, hpf_en)) != 0
            && self.hpf_freq_hz(group, ch) >= 45.0
    }

    /// 1-click HPF 100Hz 18dB/oct toggle for quick low-end cleanup (vocals, guitars, overheads)
    pub fn toggle_quick_hpf_100hz(&mut self, sel: SelectedChannel) {
        let (group, ch) = sel.group_and_index();
        if group == 4 {
            return;
        }
        let (_, hpf_en, _, _, _, _) = Self::eq_param_offsets(group);
        if self.is_hpf_active(sel) {
            self.set_group_param(group, ch, hpf_en, 0);
            self.set_hpf_freq_hz(group, ch, 20.0);
            self.toast(format!("HPF desligado em {}", self.channel_name(sel)));
        } else {
            self.set_eq_enabled(group, ch, true);
            self.set_group_param(group, ch, hpf_en, 1);
            self.set_hpf_family_slope(group, ch, FilterFamily::Butterworth, FilterSlope::Slope18dB);
            self.set_hpf_freq_hz(group, ch, 100.0);
            self.toast(format!("HPF 100 Hz (18dB/oct) ativo em {}!", self.channel_name(sel)));
        }
    }

    /// Cleans FX return mud & harsh highs (HPF 180Hz + LPF 7kHz) per live-sound best practice
    pub fn clean_fx_return_eq(&mut self, fx_idx: usize) {
        let fx = fx_idx.min(1);
        let (_, hpf_en, _, _, _, _) = Self::eq_param_offsets(2);
        self.set_eq_enabled(2, fx, true);
        self.set_group_param(2, fx, hpf_en, 1);
        self.set_hpf_family_slope(2, fx, FilterFamily::Butterworth, FilterSlope::Slope18dB);
        self.set_hpf_freq_hz(2, fx, 180.0);
        self.set_lpf_family_slope(2, fx, FilterFamily::Butterworth, FilterSlope::Slope12dB);
        self.set_lpf_freq_hz(2, fx, 7000.0);
        self.toast(format!(
            "Volta FX{} limpa: HPF 180 Hz + LPF 7.0 kHz aplicados!",
            fx + 1
        ));
    }

    pub fn load_multitrack_files(&mut self, paths: &[PathBuf]) {
        match self.multitrack.load_wav_paths(paths) {
            Ok(count) => {
                if self.multitrack_auto_name {
                    let tracks = self.multitrack.tracks.lock().unwrap().clone();
                    for tr in &tracks {
                        if tr.channel_idx < 24 {
                            self.set_channel_name(SelectedChannel::Input(tr.channel_idx), tr.name.clone());
                        }
                    }
                }
                self.toast(format!("Multitrack carregado: {} faixas prontas para mixar!", count));
            }
            Err(e) => {
                self.toast(e);
            }
        }
    }

    pub fn sync_multitrack_dsp(&self) {
        use crate::multitrack::{ChannelDspParams, ConsoleDspSnapshot, MT_MAX_CHANNELS};
        let mut snap = ConsoleDspSnapshot::default();
        for ch in 0..MT_MAX_CHANNELS {
            let sel = SelectedChannel::Input(ch);
            let (_, hpf_en, _, _, _, _) = Self::eq_param_offsets(1);
            let (hpf_fam, hpf_slp) = self.hpf_family_slope(1, ch);
            let (lpf_fam, lpf_slp) = self.lpf_family_slope(1, ch);
            let mut peq = [(PeqBandMode::Peaking, 1000.0, 0.0, 1.0); 7];
            for b_i in 0..4 {
                let b = self.peq_band(1, ch, b_i);
                peq[b_i] = (b.mode, b.freq_hz, b.gain_db, b.q);
            }
            snap.channels[ch] = ChannelDspParams {
                gain_db: self.input_preamp_gain_db(ch),
                fader_db: self.channel_fader_db(sel),
                muted: self.channel_muted(sel),
                solo: self.channel_solo(sel),
                pan: self.input_pan(ch),
                phase_invert: self.input_phase_invert(ch),
                to_main_lr: self.input_to_main_lr(ch),
                gate_enabled: self.input_gate_enabled(ch),
                gate_threshold_db: self.input_gate_threshold_db(ch),
                comp_enabled: self.comp_enabled(1, ch),
                comp_threshold_db: self.comp_threshold_db(1, ch),
                comp_ratio: self.comp_ratio(1, ch),
                comp_gain_db: self.comp_gain_db(1, ch),
                eq_enabled: self.eq_enabled(1, ch),
                hpf_enabled: self.get_reg(map_reg(1, ch, hpf_en)) != 0,
                hpf_freq: self.hpf_freq_hz(1, ch),
                hpf_family: hpf_fam,
                hpf_slope: hpf_slp,
                lpf_enabled: self.lpf_freq_hz(1, ch) < 19900.0,
                lpf_freq: self.lpf_freq_hz(1, ch),
                lpf_family: lpf_fam,
                lpf_slope: lpf_slp,
                peq_count: 4,
                is_output: false,
                peq,
            };
        }
        let build_out = |out_ch: usize| -> ChannelDspParams {
            let (_, hpf_en, _, _, _, _) = Self::eq_param_offsets(3);
            let (hpf_fam, hpf_slp) = self.hpf_family_slope(3, out_ch);
            let (lpf_fam, lpf_slp) = self.lpf_family_slope(3, out_ch);
            let mut peq = [(PeqBandMode::Peaking, 1000.0, 0.0, 1.0); 7];
            for (b_i, slot) in peq.iter_mut().enumerate() {
                let b = self.peq_band(3, out_ch, b_i);
                *slot = (b.mode, b.freq_hz, b.gain_db, b.q);
            }
            ChannelDspParams {
                comp_enabled: self.comp_enabled(3, out_ch),
                comp_threshold_db: self.comp_threshold_db(3, out_ch),
                comp_ratio: self.comp_ratio(3, out_ch),
                comp_gain_db: self.comp_gain_db(3, out_ch),
                eq_enabled: self.eq_enabled(3, out_ch),
                hpf_enabled: self.get_reg(map_reg(3, out_ch, hpf_en)) != 0,
                hpf_freq: self.hpf_freq_hz(3, out_ch),
                hpf_family: hpf_fam,
                hpf_slope: hpf_slp,
                lpf_enabled: self.lpf_freq_hz(3, out_ch) < 19900.0,
                lpf_freq: self.lpf_freq_hz(3, out_ch),
                lpf_family: lpf_fam,
                lpf_slope: lpf_slp,
                peq_count: 7,
                is_output: true,
                peq,
                ..ChannelDspParams::default()
            }
        };
        snap.main_l = build_out(8);
        snap.main_r = build_out(9);
        snap.main_l_db = self.channel_fader_db(SelectedChannel::Output(8));
        snap.main_r_db = self.channel_fader_db(SelectedChannel::Output(9));
        snap.main_l_muted = self.channel_muted(SelectedChannel::Output(8));
        snap.main_r_muted = self.channel_muted(SelectedChannel::Output(9));
        *self.multitrack.snapshot.lock().unwrap() = snap;
    }

    pub fn save_user_prefs(&self) {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "DARK_THEME={}", if self.dark_theme { 1 } else { 0 });
        let _ = writeln!(out, "AUTOMIX_ON={}", if self.automix_enabled { 1 } else { 0 });
        let _ = writeln!(
            out,
            "FW_LAYOUT={}",
            match self.hw_link.firmware_layout {
                FirmwareLayout::Ax24e2511 => "AX24E",
                FirmwareLayout::Legacy => "LEGACY",
            }
        );
        for i in 0..60 {
            if !self.channel_names[i].is_empty() {
                let clean = self.channel_names[i].replace('\n', " ").replace('\r', "");
                let _ = writeln!(out, "NAME_{}={}", i, clean);
            }
            let _ = writeln!(out, "COLOR_{}={}", i, self.channel_colors[i]);
        }
        for i in 0..24 {
            if self.custom_link_groups[i] != 0 {
                let _ = writeln!(out, "LINK_{}={}", i, self.custom_link_groups[i]);
            }
            if self.automix_channels[i] {
                let _ = writeln!(out, "AUTOMIX_CH_{}=1", i);
            }
        }
        for i in 0..42 {
            if self.recall_safe_channels[i] {
                let _ = writeln!(out, "SAFE_{}=1", i);
            }
        }
        for b in &self.midi.bindings {
            let _ = writeln!(
                out,
                "MIDI={},{},{},{},{}",
                b.channel,
                if b.is_note { 1 } else { 0 },
                b.number,
                b.target_flat_id,
                if b.is_mute { 1 } else { 0 }
            );
        }
        let _ = fs::write("axios24_prefs.ini", out);
    }

    pub fn load_user_prefs(&mut self) {
        let Ok(txt) = fs::read_to_string("axios24_prefs.ini") else {
            return;
        };
        for line in txt.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if k == "DARK_THEME" {
                self.dark_theme = v.trim() != "0";
            } else if k == "AUTOMIX_ON" {
                self.automix_enabled = v.trim() == "1";
            } else if k == "FW_LAYOUT" {
                self.hw_link.firmware_layout = if v.trim().eq_ignore_ascii_case("LEGACY") {
                    FirmwareLayout::Legacy
                } else {
                    FirmwareLayout::Ax24e2511
                };
            } else if let Some(idx_s) = k.strip_prefix("NAME_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    if idx < 60 && !v.trim().is_empty() {
                        self.channel_names[idx] = v.trim().to_string();
                    }
                }
            } else if let Some(idx_s) = k.strip_prefix("COLOR_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    if let Ok(c) = v.trim().parse::<u8>() {
                        if idx < 60 {
                            self.channel_colors[idx] = c % 32;
                        }
                    }
                }
            } else if let Some(idx_s) = k.strip_prefix("LINK_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    if let Ok(g) = v.trim().parse::<u8>() {
                        if idx < 24 {
                            self.custom_link_groups[idx] = g.min(4);
                        }
                    }
                }
            } else if let Some(idx_s) = k.strip_prefix("AUTOMIX_CH_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    if idx < 24 {
                        self.automix_channels[idx] = v.trim() == "1";
                    }
                }
            } else if let Some(idx_s) = k.strip_prefix("SAFE_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    if idx < 42 {
                        self.recall_safe_channels[idx] = v.trim() == "1";
                    }
                }
            } else if k == "MIDI" {
                let p: Vec<&str> = v.split(',').collect();
                if p.len() == 5 {
                    if let (Ok(ch), Ok(note), Ok(num), Ok(id), Ok(mute)) = (
                        p[0].parse::<u8>(),
                        p[1].parse::<u8>(),
                        p[2].parse::<u8>(),
                        p[3].parse::<usize>(),
                        p[4].parse::<u8>(),
                    ) {
                        self.midi.bindings.push(crate::midi::MidiBinding {
                            channel: ch.min(15),
                            is_note: note != 0,
                            number: num.min(127),
                            target_flat_id: id.min(41),
                            is_mute: mute != 0,
                        });
                    }
                }
            }
        }
    }

    pub fn set_demo_running(&mut self, run: bool) {
        self.demo_running = run;
        self.hw_link.send_frame(build_demo_cmd_frame(run));
        if run {
            self.toast("Modo Demo Ativo (Medidores e RTA simulados)");
        } else {
            self.toast("Modo Demo Pausado");
        }
    }

    pub fn refresh_discovery(&mut self) {
        self.available_serial_ports = crate::protocol::list_serial_ports();
        self.local_pc_ip = crate::protocol::detect_local_ipv4().map(|ip| ip.to_string());
        self.discovery_scanning = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.discovery_rx = Some(rx);
        std::thread::spawn(move || {
            let devs = probe_udp_devices();
            let _ = tx.send(devs);
        });
    }

    pub fn apply_network_config(&mut self) {
        let kv = if self.net_cfg.wifi_mode_sta {
            format!(
                "WIFI_MODE=sta\nSTA_SSID={}\nSTA_KEY={}\n",
                self.net_cfg.sta_ssid, self.net_cfg.sta_key
            )
        } else {
            format!(
                "WIFI_MODE=ap\nAP_SSID={}\nAP_KEY={}\n",
                self.net_cfg.ap_ssid, self.net_cfg.ap_key
            )
        };
        self.hw_link.send_frame(build_kv_frame(&kv, true));
        self.toast("Configuração de rede enviada! Reinicie o equipamento para aplicar.");
    }

    pub fn check_for_updates(&mut self) {
        if self.update_checking {
            return;
        }
        self.update_checking = true;
        self.update_status = "Verificando atualizações no GitHub...".to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_check_rx = Some(rx);
        std::thread::spawn(move || {
            let res = crate::updater::fetch_latest_release();
            let _ = tx.send(res);
        });
    }

    pub fn start_hot_update(&mut self) {
        if self.update_downloading {
            return;
        }
        let Some(info) = self.update_info.clone() else {
            return;
        };
        self.update_downloading = true;
        self.update_status = format!("Baixando e aplicando v{}...", info.version);
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_apply_rx = Some(rx);
        std::thread::spawn(move || {
            let res = crate::updater::download_and_hot_update(&info.download_url);
            let _ = tx.send(res);
        });
    }

    pub fn tick(&mut self, dt: f32) {
        self.sim_time += dt as f64;

        // Poll non-blocking GitHub Update check & download
        if let Some(rx) = &self.update_check_rx {
            if let Ok(res) = rx.try_recv() {
                self.update_checking = false;
                self.update_check_rx = None;
                match res {
                    Ok(Some(info)) => {
                        self.update_status = format!("Nova atualização disponível: v{}!", info.version);
                        self.update_info = Some(info);
                        self.show_update_modal = true;
                    }
                    Ok(None) => {
                        self.update_status = format!(
                            "Você já está na última versão (v{}).",
                            crate::updater::CURRENT_VERSION
                        );
                    }
                    Err(e) => {
                        self.update_status = format!("Erro ao verificar atualização: {}", e);
                    }
                }
            }
        }
        if let Some(rx) = &self.update_apply_rx {
            if let Ok(res) = rx.try_recv() {
                self.update_downloading = false;
                self.update_apply_rx = None;
                if let Err(e) = res {
                    self.update_status = e.clone();
                    self.toast(e);
                }
            }
        }

        // Poll non-blocking LAN / Wi-Fi discovery scan
        if let Some(rx) = &self.discovery_rx {
            if let Ok(devs) = rx.try_recv() {
                self.discovery_scanning = false;
                self.discovery_rx = None;
                if let Some(first) = devs.first() {
                    self.connect_ip = first.ip.clone();
                }
                self.discovered_devices = devs;
            }
        }

        // Poll hardware events & monitor ping/link health
        let mut got_hw_packet = false;
        for ev in self.hw_link.poll_events() {
            match ev {
                NetEvent::ParamUpdates(pairs) => {
                    got_hw_packet = true;
                    if pairs.iter().any(|&(r, _)| r as usize >= NUM_REGS) {
                        self.hw_link.firmware_layout = FirmwareLayout::Ax24e2511;
                    }
                    let layout = self.hw_link.firmware_layout;
                    for (reg, val) in pairs {
                        if let Some(r) = layout.from_device(reg) {
                            self.regs[r as usize] = val;
                        }
                    }
                }
                NetEvent::ChannelName { ch, name } => {
                    got_hw_packet = true;
                    if ch < self.channel_names.len() {
                        self.channel_names[ch] = name;
                    }
                }
                NetEvent::SceneName { slot, name } => {
                    got_hw_packet = true;
                    if (1..=16).contains(&slot) {
                        self.scene_names[slot - 1] = name;
                    }
                }
                NetEvent::RtaLevels(bands) => {
                    got_hw_packet = true;
                    for (i, &b) in bands.iter().enumerate() {
                        let db = -85.0 + (b as f32 / 100.0) * 85.0;
                        let idx1 = (i * 2).min(self.rta_bins_db.len() - 1);
                        let idx2 = (i * 2 + 1).min(self.rta_bins_db.len() - 1);
                        self.rta_bins_db[idx1] = db;
                        self.rta_bins_db[idx2] = db;
                    }
                    let peak_hw = bands.iter().copied().max().unwrap_or(0) as f32;
                    let master_db = if peak_hw <= 1.0 {
                        -90.0
                    } else {
                        (-60.0 + (peak_hw / 100.0) * 60.0).clamp(-90.0, 2.0)
                    };
                    self.channel_meters_db[36] = master_db;
                    self.channel_meters_db[37] = master_db;
                }
                NetEvent::DeviceDiscovered(dev) => {
                    got_hw_packet = true;
                    if let Some(layout) = FirmwareLayout::detect(&dev.name, &dev.fw_version) {
                        self.hw_link.firmware_layout = layout;
                    }
                    if !self.discovered_devices.iter().any(|d| d.ip == dev.ip) {
                        self.discovered_devices.push(dev);
                    }
                }
                NetEvent::Connected(info) => {
                    got_hw_packet = true;
                    // Stop offline demo meter animation when connected to live hardware/emulator
                    self.demo_running = false;
                    self.channel_meters_db = [-90.0; 60];
                    self.rta_bins_db = [-85.0; 64];
                    self.hw_link.read_all_registers();
                    self.toast(format!("Conectado: {} (lendo cena atual da mesa...)", info));
                }
                NetEvent::Error(err) => {
                    self.net_link_lost = true;
                    self.toast(err);
                }
            }
        }
        if got_hw_packet {
            let gap = ((self.sim_time - self.last_net_rx_time) * 1000.0).round() as u32;
            self.net_ping_ms = gap.clamp(1, 999);
            self.last_net_rx_time = self.sim_time;
            self.net_link_lost = false;
        } else if !matches!(self.hw_link.mode, crate::protocol::ConnectionMode::Demo) {
            let silent_secs = self.sim_time - self.last_net_rx_time;
            if silent_secs > 4.5 {
                self.net_link_lost = true;
            }
        } else {
            self.net_link_lost = false;
            self.net_ping_ms = 0;
        }

        // Poll USB MIDI controller messages (Faders / Mutes / MIDI Learn)
        for msg in self.midi.poll_messages() {
            let kind = msg.kind();
            let is_cc = kind == 0xB0;
            let is_note = kind == 0x90 && msg.data2 > 0;
            if !is_cc && !is_note {
                continue;
            }
            let ch = msg.channel();
            self.midi.last_event_label = format!(
                "Ch{} {} #{} Val {}",
                ch + 1,
                if is_note { "Note" } else { "CC" },
                msg.data1,
                msg.data2
            );
            if let Some((target_id, is_mute)) = self.midi.learn_target.take() {
                self.midi.bindings.retain(|b| {
                    !(b.channel == ch && b.is_note == is_note && b.number == msg.data1)
                });
                self.midi.bindings.push(crate::midi::MidiBinding {
                    channel: ch,
                    is_note,
                    number: msg.data1,
                    target_flat_id: target_id,
                    is_mute,
                });
                self.toast(format!("MIDI Learn gravado: {}", self.midi.last_event_label));
                continue;
            }

            let matched: Vec<(usize, bool)> = self
                .midi
                .bindings
                .iter()
                .filter(|b| b.channel == ch && b.is_note == is_note && b.number == msg.data1)
                .map(|b| (b.target_flat_id, b.is_mute))
                .collect();
            if !matched.is_empty() {
                for (flat_id, is_mute) in matched {
                    let target_sel = SelectedChannel::from_flat_id(flat_id);
                    if is_mute {
                        let m = self.channel_muted(target_sel);
                        self.set_channel_muted(target_sel, !m);
                    } else if is_cc {
                        let norm = (msg.data2 as f32) / 127.0;
                        let db = if norm <= 0.01 {
                            -120.0
                        } else {
                            (-60.0 + norm * 70.0).clamp(-120.0, 10.0)
                        };
                        self.set_effective_fader_db(target_sel, (db * 10.0).round() / 10.0);
                    }
                }
            } else if is_cc && (1..=8).contains(&msg.data1) {
                // Built-in default mapping: CC 1..8 control the first 8 channels of current layer!
                let base_offset = match self.layer {
                    FaderLayer::Ch9_16 => 8,
                    FaderLayer::Ch17_24 => 16,
                    _ => 0,
                };
                let target_sel = SelectedChannel::Input(base_offset + (msg.data1 as usize - 1));
                let norm = (msg.data2 as f32) / 127.0;
                let db = if norm <= 0.01 { -120.0 } else { (-60.0 + norm * 70.0).clamp(-120.0, 10.0) };
                self.set_effective_fader_db(target_sel, (db * 10.0).round() / 10.0);
            } else if is_cc && msg.data1 == 9 {
                let norm = (msg.data2 as f32) / 127.0;
                let db = if norm <= 0.01 { -120.0 } else { (-60.0 + norm * 70.0).clamp(-120.0, 10.0) };
                self.set_channel_fader_db(SelectedChannel::Output(8), (db * 10.0).round() / 10.0);
            }
        }

        // Media player progress simulation
        if self.media.playing && !self.media.tracks.is_empty() {
            self.media.position_secs += dt;
            let dur = self.media.tracks[self.media.current_track].duration_secs as f32;
            if self.media.position_secs >= dur {
                self.media.position_secs = 0.0;
                match self.media.repeat_mode {
                    RepeatMode::RepeatOne => {}
                    RepeatMode::RepeatAll => {
                        self.media.current_track = (self.media.current_track + 1) % self.media.tracks.len();
                    }
                    RepeatMode::Random => {
                        self.media.current_track = ((self.sim_time * 97.0) as usize) % self.media.tracks.len();
                    }
                }
            }
        }
        if self.media.recording {
            self.media.rec_seconds += dt;
        }

        // Demo mode animated level meters & RTA spectrum (or real Multitrack WAV meters when playing)
        let mt_playing = self.multitrack.playing.load(std::sync::atomic::Ordering::Relaxed);
        if mt_playing {
            self.sync_multitrack_dsp();
            let mt_m = *self.multitrack.channel_meters_db.lock().unwrap();
            for ch in 0..24 {
                let prev = self.channel_meters_db[ch];
                self.channel_meters_db[ch] = if mt_m[ch] > prev {
                    mt_m[ch]
                } else {
                    (prev - dt * 28.0).max(mt_m[ch])
                };
            }
            self.channel_meters_db[36] = mt_m[24]; // MAIN L
            self.channel_meters_db[37] = mt_m[25]; // MAIN R
        } else if self.demo_running {
            let t = self.sim_time as f32;
            for i in 0..60 {
                let sel = match i {
                    0..=25 => SelectedChannel::Input(i),
                    26..=27 => SelectedChannel::Fx(i - 26),
                    28..=37 => SelectedChannel::Output(i - 28),
                    38..=41 => SelectedChannel::Dca(i - 38),
                    _ => SelectedChannel::Input(0),
                };
                if self.channel_muted(sel) {
                    self.channel_meters_db[i] = -90.0;
                    continue;
                }
                let fader = self.channel_fader_db(sel);
                let phase = (i as f32) * 0.73;
                let wave = (t * (2.4 + (i % 5) as f32 * 0.35) + phase).sin() * 0.5
                    + (t * 6.1 + phase * 1.7).cos() * 0.3
                    + (t * 11.3 + phase * 0.4).sin() * 0.2;
                let base_signal = -18.0 + wave * 14.0;
                let level = if fader <= -110.0 && i < 28 {
                    // Show pre-fader input activity even when fader is down if preamp > 0
                    base_signal - 12.0
                } else {
                    (base_signal + (fader * 0.65)).clamp(-85.0, 3.0)
                };
                // Smooth ballistic decay
                let prev = self.channel_meters_db[i];
                self.channel_meters_db[i] = if level > prev {
                    level
                } else {
                    (prev - dt * 28.0).max(level)
                };
            }

            // RTA 64-band spectrum shaped by the currently selected channel's EQ!
            let (grp, ch) = self.selected_channel.group_and_index();
            let eq_on = self.eq_enabled(grp.max(1), ch);
            let band_count = Self::peq_band_count(grp.max(1));
            for b in 0..64 {
                let norm_x = (b as f32) / 63.0;
                let freq = crate::dsp::norm_x_to_freq(norm_x);
                // Natural pink-ish music spectrum roll-off + animated harmonics
                let pink_slope = -3.0 * (freq / 200.0).log2().abs();
                let harmonic = ((t * 4.2 + b as f32 * 0.31).sin() * 5.5)
                    + ((t * 9.7 - b as f32 * 0.53).cos() * 3.5);
                let mut eq_boost = 0.0;
                if eq_on {
                    for idx in 0..band_count {
                        let pb = self.peq_band(grp.max(1), ch, idx);
                        let bq = crate::dsp::peq_band_biquad(pb.mode, pb.freq_hz, pb.gain_db, pb.q);
                        eq_boost += bq.eval_db(freq as f64) as f32;
                    }
                }
                let target = (-24.0 + pink_slope + harmonic + eq_boost).clamp(-78.0, -1.0);
                let prev = self.rta_bins_db[b];
                self.rta_bins_db[b] = if target > prev {
                    prev * 0.4 + target * 0.6
                } else {
                    (prev - dt * 24.0).max(target)
                };
            }
            if !self.live_rta.active {
                let mut best_b = 32usize;
                let mut best_v = -90.0_f32;
                for (b, &v) in self.rta_bins_db.iter().enumerate().skip(6) {
                    if v > best_v {
                        best_v = v;
                        best_b = b;
                    }
                }
                self.rta_peak_freq_hz = crate::dsp::norm_x_to_freq(best_b as f32 / 63.0);
                self.rta_peak_db = best_v;
            }
        } else {
            for i in 0..42 {
                let sel = SelectedChannel::from_flat_id(i);
                if self.channel_meters_db[i] >= -0.5 && !self.channel_muted(sel) {
                    self.channel_clip_hold[i] = true;
                }
            }
            for m in &mut self.channel_meters_db {
                *m = (*m - dt * 40.0).max(-90.0);
            }
            if !self.live_rta.active {
                for r in &mut self.rta_bins_db {
                    *r = (*r - dt * 40.0).max(-85.0);
                }
            }
        }

        // If Real Audio FFT RTA (USB / Mic) is active, override RTA bins with true 2048-point FFT spectrum!
        if self.live_rta.active {
            let snap = self.live_rta.latest_snapshot();
            for b in 0..64 {
                let target = snap.bins_db[b];
                let prev = self.rta_bins_db[b];
                self.rta_bins_db[b] = if target > prev {
                    prev * 0.35 + target * 0.65
                } else {
                    (prev - dt * 32.0).max(target)
                };
            }
            self.rta_peak_freq_hz = snap.peak_freq_hz;
            self.rta_peak_db = snap.peak_db;
            // Also reflect real input RMS on selected channel meter when demo is paused
            if !self.demo_running && snap.input_rms_db > -85.0 {
                let flat = self.selected_channel.flat_id();
                self.channel_meters_db[flat] = snap.input_rms_db;
            }
        }

        // Update persistent CLIP hold & live Compressor Gain Reduction (GR) across all channels
        for i in 0..42 {
            let sel = SelectedChannel::from_flat_id(i);
            if self.channel_meters_db[i] >= -0.5 && !self.channel_muted(sel) {
                self.channel_clip_hold[i] = true;
            }
            let (c_grp, c_ch) = sel.group_and_index();
            if (c_grp == 1 || c_grp == 3) && self.comp_enabled(c_grp, c_ch) {
                let th = self.comp_threshold_db(c_grp, c_ch);
                let ratio = self.comp_ratio(c_grp, c_ch).max(1.0);
                let over = (self.channel_meters_db[i] - th).max(0.0);
                let gr = over * (1.0 - 1.0 / ratio);
                self.comp_gr_db[i] = gr.clamp(0.0, 24.0);
            } else {
                self.comp_gr_db[i] = 0.0;
            }
        }

        // Dugan-style Auto-Mixer gain sharing across enabled speech channels
        if self.automix_enabled {
            let active_chs: Vec<usize> = (0..24)
                .filter(|&c| self.automix_channels[c] && !self.channel_muted(SelectedChannel::Input(c)))
                .collect();
            if active_chs.len() >= 2 {
                let mut sum_lin = 0.0_f32;
                for &c in &active_chs {
                    sum_lin += 10.0_f32.powf(self.channel_meters_db[c] / 20.0);
                }
                let sum_lin = sum_lin.max(1e-6);
                for c in 0..24 {
                    if self.automix_channels[c] {
                        let ch_lin = 10.0_f32.powf(self.channel_meters_db[c] / 20.0);
                        let ratio = (ch_lin / sum_lin).clamp(0.25, 1.0);
                        let atten = (20.0 * ratio.log10()).clamp(-12.0, 0.0);
                        self.automix_atten_db[c] =
                            self.automix_atten_db[c] * 0.8 + atten * 0.2;
                    } else {
                        self.automix_atten_db[c] = 0.0;
                    }
                }
            } else {
                self.automix_atten_db = [0.0; 24];
            }
        } else {
            self.automix_atten_db = [0.0; 24];
        }
    }

    // =========================================================================
    // PRO WORKFLOW & FORUM COMPLAINT FIXES
    // =========================================================================

    /// Effective fader value depending on whether Sends on Fader (SOF) is active
    pub fn effective_fader_db(&self, sel: SelectedChannel) -> f32 {
        match (self.sends_on_fader, sel) {
            (SendsOnFaderTarget::MainLR, _) => self.channel_fader_db(sel),
            (SendsOnFaderTarget::Aux(aux), SelectedChannel::Input(ch)) => {
                self.input_aux_send_db(ch, aux)
            }
            (SendsOnFaderTarget::Aux(aux), SelectedChannel::Fx(fx)) => self.fx_aux_send_db(fx, aux),
            (SendsOnFaderTarget::Fx(fx), SelectedChannel::Input(ch)) => {
                self.input_fx_send_db(ch, fx)
            }
            _ => self.channel_fader_db(sel),
        }
    }

    pub fn set_effective_fader_db(&mut self, sel: SelectedChannel, db: f32) {
        match (self.sends_on_fader, sel) {
            (SendsOnFaderTarget::MainLR, _) => self.set_channel_fader_db(sel, db),
            (SendsOnFaderTarget::Aux(aux), SelectedChannel::Input(ch)) => {
                self.set_input_aux_send_db(ch, aux, db);
            }
            (SendsOnFaderTarget::Aux(aux), SelectedChannel::Fx(fx)) => {
                self.set_fx_aux_send_db(fx, aux, db);
            }
            (SendsOnFaderTarget::Fx(fx), SelectedChannel::Input(ch)) => {
                self.set_input_fx_send_db(ch, fx, db);
            }
            _ => self.set_channel_fader_db(sel, db),
        }
    }

    pub fn effective_fader_max_db(&self, sel: SelectedChannel) -> f32 {
        match (self.sends_on_fader, sel) {
            (SendsOnFaderTarget::MainLR, _) => 10.0,
            (_, SelectedChannel::Output(8 | 9)) => 10.0,
            (SendsOnFaderTarget::Aux(_), SelectedChannel::Input(_) | SelectedChannel::Fx(_)) => 0.0,
            (SendsOnFaderTarget::Fx(_), SelectedChannel::Input(_)) => 0.0,
            _ => 10.0,
        }
    }

    /// Safe Unplug ("Modo Desplugar Cabo"): mutes channel & drops fader to -120dB so musicians can swap cables with zero pop
    pub fn is_safe_unplugged(&self, ch: usize) -> bool {
        ch < 25 && self.safe_unplugged_inputs[ch]
    }

    pub fn all_safe_unplugged(&self) -> bool {
        (0..24).all(|ch| self.safe_unplugged_inputs[ch])
    }

    fn set_safe_unplug_state(&mut self, ch: usize, active: bool) {
        if ch >= 25 || self.safe_unplugged_inputs[ch] == active {
            return;
        }
        let sel = SelectedChannel::Input(ch);
        if active {
            self.saved_fader_before_unplug[ch] = self.channel_fader_db(sel);
            self.safe_unplugged_inputs[ch] = true;
            self.set_channel_muted(sel, true);
            self.set_channel_fader_db(sel, -120.0);
        } else {
            self.safe_unplugged_inputs[ch] = false;
            let restored = self.saved_fader_before_unplug[ch];
            self.set_channel_fader_db(sel, restored);
            self.set_channel_muted(sel, false);
        }
    }

    pub fn toggle_safe_unplug(&mut self, ch: usize) {
        if ch >= 25 {
            return;
        }
        let sel = SelectedChannel::Input(ch);
        let next = !self.safe_unplugged_inputs[ch];
        self.set_safe_unplug_state(ch, next);
        if next {
            self.toast(format!(
                "Modo Desplugar ATIVO em {}: Pode desconectar o cabo sem estalo no PA!",
                self.channel_name(sel)
            ));
        } else {
            let restored = self.saved_fader_before_unplug[ch];
            self.toast(format!(
                "Cabo reconectado em {}: Volume restaurado para {:+.1} dB!",
                self.channel_name(sel),
                restored
            ));
        }
    }

    /// Toggles Safe Unplug across all 24 input channels at once (end-of-service / stage shutdown)
    pub fn toggle_safe_unplug_all(&mut self) {
        let activate = !self.all_safe_unplugged();
        for ch in 0..24 {
            self.set_safe_unplug_state(ch, activate);
        }
        if activate {
            self.toast("Desplugar TODOS ATIVO (CH1-24): Canais mutados em -120 dB para desligar com segurança!");
        } else {
            self.toast("Desplugar TODOS DESATIVADO (CH1-24): Volumes e mutes restaurados!");
        }
    }

    /// Returns list of Input channels (0..25) that are disconnected from MAIN L/R (common cause of "no sound on L/R" forum complaint)
    pub fn disconnected_lr_channels(&self) -> Vec<usize> {
        (0..25).filter(|&ch| !self.input_to_main_lr(ch)).collect()
    }

    /// 1-Click Fix for "No Sound in Main L/R" / Stuck Mute Groups / Stuck DCA
    pub fn fix_all_routing_to_main_lr(&mut self) {
        for ch in 0..25 {
            self.set_input_to_main_lr(ch, true);
        }
        for mg in 0..4 {
            if self.mute_group_active(mg) {
                self.set_mute_group_active(mg, false);
            }
        }
        self.set_channel_muted(SelectedChannel::Output(8), false);
        self.set_channel_muted(SelectedChannel::Output(9), false);
        self.toast("Roteamento L/R restaurado em todos os 24 canais + DIGI e Mute Groups liberados!");
    }

    /// Pushes all 3,413 DSP registers to the hardware mixer at once
    pub fn sync_full_console_to_hardware(&mut self) {
        if matches!(self.hw_link.mode, crate::protocol::ConnectionMode::Demo) {
            self.toast("Conecte-se via UDP, TCP ou Serial primeiro para sincronizar com a mesa física.");
        } else {
            self.hw_link.sync_all_registers(&self.regs);
            self.toast("Todos os registradores DSP foram enviados para a Axios 24!");
        }
    }

    /// Reads the current live DSP state from the connected hardware mixer into the app
    pub fn sync_from_hardware(&mut self) {
        if matches!(self.hw_link.mode, crate::protocol::ConnectionMode::Demo) {
            self.toast("Conecte-se via UDP, TCP ou Serial primeiro para ler a cena da mesa.");
        } else {
            self.hw_link.read_all_registers();
            self.toast("Lendo cena atual da Axios 24...");
        }
    }

    /// Sanitizes scene / track names to safe ASCII so Axios 24 MCU never chokes on accents
    pub fn sanitize_ascii_name(input: &str) -> String {
        input
            .chars()
            .map(|c| match c {
                'á' | 'à' | 'ã' | 'â' | 'ä' => 'a',
                'Á' | 'À' | 'Ã' | 'Â' | 'Ä' => 'A',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'É' | 'È' | 'Ê' | 'Ë' => 'E',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
                'ó' | 'ò' | 'õ' | 'ô' | 'ö' => 'o',
                'Ó' | 'Ò' | 'Õ' | 'Ô' | 'Ö' => 'O',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
                'ç' => 'c',
                'Ç' => 'C',
                'ñ' => 'n',
                'Ñ' => 'N',
                c if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' => c,
                _ => '_',
            })
            .take(14)
            .collect()
    }

    /// Auto-Gain Assistant: sets preamp gain so channel peak targets -18 dBFS nominal headroom
    pub fn run_auto_gain(&mut self, ch: usize) {
        if ch >= 24 {
            return;
        }
        let meter = self.channel_meters_db[ch];
        let cur_gain = self.input_preamp_gain_db(ch);
        let target_db = -18.0_f32;
        let new_gain = if meter <= -80.0 {
            24.0
        } else {
            (cur_gain + (target_db - meter)).clamp(6.0, 48.0).round()
        };
        self.set_input_preamp_gain_db(ch, new_gain);
        self.toast(format!(
            "Auto-Gain CH{}: Ganho ajustado para +{:.0} dB (Alvo nominal -18 dB)",
            ch + 1,
            new_gain
        ));
    }

    /// Anti-Feedback Notch Assistant: detects the loudest frequency peak and applies a narrow surgical Notch (Q=18, -9 dB)
    pub fn apply_anti_feedback_notch(&mut self, group: u8, ch: usize) {
        let g = group.max(1).min(3);
        let band_count = Self::peq_band_count(g);
        let peak_freq = if self.live_rta.active {
            // Exact parabolic-interpolated FFT peak frequency from real USB / Mic audio!
            self.rta_peak_freq_hz.clamp(80.0, 16000.0)
        } else {
            let mut best_bin = 32usize;
            let mut best_val = -120.0_f32;
            for (i, &val) in self.rta_bins_db.iter().enumerate().skip(10) {
                if val > best_val {
                    best_val = val;
                    best_bin = i;
                }
            }
            crate::dsp::norm_x_to_freq(best_bin as f32 / 63.0).clamp(120.0, 12000.0)
        };
        let target_band = band_count - 1;
        self.set_eq_enabled(g, ch, true);
        self.set_peq_band(
            g,
            ch,
            target_band,
            PeqBandData {
                mode: PeqBandMode::Peaking,
                freq_hz: peak_freq,
                gain_db: -9.0,
                q: 18.0,
            },
        );
        self.selected_eq_band = target_band + 1;
        self.toast(format!(
            "Filtro Anti-Microfonia (Notch Q=18, -9dB) aplicado em {:.0} Hz (Banda {})!",
            peak_freq,
            target_band + 1
        ));
    }

    /// Toggles Master L/R -20dB Dimming (Emergency / Conversation attenuator)
    pub fn toggle_master_dim(&mut self) {
        let l_sel = SelectedChannel::Output(8);
        let r_sel = SelectedChannel::Output(9);
        if !self.master_dim_active {
            let cur_l = self.channel_fader_db(l_sel);
            let cur_r = self.channel_fader_db(r_sel);
            self.saved_master_lr_before_dim = (cur_l, cur_r);
            self.master_dim_active = true;
            self.set_channel_fader_db(l_sel, (cur_l - 20.0).max(-120.0));
            self.set_channel_fader_db(r_sel, (cur_r - 20.0).max(-120.0));
            self.toast("DIM -20 dB ATIVO no Master L/R!");
        } else {
            self.master_dim_active = false;
            let (rest_l, rest_r) = self.saved_master_lr_before_dim;
            self.set_channel_fader_db(l_sel, rest_l);
            self.set_channel_fader_db(r_sel, rest_r);
            self.toast("Master L/R restaurado ao nível normal!");
        }
    }

    pub fn any_channel_soloed(&self) -> bool {
        (0..25).any(|i| self.channel_solo(SelectedChannel::Input(i)))
            || (0..2).any(|i| self.channel_solo(SelectedChannel::Fx(i)))
            || (0..10).any(|i| self.channel_solo(SelectedChannel::Output(i)))
    }

    pub fn clear_all_solos(&mut self) {
        for i in 0..25 {
            self.set_channel_solo(SelectedChannel::Input(i), false);
        }
        for i in 0..2 {
            self.set_channel_solo(SelectedChannel::Fx(i), false);
        }
        for i in 0..10 {
            self.set_channel_solo(SelectedChannel::Output(i), false);
        }
        self.toast("Todos os Solos (S) foram desativados!");
    }

    /// Executes 1 of the 8 DS Mixer-style Stream Deck Macros
    pub fn trigger_stream_deck_macro(&mut self, idx: usize) {
        match idx {
            0 => {
                // Macro 1: Modo Palavra / Pastor (CH1 aberto com preset Locução, FX1/FX2 mutados, CH2..16 mutados)
                self.set_channel_muted(SelectedChannel::Input(0), false);
                self.set_channel_fader_db(SelectedChannel::Input(0), 0.0);
                self.apply_channel_preset(SelectedChannel::Input(0), ChannelPreset::LocucaoPastor);
                for ch in 1..16 {
                    self.set_channel_muted(SelectedChannel::Input(ch), true);
                }
                self.set_channel_muted(SelectedChannel::Fx(0), true);
                self.set_channel_muted(SelectedChannel::Fx(1), true);
                self.toast("Macro 1 [PALAVRA / PASTOR]: CH1 em 0dB c/ EQ Locução, Banda e Efeitos mutados!");
            }
            1 => {
                // Macro 2: Modo Louvor / Banda Full (Desmuta CH1..16, ativa FX1 e FX2, Master L/R em 0dB)
                for ch in 0..16 {
                    self.set_channel_muted(SelectedChannel::Input(ch), false);
                }
                self.set_channel_muted(SelectedChannel::Fx(0), false);
                self.set_channel_muted(SelectedChannel::Fx(1), false);
                self.set_channel_fader_db(SelectedChannel::Output(8), 0.0);
                self.set_channel_fader_db(SelectedChannel::Output(9), 0.0);
                self.master_dim_active = false;
                self.toast("Macro 2 [LOUVOR / BANDA FULL]: Canais 1-16 e Efeitos FX1/FX2 ativos!");
            }
            2 => {
                // Macro 3: FX Mute (Secar Voz para Fala entre músicas)
                let fx_muted = self.channel_muted(SelectedChannel::Fx(0));
                self.set_channel_muted(SelectedChannel::Fx(0), !fx_muted);
                self.set_channel_muted(SelectedChannel::Fx(1), !fx_muted);
                self.toast(if !fx_muted {
                    "Macro 3 [FX MUTE]: Reverb/Delay mutados para fala limpa!"
                } else {
                    "Macro 3 [FX ON]: Efeitos FX1 e FX2 reativados!"
                });
            }
            3 => {
                // Macro 4: Troca de Palco Segura (Muta instrumentos CH1..24 e libera apenas DIGI/USB)
                for ch in 0..24 {
                    self.set_channel_muted(SelectedChannel::Input(ch), true);
                }
                self.set_channel_muted(SelectedChannel::Input(24), false);
                self.toast("Macro 4 [TROCA DE PALCO]: CH 1-24 mutados com segurança (apenas DIGI/USB aberto)!");
            }
            4 => {
                // Macro 5: Som Ambiente / Pós-Evento (Sobe DIGI/USB para -4dB e inicia Player)
                self.set_channel_muted(SelectedChannel::Input(24), false);
                self.set_channel_fader_db(SelectedChannel::Input(24), -4.0);
                self.media.playing = true;
                self.toast("Macro 5 [SOM AMBIENTE USB]: Canal DIGI em -4dB e Player reproduzindo!");
            }
            5 => {
                // Macro 6: Cortar Microfonia de Emergência no Canal Selecionado
                let (grp, ch) = self.selected_channel.group_and_index();
                self.apply_anti_feedback_notch(grp, ch);
            }
            6 => {
                // Macro 7: Clear All Solos + Fix L/R Routing
                self.clear_all_solos();
                self.fix_all_routing_to_main_lr();
            }
            7 => {
                // Macro 8: Salvar Cena Atual Instantaneamente
                let sc = self.active_scene_idx;
                self.save_scene(sc);
            }
            _ => {}
        }
    }

    /// Applies a complete studio Channel Preset (HPF + 4-Band PEQ + Gate + Compressor)
    pub fn apply_channel_preset(&mut self, sel: SelectedChannel, preset: ChannelPreset) {
        self.push_undo(format!("Preset {}", preset.label()));
        let (group, ch) = sel.group_and_index();
        let g = group.max(1).min(3);
        let c = if group == 4 { 0 } else { ch };

        match preset {
            ChannelPreset::FlatReset => {
                self.reset_peq(g, c);
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 0.0);
                    self.set_input_gate_enabled(c, false);
                    self.set_comp_enabled(1, c, false);
                }
            }
            ChannelPreset::VocalMasculino => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 95.0);
                self.set_lpf_freq_hz(g, c, 18000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 220.0, gain_db: -3.5, q: 2.2 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 480.0, gain_db: -2.0, q: 2.5 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 3200.0, gain_db: 2.5, q: 1.4 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 10500.0, gain_db: 2.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 26.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -54.0);
                    self.set_input_gate_attack_ms(c, 5.0);
                    self.set_input_gate_release_ms(c, 180.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -18.0);
                    self.set_comp_ratio(1, c, 3.2);
                    self.set_comp_attack_ms(1, c, 12.0);
                    self.set_comp_release_ms(1, c, 140.0);
                    self.set_comp_gain_db(1, c, 2.5);
                }
            }
            ChannelPreset::VocalFeminino => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 120.0);
                self.set_lpf_freq_hz(g, c, 18500.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 260.0, gain_db: -2.5, q: 2.0 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 2400.0, gain_db: -1.5, q: 2.8 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 4200.0, gain_db: 2.0, q: 1.5 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 11500.0, gain_db: 3.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 26.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -55.0);
                    self.set_input_gate_attack_ms(c, 5.0);
                    self.set_input_gate_release_ms(c, 180.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -18.0);
                    self.set_comp_ratio(1, c, 3.0);
                    self.set_comp_attack_ms(1, c, 12.0);
                    self.set_comp_release_ms(1, c, 150.0);
                    self.set_comp_gain_db(1, c, 2.0);
                }
            }
            ChannelPreset::LocucaoPastor => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 110.0);
                self.set_lpf_freq_hz(g, c, 15000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 180.0, gain_db: -4.0, q: 2.2 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 400.0, gain_db: -3.0, q: 2.5 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 2800.0, gain_db: 3.0, q: 1.4 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 6800.0, gain_db: -3.0, q: 4.5 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 28.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -50.0);
                    self.set_input_gate_attack_ms(c, 8.0);
                    self.set_input_gate_release_ms(c, 240.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -20.0);
                    self.set_comp_ratio(1, c, 4.0);
                    self.set_comp_attack_ms(1, c, 8.0);
                    self.set_comp_release_ms(1, c, 160.0);
                    self.set_comp_gain_db(1, c, 3.0);
                }
            }
            ChannelPreset::BumboKick => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 35.0);
                self.set_lpf_freq_hz(g, c, 10000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 62.0, gain_db: 4.5, q: 1.6 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 320.0, gain_db: -6.0, q: 2.4 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 3800.0, gain_db: 4.0, q: 1.8 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 8000.0, gain_db: -2.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 16.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -36.0);
                    self.set_input_gate_attack_ms(c, 3.0);
                    self.set_input_gate_release_ms(c, 90.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -16.0);
                    self.set_comp_ratio(1, c, 4.5);
                    self.set_comp_attack_ms(1, c, 15.0);
                    self.set_comp_release_ms(1, c, 80.0);
                    self.set_comp_gain_db(1, c, 3.0);
                }
            }
            ChannelPreset::CaixaSnare => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 80.0);
                self.set_lpf_freq_hz(g, c, 15000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 190.0, gain_db: 3.0, q: 1.8 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 520.0, gain_db: -4.5, q: 2.8 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 4500.0, gain_db: 3.5, q: 1.4 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 9000.0, gain_db: 2.5, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 16.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -40.0);
                    self.set_input_gate_attack_ms(c, 3.0);
                    self.set_input_gate_release_ms(c, 110.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -16.0);
                    self.set_comp_ratio(1, c, 4.0);
                    self.set_comp_attack_ms(1, c, 12.0);
                    self.set_comp_release_ms(1, c, 100.0);
                    self.set_comp_gain_db(1, c, 2.5);
                }
            }
            ChannelPreset::TonsSurdo => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 55.0);
                self.set_lpf_freq_hz(g, c, 12000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 110.0, gain_db: 3.5, q: 1.8 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 420.0, gain_db: -5.5, q: 2.5 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 4200.0, gain_db: 3.0, q: 1.6 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 10000.0, gain_db: 1.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 18.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -42.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -16.0);
                    self.set_comp_ratio(1, c, 3.5);
                }
            }
            ChannelPreset::OverheadsPratos => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 320.0);
                self.set_lpf_freq_hz(g, c, 20000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 480.0, gain_db: -3.5, q: 2.0 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 2800.0, gain_db: -2.0, q: 2.5 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 7500.0, gain_db: 1.5, q: 1.4 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 12000.0, gain_db: 3.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 22.0);
                    self.set_input_gate_enabled(c, false);
                    self.set_comp_enabled(1, c, false);
                }
            }
            ChannelPreset::BaixoEletrico => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 35.0);
                self.set_lpf_freq_hz(g, c, 6000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 75.0, gain_db: 3.0, q: 1.6 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 250.0, gain_db: -3.5, q: 2.2 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 900.0, gain_db: 2.5, q: 1.8 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 2200.0, gain_db: 1.5, q: 1.6 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 12.0);
                    self.set_input_gate_enabled(c, false);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -18.0);
                    self.set_comp_ratio(1, c, 4.0);
                    self.set_comp_attack_ms(1, c, 30.0);
                    self.set_comp_release_ms(1, c, 100.0);
                    self.set_comp_gain_db(1, c, 3.0);
                }
            }
            ChannelPreset::ViolaoAco => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 110.0);
                self.set_lpf_freq_hz(g, c, 16000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 220.0, gain_db: -4.0, q: 4.5 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 450.0, gain_db: -2.5, q: 2.4 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 3800.0, gain_db: 2.0, q: 1.5 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 10000.0, gain_db: 2.5, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 20.0);
                    self.set_input_gate_enabled(c, false);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -16.0);
                    self.set_comp_ratio(1, c, 2.5);
                    self.set_comp_attack_ms(1, c, 15.0);
                    self.set_comp_release_ms(1, c, 80.0);
                    self.set_comp_gain_db(1, c, 1.5);
                }
            }
            ChannelPreset::GuitarraEletrica => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 90.0);
                self.set_lpf_freq_hz(g, c, 7500.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 160.0, gain_db: 1.5, q: 1.8 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 450.0, gain_db: -2.5, q: 2.2 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 2200.0, gain_db: 2.0, q: 1.6 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 3800.0, gain_db: -2.5, q: 3.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 18.0);
                    self.set_input_gate_enabled(c, true);
                    self.set_input_gate_threshold_db(c, -52.0);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -14.0);
                    self.set_comp_ratio(1, c, 2.0);
                }
            }
            ChannelPreset::TecladoPiano => {
                self.set_eq_enabled(g, c, true);
                self.set_hpf_freq_hz(g, c, 70.0);
                self.set_lpf_freq_hz(g, c, 18000.0);
                self.set_peq_band(g, c, 0, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 320.0, gain_db: -2.5, q: 2.0 });
                self.set_peq_band(g, c, 1, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 1400.0, gain_db: -1.5, q: 2.0 });
                self.set_peq_band(g, c, 2, PeqBandData { mode: PeqBandMode::Peaking, freq_hz: 3500.0, gain_db: 1.5, q: 1.4 });
                self.set_peq_band(g, c, 3, PeqBandData { mode: PeqBandMode::HighShelf, freq_hz: 8500.0, gain_db: 2.0, q: 1.0 });
                if g == 1 {
                    self.set_input_preamp_gain_db(c, 10.0);
                    self.set_input_gate_enabled(c, false);
                    self.set_comp_enabled(1, c, true);
                    self.set_comp_threshold_db(1, c, -12.0);
                    self.set_comp_ratio(1, c, 2.0);
                    self.set_comp_attack_ms(1, c, 5.0);
                    self.set_comp_release_ms(1, c, 100.0);
                    self.set_comp_gain_db(1, c, 0.0);
                }
            }
        }
        self.toast(format!(
            "Preset '{}' aplicado em {}!",
            preset.label(),
            self.channel_name(sel)
        ));
    }
}
