#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod audio_rta;
pub mod default_scene;
pub mod dsp;
pub mod midi;
pub mod model;
pub mod multitrack;
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

        // Continuous repaint when connected to hardware, Demo mode, Live Audio FFT RTA, Media player, or LAN scan is active
        if !matches!(self.state.hw_link.mode, protocol::ConnectionMode::Demo)
            || self.state.demo_running
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.save_user_prefs();
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
        use crate::model::map_reg;
        use crate::protocol::{decode_official_scn, encode_official_scn, FirmwareLayout};
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
        let reencoded_legacy =
            encode_official_scn(1, &scn.name, &ch_names, &scn.regs, FirmwareLayout::Legacy);
        let scn2 = decode_official_scn(&reencoded_legacy).expect("roundtrip legacy official scn");
        assert_eq!(scn2.name, "Padrao");
        assert_eq!(scn2.regs, scn.regs);
        assert_eq!(scn2.text[0], "Vermelho");

        // Also verify AX24E.25.11.01.001 (3518-register) .scn roundtrip and register translation
        let ax = FirmwareLayout::Ax24e2511;
        assert_eq!(ax.to_device(map_reg(1, 0, 0xEA) as u16), 0x004C); // CH1 Fader
        assert_eq!(ax.to_device(map_reg(1, 0, 0xEB) as u16), 0x004A); // CH1 Mute (0=MUTED, 1=ON)
        assert_eq!(ax.to_device(map_reg(1, 0, 0xEC) as u16), 0x0057); // CH1 Solo
        assert_eq!(ax.to_device(map_reg(3, 8, 0xEA) as u16), 0x09F4); // MAIN L Fader
        assert_eq!(ax.to_device(map_reg(3, 8, 0xEB) as u16), 0x09F6); // MAIN L Mute
        assert_eq!(ax.to_device(map_reg(3, 8, 0xEC) as u16), 0x0A00); // MAIN L Solo
        assert_eq!(ax.to_device(map_reg(2, 0, 0xEA) as u16), 0x0B53); // FX1 Fader 0xB2F -> 0xB53
        assert_eq!(ax.to_device(map_reg(2, 0, 0xEB) as u16), 0x0B54); // FX1 Mute 0xB30 -> 0xB54
        assert_eq!(ax.to_device(map_reg(2, 0, 0xEC) as u16), 0x0B5F); // FX1 Solo 0xB3B -> 0xB5F
        assert_eq!(ax.to_device(map_reg(4, 0, 0xEA) as u16), 0x0BCC); // DCA1 Fader 0xBA8 -> 0xBCC
        assert_eq!(ax.to_device(map_reg(4, 0, 0xD9) as u16), 0x0BCB); // DCA1 Mute 0xBA7 -> 0xBCB
        assert_eq!(ax.to_device(0x0BCC), 0x0BF0); // MAIN L/R stereo link bitmask 0xBCC -> 0xBF0

        let reencoded_ax24e =
            encode_official_scn(1, &scn.name, &ch_names, &scn.regs, FirmwareLayout::Ax24e2511);
        let scn3 = decode_official_scn(&reencoded_ax24e).expect("roundtrip AX24E 25.11 official scn");
        assert_eq!(scn3.regs[..0xB2F], scn.regs[..0xB2F]);
        assert_eq!(scn3.regs[0xB2F..=0xC00], scn.regs[0xB2F..=0xC00]);
        assert_eq!(scn3.regs[0xC01..], scn.regs[0xC01..]);
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

        let bt_frame = crate::protocol::build_media_source_frame(1);
        let (bt_cmd, bt_payload) = parse_frame(&bt_frame).expect("valid bt frame");
        assert_eq!(bt_cmd, 0x72);
        assert_eq!(bt_payload, &[0x00, 0x00, 0xC7]);
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

    #[test]
    fn test_automix_clip_and_prefs_roundtrip() {
        use crate::model::{Axios24State, SelectedChannel};
        let mut st = Axios24State::new();
        st.automix_enabled = true;
        st.automix_channels[0] = true;
        st.automix_channels[1] = true;
        st.channel_meters_db[0] = -10.0;
        st.channel_meters_db[1] = -30.0;
        st.demo_running = false;
        st.tick(0.05);
        // Quieter mic CH2 should be attenuated more than active mic CH1
        assert!(st.automix_atten_db[1] < st.automix_atten_db[0]);

        st.channel_meters_db[0] = 0.0;
        st.set_channel_muted(SelectedChannel::Input(0), false);
        st.tick(0.02);
        assert!(st.channel_clip_hold[0]);
        st.clear_all_clips();
        assert!(!st.channel_clip_hold[0]);

        // Test Desplugar Todos (toggle_safe_unplug_all) preserving individual unplugged faders
        st.set_channel_fader_db(SelectedChannel::Input(0), -6.0);
        st.set_channel_fader_db(SelectedChannel::Input(1), -3.0);
        st.toggle_safe_unplug(0);
        assert!(st.is_safe_unplugged(0));
        assert!(!st.all_safe_unplugged());
        st.toggle_safe_unplug_all();
        assert!(st.all_safe_unplugged());
        assert!(st.channel_muted(SelectedChannel::Input(1)));
        assert!((st.channel_fader_db(SelectedChannel::Input(1)) - (-120.0)).abs() < 0.2);
        st.toggle_safe_unplug_all();
        assert!(!st.all_safe_unplugged());
        assert!((st.channel_fader_db(SelectedChannel::Input(0)) - (-6.0)).abs() < 0.2);
        assert!((st.channel_fader_db(SelectedChannel::Input(1)) - (-3.0)).abs() < 0.2);
        assert!(!st.channel_muted(SelectedChannel::Input(0)));

        // Test Quick HPF 100Hz, A/B Compare, and Clean FX Return EQ
        let ch0 = SelectedChannel::Input(0);
        if st.is_hpf_active(ch0) {
            st.toggle_quick_hpf_100hz(ch0);
        }
        st.toggle_quick_hpf_100hz(ch0);
        assert!(st.is_hpf_active(ch0));
        assert!((st.hpf_freq_hz(1, 0) - 100.0).abs() < 1.0);

        st.toggle_ab_compare(ch0); // enters B
        assert!(st.is_ab_b_active(ch0));
        st.toggle_quick_hpf_100hz(ch0); // disable HPF in B
        assert!(!st.is_hpf_active(ch0));
        st.toggle_ab_compare(ch0); // back to A (HPF 100Hz active)
        assert!(!st.is_ab_b_active(ch0));
        assert!(st.is_hpf_active(ch0));

        st.clean_fx_return_eq(0);
        assert!(st.eq_enabled(2, 0));
        assert!((st.hpf_freq_hz(2, 0) - 180.0).abs() < 1.0);
        assert!((st.lpf_freq_hz(2, 0) - 7000.0).abs() < 10.0);
    }

    #[test]
    fn test_multitrack_wav_loader() {
        use crate::multitrack::{clean_track_name, MultitrackEngine, MT_SAMPLE_RATE};
        let dir = std::env::temp_dir().join("axios24_mtk_test");
        let _ = std::fs::create_dir_all(&dir);
        let wav_path = dir.join("01_Voz_Lider.wav");
        // Write a tiny valid 16-bit PCM mono 48kHz WAV (480 samples = 10ms)
        let num_samples: u32 = 480;
        let data_bytes = num_samples * 2;
        let mut buf = Vec::with_capacity(44 + data_bytes as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        buf.extend_from_slice(b"WAVEfmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&1u16.to_le_bytes()); // mono
        buf.extend_from_slice(&(MT_SAMPLE_RATE as u32).to_le_bytes());
        buf.extend_from_slice(&((MT_SAMPLE_RATE as u32) * 2).to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_samples {
            let s = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / MT_SAMPLE_RATE as f32).sin()
                * 16000.0) as i16;
            buf.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&wav_path, &buf).unwrap();

        assert_eq!(clean_track_name(&wav_path), "Voz Lider");
        let mut eng = MultitrackEngine::default();
        let n = eng.load_wav_paths(&[wav_path.clone()]).expect("load wav");
        assert_eq!(n, 1);
        assert_eq!(eng.total_samples.load(std::sync::atomic::Ordering::Relaxed), 480);
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_official_ax24e_register_contract() {
        use crate::model::{map_reg, Axios24State, SelectedChannel};
        let mut st = Axios24State::new();

        // 1. Mute uses param 0xEB with inverted polarity (0 = MUTED, 1 = UNMUTED/ON)
        st.set_channel_muted(SelectedChannel::Input(0), true);
        assert_eq!(st.get_reg(map_reg(1, 0, 0xEB)), 0);
        assert!(st.channel_muted(SelectedChannel::Input(0)));
        st.set_channel_muted(SelectedChannel::Input(0), false);
        assert_eq!(st.get_reg(map_reg(1, 0, 0xEB)), 1);
        assert!(!st.channel_muted(SelectedChannel::Input(0)));

        // Output AUX1 & MAIN L/R Mute
        st.set_channel_muted(SelectedChannel::Output(0), true);
        assert_eq!(st.get_reg(map_reg(3, 0, 0xEB)), 0);
        assert!(st.channel_muted(SelectedChannel::Output(0)));
        st.set_channel_muted(SelectedChannel::Output(8), true);
        assert_eq!(st.get_reg(0x09F6), 0);
        assert_eq!(st.get_reg(0x0A63), 0); // stereo linked MAIN R
        st.set_channel_muted(SelectedChannel::Output(8), false);
        assert_eq!(st.get_reg(0x09F6), 1);

        // 2. Solo uses param 0xEC (0x04B0 = SOLO ON, 0x0000 = OFF)
        st.set_channel_solo(SelectedChannel::Input(0), true);
        assert_eq!(st.get_reg(0x0057) & 0x3FFF, 0x04B0);
        assert_eq!(st.get_reg(0x0058) & 0x3FFF, 0x04B0);
        assert!(st.channel_solo(SelectedChannel::Input(0)));
        st.set_channel_solo(SelectedChannel::Input(0), false);
        assert_eq!(st.get_reg(0x0057) & 0x3FFF, 0);
        assert!(!st.channel_solo(SelectedChannel::Input(0)));

        // 3. Input -> Main L/R uses params 0x15 & 0x16 (0x0055 & 0x0056), NOT 0x17/0x18 (Solo)!
        st.set_input_to_main_lr(0, false);
        assert_eq!(st.get_reg(0x0055) & 0x3FFF, 0);
        assert_eq!(st.get_reg(0x0056) & 0x3FFF, 0);
        assert!(!st.input_to_main_lr(0));
        st.set_input_to_main_lr(0, true);
        assert_eq!(st.get_reg(0x0055) & 0x3FFF, 0x04B0);
        assert_eq!(st.get_reg(0x0056) & 0x3FFF, 0x04B0);
        assert!(st.input_to_main_lr(0));

        // 4. +48V Phantom uses param 0x01 (0x0041), NOT 0xADF!
        let old_patch = st.get_reg(0x0ADF);
        st.set_input_phantom_48v(0, true);
        assert_eq!(st.get_reg(0x0041), 1);
        assert_eq!(st.get_reg(0x0ADF), old_patch);
        assert!(st.input_phantom_48v(0));

        // 5. Input Phase uses param 0x09 (0 = INVERTED, 1 = NORMAL) & Output Phase uses param 0x3F (0 = INVERTED, 1 = NORMAL)
        assert!(!st.input_phase_invert(0));
        st.set_input_phase_invert(0, true);
        assert_eq!(st.get_reg(0x0049), 0);
        assert!(st.input_phase_invert(0));
        assert!(!st.output_phase_invert(0)); // AUX1 starts normal (0x68D == 1), NOT orange!
        st.set_output_phase_invert(0, true);
        assert_eq!(st.get_reg(0x068D), 0);
        assert!(st.output_phase_invert(0));

        // 6. Hi-Z uses param 0x00 bit 0 (0x0040)
        st.set_input_hiz(0, true);
        assert_eq!(st.get_reg(0x0040) & 1, 1);
        assert!(st.input_hiz(0));

        // 7. AUX Send Pre/Post uses bit 15 (0x8000 = PRE, 0 = POST)
        st.set_input_aux_pre_fader(0, 0, true);
        assert_eq!(st.get_reg(0x004D) & 0x8000, 0x8000);
        assert!(st.input_aux_pre_fader(0, 0));
        st.set_input_aux_pre_fader(0, 0, false);
        assert_eq!(st.get_reg(0x004D) & 0x8000, 0);
        assert!(!st.input_aux_pre_fader(0, 0));
    }
}
