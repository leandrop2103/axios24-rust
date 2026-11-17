#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod audio_rta;
pub mod default_scene;
pub mod dsp;
pub mod midi;
pub mod model;
pub mod protocol;
pub mod ui;
pub mod updater;

use eframe::egui;
use model::Axios24State;
use std::sync::Arc;
use std::time::Instant;

struct Axios24App {
    state: Axios24State,
    last_frame: Instant,
    last_dark_theme: bool,
    ferris_logo: Option<egui::TextureHandle>,
}

fn load_ferris_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let bytes = include_bytes!("../assets/ferris_mixer_icon.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let raw = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => raw
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

impl Axios24App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(1.15);
        let state = Axios24State::new();
        configure_theme(&cc.egui_ctx, state.dark_theme);
        let ferris_logo = load_ferris_rgba().map(|(rgba, w, h)| {
            let color_img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            cc.egui_ctx.load_texture("ferris_mixer_logo", color_img, egui::TextureOptions::LINEAR)
        });
        Self {
            last_dark_theme: state.dark_theme,
            state,
            last_frame: Instant::now(),
            ferris_logo,
        }
    }
}

impl eframe::App for Axios24App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().clamp(0.001, 0.1);
        self.last_frame = now;

        if self.state.dark_theme != self.last_dark_theme {
            self.last_dark_theme = self.state.dark_theme;
            configure_theme(ctx, self.state.dark_theme);
        }

        self.state.tick(dt);
        ui::render_app(ctx, &mut self.state, self.ferris_logo.as_ref());

        // Continuous repaint when Demo mode, Live Audio FFT RTA, Media player, or LAN scan is active
        if self.state.demo_running
            || self.state.live_rta.active
            || self.state.media.playing
            || self.state.media.recording
            || self.state.discovery_scanning
            || self.state.update_checking
            || self.state.update_downloading
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();

    // Load Roboto Medium (clean modern UI font) + Nerd Fonts Symbols (icons & glyphs)
    fonts.font_data.insert(
        "roboto".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Roboto-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "nerd_symbols".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/SymbolsNerdFontMono-Regular.ttf"
        ))),
    );

    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["roboto".to_owned(), "nerd_symbols".to_owned()],
    );
    fonts.families.insert(
        egui::FontFamily::Monospace,
        vec!["roboto".to_owned(), "nerd_symbols".to_owned()],
    );

    ctx.set_fonts(fonts);
}

fn configure_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        let mut v = egui::Visuals::dark();
        v.panel_fill = egui::Color32::from_rgb(11, 15, 22);
        v.window_fill = egui::Color32::from_rgb(16, 22, 32);
        v.extreme_bg_color = egui::Color32::from_rgb(8, 11, 16);
        v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(18, 24, 35);
        v.widgets.inactive.bg_fill = egui::Color32::from_rgb(26, 34, 48);
        v.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 50, 70);
        v
    } else {
        let mut v = egui::Visuals::light();
        v.panel_fill = egui::Color32::from_rgb(236, 240, 246);
        v.window_fill = egui::Color32::from_rgb(246, 248, 252);
        v.extreme_bg_color = egui::Color32::from_rgb(220, 226, 236);
        v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(228, 233, 242);
        v.widgets.inactive.bg_fill = egui::Color32::from_rgb(212, 220, 232);
        v.widgets.hovered.bg_fill = egui::Color32::from_rgb(192, 206, 226);
        v
    };
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 175, 215);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0, 160, 205);
    ctx.set_visuals(visuals);
    ctx.style_mut(|s| {
        s.interaction.tooltip_delay = 0.6;
        s.interaction.show_tooltips_only_when_still = true;
    });
}

fn main() -> eframe::Result<()> {
    updater::cleanup_old_exe();
    let title = format!(
        "Duonn Axios 24 — DS Mixer Pro v{} (Rust)",
        updater::CURRENT_VERSION
    );
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(&title)
        .with_inner_size([1360.0, 820.0])
        .with_min_inner_size([1024.0, 640.0]);
    if let Some((rgba, width, height)) = load_ferris_rgba() {
        viewport = viewport.with_icon(Arc::new(egui::IconData {
            rgba,
            width,
            height,
        }));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Duonn Axios 24 — DS Mixer Pro",
        native_options,
        Box::new(|cc| Ok(Box::new(Axios24App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use crate::default_scene::DEFAULT_REGS;
    use crate::dsp::{eval_stages_db, hpf_lpf_stages, peq_band_biquad, FilterFamily, FilterSlope, PeqBandMode};
    use crate::protocol::{
        build_demo_cmd_frame, build_param_write_frame, crc16_axios, decode_scene_block,
        encode_scene_block, parse_frame, SCENE_BLOCK_SIZE,
    };

    #[test]
    fn test_scene_block_roundtrip_and_crc() {
        let block = encode_scene_block(&DEFAULT_REGS);
        assert_eq!(block.len(), SCENE_BLOCK_SIZE);
        let crc = crc16_axios(&block[..0x2FFE]);
        let stored_crc = u16::from_le_bytes([block[0x2FFE], block[0x2FFF]]);
        assert_eq!(crc, stored_crc);

        let decoded = decode_scene_block(&block).expect("valid scene block");
        assert_eq!(decoded[0x4B], DEFAULT_REGS[0x4B]);
    }

    #[test]
    fn test_import_igreja_scn_and_official_roundtrip() {
        use crate::protocol::{decode_official_scn, encode_official_scn};
        if !std::path::Path::new("igreja.scn").exists() {
            return;
        }
        let bytes = std::fs::read("igreja.scn").expect("igreja.scn exists in workspace");
        let scn = decode_official_scn(&bytes).expect("decode igreja.scn official format");
        assert_eq!(scn.name, "Padrao");
        assert_eq!(scn.text.len(), 60);
        assert_eq!(scn.text[0], "Vermelho");
        assert_eq!(scn.text[4], "BUMBO");
        assert_eq!(scn.text[38], "Microfones");

        let ch_names: [String; 60] = std::array::from_fn(|i| {
            scn.text.get(i).cloned().unwrap_or_default()
        });
        let reencoded = encode_official_scn(1, &scn.name, &ch_names, &scn.regs);
        let scn2 = decode_official_scn(&reencoded).expect("roundtrip official scn");
        assert_eq!(scn2.name, "Padrao");
        assert_eq!(scn2.regs, scn.regs);
        assert_eq!(scn2.text[0], "Vermelho");
    }

    #[test]
    fn test_frame_build_and_parse() {
        let frame = build_param_write_frame(&[(0x0057, 0x04B0)]);
        assert_eq!(frame[0], 0x80);
        let (cmd, payload) = parse_frame(&frame).expect("valid frame");
        assert_eq!(cmd, 0x03);
        assert_eq!(payload, &[0x00, 0x57, 0x04, 0xB0]);

        let demo_start = build_demo_cmd_frame(true);
        assert_eq!(demo_start.len(), 9);
        assert_eq!(&demo_start[..5], &[0x80, 0x07, 0x26, 0x13, 0x01]);
        assert!(parse_frame(&demo_start).is_some());
    }

    #[test]
    fn test_peq_and_hpf_lpf_biquad_response() {
        let bq = peq_band_biquad(PeqBandMode::Peaking, 1000.0, 9.0, 2.0);
        let center_db = bq.eval_db(1000.0);
        assert!((center_db - 9.0).abs() < 0.15, "center_db={}", center_db);
        let far_db = bq.eval_db(50.0);
        assert!(far_db.abs() < 0.2, "far_db={}", far_db);

        let hpf_bw = hpf_lpf_stages(100.0, FilterFamily::Butterworth, FilterSlope::Slope24dB, false);
        let bw_cutoff_db = eval_stages_db(&hpf_bw, 100.0);
        assert!((bw_cutoff_db - (-3.01)).abs() < 0.1, "bw_cutoff_db={}", bw_cutoff_db);

        let hpf_lr = hpf_lpf_stages(100.0, FilterFamily::LinkwitzRiley, FilterSlope::Slope24dB, false);
        let lr_cutoff_db = eval_stages_db(&hpf_lr, 100.0);
        assert!((lr_cutoff_db - (-6.02)).abs() < 0.1, "lr_cutoff_db={}", lr_cutoff_db);

        let pass_db = eval_stages_db(&hpf_bw, 5000.0);
        assert!(pass_db.abs() < 0.1);
    }

    #[test]
    fn test_real_fft_rta_1000hz_sine_detection() {
        use crate::audio_rta::{compute_rta_from_pcm, FFT_SIZE, SAMPLE_RATE};
        let mut pcm = vec![0.0_f32; FFT_SIZE];
        let target_freq = 1000.0_f32;
        for i in 0..FFT_SIZE {
            let t = (i as f32) / (SAMPLE_RATE as f32);
            pcm[i] = (2.0 * std::f32::consts::PI * target_freq * t).sin() * 0.5;
        }
        let snap = compute_rta_from_pcm(&pcm, SAMPLE_RATE);
        assert!(
            (snap.peak_freq_hz - target_freq).abs() < 12.0,
            "Expected ~1000 Hz, got {} Hz",
            snap.peak_freq_hz
        );
        assert!(snap.peak_db > -10.0, "Expected peak_db > -10 dB, got {}", snap.peak_db);
    }

    #[test]
    fn test_updater_semver_and_release_json() {
        use crate::updater::{is_newer_version, parse_github_release_json};
        assert!(is_newer_version("0.2.5", "0.2.4"));
        assert!(is_newer_version("v1.0.0", "0.2.4"));
        assert!(!is_newer_version("0.2.4", "0.2.4"));
        assert!(!is_newer_version("0.2.3", "0.2.4"));

        let sample = r#"{"tag_name":"v0.2.5","body":"- Novo EQ\n- Correcao Wi-Fi","assets":[{"browser_download_url":"https://example.com/axios24_rust.exe"}]}"#;
        let info = parse_github_release_json(sample).expect("parse release json");
        assert_eq!(info.version, "0.2.5");
        assert!(info.notes.contains("Novo EQ"));
        assert!(info.download_url.ends_with(".exe"));
    }

    #[test]
    fn test_channel_links_preserve_pan() {
        use crate::model::{Axios24State, SelectedChannel};
        let mut st = Axios24State::new();
        // 1. MAIN L & R link moves both together (Fader, Mute, EQ, GEQ)
        assert!(st.is_output_stereo_linked(8));
        st.set_channel_fader_db(SelectedChannel::Output(8), -5.5);
        assert!((st.channel_fader_db(SelectedChannel::Output(9)) - (-5.5)).abs() < 0.2);
        st.set_channel_muted(SelectedChannel::Output(8), true);
        assert!(st.channel_muted(SelectedChannel::Output(9)));
        st.set_channel_muted(SelectedChannel::Output(8), false);
        st.set_output_geq_db(8, 10, 4.5);
        assert!((st.output_geq_db[9][10] - 4.5).abs() < 0.1);
        let mut bd = st.peq_band(3, 8, 0);
        bd.gain_db = 3.5;
        st.set_peq_band(3, 8, 0, bd);
        assert!((st.peq_band(3, 9, 0).gain_db - 3.5).abs() < 0.15);

        // 2. Custom link group (e.g. CH 14 & CH 15 for Stereo Keyboard) links everything EXCEPT Pan
        st.set_input_pan(13, -100);
        st.set_input_pan(14, 100);
        st.set_input_custom_link_group(13, 1);
        st.set_input_custom_link_group(14, 1);

        st.set_channel_fader_db(SelectedChannel::Input(13), -3.0);
        assert!((st.channel_fader_db(SelectedChannel::Input(14)) - (-3.0)).abs() < 0.2);
        st.set_input_preamp_gain_db(13, 24.0);
        assert!((st.input_preamp_gain_db(14) - 24.0).abs() < 0.5);
        // Pan must remain completely independent!
        assert_eq!(st.input_pan(13), -100);
        assert_eq!(st.input_pan(14), 100);

        // 3. Windows-style Ctrl+Click joins CH 16 into the same group, and Ctrl+Click again removes it
        st.selected_channel = SelectedChannel::Input(13);
        st.ctrl_link_channel(SelectedChannel::Input(15));
        assert_eq!(st.input_custom_link_group(15), 1);
        // Changing color on one linked channel propagates to all linked partners
        st.set_channel_color_idx(SelectedChannel::Input(13), 21);
        assert_eq!(st.channel_color_idx(SelectedChannel::Input(14)), 21);
        assert_eq!(st.channel_color_idx(SelectedChannel::Input(15)), 21);
        // MAIN L & R color sync
        st.set_channel_color_idx(SelectedChannel::Output(8), 30);
        assert_eq!(st.channel_color_idx(SelectedChannel::Output(9)), 30);

        st.ctrl_link_channel(SelectedChannel::Input(15));
        assert_eq!(st.input_custom_link_group(15), 0);

        // 4. DCA group assignment & clear
        st.set_in_dca_group(0, 4, true);
        assert!(st.is_in_dca_group(0, 4));
        st.clear_dca_group(0);
        assert!(!st.is_in_dca_group(0, 4));
    }

    #[test]
    fn test_recall_safe_and_undo() {
        use crate::model::{Axios24State, SelectedChannel};
        let mut st = Axios24State::new();
        // Protect CH1 (flat_id 0) with Recall Safe and set fader to -2.0 dB
        st.recall_safe_channels[0] = true;
        st.set_channel_fader_db(SelectedChannel::Input(0), -2.0);
        st.set_channel_fader_db(SelectedChannel::Input(1), -12.0);

        // Recall Scene 2 (which has default fader values)
        st.recall_scene(1);
        // CH1 must remain at -2.0 dB because of Recall Safe!
        assert!((st.channel_fader_db(SelectedChannel::Input(0)) - (-2.0)).abs() < 0.2);

        // Undo must restore CH2 back to -12.0 dB
        st.undo();
        assert!((st.channel_fader_db(SelectedChannel::Input(1)) - (-12.0)).abs() < 0.2);
    }
}
