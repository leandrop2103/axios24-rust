pub mod widgets;

use crate::dsp::{FilterFamily, FilterSlope, PeqBandMode, GEQ_LABELS};
use crate::model::{
    format_fx_param1, format_fx_param2, Axios24State, ChannelPreset, FaderLayer, MediaSource,
    PeqBandData, RepeatMode, SelectedChannel, SendsOnFaderTarget, SettingsSubTab, TopNavTab,
    ViewMode, FX_ALGORITHMS,
};
use crate::protocol::{ConnectionMode, DUONN_FACTORY_IPS, TCP_REMOTE_PORT, UDP_REMOTE_PORT};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, Ui, Vec2};
use widgets::{
    draw_channel_strip, draw_color_palette_grid, draw_comp_curve_graph,
    draw_gate_curve_graph, draw_geq_response_graph, draw_interactive_peq_graph, draw_pill_button,
    draw_rotary_knob, format_fader_db, format_freq, EQ_BAND_COLORS, STRIP_PALETTE,
};

pub fn render_app(
    ctx: &egui::Context,
    state: &mut Axios24State,
    ferris_logo: Option<&egui::TextureHandle>,
) {
    // Console Lock Modal
    if state.console_locked {
        render_lock_screen(ctx, state);
        return;
    }

    // Global Keyboard Shortcuts (Fine Fader 0.1dB/1dB, Mute, Solo, Unity 0dB, Search Ctrl+F, Layers F1..F7)
    handle_keyboard_shortcuts(ctx, state);

    // Connection / Discovery / Demo Modal
    if state.show_connect_modal {
        render_connect_modal(ctx, state);
    }

    // GitHub Auto-Update Modal (appears automatically if a newer version is published)
    if state.show_update_modal {
        render_update_modal(ctx, state);
    }

    let dark = state.dark_theme;
    let bar_bg = if dark { Color32::from_rgb(13, 17, 25) } else { Color32::from_rgb(228, 234, 244) };
    let bar_border = if dark { Color32::from_rgb(34, 45, 62) } else { Color32::from_rgb(185, 198, 215) };
    let dock_bg = if dark { Color32::from_rgb(11, 15, 23) } else { Color32::from_rgb(220, 228, 240) };
    let main_bg = if dark { Color32::from_rgb(9, 12, 18) } else { Color32::from_rgb(238, 242, 248) };

    // Top Navigation & Status Bar (2 non-overlapping rows)
    egui::TopBottomPanel::top("top_header")
        .frame(
            Frame::NONE
                .fill(bar_bg)
                .inner_margin(Margin::symmetric(10, 6))
                .stroke(Stroke::new(1.0_f32, bar_border)),
        )
        .show(ctx, |ui| {
            render_top_bar(ui, state, ferris_logo);
        });

    // Bottom Layer Bar + Toast Status
    egui::TopBottomPanel::bottom("bottom_layer_bar")
        .frame(
            Frame::NONE
                .fill(bar_bg)
                .inner_margin(Margin::symmetric(10, 5))
                .stroke(Stroke::new(1.0_f32, bar_border)),
        )
        .show(ctx, |ui| {
            render_bottom_bar(ui, state);
        });

    // Right-Docked Master L & R Section (Always Pinned to Right Side of Window)
    egui::SidePanel::right("master_lr_dock")
        .resizable(false)
        .exact_width(146.0)
        .frame(
            Frame::NONE
                .fill(dock_bg)
                .inner_margin(Margin::symmetric(6, 6))
                .stroke(Stroke::new(1.5_f32, Color32::from_rgb(0, 155, 205))),
        )
        .show(ctx, |ui| {
            let each_w = ((ui.available_width() - 4.0) * 0.5).max(56.0);
            // Master Quick Utility Row: DIM -20dB & CLEAR SOLO
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if draw_pill_button(
                    ui,
                    if state.master_dim_active { "DIM ON" } else { "DIM -20" },
                    state.master_dim_active,
                    Color32::from_rgb(230, 126, 34),
                    Vec2::new(each_w, 18.0),
                )
                .on_hover_text("Atenua imediatamente o Master L/R em -20 dB (Clique novamente para restaurar)")
                .clicked()
                {
                    state.toggle_master_dim();
                }

                let any_solo = state.any_channel_soloed();
                if draw_pill_button(
                    ui,
                    "CLR SOLO",
                    any_solo,
                    Color32::from_rgb(243, 156, 18),
                    Vec2::new(each_w, 18.0),
                )
                .on_hover_text("Desativa todos os botões Solo (S) ativos na mesa")
                .clicked()
                {
                    state.clear_all_solos();
                }
            });
            ui.add_space(3.0);
            let dock_h = ui.available_height().max(120.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                draw_channel_strip(ui, state, SelectedChannel::Output(8), each_w, dock_h, true);
                draw_channel_strip(ui, state, SelectedChannel::Output(9), each_w, dock_h, true);
            });
        });

    // Main Central Workspace
    egui::CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(main_bg)
                .inner_margin(Margin::same(8)),
        )
        .show(ctx, |ui| match state.top_tab {
            TopNavTab::Mixer => render_mixer_workspace(ui, state),
            TopNavTab::EditChannel => render_edit_channel_page(ui, state),
            TopNavTab::PeqGeq => render_peq_geq_page(ui, state),
            TopNavTab::Effects => render_effects_page(ui, state),
            TopNavTab::MediaRecord => render_media_record_page(ui, state),
            TopNavTab::Settings => render_settings_page(ui, state),
        });
}

fn render_top_bar(
    ui: &mut Ui,
    state: &mut Axios24State,
    ferris_logo: Option<&egui::TextureHandle>,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 5.0;

        // ROW 1: Brand, Connection, Demo, Active Scene on Left | View Mode Switcher & Lock on Right
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Ferris Mixer Icon inside app
            if let Some(tex) = ferris_logo {
                ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::new(26.0, 26.0)));
            }

            // Brand badge
            Frame::NONE
                .fill(Color32::from_rgb(0, 175, 215))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("DUONN AXIOS 24 v{}", crate::updater::CURRENT_VERSION))
                            .size(12.5)
                            .strong()
                            .color(Color32::from_rgb(10, 14, 20)),
                    );
                });

            // Connection / Demo badge button
            let conn_col = match state.hw_link.mode {
                ConnectionMode::Demo => Color32::from_rgb(46, 204, 113),
                _ => Color32::from_rgb(0, 210, 255),
            };
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("\u{f1e6} {}", state.hw_link.mode.label()))
                            .size(11.0)
                            .strong()
                            .color(conn_col),
                    )
                    .fill(Color32::from_rgb(20, 28, 40)),
                )
                .on_hover_text("Clique para abrir a lista de dispositivos (UDP / TCP / Serial / Demo)")
                .clicked()
            {
                state.show_connect_modal = true;
            }

            // Demo Simulation Toggle
            let demo_lbl = if state.demo_running {
                "\u{f04b} DEMO ON"
            } else {
                "\u{f04c} DEMO PAUSADO"
            };
            if draw_pill_button(
                ui,
                demo_lbl,
                state.demo_running,
                Color32::from_rgb(39, 174, 96),
                Vec2::new(106.0, 22.0),
            )
            .on_hover_text("Ativa/Pausa a simulação de sinal, medidores LED e RTA em tempo real")
            .clicked()
            {
                let next = !state.demo_running;
                state.set_demo_running(next);
            }

            // Active Scene quick badge
            let sc_label = format!(
                "\u{f008} CENA {:02}: {}",
                state.active_scene_idx + 1,
                state.scene_names[state.active_scene_idx]
            );
            if draw_pill_button(
                ui,
                &sc_label,
                true,
                Color32::from_rgb(44, 62, 80),
                Vec2::new(126.0, 22.0),
            )
            .on_hover_text("Clique para gerenciar as 16 cenas")
            .clicked()
            {
                state.top_tab = TopNavTab::Settings;
                state.settings_tab = SettingsSubTab::Scene;
            }

            // Real USB / Microphone Audio FFT RTA Toggle
            let rta_real_on = state.live_rta.active;
            if draw_pill_button(
                ui,
                if rta_real_on {
                    "\u{f080} RTA REAL ON"
                } else {
                    "\u{f080} RTA USB/MIC"
                },
                rta_real_on,
                Color32::from_rgb(39, 174, 96),
                Vec2::new(98.0, 22.0),
            )
            .on_hover_text("Captura áudio REAL da placa USB / Microfone do Windows e calcula FFT de 2048 pontos em tempo real!")
            .clicked()
            {
                if rta_real_on {
                    state.live_rta.stop();
                    state.toast("RTA de Áudio Real (USB/Mic) desativado.");
                } else {
                    state.live_rta.refresh_devices();
                    let dev_id = state.live_rta.selected_device_id;
                    let dev_name = state
                        .live_rta
                        .available_devices
                        .iter()
                        .find(|(id, _)| *id == dev_id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "Entrada Padrão do Windows".to_string());
                    state.live_rta.start(dev_id);
                    state.show_rta_on_eq = true;
                    state.toast(format!("RTA FFT Real ativado via '{}'!", dev_name));
                }
            }

            // Stream Deck Macros Bar Toggle
            if draw_pill_button(
                ui,
                "\u{f0e7} MACROS",
                state.show_stream_deck_macros,
                Color32::from_rgb(230, 126, 34),
                Vec2::new(76.0, 22.0),
            )
            .on_hover_text("Abre a barra de 8 Macros Rápidas estilo Stream Deck (Palavra, Louvor, FX Mute, Troca de Palco, etc.)")
            .clicked()
            {
                state.show_stream_deck_macros = !state.show_stream_deck_macros;
            }

            // Right-aligned Console Mode Switcher + Lock
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                if draw_pill_button(
                    ui,
                    "\u{f023} LOCK",
                    false,
                    Color32::from_rgb(231, 76, 60),
                    Vec2::new(66.0, 22.0),
                )
                .clicked()
                {
                    state.console_locked = true;
                    state.lock_password_input.clear();
                }

                ui.separator();

                for vm in [ViewMode::Analog, ViewMode::Normal, ViewMode::BigDesktop] {
                    let active = state.view_mode == vm && state.top_tab == TopNavTab::Mixer;
                    if draw_pill_button(
                        ui,
                        vm.label(),
                        active,
                        Color32::from_rgb(142, 68, 173),
                        Vec2::new(98.0, 22.0),
                    )
                    .on_hover_text("Alterna o modo visual da console (inspirado no DS Mixer)")
                    .clicked()
                    {
                        state.view_mode = vm;
                        state.top_tab = TopNavTab::Mixer;
                    }
                }
            });
        });

        // ROW 2: 6 Full-Width Responsive Navigation Tabs (Zero Overlap)
        ui.horizontal(|ui| {
            let spacing = 6.0;
            ui.spacing_mut().item_spacing.x = spacing;
            let tabs = [
                (TopNavTab::Mixer, "\u{f1de} CONSOLE MIXER"),
                (TopNavTab::EditChannel, "\u{f085} EDITAR CANAL"),
                (TopNavTab::PeqGeq, "\u{f080} EQ / RTA & GEQ"),
                (TopNavTab::Effects, "\u{f0e7} EFEITOS FX1/2"),
                (TopNavTab::MediaRecord, "\u{f001} PLAYER & REC"),
                (TopNavTab::Settings, "\u{f013} CONFIG / CENAS"),
            ];
            let tab_w = ((ui.available_width() - spacing * (tabs.len() as f32 - 1.0))
                / (tabs.len() as f32))
                .max(90.0);
            for (tab, label) in tabs {
                let active = state.top_tab == tab;
                if draw_pill_button(
                    ui,
                    label,
                    active,
                    Color32::from_rgb(0, 150, 215),
                    Vec2::new(tab_w, 25.0),
                )
                .clicked()
                {
                    state.top_tab = tab;
                }
            }
        });

        // OPTIONAL ROW 3: Stream Deck 8 Programmable Live Macros Bar (DS Mixer style)
        if state.show_stream_deck_macros {
            ui.horizontal(|ui| {
                let spacing = 5.0_f32;
                ui.spacing_mut().item_spacing.x = spacing;
                let macros: [(&str, Color32, &str); 8] = [
                    (
                        "\u{f028} 1: PALAVRA / PASTOR",
                        Color32::from_rgb(41, 128, 185),
                        "Abre CH1 em 0dB com EQ de Locução, muta banda (CH2-16) e desliga FX1/FX2",
                    ),
                    (
                        "\u{f001} 2: LOUVOR / BANDA",
                        Color32::from_rgb(39, 174, 96),
                        "Desmuta CH1-16, ativa efeitos FX1/FX2 e coloca Master L/R em 0.0 dB",
                    ),
                    (
                        "\u{f0e7} 3: FX MUTE (SECAR VOZ)",
                        Color32::from_rgb(194, 24, 91),
                        "Muta/Desmuta FX1 e FX2 instantaneamente para o cantor falar sem reverb",
                    ),
                    (
                        "\u{f1e6} 4: TROCA DE PALCO",
                        Color32::from_rgb(211, 84, 0),
                        "Muta CH1-24 com segurança para plugar/desplugar cabos deixando apenas USB/DIGI aberto",
                    ),
                    (
                        "\u{f04b} 5: SOM AMBIENTE USB",
                        Color32::from_rgb(142, 68, 173),
                        "Sobe canal DIGI/USB para -4 dB e inicia a reprodução do Player USB",
                    ),
                    (
                        "\u{f071} 6: CORTAR MICROFONIA",
                        Color32::from_rgb(192, 57, 43),
                        "Aplica filtro Notch Q=18 (-9dB) exatamente na frequência de pico atual do canal",
                    ),
                    (
                        "\u{f00c} 7: LIMPAR SOLOS + L/R",
                        Color32::from_rgb(22, 160, 133),
                        "Limpa todos os Solos ativos e garante que todos os canais estejam roteados para o L/R",
                    ),
                    (
                        "\u{f0c7} 8: SALVAR SNAPSHOT",
                        Color32::from_rgb(44, 62, 80),
                        "Salva imediatamente o estado atual na Cena ativa em preset.bin",
                    ),
                ];
                let mw = ((ui.available_width() - spacing * 7.0) / 8.0).max(85.0);
                for (idx, (lbl, col, tip)) in macros.iter().enumerate() {
                    if draw_pill_button(ui, lbl, true, *col, Vec2::new(mw, 23.0))
                        .on_hover_text(*tip)
                        .clicked()
                    {
                        state.trigger_stream_deck_macro(idx);
                    }
                }
            });
        }
    });
}

fn render_bottom_bar(ui: &mut Ui, state: &mut Axios24State) {
    ui.horizontal(|ui| {
        let avail_w = ui.available_width();
        let compact = avail_w < 1180.0;
        ui.spacing_mut().item_spacing.x = if compact { 4.0 } else { 6.0 };

        if !compact {
            ui.label(
                RichText::new("LAYERS:")
                    .size(10.5)
                    .strong()
                    .color(Color32::from_rgb(145, 165, 190)),
            );
        }

        let layers = [
            (FaderLayer::Ch1_8, "CH 1-8"),
            (FaderLayer::Ch9_16, "CH 9-16"),
            (FaderLayer::Ch17_24, "CH 17-24"),
            (FaderLayer::DigiFx, "DIGI / FX"),
            (FaderLayer::Aux1_8, "AUX 1-8"),
            (FaderLayer::Dca1_4, "DCA 1-4"),
            (FaderLayer::AllInputs, "TODOS 1-24"),
        ];
        let ly_w = if compact { 68.0 } else { 80.0 };
        for (ly, lbl) in layers {
            let active = state.layer == ly;
            if draw_pill_button(
                ui,
                lbl,
                active,
                Color32::from_rgb(0, 168, 215),
                Vec2::new(ly_w, 24.0),
            )
            .clicked()
            {
                state.layer = ly;
            }
        }

        ui.separator();

        // Quick Mute Groups 1..4
        if !compact {
            ui.label(
                RichText::new("MUTE:")
                    .size(10.0)
                    .color(Color32::from_rgb(145, 165, 190)),
            );
        }
        for mg in 0..4 {
            let active = state.mute_group_active(mg);
            if draw_pill_button(
                ui,
                &format!("M{}", mg + 1),
                active,
                Color32::from_rgb(231, 76, 60),
                Vec2::new(30.0, 22.0),
            )
            .clicked()
            {
                state.set_mute_group_active(mg, !active);
            }
        }

        ui.separator();

        // Quick Signal Generator indicator
        let sig_on = state.signal_gen_enabled();
        if draw_pill_button(
            ui,
            if sig_on {
                "\u{f028} SIG ON"
            } else {
                "\u{f026} SIG OFF"
            },
            sig_on,
            Color32::from_rgb(243, 156, 18),
            Vec2::new(72.0, 22.0),
        )
        .clicked()
        {
            state.set_signal_gen_enabled(!sig_on);
        }

        // Right side: Phones level + Toast notification
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut ph_db = state.phones_level_db();
            ui.add_sized(
                [if compact { 82.0 } else { 105.0 }, 20.0],
                egui::Slider::new(&mut ph_db, -80.0..=0.0)
                    .text("\u{f025}")
                    .suffix("dB"),
            );
            if (ph_db - state.phones_level_db()).abs() > 0.1 {
                state.set_phones_level_db(ph_db);
            }

            if let Some((msg, exp)) = &state.status_toast {
                if state.sim_time < *exp {
                    ui.label(
                        RichText::new(format!("\u{f00c} {}", msg))
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(46, 213, 115)),
                    );
                }
            }
        });
    });
}

// =============================================================================
// GLOBAL KEYBOARD SHORTCUTS (DS Mixer Style Precision Control)
// =============================================================================
fn handle_keyboard_shortcuts(ctx: &egui::Context, state: &mut Axios24State) {
    // Ctrl+F toggles quick channel search bar anywhere
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
        state.show_search_bar = !state.show_search_bar;
        if !state.show_search_bar {
            state.channel_search_query.clear();
        }
        return;
    }

    if ctx.wants_keyboard_input() {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.show_search_bar = false;
            state.channel_search_query.clear();
            state.renaming_channel = None;
        }
        return;
    }

    ctx.input(|i| {
        let sel = state.selected_channel;
        let step = if i.modifiers.shift { 0.1_f32 } else { 1.0_f32 };

        // Ctrl + 1..6: Quick Tab Navigation
        if i.modifiers.command {
            if i.key_pressed(egui::Key::Num1) {
                state.top_tab = TopNavTab::Mixer;
                return;
            } else if i.key_pressed(egui::Key::Num2) {
                state.top_tab = TopNavTab::EditChannel;
                return;
            } else if i.key_pressed(egui::Key::Num3) {
                state.top_tab = TopNavTab::PeqGeq;
                return;
            } else if i.key_pressed(egui::Key::Num4) {
                state.top_tab = TopNavTab::Effects;
                return;
            } else if i.key_pressed(egui::Key::Num5) {
                state.top_tab = TopNavTab::MediaRecord;
                return;
            } else if i.key_pressed(egui::Key::Num6) {
                state.top_tab = TopNavTab::Settings;
                return;
            } else if i.key_pressed(egui::Key::R) {
                state.renaming_channel = Some(sel);
                state.rename_buffer = state.channel_name(sel).to_string();
                return;
            } else if i.key_pressed(egui::Key::C) {
                state.clipboard_channel = Some(sel);
                state.toast(format!("Canal {} copiado (Ctrl+C)!", state.channel_name(sel)));
                return;
            } else if i.key_pressed(egui::Key::V) {
                if let Some(src) = state.clipboard_channel {
                    state.paste_channel_from(src, sel);
                }
                return;
            } else if i.key_pressed(egui::Key::D) {
                state.toggle_master_dim();
                return;
            } else if i.modifiers.shift && i.key_pressed(egui::Key::S) {
                state.clear_all_solos();
                return;
            }
        }

        // Alt + 1..8: Sends on Fader AUX1..8 | Alt + 0: MAIN L/R
        if i.modifiers.alt {
            let aux_keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
            ];
            for (idx, key) in aux_keys.iter().enumerate() {
                if i.key_pressed(*key) {
                    let target = SendsOnFaderTarget::Aux(idx);
                    state.sends_on_fader = if state.sends_on_fader == target {
                        SendsOnFaderTarget::MainLR
                    } else {
                        target
                    };
                    return;
                }
            }
            if i.key_pressed(egui::Key::Num0) {
                state.sends_on_fader = SendsOnFaderTarget::MainLR;
                return;
            }
        }

        if i.key_pressed(egui::Key::ArrowUp) {
            let max_db = state.effective_fader_max_db(sel);
            let cur = state.effective_fader_db(sel);
            let next = if cur <= -115.0 {
                -60.0
            } else {
                (cur + step).clamp(-120.0, max_db)
            };
            state.set_effective_fader_db(sel, (next * 10.0).round() / 10.0);
        } else if i.key_pressed(egui::Key::ArrowDown) {
            let max_db = state.effective_fader_max_db(sel);
            let cur = state.effective_fader_db(sel);
            let next = (cur - step).clamp(-120.0, max_db);
            state.set_effective_fader_db(sel, (next * 10.0).round() / 10.0);
        } else if i.key_pressed(egui::Key::ArrowLeft) {
            if let SelectedChannel::Input(ch) = sel {
                state.selected_channel = SelectedChannel::Input(ch.saturating_sub(1));
            }
        } else if i.key_pressed(egui::Key::ArrowRight) {
            if let SelectedChannel::Input(ch) = sel {
                state.selected_channel = SelectedChannel::Input((ch + 1).min(24));
            }
        } else if i.key_pressed(egui::Key::M) {
            let m = state.channel_muted(sel);
            state.set_channel_muted(sel, !m);
        } else if i.key_pressed(egui::Key::S) {
            let s = state.channel_solo(sel);
            state.set_channel_solo(sel, !s);
        } else if i.key_pressed(egui::Key::L) {
            if let (1, ch) = sel.group_and_index() {
                let lr = state.input_to_main_lr(ch);
                state.set_input_to_main_lr(ch, !lr);
            }
        } else if i.key_pressed(egui::Key::P) {
            if let (1, ch) = sel.group_and_index() {
                if let SendsOnFaderTarget::Aux(aux_idx) = state.sends_on_fader {
                    let pre = state.input_aux_pre_fader(ch, aux_idx);
                    state.set_input_aux_pre_fader(ch, aux_idx, !pre);
                }
            }
        } else if i.key_pressed(egui::Key::Num0) || i.key_pressed(egui::Key::Home) {
            state.set_effective_fader_db(sel, 0.0);
            state.toast(format!("{} -> 0.0 dB (Unity)", state.channel_name(sel)));
        } else if i.key_pressed(egui::Key::Escape) {
            state.sends_on_fader = SendsOnFaderTarget::MainLR;
            state.show_search_bar = false;
            state.channel_search_query.clear();
            state.show_connect_modal = false;
        } else if i.key_pressed(egui::Key::F1) {
            state.layer = FaderLayer::Ch1_8;
        } else if i.key_pressed(egui::Key::F2) {
            state.layer = FaderLayer::Ch9_16;
        } else if i.key_pressed(egui::Key::F3) {
            state.layer = FaderLayer::Ch17_24;
        } else if i.key_pressed(egui::Key::F4) {
            state.layer = FaderLayer::DigiFx;
        } else if i.key_pressed(egui::Key::F5) {
            state.layer = FaderLayer::Aux1_8;
        } else if i.key_pressed(egui::Key::F6) {
            state.layer = FaderLayer::Dca1_4;
        } else if i.key_pressed(egui::Key::F7) {
            state.layer = FaderLayer::AllInputs;
        } else if i.key_pressed(egui::Key::F8) {
            state.view_mode = match state.view_mode {
                ViewMode::Normal => ViewMode::Analog,
                ViewMode::Analog => ViewMode::BigDesktop,
                ViewMode::BigDesktop => ViewMode::Normal,
            };
            state.toast(format!("Modo de Visão: {}", state.view_mode.label()));
        } else if i.key_pressed(egui::Key::F9) {
            let next = !state.demo_running;
            state.set_demo_running(next);
        } else if i.key_pressed(egui::Key::F10) {
            state.show_connect_modal = !state.show_connect_modal;
        } else if i.key_pressed(egui::Key::F11) {
            state.dark_theme = !state.dark_theme;
        } else if i.key_pressed(egui::Key::F12) {
            state.console_locked = true;
        }
    });
}

// =============================================================================
// 1. MIXER WORKSPACE (Big Desktop Split View / Normal Console / Analog Console)
// =============================================================================
fn render_mixer_workspace(ui: &mut Ui, state: &mut Axios24State) {
    let total_h = ui.available_height();
    let max_w = ui.available_width();

    if state.view_mode == ViewMode::BigDesktop && total_h >= 380.0 {
        // DS Mixer "Big Desktop" Split View:
        // Upper Half: Complete Selected Channel Inspector (Preamp + Interactive PEQ Graph + Gate/Comp Curves + Sends)
        // Lower Half: Sends-on-Fader Toolbar + Full-Width Channel Fader Bank (while MAIN L & R sit in the right dock)
        let top_h = (total_h * 0.42).clamp(180.0, 315.0);
        ui.allocate_ui_with_layout(
            Vec2::new(max_w, top_h),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_clip_rect(ui.max_rect());
                render_big_desktop_upper_inspector(ui, state, top_h);
            },
        );
        ui.add_space(3.0);
    } else {
        // Quick Selected-Channel Rename & Color Bar in Normal / Analog mode
        let sel = state.selected_channel;
        let dark = state.dark_theme;
        Frame::NONE
            .fill(if dark { Color32::from_rgb(16, 22, 32) } else { Color32::from_rgb(228, 234, 243) })
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin::symmetric(8, 4))
            .stroke(Stroke::new(
                1.0_f32,
                if dark { Color32::from_rgb(36, 48, 66) } else { Color32::from_rgb(185, 198, 215) },
            ))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new("CANAL SELECIONADO:")
                            .size(10.5)
                            .strong()
                            .color(Color32::from_rgb(0, 185, 225)),
                    );
                    let mut name = state.channel_name(sel).to_string();
                    ui.label(RichText::new("Nome:").size(11.0));
                    if ui
                        .add_sized(
                            [130.0, 20.0],
                            egui::TextEdit::singleline(&mut name).hint_text("Nome do canal..."),
                        )
                        .changed()
                    {
                        state.set_channel_name(sel, name);
                    }

                    ui.label(RichText::new("Cor:").size(11.0));
                    for (c_idx, &col) in STRIP_PALETTE.iter().take(8).enumerate() {
                        let is_cur = state.channel_color_idx(sel) == c_idx as u8;
                        let (c_rect, c_resp) =
                            ui.allocate_exact_size(Vec2::new(15.0, 15.0), egui::Sense::click());
                        ui.painter().rect_filled(c_rect, CornerRadius::same(3), col);
                        if is_cur {
                            ui.painter().rect_stroke(
                                c_rect,
                                CornerRadius::same(3),
                                Stroke::new(2.0_f32, if dark { Color32::WHITE } else { Color32::BLACK }),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if c_resp.clicked() {
                            state.set_channel_color_idx(sel, c_idx as u8);
                        }
                    }
                    let cur_col = STRIP_PALETTE[(state.channel_color_idx(sel) as usize) % STRIP_PALETTE.len()];
                    ui.menu_button(
                        RichText::new("32 Cores +").size(11.0).strong().color(cur_col),
                        |ui| {
                            draw_color_palette_grid(ui, state, sel, 8, 18.0);
                        },
                    );

                    if let (1, ch) = sel.group_and_index() {
                        if let SendsOnFaderTarget::Aux(aux_idx) = state.sends_on_fader {
                            ui.separator();
                            let pre = state.input_aux_pre_fader(ch, aux_idx);
                            let pre_lbl = if pre {
                                format!("AUX{}: PRE FADER", aux_idx + 1)
                            } else {
                                format!("AUX{}: POST FADER", aux_idx + 1)
                            };
                            if draw_pill_button(
                                ui,
                                &pre_lbl,
                                true,
                                if pre {
                                    Color32::from_rgb(26, 188, 156)
                                } else {
                                    Color32::from_rgb(230, 126, 34)
                                },
                                Vec2::new(122.0, 20.0),
                            )
                            .clicked()
                            {
                                state.set_input_aux_pre_fader(ch, aux_idx, !pre);
                            }
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("\u{f085} Editar Canal Completo").clicked() {
                            state.top_tab = TopNavTab::EditChannel;
                        }
                    });
                });
            });
        ui.add_space(3.0);
    }

    // Sends on Fader (SOF) + Search (Ctrl+F) + Diagnostic Bar (Fixes #1 Duonn Forum Complaints)
    let dark = state.dark_theme;
    Frame::NONE
        .fill(if state.sends_on_fader != SendsOnFaderTarget::MainLR {
            if dark { Color32::from_rgb(32, 24, 14) } else { Color32::from_rgb(252, 236, 210) }
        } else if dark {
            Color32::from_rgb(14, 19, 28)
        } else {
            Color32::from_rgb(225, 232, 242)
        })
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 3))
        .stroke(Stroke::new(
            1.0_f32,
            if state.sends_on_fader != SendsOnFaderTarget::MainLR {
                Color32::from_rgb(243, 156, 18)
            } else if dark {
                Color32::from_rgb(32, 42, 58)
            } else {
                Color32::from_rgb(185, 198, 215)
            },
        ))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(
                    RichText::new("FADERS:")
                        .size(10.0)
                        .strong()
                        .color(Color32::from_rgb(160, 180, 205)),
                );
                if draw_pill_button(
                    ui,
                    "MAIN L/R",
                    state.sends_on_fader == SendsOnFaderTarget::MainLR,
                    Color32::from_rgb(0, 175, 215),
                    Vec2::new(62.0, 20.0),
                )
                .on_hover_text("Faders controlam o volume principal para o PA (MAIN L/R)")
                .clicked()
                {
                    state.sends_on_fader = SendsOnFaderTarget::MainLR;
                }

                for aux in 0..8 {
                    let target = SendsOnFaderTarget::Aux(aux);
                    if draw_pill_button(
                        ui,
                        &format!("AUX{}", aux + 1),
                        state.sends_on_fader == target,
                        Color32::from_rgb(26, 188, 156),
                        Vec2::new(40.0, 20.0),
                    )
                    .on_hover_text("Sends on Fader: Faz todos os faders controlarem o envio de retorno para este AUX")
                    .clicked()
                    {
                        state.sends_on_fader = if state.sends_on_fader == target {
                            SendsOnFaderTarget::MainLR
                        } else {
                            target
                        };
                    }
                }

                for fx in 0..2 {
                    let target = SendsOnFaderTarget::Fx(fx);
                    if draw_pill_button(
                        ui,
                        &format!("FX{}", fx + 1),
                        state.sends_on_fader == target,
                        Color32::from_rgb(233, 30, 99),
                        Vec2::new(36.0, 20.0),
                    )
                    .on_hover_text("Sends on Fader: Faz todos os faders controlarem a mandada de efeito para este FX")
                    .clicked()
                    {
                        state.sends_on_fader = if state.sends_on_fader == target {
                            SendsOnFaderTarget::MainLR
                        } else {
                            target
                        };
                    }
                }

                ui.separator();

                // Filter unused channels toggle
                let hide = state.hide_unused_channels;
                if draw_pill_button(
                    ui,
                    "Só Ativos",
                    hide,
                    Color32::from_rgb(142, 68, 173),
                    Vec2::new(60.0, 20.0),
                )
                .on_hover_text("Oculta canais mutados e em -∞ dB para focar apenas nos canais em uso")
                .clicked()
                {
                    state.hide_unused_channels = !hide;
                }

                // Quick Search toggle (Ctrl+F)
                if draw_pill_button(
                    ui,
                    "\u{f00c} Buscar (Ctrl+F)",
                    state.show_search_bar || !state.channel_search_query.is_empty(),
                    Color32::from_rgb(52, 152, 219),
                    Vec2::new(92.0, 20.0),
                )
                .clicked()
                {
                    state.show_search_bar = !state.show_search_bar;
                    if !state.show_search_bar {
                        state.channel_search_query.clear();
                    }
                }

                if state.show_search_bar {
                    ui.add_sized(
                        [110.0, 20.0],
                        egui::TextEdit::singleline(&mut state.channel_search_query)
                            .hint_text("Nome do canal..."),
                    );
                }

                // Automatic Diagnostic Alert if any input is disconnected from L/R or a Mute Group is active!
                let dis_lr = state.disconnected_lr_channels();
                let any_mg = (0..4).any(|mg| state.mute_group_active(mg));
                if !dis_lr.is_empty() || any_mg {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let warn_lbl = if !dis_lr.is_empty() {
                            format!("\u{f071} {} Canal(is) sem L/R (Corrigir)", dis_lr.len())
                        } else {
                            "\u{f071} Mute Group Ativo (Liberar)".to_string()
                        };
                        if draw_pill_button(
                            ui,
                            &warn_lbl,
                            true,
                            Color32::from_rgb(211, 84, 0),
                            Vec2::new(178.0, 20.0),
                        )
                        .on_hover_text("Clique para ativar o envio L/R em todos os canais e liberar grupos de mute travados")
                        .clicked()
                        {
                            state.fix_all_routing_to_main_lr();
                        }
                    });
                } else if state.sends_on_fader != SendsOnFaderTarget::MainLR {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "\u{f071} MODO SENDS ON FADER: {} (ESC p/ L/R)",
                                state.sends_on_fader.label()
                            ))
                            .size(10.5)
                            .strong()
                            .color(Color32::from_rgb(255, 205, 50)),
                        );
                    });
                }
            });
        });

    ui.add_space(3.0);

    // Channel Strips Fader Bank — Expands across 100% of the available central width!
    let strips_h = ui.available_height().max(120.0);
    let avail_w = ui.available_width();
    let mut channels = if !state.channel_search_query.trim().is_empty() {
        let q = state.channel_search_query.trim().to_lowercase();
        let all_candidates: Vec<SelectedChannel> = (0..25)
            .map(SelectedChannel::Input)
            .chain((0..2).map(SelectedChannel::Fx))
            .chain((0..8).map(SelectedChannel::Output))
            .chain((0..4).map(SelectedChannel::Dca))
            .collect();
        all_candidates
            .into_iter()
            .filter(|&sel| state.channel_name(sel).to_lowercase().contains(&q))
            .collect()
    } else {
        channels_for_layer(state.layer)
    };

    if state.hide_unused_channels {
        channels.retain(|&sel| {
            !state.channel_muted(sel) || state.channel_fader_db(sel) > -90.0 || state.selected_channel == sel
        });
    }
    if channels.is_empty() {
        channels.push(state.selected_channel);
    }

    let n = channels.len().max(1);
    let spacing = 4.0_f32;
    let total_spacing = spacing * (n.saturating_sub(1) as f32);
    let expanded_strip_w = (avail_w - total_spacing) / (n as f32);

    if expanded_strip_w >= 45.0 {
        // Fit all channels in the layer across 100% of the window width with zero scrollbar
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            for sel in channels {
                draw_channel_strip(ui, state, sel, expanded_strip_w, strips_h, false);
            }
        });
    } else {
        // Fallback horizontal scroll only when viewing 24 channels on a narrow window
        let scroll_strip_h = (strips_h - 12.0).max(120.0);
        ScrollArea::horizontal()
            .id_salt("fader_bank_scroll")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    for sel in channels {
                        draw_channel_strip(ui, state, sel, 52.0, scroll_strip_h, false);
                    }
                });
            });
    }
}

fn channels_for_layer(layer: FaderLayer) -> Vec<SelectedChannel> {
    match layer {
        FaderLayer::Ch1_8 => (0..8).map(SelectedChannel::Input).collect(),
        FaderLayer::Ch9_16 => (8..16).map(SelectedChannel::Input).collect(),
        FaderLayer::Ch17_24 => (16..24).map(SelectedChannel::Input).collect(),
        FaderLayer::DigiFx => vec![
            SelectedChannel::Input(24),
            SelectedChannel::Fx(0),
            SelectedChannel::Fx(1),
            SelectedChannel::Output(0),
            SelectedChannel::Output(1),
            SelectedChannel::Output(2),
            SelectedChannel::Output(3),
            SelectedChannel::Dca(0),
        ],
        FaderLayer::Aux1_8 => (0..8).map(SelectedChannel::Output).collect(),
        FaderLayer::Dca1_4 => vec![
            SelectedChannel::Dca(0),
            SelectedChannel::Dca(1),
            SelectedChannel::Dca(2),
            SelectedChannel::Dca(3),
            SelectedChannel::Fx(0),
            SelectedChannel::Fx(1),
            SelectedChannel::Input(24),
        ],
        FaderLayer::AllInputs => (0..24).map(SelectedChannel::Input).collect(),
    }
}

/// Renders the upper inspector rack in "Big Desktop" mode (Preamp, Interactive PEQ Graph, Gate & Comp Curve Graphs, Aux Sends)
fn render_big_desktop_upper_inspector(ui: &mut Ui, state: &mut Axios24State, height: f32) {
    let sel = state.selected_channel;
    let (group, ch) = sel.group_and_index();
    let flat_id = sel.flat_id();
    let meter_db = state.channel_meters_db[flat_id];
    let gap = 6.0_f32;
    let usable_w = (ui.available_width() - gap * 3.0).max(400.0);

    let pre_w = usable_w * 0.21;
    let eq_w = usable_w * 0.41;
    let dyn_w = usable_w * 0.21;
    let send_w = usable_w * 0.17;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;

        // 1. Preamp & Channel Card (21% width)
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(6))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(ui, |ui| {
                let inner_w = (pre_w - 12.0).max(110.0);
                let inner_h = (height - 12.0).max(120.0);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.set_height(inner_h);
                ui.set_max_height(inner_h);

                let knob_d = if inner_h < 220.0 || inner_w < 175.0 {
                    32.0
                } else {
                    38.0
                };

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("CANAL / PREAMP")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(0, 215, 255)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Editar").clicked() {
                                state.top_tab = TopNavTab::EditChannel;
                            }
                        });
                    });

                    // Channel Name & Color Dots
                    let mut name = state.channel_name(sel).to_string();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Nome:").size(10.5));
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut name)
                                    .desired_width((inner_w - 46.0).max(55.0)),
                            )
                            .changed()
                        {
                            state.set_channel_name(sel, name);
                        }
                    });
                    let dot_w = ((inner_w - 7.0 * 3.0) / 8.0).clamp(11.0, 15.0);
                    draw_color_palette_grid(ui, state, sel, 8, dot_w);

                    if group == 1 {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let mut gain = state.input_preamp_gain_db(ch);
                            let g_txt = format!("+{:.0}dB", gain);
                            if draw_rotary_knob(
                                ui,
                                "GANHO",
                                &mut gain,
                                0.0,
                                60.0,
                                0.0,
                                &g_txt,
                                Color32::from_rgb(231, 76, 60),
                                knob_d,
                            ) {
                                state.set_input_preamp_gain_db(ch, gain);
                            }

                            let mut dly = state.input_delay_ms(ch);
                            let d_txt = format!("{:.1}ms", dly);
                            if draw_rotary_knob(
                                ui,
                                "DELAY",
                                &mut dly,
                                0.0,
                                10.0,
                                0.0,
                                &d_txt,
                                Color32::from_rgb(52, 152, 219),
                                knob_d,
                            ) {
                                state.set_input_delay_ms(ch, dly);
                            }

                            let mut pan = state.input_pan(ch) as f32;
                            let p_txt = if pan.abs() < 1.0 {
                                "L=R".to_string()
                            } else if pan < 0.0 {
                                format!("L{:.0}", -pan)
                            } else {
                                format!("R{:.0}", pan)
                            };
                            if draw_rotary_knob(
                                ui,
                                "PAN",
                                &mut pan,
                                -100.0,
                                100.0,
                                0.0,
                                &p_txt,
                                Color32::from_rgb(46, 204, 113),
                                knob_d,
                            ) {
                                state.set_input_pan(ch, pan.round() as i32);
                            }
                        });

                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                            if ch < 24 {
                                let p48 = state.input_phantom_48v(ch);
                                if draw_pill_button(
                                    ui,
                                    "+48V",
                                    p48,
                                    Color32::from_rgb(231, 76, 60),
                                    Vec2::new(42.0, 19.0),
                                )
                                .clicked()
                                {
                                    state.set_input_phantom_48v(ch, !p48);
                                }
                            }
                            let inv = state.input_phase_invert(ch);
                            if draw_pill_button(
                                ui,
                                "Ø FASE",
                                inv,
                                Color32::from_rgb(243, 156, 18),
                                Vec2::new(48.0, 19.0),
                            )
                            .clicked()
                            {
                                state.set_input_phase_invert(ch, !inv);
                            }
                            if ch < 2 {
                                let hiz = state.input_hiz(ch);
                                if draw_pill_button(
                                    ui,
                                    "HI-Z",
                                    hiz,
                                    Color32::from_rgb(155, 89, 182),
                                    Vec2::new(40.0, 19.0),
                                )
                                .clicked()
                                {
                                    state.set_input_hiz(ch, !hiz);
                                }
                            }
                            if ch < 24 {
                                let lnk = state.is_input_stereo_linked(ch);
                                if draw_pill_button(
                                    ui,
                                    "LINK",
                                    lnk,
                                    Color32::from_rgb(0, 188, 212),
                                    Vec2::new(44.0, 19.0),
                                )
                                .clicked()
                                {
                                    state.set_input_stereo_linked(ch, !lnk);
                                }
                            }
                        });
                    } else if group == 3 {
                        let mut dly = state.output_delay_ms(ch);
                        let d_txt = format!("{:.1} ms", dly);
                        if draw_rotary_knob(
                            ui,
                            "DELAY SAÍDA",
                            &mut dly,
                            0.0,
                            40.0,
                            0.0,
                            &d_txt,
                            Color32::from_rgb(52, 152, 219),
                            knob_d,
                        ) {
                            state.set_output_delay_ms(ch, dly);
                        }
                        ui.horizontal(|ui| {
                            let inv = state.output_phase_invert(ch);
                            if draw_pill_button(
                                ui,
                                "Ø FASE",
                                inv,
                                Color32::from_rgb(243, 156, 18),
                                Vec2::new(56.0, 20.0),
                            )
                            .clicked()
                            {
                                state.set_output_phase_invert(ch, !inv);
                            }
                            let lnk = state.is_output_stereo_linked(ch);
                            if draw_pill_button(
                                ui,
                                "LINK PAR",
                                lnk,
                                Color32::from_rgb(0, 188, 212),
                                Vec2::new(62.0, 20.0),
                            )
                            .clicked()
                            {
                                state.set_output_stereo_linked(ch, !lnk);
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if group == 1 && ch < 24 {
                            if ui
                                .small_button("\u{f0e7} Auto-Gain")
                                .on_hover_text("Ajusta o ganho do pré-amplificador automaticamente para o nível nominal (-18 dB)")
                                .clicked()
                            {
                                state.run_auto_gain(ch);
                            }
                        }
                        egui::ComboBox::from_id_salt("big_desk_preset_combo")
                            .selected_text("Presets")
                            .width(68.0)
                            .show_ui(ui, |ui| {
                                for p in ChannelPreset::ALL {
                                    if ui.selectable_label(false, p.label()).clicked() {
                                        state.apply_channel_preset(sel, p);
                                    }
                                }
                            });
                        if ui.small_button("\u{f0c5}").on_hover_text("Copiar Canal").clicked() {
                            state.clipboard_channel = Some(sel);
                            state.toast(format!("Canal {} copiado!", state.channel_name(sel)));
                        }
                        if let Some(src) = state.clipboard_channel {
                            if ui.small_button("\u{f0ea}").on_hover_text("Colar Canal").clicked() {
                                state.paste_channel_from(src, sel);
                            }
                        }
                    });
                });
            });

        // 2. Interactive Parametric Equalizer Graph (41% width)
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(6))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(ui, |ui| {
                let inner_w = (eq_w - 12.0).max(200.0);
                let inner_h = (height - 12.0).max(120.0);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.set_height(inner_h);
                ui.set_max_height(inner_h);
                let eq_group = group.max(1).min(3);
                let eq_ch = if group == 4 { 0 } else { ch };

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("EQUALIZADOR (PEQ)")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(0, 230, 255)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if ui.small_button("GEQ / Full").clicked() {
                            state.top_tab = TopNavTab::PeqGeq;
                        }
                        if ui
                            .small_button("\u{f071} Notch")
                            .on_hover_text("Anti-Microfonia: aplica um filtro Notch estreito (Q=18, -9dB) no pico de frequência atual")
                            .clicked()
                        {
                            state.apply_anti_feedback_notch(eq_group, eq_ch);
                        }
                        if ui.small_button("Reset").clicked() {
                            state.reset_peq(eq_group, eq_ch);
                        }
                        let rta = state.show_rta_on_eq;
                        if draw_pill_button(
                            ui,
                            "RTA",
                            rta,
                            Color32::from_rgb(0, 180, 215),
                            Vec2::new(34.0, 18.0),
                        )
                        .clicked()
                        {
                            state.show_rta_on_eq = !rta;
                        }
                        let eq_on = state.eq_enabled(eq_group, eq_ch);
                        if draw_pill_button(
                            ui,
                            if eq_on { "EQ ON" } else { "BYP" },
                            eq_on,
                            Color32::from_rgb(46, 204, 113),
                            Vec2::new(44.0, 18.0),
                        )
                        .clicked()
                        {
                            state.set_eq_enabled(eq_group, eq_ch, !eq_on);
                        }
                    });
                });

                let graph_h = (ui.available_height() - 24.0).max(90.0);
                draw_interactive_peq_graph(
                    ui,
                    state,
                    eq_group,
                    eq_ch,
                    Vec2::new(ui.available_width(), graph_h),
                );

                // Quick band selector bar under the EQ curve
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let nb = Axios24State::peq_band_count(eq_group);
                    if draw_pill_button(
                        ui,
                        "HPF",
                        state.selected_eq_band == 0,
                        EQ_BAND_COLORS[0],
                        Vec2::new(34.0, 18.0),
                    )
                    .clicked()
                    {
                        state.selected_eq_band = 0;
                    }
                    for b in 1..=nb {
                        if draw_pill_button(
                            ui,
                            &format!("B{}", b),
                            state.selected_eq_band == b,
                            EQ_BAND_COLORS[b.min(7)],
                            Vec2::new(28.0, 18.0),
                        )
                        .clicked()
                        {
                            state.selected_eq_band = b;
                        }
                    }
                    if draw_pill_button(
                        ui,
                        "LPF",
                        state.selected_eq_band == nb + 1,
                        EQ_BAND_COLORS[8],
                        Vec2::new(34.0, 18.0),
                    )
                    .clicked()
                    {
                        state.selected_eq_band = nb + 1;
                    }
                });
            });

        // 3. Dynamics Card: Noise Gate & Compressor Transfer Curve Graphs (21% width)
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(6))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(ui, |ui| {
                let inner_w = (dyn_w - 12.0).max(140.0);
                let inner_h = (height - 12.0).max(120.0);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.set_height(inner_h);
                ui.set_max_height(inner_h);
                ui.label(
                    RichText::new("DINÂMICA (GATE & COMP)")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(255, 195, 0)),
                );

                let c_grp = if group == 3 { 3 } else { 1 };
                let c_ch = if group == 3 { ch } else { ch.min(25) };

                ui.horizontal(|ui| {
                    let sub_w = ((ui.available_width() - 6.0) * 0.5).max(60.0);
                    let curve_h = (ui.available_height() - 26.0).max(80.0);

                    // Gate Curve
                    ui.vertical(|ui| {
                        let g_on = state.input_gate_enabled(c_ch);
                        let mut g_th = state.input_gate_threshold_db(c_ch);
                        if draw_gate_curve_graph(
                            ui,
                            &mut g_th,
                            g_on,
                            meter_db,
                            Vec2::new(sub_w, curve_h),
                        ) {
                            state.set_input_gate_threshold_db(c_ch, g_th);
                        }
                        if draw_pill_button(
                            ui,
                            if g_on { "GATE ON" } else { "GATE OFF" },
                            g_on,
                            Color32::from_rgb(46, 204, 113),
                            Vec2::new(sub_w, 18.0),
                        )
                        .clicked()
                        {
                            state.set_input_gate_enabled(c_ch, !g_on);
                        }
                    });

                    // Compressor Curve
                    ui.vertical(|ui| {
                        let c_on = state.comp_enabled(c_grp, c_ch);
                        let mut c_th = state.comp_threshold_db(c_grp, c_ch);
                        let c_rat = state.comp_ratio(c_grp, c_ch);
                        let c_gn = state.comp_gain_db(c_grp, c_ch);
                        if draw_comp_curve_graph(
                            ui,
                            &mut c_th,
                            c_rat,
                            c_gn,
                            c_on,
                            meter_db,
                            Vec2::new(sub_w, curve_h),
                        ) {
                            state.set_comp_threshold_db(c_grp, c_ch, c_th);
                        }
                        if draw_pill_button(
                            ui,
                            if c_on { "COMP ON" } else { "COMP OFF" },
                            c_on,
                            Color32::from_rgb(243, 156, 18),
                            Vec2::new(sub_w, 18.0),
                        )
                        .clicked()
                        {
                            state.set_comp_enabled(c_grp, c_ch, !c_on);
                        }
                    });
                });
            });

        // 4. Bus Sends Card: AUX 1-8 & FX 1-2 (17% width)
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(6))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(ui, |ui| {
                let inner_w = (send_w - 12.0).max(110.0);
                let inner_h = (height - 12.0).max(120.0);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.set_height(inner_h);
                ui.set_max_height(inner_h);
                ui.label(
                    RichText::new("AUX 1-8 & FX 1-2")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(46, 213, 115)),
                );

                let in_ch = if group == 1 { ch } else { 0 };
                ScrollArea::vertical()
                    .id_salt("big_desktop_sends")
                    .show(ui, |ui| {
                        let sl_w = (inner_w - 72.0).max(36.0);
                        ui.spacing_mut().slider_width = sl_w;
                        for aux in 0..8 {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                let mut s_db = state.input_aux_send_db(in_ch, aux);
                                let pre = state.input_aux_pre_fader(in_ch, aux);
                                if draw_pill_button(
                                    ui,
                                    if pre { "PRE" } else { "PST" },
                                    pre,
                                    Color32::from_rgb(26, 188, 156),
                                    Vec2::new(28.0, 16.0),
                                )
                                .clicked()
                                {
                                    state.set_input_aux_pre_fader(in_ch, aux, !pre);
                                }
                                ui.label(
                                    RichText::new(format!("A{}", aux + 1))
                                        .size(10.0)
                                        .monospace(),
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut s_db, -120.0..=0.0)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    state.set_input_aux_send_db(in_ch, aux, s_db);
                                }
                            });
                        }
                        for fx in 0..2 {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                let mut f_db = state.input_fx_send_db(in_ch, fx);
                                ui.label(
                                    RichText::new(format!("FX{}", fx + 1))
                                        .size(10.0)
                                        .monospace()
                                        .color(Color32::from_rgb(233, 30, 99)),
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut f_db, -120.0..=0.0)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    state.set_input_fx_send_db(in_ch, fx, f_db);
                                }
                            });
                        }
                    });
            });
    });
}

// =============================================================================
// 2. DEDICATED CHANNEL EDIT PAGE (Preamp, Recording, Gate, Comp, Sends, DCA/Mute)
// =============================================================================
fn render_edit_channel_page(ui: &mut Ui, state: &mut Axios24State) {
    // Top Quick Channel Selector Matrix (CH1..24, DIGI, FX1..2, AUX1..8, MAIN L/R)
    Frame::NONE
        .fill(Color32::from_rgb(15, 20, 30))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(36, 48, 66)))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for i in 0..24 {
                    let s = SelectedChannel::Input(i);
                    if draw_pill_button(
                        ui,
                        &format!("IN{}", i + 1),
                        state.selected_channel == s,
                        Color32::from_rgb(0, 180, 215),
                        Vec2::new(40.0, 22.0),
                    )
                    .clicked()
                    {
                        state.selected_channel = s;
                    }
                }
                let s_digi = SelectedChannel::Input(24);
                if draw_pill_button(
                    ui,
                    "DIGI",
                    state.selected_channel == s_digi,
                    Color32::from_rgb(155, 89, 182),
                    Vec2::new(46.0, 22.0),
                )
                .clicked()
                {
                    state.selected_channel = s_digi;
                }
                for i in 0..8 {
                    let s = SelectedChannel::Output(i);
                    if draw_pill_button(
                        ui,
                        &format!("AUX{}", i + 1),
                        state.selected_channel == s,
                        Color32::from_rgb(26, 188, 156),
                        Vec2::new(46.0, 22.0),
                    )
                    .clicked()
                    {
                        state.selected_channel = s;
                    }
                }
                for (i, lbl) in [(8, "MAIN L"), (9, "MAIN R")] {
                    let s = SelectedChannel::Output(i);
                    if draw_pill_button(
                        ui,
                        lbl,
                        state.selected_channel == s,
                        Color32::from_rgb(0, 215, 255),
                        Vec2::new(56.0, 22.0),
                    )
                    .clicked()
                    {
                        state.selected_channel = s;
                    }
                }
            });
        });

    ui.add_space(6.0);
    let sel = state.selected_channel;
    let (group, ch) = sel.group_and_index();
    let flat_id = sel.flat_id();
    let meter_db = state.channel_meters_db[flat_id];

    ui.columns(3, |cols| {
        // Column 0: Preamp, Gate & Recording Routing
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(10))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(&mut cols[0], |ui| {
                ui.heading(
                    RichText::new(format!("1. PREAMP & NOISE GATE — {}", state.channel_name(sel)))
                        .size(14.0)
                        .color(Color32::from_rgb(0, 220, 255)),
                );
                ui.separator();

                let c_ch = if group == 1 { ch } else { 0 };
                let mut g_on = state.input_gate_enabled(c_ch);
                let mut g_th = state.input_gate_threshold_db(c_ch);
                if draw_gate_curve_graph(
                    ui,
                    &mut g_th,
                    g_on,
                    meter_db,
                    Vec2::new(ui.available_width(), 170.0),
                ) {
                    state.set_input_gate_threshold_db(c_ch, g_th);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut g_on, "Ativar Noise Gate").changed() {
                        state.set_input_gate_enabled(c_ch, g_on);
                    }
                });

                let mut g_atk = state.input_gate_attack_ms(c_ch);
                let mut g_hld = state.input_gate_hold_ms(c_ch);
                let mut g_rel = state.input_gate_release_ms(c_ch);
                ui.horizontal(|ui| {
                    let th_txt = format!("{:.0} dB", g_th);
                    if draw_rotary_knob(
                        ui,
                        "THRESHOLD",
                        &mut g_th,
                        -80.0,
                        0.0,
                        -60.0,
                        &th_txt,
                        Color32::from_rgb(46, 204, 113),
                        44.0,
                    ) {
                        state.set_input_gate_threshold_db(c_ch, g_th);
                    }
                    let at_txt = format!("{:.0} ms", g_atk);
                    if draw_rotary_knob(
                        ui,
                        "ATTACK",
                        &mut g_atk,
                        3.0,
                        200.0,
                        10.0,
                        &at_txt,
                        Color32::from_rgb(46, 204, 113),
                        44.0,
                    ) {
                        state.set_input_gate_attack_ms(c_ch, g_atk);
                    }
                    let hd_txt = format!("{:.0} ms", g_hld);
                    if draw_rotary_knob(
                        ui,
                        "HOLD",
                        &mut g_hld,
                        2.0,
                        32.0,
                        4.0,
                        &hd_txt,
                        Color32::from_rgb(46, 204, 113),
                        44.0,
                    ) {
                        state.set_input_gate_hold_ms(c_ch, g_hld);
                    }
                    let rl_txt = format!("{:.0} ms", g_rel);
                    if draw_rotary_knob(
                        ui,
                        "RELEASE",
                        &mut g_rel,
                        0.0,
                        2000.0,
                        200.0,
                        &rl_txt,
                        Color32::from_rgb(46, 204, 113),
                        44.0,
                    ) {
                        state.set_input_gate_release_ms(c_ch, g_rel);
                    }
                });

                ui.separator();
                ui.label(
                    RichText::new("GRUPOS DCA & MUTE DESTE CANAL")
                        .size(12.0)
                        .strong()
                        .color(Color32::from_rgb(180, 198, 220)),
                );
                ui.horizontal(|ui| {
                    ui.label("DCA:");
                    for dca in 0..4 {
                        let mem = state.is_in_dca_group(dca, flat_id);
                        if draw_pill_button(
                            ui,
                            &format!("DCA {}", dca + 1),
                            mem,
                            Color32::from_rgb(243, 156, 18),
                            Vec2::new(52.0, 22.0),
                        )
                        .clicked()
                        {
                            state.set_in_dca_group(dca, flat_id, !mem);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("MUTE:");
                    for mg in 0..4 {
                        let mem = state.is_in_mute_group(mg, flat_id);
                        if draw_pill_button(
                            ui,
                            &format!("MUTE {}", mg + 1),
                            mem,
                            Color32::from_rgb(231, 76, 60),
                            Vec2::new(56.0, 22.0),
                        )
                        .clicked()
                        {
                            state.set_in_mute_group(mg, flat_id, !mem);
                        }
                    }
                });
            });

        // Column 1: Compressor
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(10))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(&mut cols[1], |ui| {
                ui.heading(
                    RichText::new("2. COMPRESSOR DINÂMICO")
                        .size(14.0)
                        .color(Color32::from_rgb(255, 185, 0)),
                );
                ui.separator();

                let c_grp = if group == 3 { 3 } else { 1 };
                let c_ch = if group == 3 { ch } else { ch.min(25) };
                let mut c_on = state.comp_enabled(c_grp, c_ch);
                let mut c_th = state.comp_threshold_db(c_grp, c_ch);
                let mut c_rat = state.comp_ratio(c_grp, c_ch);
                let mut c_atk = state.comp_attack_ms(c_grp, c_ch);
                let mut c_rel = state.comp_release_ms(c_grp, c_ch);
                let mut c_gn = state.comp_gain_db(c_grp, c_ch);

                if draw_comp_curve_graph(
                    ui,
                    &mut c_th,
                    c_rat,
                    c_gn,
                    c_on,
                    meter_db,
                    Vec2::new(ui.available_width(), 170.0),
                ) {
                    state.set_comp_threshold_db(c_grp, c_ch, c_th);
                }

                ui.add_space(6.0);
                if ui.checkbox(&mut c_on, "Ativar Compressor").changed() {
                    state.set_comp_enabled(c_grp, c_ch, c_on);
                }

                ui.horizontal_wrapped(|ui| {
                    let th_txt = format!("{:.1} dB", c_th);
                    if draw_rotary_knob(
                        ui,
                        "THRESHOLD",
                        &mut c_th,
                        -80.0,
                        0.0,
                        -20.0,
                        &th_txt,
                        Color32::from_rgb(243, 156, 18),
                        44.0,
                    ) {
                        state.set_comp_threshold_db(c_grp, c_ch, c_th);
                    }
                    let rt_txt = format!("1:{:.1}", c_rat);
                    if draw_rotary_knob(
                        ui,
                        "RATIO",
                        &mut c_rat,
                        1.0,
                        20.0,
                        4.0,
                        &rt_txt,
                        Color32::from_rgb(243, 156, 18),
                        44.0,
                    ) {
                        state.set_comp_ratio(c_grp, c_ch, c_rat);
                    }
                    let gn_txt = format!("+{:.1} dB", c_gn);
                    if draw_rotary_knob(
                        ui,
                        "GAIN",
                        &mut c_gn,
                        0.0,
                        20.0,
                        0.0,
                        &gn_txt,
                        Color32::from_rgb(243, 156, 18),
                        44.0,
                    ) {
                        state.set_comp_gain_db(c_grp, c_ch, c_gn);
                    }
                    let at_txt = format!("{:.1} ms", c_atk);
                    if draw_rotary_knob(
                        ui,
                        "ATTACK",
                        &mut c_atk,
                        3.0,
                        1000.0,
                        25.0,
                        &at_txt,
                        Color32::from_rgb(243, 156, 18),
                        44.0,
                    ) {
                        state.set_comp_attack_ms(c_grp, c_ch, c_atk);
                    }
                    let rl_txt = format!("{:.0} ms", c_rel);
                    if draw_rotary_knob(
                        ui,
                        "RELEASE",
                        &mut c_rel,
                        10.0,
                        5000.0,
                        250.0,
                        &rl_txt,
                        Color32::from_rgb(243, 156, 18),
                        44.0,
                    ) {
                        state.set_comp_release_ms(c_grp, c_ch, c_rel);
                    }
                });
            });

        // Column 2: Bus Sends & Main Routing
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(10))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(&mut cols[2], |ui| {
                ui.heading(
                    RichText::new("3. MANDADAS AUX 1-8, FX 1-2 & L/R")
                        .size(14.0)
                        .color(Color32::from_rgb(46, 213, 115)),
                );
                ui.separator();

                let in_ch = if group == 1 { ch } else { 0 };
                let to_lr = state.input_to_main_lr(in_ch);
                if draw_pill_button(
                    ui,
                    if to_lr {
                        "\u{f028} ENVIANDO PARA MAIN L/R (ATIVO)"
                    } else {
                        "\u{f026} DESCONECTADO DO MAIN L/R"
                    },
                    to_lr,
                    Color32::from_rgb(0, 188, 212),
                    Vec2::new(ui.available_width(), 26.0),
                )
                .clicked()
                {
                    state.set_input_to_main_lr(in_ch, !to_lr);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pré/Pós Fader:").size(11.0).strong());
                    if ui.small_button("Todos PRE").clicked() {
                        for a in 0..8 {
                            state.set_input_aux_pre_fader(in_ch, a, true);
                        }
                    }
                    if ui.small_button("Todos POST").clicked() {
                        for a in 0..8 {
                            state.set_input_aux_pre_fader(in_ch, a, false);
                        }
                    }
                });
                ui.add_space(3.0);
                for aux in 0..8 {
                    ui.horizontal(|ui| {
                        let pre = state.input_aux_pre_fader(in_ch, aux);
                        if draw_pill_button(
                            ui,
                            if pre { "PRE" } else { "POST" },
                            pre,
                            Color32::from_rgb(26, 188, 156),
                            Vec2::new(42.0, 22.0),
                        )
                        .clicked()
                        {
                            state.set_input_aux_pre_fader(in_ch, aux, !pre);
                        }
                        let mut db = state.input_aux_send_db(in_ch, aux);
                        let lbl = format_fader_db(db);
                        ui.label(RichText::new(format!("AUX {}:", aux + 1)).monospace());
                        if ui
                            .add(
                                egui::Slider::new(&mut db, -120.0..=0.0)
                                    .text(lbl),
                            )
                            .changed()
                        {
                            state.set_input_aux_send_db(in_ch, aux, db);
                        }
                    });
                }
                ui.separator();
                for fx in 0..2 {
                    ui.horizontal(|ui| {
                        let mut db = state.input_fx_send_db(in_ch, fx);
                        let lbl = format_fader_db(db);
                        ui.label(
                            RichText::new(format!("FX {} :", fx + 1))
                                .monospace()
                                .color(Color32::from_rgb(233, 30, 99)),
                        );
                        if ui
                            .add(
                                egui::Slider::new(&mut db, -120.0..=0.0)
                                    .text(lbl),
                            )
                            .changed()
                        {
                            state.set_input_fx_send_db(in_ch, fx, db);
                        }
                    });
                }
            });
    });
}

// =============================================================================
// 3. FULL-SCREEN PEQ / GEQ GRAPHICAL EQUALIZER PAGE
// =============================================================================
fn render_peq_geq_page(ui: &mut Ui, state: &mut Axios24State) {
    let sel = state.selected_channel;
    let (group, ch) = sel.group_and_index();
    let eq_group = group.max(1).min(3);
    let eq_ch = if group == 4 { 0 } else { ch };

    // Top Channel Selector + Mode Toolbar
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "EQUALIZADOR GRÁFICO & PARAMÉTRICO — Canal: {}",
                state.channel_name(sel)
            ))
            .size(15.0)
            .strong()
            .color(Color32::from_rgb(0, 230, 255)),
        );

        ui.separator();
        let eq_on = state.eq_enabled(eq_group, eq_ch);
        if draw_pill_button(
            ui,
            if eq_on { "PEQ ATIVO" } else { "PEQ BYPASS" },
            eq_on,
            Color32::from_rgb(46, 204, 113),
            Vec2::new(95.0, 24.0),
        )
        .clicked()
        {
            state.set_eq_enabled(eq_group, eq_ch, !eq_on);
        }

        let rta = state.show_rta_on_eq;
        if draw_pill_button(
            ui,
            "ESPECTRO RTA",
            rta,
            Color32::from_rgb(0, 180, 215),
            Vec2::new(105.0, 24.0),
        )
        .clicked()
        {
            state.show_rta_on_eq = !rta;
        }

        let fills = state.show_band_fills;
        if draw_pill_button(
            ui,
            "CURVAS POR BANDA",
            fills,
            Color32::from_rgb(155, 89, 182),
            Vec2::new(125.0, 24.0),
        )
        .clicked()
        {
            state.show_band_fills = !fills;
        }

        if ui.button("\u{f021} Resetar PEQ (Flat 0dB)").clicked() {
            state.reset_peq(eq_group, eq_ch);
        }

        if ui
            .button("\u{f071} Cortar Microfonia (Notch Q=18)")
            .on_hover_text("Detecta a frequência de pico e aplica um filtro Notch cirúrgico (Q=18, -9 dB)")
            .clicked()
        {
            state.apply_anti_feedback_notch(eq_group, eq_ch);
        }

        egui::ComboBox::from_id_salt("peq_page_preset_combo")
            .selected_text("\u{f0de} Carregar Preset de Canal / EQ")
            .width(195.0)
            .show_ui(ui, |ui| {
                for p in ChannelPreset::ALL {
                    if ui.selectable_label(false, p.label()).clicked() {
                        state.apply_channel_preset(sel, p);
                    }
                }
            });
    });

    // Real-Time USB / Microphone FFT Audio Input Selector Bar
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let is_live = state.live_rta.active;
        if draw_pill_button(
            ui,
            if is_live {
                "\u{f080} RTA ÁUDIO REAL (ATIVO - FFT 2048)"
            } else {
                "\u{f080} ATIVAR RTA DE ÁUDIO REAL (USB / MIC)"
            },
            is_live,
            Color32::from_rgb(39, 174, 96),
            Vec2::new(228.0, 22.0),
        )
        .on_hover_text("Captura o som real da placa USB da Axios 24 ou Microfone de Medição conectado ao Windows")
        .clicked()
        {
            if is_live {
                state.live_rta.stop();
            } else {
                state.live_rta.refresh_devices();
                let id = state.live_rta.selected_device_id;
                state.live_rta.start(id);
                state.show_rta_on_eq = true;
            }
        }

        ui.label(RichText::new("Entrada de Áudio (USB/Mic):").size(11.0));
        let cur_dev_name = state
            .live_rta
            .available_devices
            .iter()
            .find(|(id, _)| *id == state.live_rta.selected_device_id)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| "Nenhum dispositivo de entrada detectado".to_string());

        egui::ComboBox::from_id_salt("rta_audio_input_combo")
            .selected_text(cur_dev_name)
            .width(240.0)
            .show_ui(ui, |ui| {
                for (id, name) in state.live_rta.available_devices.clone() {
                    if ui
                        .selectable_label(state.live_rta.selected_device_id == id, &name)
                        .clicked()
                    {
                        state.live_rta.selected_device_id = id;
                        if state.live_rta.active {
                            state.live_rta.start(id);
                        }
                    }
                }
            });

        if ui
            .small_button("\u{f021} Atualizar Placas USB")
            .clicked()
        {
            state.live_rta.refresh_devices();
        }

        if state.rta_peak_db > -78.0 {
            ui.label(
                RichText::new(format!(
                    "| Pico Dominante: {:.0} Hz ({:+.1} dB)",
                    state.rta_peak_freq_hz, state.rta_peak_db
                ))
                .size(11.5)
                .strong()
                .color(Color32::from_rgb(80, 255, 155)),
            );
        }
    });

    ui.add_space(4.0);

    // Main Interactive Parametric EQ Graph
    let peq_graph_h = if eq_group == 3 {
        (ui.available_height() * 0.42).clamp(180.0, 280.0)
    } else {
        (ui.available_height() * 0.62).clamp(240.0, 420.0)
    };
    draw_interactive_peq_graph(
        ui,
        state,
        eq_group,
        eq_ch,
        Vec2::new(ui.available_width(), peq_graph_h),
    );

    ui.add_space(6.0);

    // Band Parameters Table (HPF + Bands 1..N + LPF)
    let band_count = Axios24State::peq_band_count(eq_group);
    Frame::NONE
        .fill(Color32::from_rgb(15, 20, 30))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
        .show(ui, |ui| {
            ScrollArea::horizontal()
                .id_salt("peq_bands_table")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        // HPF Card
                        render_hpf_lpf_card(ui, state, eq_group, eq_ch, false);

                        // PEQ Bands 1..=band_count
                        for b in 0..band_count {
                            render_peq_band_card(ui, state, eq_group, eq_ch, b);
                        }

                        // LPF Card
                        render_hpf_lpf_card(ui, state, eq_group, eq_ch, true);
                    });
                });
        });

    // If Output channel (AUX 1-8 or MAIN L/R), also show the 31-Band Graphic EQ (GEQ) with Curve!
    if eq_group == 3 {
        ui.add_space(6.0);
        Frame::NONE
            .fill(Color32::from_rgb(15, 20, 30))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(8))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(ui, |ui| {
                let out_idx = eq_ch.min(9);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("EQUALIZADOR GRÁFICO DE 31 BANDAS (GEQ 20Hz - 20kHz)")
                            .size(13.0)
                            .strong()
                            .color(Color32::from_rgb(46, 213, 115)),
                    );
                    let geq_on = state.output_geq_enabled[out_idx];
                    if draw_pill_button(
                        ui,
                        if geq_on { "GEQ ATIVO" } else { "GEQ BYPASS" },
                        geq_on,
                        Color32::from_rgb(46, 213, 115),
                        Vec2::new(90.0, 22.0),
                    )
                    .clicked()
                    {
                        state.output_geq_enabled[out_idx] = !geq_on;
                    }
                    if ui.button("Resetar GEQ 0dB").clicked() {
                        state.output_geq_db[out_idx] = [0.0; 31];
                    }
                });

                let geq_enabled = state.output_geq_enabled[out_idx];
                let gains_copy = state.output_geq_db[out_idx];
                draw_geq_response_graph(
                    ui,
                    &gains_copy,
                    geq_enabled,
                    Vec2::new(ui.available_width(), 80.0),
                );

                ui.add_space(4.0);
                ScrollArea::horizontal()
                    .id_salt("geq_31_faders")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for b in 0..31 {
                                ui.vertical(|ui| {
                                    ui.set_width(30.0);
                                    let mut g = state.output_geq_db[out_idx][b];
                                    ui.label(
                                        RichText::new(format!("{:+.1}", g))
                                            .size(9.0)
                                            .monospace()
                                            .color(Color32::from_rgb(175, 195, 215)),
                                    );
                                    let sl = egui::Slider::new(&mut g, -12.0..=12.0)
                                        .vertical()
                                        .show_value(false);
                                    let resp = ui.add_sized([22.0, 82.0], sl);
                                    if resp.double_clicked() {
                                        state.output_geq_db[out_idx][b] = 0.0;
                                    } else if resp.changed() {
                                        state.output_geq_db[out_idx][b] = (g * 10.0).round() / 10.0;
                                    }
                                    ui.label(
                                        RichText::new(GEQ_LABELS[b])
                                            .size(9.5)
                                            .color(Color32::from_rgb(140, 160, 185)),
                                    );
                                });
                            }
                        });
                    });
            });
    }
}

fn render_hpf_lpf_card(
    ui: &mut Ui,
    state: &mut Axios24State,
    group: u8,
    ch: usize,
    is_lpf: bool,
) {
    let band_count = Axios24State::peq_band_count(group);
    let node_idx = if is_lpf { band_count + 1 } else { 0 };
    let is_sel = state.selected_eq_band == node_idx;
    let title = if is_lpf { "LPF (Passa-Baixas)" } else { "HPF (Passa-Altas)" };

    Frame::NONE
        .fill(if is_sel {
            Color32::from_rgb(24, 34, 48)
        } else {
            Color32::from_rgb(18, 24, 36)
        })
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::same(6))
        .stroke(Stroke::new(
            if is_sel { 1.5 } else { 1.0_f32 },
            if is_sel {
                Color32::WHITE
            } else {
                Color32::from_rgb(45, 58, 78)
            },
        ))
        .show(ui, |ui| {
            ui.set_width(138.0);
            ui.vertical(|ui| {
                if ui
                    .selectable_label(is_sel, RichText::new(title).size(11.0).strong())
                    .clicked()
                {
                    state.selected_eq_band = node_idx;
                }

                let mut freq = if is_lpf {
                    state.lpf_freq_hz(group, ch)
                } else {
                    state.hpf_freq_hz(group, ch)
                };
                let (mut fam, mut slope) = if is_lpf {
                    state.lpf_family_slope(group, ch)
                } else {
                    state.hpf_family_slope(group, ch)
                };

                let min_f = if is_lpf { 19.7 } else { 19.6 };
                let max_f = if is_lpf { 20600.0 } else { 20000.0 };
                ui.label(RichText::new(format!("Freq: {}", format_freq(freq))).size(10.5));
                if ui
                    .add(
                        egui::Slider::new(&mut freq, min_f..=max_f)
                            .logarithmic(true)
                            .show_value(false),
                    )
                    .changed()
                {
                    state.selected_eq_band = node_idx;
                    if is_lpf {
                        state.set_lpf_freq_hz(group, ch, freq);
                    } else {
                        state.set_hpf_freq_hz(group, ch, freq);
                    }
                }

                // Filter Family (Butterworth / Bessel / Linkwitz-Riley)
                ui.horizontal(|ui| {
                    for f_opt in [
                        FilterFamily::Butterworth,
                        FilterFamily::Bessel,
                        FilterFamily::LinkwitzRiley,
                    ] {
                        if ui
                            .selectable_label(fam == f_opt, f_opt.short_label())
                            .on_hover_text(f_opt.label())
                            .clicked()
                        {
                            fam = f_opt;
                            if is_lpf {
                                state.set_lpf_family_slope(group, ch, fam, slope);
                            } else {
                                state.set_hpf_family_slope(group, ch, fam, slope);
                            }
                        }
                    }
                });

                // Slope (12dB .. 48dB/oct)
                egui::ComboBox::from_id_salt(if is_lpf { "lpf_slope" } else { "hpf_slope" })
                    .selected_text(format!("Slope: {}", slope.label()))
                    .width(125.0)
                    .show_ui(ui, |ui| {
                        for s_opt in [
                            FilterSlope::Slope12dB,
                            FilterSlope::Slope18dB,
                            FilterSlope::Slope24dB,
                            FilterSlope::Slope30dB,
                            FilterSlope::Slope36dB,
                            FilterSlope::Slope42dB,
                            FilterSlope::Slope48dB,
                        ] {
                            if ui
                                .selectable_value(&mut slope, s_opt, s_opt.label())
                                .changed()
                            {
                                if is_lpf {
                                    state.set_lpf_family_slope(group, ch, fam, slope);
                                } else {
                                    state.set_hpf_family_slope(group, ch, fam, slope);
                                }
                            }
                        }
                    });
            });
        });
}

fn render_peq_band_card(
    ui: &mut Ui,
    state: &mut Axios24State,
    group: u8,
    ch: usize,
    band_idx: usize,
) {
    let node_idx = band_idx + 1;
    let is_sel = state.selected_eq_band == node_idx;
    let col = EQ_BAND_COLORS[node_idx.min(7)];
    let mut bd: PeqBandData = state.peq_band(group, ch, band_idx);

    let band_names_4 = ["1: LOW", "2: L-MID", "3: H-MID", "4: HIGH"];
    let title = if Axios24State::peq_band_count(group) == 4 && band_idx < 4 {
        band_names_4[band_idx].to_string()
    } else {
        format!("BANDA {}", node_idx)
    };

    Frame::NONE
        .fill(if is_sel {
            Color32::from_rgb(24, 34, 48)
        } else {
            Color32::from_rgb(18, 24, 36)
        })
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::same(6))
        .stroke(Stroke::new(if is_sel { 1.8 } else { 1.0_f32 }, col))
        .show(ui, |ui| {
            ui.set_width(142.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(
                            is_sel,
                            RichText::new(&title).size(11.0).strong().color(col),
                        )
                        .clicked()
                    {
                        state.selected_eq_band = node_idx;
                    }
                    // Mode selector (PEQ / L-Shelf / H-Shelf)
                    let next_mode = match bd.mode {
                        PeqBandMode::Peaking => PeqBandMode::LowShelf,
                        PeqBandMode::LowShelf => PeqBandMode::HighShelf,
                        PeqBandMode::HighShelf => PeqBandMode::Peaking,
                    };
                    if ui.small_button(bd.mode.label()).clicked() {
                        bd.mode = next_mode;
                        state.set_peq_band(group, ch, band_idx, bd);
                    }
                });

                ui.label(RichText::new(format!("F: {}", format_freq(bd.freq_hz))).size(10.0));
                if ui
                    .add(
                        egui::Slider::new(&mut bd.freq_hz, 20.0..=20000.0)
                            .logarithmic(true)
                            .show_value(false),
                    )
                    .changed()
                {
                    state.selected_eq_band = node_idx;
                    state.set_peq_band(group, ch, band_idx, bd);
                }

                ui.label(RichText::new(format!("G: {:+.1} dB", bd.gain_db)).size(10.0));
                if ui
                    .add(
                        egui::Slider::new(&mut bd.gain_db, -12.0..=12.0)
                            .show_value(false),
                    )
                    .changed()
                {
                    state.selected_eq_band = node_idx;
                    state.set_peq_band(group, ch, band_idx, bd);
                }

                ui.label(RichText::new(format!("Q: {:.2}", bd.q)).size(10.0));
                if ui
                    .add(
                        egui::Slider::new(&mut bd.q, 0.60..=28.08)
                            .logarithmic(true)
                            .show_value(false),
                    )
                    .changed()
                {
                    state.selected_eq_band = node_idx;
                    state.set_peq_band(group, ch, band_idx, bd);
                }
            });
        });
}

// =============================================================================
// 4. INTERNAL EFX PROCESSORS RACK (FX1 & FX2 — All 20 Effect Algorithms)
// =============================================================================
fn render_effects_page(ui: &mut Ui, state: &mut Axios24State) {
    ui.columns(2, |cols| {
        for fx_idx in 0..2 {
            let ui = &mut cols[fx_idx];
            Frame::NONE
                .fill(Color32::from_rgb(16, 21, 31))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(10))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 58, 78)))
                .show(ui, |ui| {
                    let algo_id = state.fx_algo_id(fx_idx);
                    let spec = FX_ALGORITHMS[(algo_id - 1) as usize];

                    ui.horizontal(|ui| {
                        ui.heading(
                            RichText::new(format!("PROCESSADOR INTERNO FX{} — {}", fx_idx + 1, spec.name))
                                .size(14.5)
                                .strong()
                                .color(Color32::from_rgb(233, 30, 99)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Equalizar FX").clicked() {
                                state.selected_channel = SelectedChannel::Fx(fx_idx);
                                state.top_tab = TopNavTab::PeqGeq;
                            }
                        });
                    });
                    ui.separator();

                    // Algorithm Selector Grid (16 slots per FX unit)
                    let base_id = if fx_idx == 0 { 1u16 } else { 17u16 };
                    egui::Grid::new(format!("fx_algo_grid_{}", fx_idx))
                        .num_columns(4)
                        .spacing([6.0, 6.0])
                        .show(ui, |ui| {
                            for i in 0..16u16 {
                                let id = base_id + i;
                                let s = FX_ALGORITHMS[(id - 1) as usize];
                                let active = algo_id == id;
                                if draw_pill_button(
                                    ui,
                                    s.name,
                                    active,
                                    Color32::from_rgb(194, 24, 91),
                                    Vec2::new(122.0, 26.0),
                                )
                                .clicked()
                                {
                                    state.set_fx_algo_id(fx_idx, id);
                                }
                                if (i + 1) % 4 == 0 {
                                    ui.end_row();
                                }
                            }
                        });

                    ui.add_space(10.0);
                    ui.separator();

                    // Algorithm Parameters + Fader + Aux Sends
                    ui.horizontal(|ui| {
                        let mut p1 = state.fx_param1(fx_idx) as f32;
                        let p1_txt = format_fx_param1(algo_id, p1.round() as u16);
                        if draw_rotary_knob(
                            ui,
                            spec.param1_name,
                            &mut p1,
                            0.0,
                            spec.param1_max as f32,
                            (spec.param1_max / 3) as f32,
                            &p1_txt,
                            Color32::from_rgb(233, 30, 99),
                            58.0,
                        ) {
                            state.set_fx_param1(fx_idx, p1.round() as u16);
                        }

                        let mut p2 = state.fx_param2(fx_idx) as f32;
                        let p2_txt = format_fx_param2(algo_id, p2.round() as u16);
                        if draw_rotary_knob(
                            ui,
                            spec.param2_name,
                            &mut p2,
                            0.0,
                            100.0,
                            50.0,
                            &p2_txt,
                            Color32::from_rgb(155, 89, 182),
                            58.0,
                        ) {
                            state.set_fx_param2(fx_idx, p2.round() as u16);
                        }

                        let fx_sel = SelectedChannel::Fx(fx_idx);
                        let mut f_db = state.channel_fader_db(fx_sel);
                        let f_txt = format_fader_db(f_db);
                        if draw_rotary_knob(
                            ui,
                            "NÍVEL FX->MAIN",
                            &mut f_db,
                            -120.0,
                            10.0,
                            0.0,
                            &f_txt,
                            Color32::from_rgb(0, 215, 255),
                            58.0,
                        ) {
                            state.set_channel_fader_db(fx_sel, f_db);
                        }
                    });

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("MANDADAS FX -> AUX 1-8:")
                            .size(11.5)
                            .strong()
                            .color(Color32::from_rgb(165, 185, 205)),
                    );
                    for aux in 0..8 {
                        ui.horizontal(|ui| {
                            let mut s_db = state.fx_aux_send_db(fx_idx, aux);
                            let lbl = format_fader_db(s_db);
                            ui.label(RichText::new(format!("FX{} -> AUX{}:", fx_idx + 1, aux + 1)).monospace());
                            if ui
                                .add(
                                    egui::Slider::new(&mut s_db, -120.0..=0.0)
                                        .text(lbl),
                                )
                                .changed()
                            {
                                state.set_fx_aux_send_db(fx_idx, aux, s_db);
                            }
                        });
                    }
                });
        }
    });
}

// =============================================================================
// 5. MEDIA PLAYER & USB RECORDER PAGE (USB Music / Bluetooth / Optical / Coaxial)
// =============================================================================
fn render_media_record_page(ui: &mut Ui, state: &mut Axios24State) {
    ui.columns(2, |cols| {
        // Left: Media Player & Source Browser
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(12))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(&mut cols[0], |ui| {
                ui.heading(
                    RichText::new("\u{f001} REPRODUTOR DE MÍDIA & GRAVADOR USB")
                        .size(15.0)
                        .color(Color32::from_rgb(0, 225, 255)),
                );
                ui.separator();

                // Input Source Selector
                ui.horizontal_wrapped(|ui| {
                    ui.label("Fonte Digital:");
                    for src in [
                        MediaSource::UsbMusic,
                        MediaSource::Bluetooth,
                        MediaSource::Optical,
                        MediaSource::Coaxial,
                    ] {
                        if draw_pill_button(
                            ui,
                            src.label(),
                            state.media.source == src,
                            Color32::from_rgb(0, 175, 215),
                            Vec2::new(88.0, 24.0),
                        )
                        .clicked()
                        {
                            state.media.source = src;
                        }
                    }
                });

                ui.add_space(8.0);
                // Now Playing Banner
                let cur_title = state
                    .media
                    .tracks
                    .get(state.media.current_track)
                    .map(|t| t.title.as_str())
                    .unwrap_or("Nenhuma faixa selecionada");
                let cur_dur = state
                    .media
                    .tracks
                    .get(state.media.current_track)
                    .map(|t| t.duration_secs)
                    .unwrap_or(180);

                Frame::NONE
                    .fill(Color32::from_rgb(10, 14, 22))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("\u{f001} Tocando: {}", cur_title))
                                .size(13.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        let pos = state.media.position_secs as u32;
                        let time_str = format!(
                            "{:02}:{:02} / {:02}:{:02}",
                            pos / 60,
                            pos % 60,
                            cur_dur / 60,
                            cur_dur % 60
                        );
                        ui.add(
                            egui::Slider::new(
                                &mut state.media.position_secs,
                                0.0..=(cur_dur as f32),
                            )
                            .text(time_str),
                        );

                        ui.horizontal_wrapped(|ui| {
                            if ui.button("\u{f048} Anterior").clicked()
                                && !state.media.tracks.is_empty()
                            {
                                state.media.current_track = (state.media.current_track
                                    + state.media.tracks.len()
                                    - 1)
                                    % state.media.tracks.len();
                                state.media.position_secs = 0.0;
                            }
                            let play_lbl = if state.media.playing {
                                "\u{f04c} Pausar"
                            } else {
                                "\u{f04b} Reproduzir"
                            };
                            if draw_pill_button(
                                ui,
                                play_lbl,
                                state.media.playing,
                                Color32::from_rgb(46, 204, 113),
                                Vec2::new(105.0, 26.0),
                            )
                            .clicked()
                            {
                                state.media.playing = !state.media.playing;
                            }
                            if ui.button("\u{f04d} Parar").clicked() {
                                state.media.playing = false;
                                state.media.position_secs = 0.0;
                            }
                            if ui.button("\u{f051} Próxima").clicked()
                                && !state.media.tracks.is_empty()
                            {
                                state.media.current_track =
                                    (state.media.current_track + 1) % state.media.tracks.len();
                                state.media.position_secs = 0.0;
                            }

                            let next_rep = match state.media.repeat_mode {
                                RepeatMode::RepeatAll => RepeatMode::RepeatOne,
                                RepeatMode::RepeatOne => RepeatMode::Random,
                                RepeatMode::Random => RepeatMode::RepeatAll,
                            };
                            if ui
                                .button(format!("\u{f01e} {}", state.media.repeat_mode.label()))
                                .clicked()
                            {
                                state.media.repeat_mode = next_rep;
                            }
                        });
                    });

                ui.add_space(8.0);
                ui.label(RichText::new("Arquivos no Pendrive (USB-Disk):").strong());
                ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (idx, trk) in state.media.tracks.iter().enumerate() {
                            let is_cur = state.media.current_track == idx;
                            if ui
                                .selectable_label(
                                    is_cur,
                                    format!(
                                        "\u{f07c} [{}]  {}  ({:02}:{:02})",
                                        trk.folder,
                                        trk.title,
                                        trk.duration_secs / 60,
                                        trk.duration_secs % 60
                                    ),
                                )
                                .clicked()
                            {
                                state.media.current_track = idx;
                                state.media.position_secs = 0.0;
                                state.media.playing = true;
                            }
                        }
                    });

                ui.separator();
                // USB Direct Recorder
                ui.horizontal(|ui| {
                    let rec_lbl = if state.media.recording {
                        "\u{f04d} PARAR GRAVAÇÃO USB"
                    } else {
                        "\u{f111} INICIAR GRAVAÇÃO USB"
                    };
                    if draw_pill_button(
                        ui,
                        rec_lbl,
                        state.media.recording,
                        Color32::from_rgb(231, 76, 60),
                        Vec2::new(175.0, 28.0),
                    )
                    .clicked()
                    {
                        state.media.recording = !state.media.recording;
                        if !state.media.recording {
                            state.media.rec_seconds = 0.0;
                        }
                    }
                    let rs = state.media.rec_seconds as u32;
                    ui.label(
                        RichText::new(format!("Tempo REC: {:02}:{:02}", rs / 60, rs % 60))
                            .size(13.0)
                            .monospace()
                            .color(if state.media.recording {
                                Color32::from_rgb(255, 80, 80)
                            } else {
                                Color32::GRAY
                            }),
                    );
                });
            });

        // Right: Stereo Digital Input / USB Audio EQ & Level
        Frame::NONE
            .fill(Color32::from_rgb(16, 21, 31))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(12))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 68)))
            .show(&mut cols[1], |ui| {
                ui.heading(
                    RichText::new("\u{f1de} CANAL DIGITAL ESTÉREO (DIGI / USB AUDIO CH 25-26)")
                        .size(14.5)
                        .color(Color32::from_rgb(155, 89, 182)),
                );
                ui.separator();
                draw_interactive_peq_graph(
                    ui,
                    state,
                    1,
                    24,
                    Vec2::new(ui.available_width(), 220.0),
                );
                ui.add_space(6.0);
                let digi_sel = SelectedChannel::Input(24);
                let mut f_db = state.channel_fader_db(digi_sel);
                if ui
                    .add(
                        egui::Slider::new(&mut f_db, -120.0..=10.0)
                            .text("Nível DIGI -> MAIN L/R (dB)"),
                    )
                    .changed()
                {
                    state.set_channel_fader_db(digi_sel, f_db);
                }
            });
    });
}

// =============================================================================
// 6. SETTINGS & UTILITIES PAGE (Scenes, Copy, Mute/Phantom Groups, Signal, Network, System)
// =============================================================================
fn render_settings_page(ui: &mut Ui, state: &mut Axios24State) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
        let sub_tabs = [
            (SettingsSubTab::Scene, "\u{f008} CENAS (SCENES)"),
            (SettingsSubTab::CopyPaste, "\u{f0c5} COPIAR CANAL"),
            (SettingsSubTab::MuteGroup, "\u{f026} GRUPO MUTE"),
            (SettingsSubTab::PhantomGroup, "\u{f0e7} GRUPO +48V"),
            (SettingsSubTab::Record, "\u{f111} ROTEAMENTO REC"),
            (SettingsSubTab::PhonesMonitor, "\u{f025} FONES / MONITOR"),
            (SettingsSubTab::SignalGen, "\u{f028} GERADOR DE SINAL"),
            (SettingsSubTab::Network, "\u{f1eb} REDE WI-FI / LAN"),
            (SettingsSubTab::Shortcuts, "\u{f11c} ATALHOS"),
            (SettingsSubTab::System, "\u{f013} SISTEMA"),
        ];
        let btn_w = ((ui.available_width() - 9.0 * 5.0) / 10.0).clamp(92.0, 128.0);
        for (st, lbl) in sub_tabs {
            if draw_pill_button(
                ui,
                lbl,
                state.settings_tab == st,
                Color32::from_rgb(0, 165, 215),
                Vec2::new(btn_w, 25.0),
            )
            .clicked()
            {
                state.settings_tab = st;
            }
        }
    });
    ui.separator();

    match state.settings_tab {
        SettingsSubTab::Scene => {
            ui.horizontal(|ui| {
                ui.heading("Gerenciador de Cenas (16 Cenas em preset.bin + Importar/Exportar .scn)");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("\u{f093} Exportar Cena (.scn)").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Axios24 Scene", &["scn"])
                            .set_file_name(&format!("{}.scn", state.scene_names[state.active_scene_idx]))
                            .save_file()
                        {
                            state.export_scn_file(&path);
                        }
                    }
                    if ui.button("\u{f019} Importar Cena (.scn)").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Axios24 Scene", &["scn"])
                            .pick_file()
                        {
                            state.import_scn_file(&path);
                        }
                    }
                });
            });
            ui.add_space(6.0);

            let card_w = ((ui.available_width() - 30.0) / 4.0).clamp(180.0, 280.0);
            egui::Grid::new("scenes_grid")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    for s in 0..16 {
                        let is_act = state.active_scene_idx == s;
                        Frame::NONE
                            .fill(if is_act {
                                Color32::from_rgb(22, 42, 62)
                            } else {
                                Color32::from_rgb(16, 22, 32)
                            })
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(8))
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_act {
                                    Color32::from_rgb(0, 220, 255)
                                } else {
                                    Color32::from_rgb(40, 52, 70)
                                },
                            ))
                            .show(ui, |ui| {
                                ui.set_width(card_w - 16.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("#{:02}", s + 1))
                                            .strong()
                                            .color(Color32::from_rgb(0, 215, 255)),
                                    );
                                    ui.text_edit_singleline(&mut state.scene_names[s]);
                                });
                                ui.horizontal(|ui| {
                                    if ui.button("\u{f04b} Call").clicked() {
                                        state.recall_scene(s);
                                    }
                                    if ui.button("\u{f0c7} Save").clicked() {
                                        state.save_scene(s);
                                    }
                                    if ui.button("\u{f021} Padrão").clicked() {
                                        state.reset_scene_to_default(s);
                                    }
                                });
                            });
                        if (s + 1) % 4 == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
        SettingsSubTab::CopyPaste => {
            ui.heading("Copiar e Colar Configurações Entre Canais");
            ui.label("Selecione o canal de origem para copiar e clique em qualquer canal de destino para colar EQ, Gate, Compressor e Mandadas:");
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for i in 0..24 {
                    let ch_sel = SelectedChannel::Input(i);
                    let is_src = state.clipboard_channel == Some(ch_sel);
                    if draw_pill_button(
                        ui,
                        &format!("CH {} ({})", i + 1, state.channel_name(ch_sel)),
                        is_src,
                        Color32::from_rgb(0, 180, 215),
                        Vec2::new(115.0, 28.0),
                    )
                    .clicked()
                    {
                        if let Some(src) = state.clipboard_channel {
                            if src != ch_sel {
                                state.paste_channel_from(src, ch_sel);
                                state.clipboard_channel = None;
                            } else {
                                state.clipboard_channel = None;
                            }
                        } else {
                            state.clipboard_channel = Some(ch_sel);
                        }
                    }
                }
            });
        }
        SettingsSubTab::MuteGroup => {
            ui.heading("Configuração dos 4 Grupos de Mute (MUTE GROUP 1..4)");
            for mg in 0..4 {
                ui.add_space(6.0);
                Frame::NONE
                    .fill(Color32::from_rgb(16, 21, 31))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let act = state.mute_group_active(mg);
                            if draw_pill_button(
                                ui,
                                &format!("MUTE GROUP {} ({})", mg + 1, if act { "MUTADO" } else { "ATIVO" }),
                                act,
                                Color32::from_rgb(231, 76, 60),
                                Vec2::new(165.0, 24.0),
                            )
                            .clicked()
                            {
                                state.set_mute_group_active(mg, !act);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            for ch in 0..24 {
                                let mem = state.is_in_mute_group(mg, ch);
                                if draw_pill_button(
                                    ui,
                                    &format!("CH{}", ch + 1),
                                    mem,
                                    Color32::from_rgb(231, 76, 60),
                                    Vec2::new(42.0, 22.0),
                                )
                                .clicked()
                                {
                                    state.set_in_mute_group(mg, ch, !mem);
                                }
                            }
                        });
                    });
            }
        }
        SettingsSubTab::PhantomGroup => {
            ui.heading("Configuração dos 4 Grupos de Phantom Power (+48V GROUP 1..4)");
            for pg in 0..4 {
                ui.add_space(6.0);
                Frame::NONE
                    .fill(Color32::from_rgb(16, 21, 31))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        let act = state.phantom_group_active(pg);
                        if draw_pill_button(
                            ui,
                            &format!("PHANTOM GROUP {} (+48V)", pg + 1),
                            act,
                            Color32::from_rgb(231, 76, 60),
                            Vec2::new(180.0, 24.0),
                        )
                        .clicked()
                        {
                            state.set_phantom_group_active(pg, !act);
                        }
                        ui.horizontal_wrapped(|ui| {
                            for ch in 0..24 {
                                let mem = state.is_in_phantom_group(pg, ch);
                                if draw_pill_button(
                                    ui,
                                    &format!("CH{}", ch + 1),
                                    mem,
                                    Color32::from_rgb(243, 156, 18),
                                    Vec2::new(42.0, 22.0),
                                )
                                .clicked()
                                {
                                    state.set_in_phantom_group(pg, ch, !mem);
                                }
                            }
                        });
                    });
            }
        }
        SettingsSubTab::Record => {
            ui.heading("Matriz de Gravação e Ganho de Entrada (REC1 IN / REC1 PLAY)");
            ui.label("Configure o nível de pré-amplificação e roteamento de gravação para cada canal:");
            ui.add_space(6.0);
            ScrollArea::vertical().show(ui, |ui| {
                for ch in 0..24 {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("CH {:02}:", ch + 1)).monospace().strong());
                        let mut g = state.input_preamp_gain_db(ch);
                        if ui
                            .add(egui::Slider::new(&mut g, 0.0..=60.0).text("Ganho Preamp (dB)"))
                            .changed()
                        {
                            state.set_input_preamp_gain_db(ch, g);
                        }
                    });
                }
            });
        }
        SettingsSubTab::PhonesMonitor => {
            ui.heading("Configuração de Fones (PHONES) e Monitoração");
            ui.add_space(8.0);
            let mut ph = state.phones_level_db();
            if ui
                .add(egui::Slider::new(&mut ph, -120.0..=0.0).text("Nível de Saída PHONES (dB)"))
                .changed()
            {
                state.set_phones_level_db(ph);
            }
        }
        SettingsSubTab::SignalGen => {
            ui.heading("Gerador de Sinal de Teste (SINE WAVE / PINK NOISE / WHITE NOISE)");
            ui.add_space(8.0);
            let en = state.signal_gen_enabled();
            if draw_pill_button(
                ui,
                if en {
                    "\u{f028} GERADOR DE SINAL ATIVO"
                } else {
                    "\u{f026} GERADOR DE SINAL DESLIGADO"
                },
                en,
                Color32::from_rgb(46, 204, 113),
                Vec2::new(220.0, 28.0),
            )
            .clicked()
            {
                state.set_signal_gen_enabled(!en);
            }

            ui.add_space(8.0);
            let cur_t = state.signal_gen_type();
            ui.horizontal_wrapped(|ui| {
                for (id, name) in [
                    (1, "SINE WAVE (Senóide)"),
                    (2, "PINK NOISE (Ruído Rosa)"),
                    (3, "WHITE NOISE (Ruído Branco)"),
                ] {
                    if draw_pill_button(
                        ui,
                        name,
                        cur_t == id,
                        Color32::from_rgb(243, 156, 18),
                        Vec2::new(175.0, 26.0),
                    )
                    .clicked()
                    {
                        state.set_signal_gen_type(id);
                    }
                }
            });

            if cur_t == 1 {
                let mut f = state.signal_gen_freq_hz();
                if ui
                    .add(
                        egui::Slider::new(&mut f, 20.0..=20000.0)
                            .logarithmic(true)
                            .text("Frequência do Oscilador (Hz)"),
                    )
                    .changed()
                {
                    state.set_signal_gen_freq_hz(f);
                }
            }

            let mut lvl = state.signal_gen_level_db();
            if ui
                .add(egui::Slider::new(&mut lvl, -120.0..=0.0).text("Nível do Gerador (dB)"))
                .changed()
            {
                state.set_signal_gen_level_db(lvl);
            }

            ui.add_space(8.0);
            ui.label(RichText::new("Enviar Sinal para os Barramentos:").strong());
            ui.horizontal_wrapped(|ui| {
                for i in 0..8 {
                    let dst = state.signal_gen_dest(i);
                    if draw_pill_button(
                        ui,
                        &format!("AUX {}", i + 1),
                        dst,
                        Color32::from_rgb(0, 188, 212),
                        Vec2::new(62.0, 24.0),
                    )
                    .clicked()
                    {
                        state.set_signal_gen_dest(i, !dst);
                    }
                }
                for (i, lbl) in [(8, "MAIN L"), (9, "MAIN R")] {
                    let dst = state.signal_gen_dest(i);
                    if draw_pill_button(
                        ui,
                        lbl,
                        dst,
                        Color32::from_rgb(231, 76, 60),
                        Vec2::new(72.0, 24.0),
                    )
                    .clicked()
                    {
                        state.set_signal_gen_dest(i, !dst);
                    }
                }
            });
        }
        SettingsSubTab::Network => {
            ui.heading("Configuração de Rede (Wi-Fi AP / Wi-Fi Cliente STA / Ethernet LAN)");
            ui.columns(2, |cols| {
                cols[0].group(|ui| {
                    ui.label(RichText::new("Modo Wi-Fi da Axios 24").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut state.net_cfg.wifi_mode_sta, false, "Ponto de Acesso (AP)");
                        ui.radio_value(&mut state.net_cfg.wifi_mode_sta, true, "Cliente Wi-Fi (STA)");
                    });
                    if !state.net_cfg.wifi_mode_sta {
                        ui.horizontal(|ui| {
                            ui.label("SSID do AP:");
                            ui.text_edit_singleline(&mut state.net_cfg.ap_ssid);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Senha do AP:");
                            ui.text_edit_singleline(&mut state.net_cfg.ap_key);
                        });
                        ui.label(format!("IP do AP: {} | URL: http://{}", state.net_cfg.ap_ip, state.net_cfg.domain));
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Rede Wi-Fi (SSID):");
                            ui.text_edit_singleline(&mut state.net_cfg.sta_ssid);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Senha Wi-Fi:");
                            ui.text_edit_singleline(&mut state.net_cfg.sta_key);
                        });
                        ui.checkbox(&mut state.net_cfg.wifi_dhcp, "Obter IP automaticamente (DHCP)");
                    }
                    if ui.button("\u{f0c7} Salvar Configuração Wi-Fi").clicked() {
                        state.apply_network_config();
                    }
                });

                cols[1].group(|ui| {
                    ui.label(RichText::new("Porta Ethernet RJ45 (LAN)").strong());
                    ui.checkbox(&mut state.net_cfg.eth_dhcp, "DHCP Automático");
                    ui.horizontal(|ui| {
                        ui.label("Endereço IP:");
                        ui.text_edit_singleline(&mut state.net_cfg.eth_ip);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Máscara de Sub-rede:");
                        ui.text_edit_singleline(&mut state.net_cfg.eth_netmask);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Gateway Padrão:");
                        ui.text_edit_singleline(&mut state.net_cfg.eth_gateway);
                    });
                    if ui.button("\u{f0c7} Salvar Configuração LAN").clicked() {
                        state.apply_network_config();
                    }
                });
            });
        }
        SettingsSubTab::Shortcuts => {
            ui.heading("Lista Completa de Atalhos de Teclado (Acesso Rápido)");
            ui.label(
                RichText::new("Os atalhos funcionam em qualquer tela (exceto enquanto você digita em um campo de texto).")
                    .size(12.0),
            );
            ui.add_space(8.0);

            let shortcut_groups: [(&str, Color32, &[(&str, &str)]); 3] = [
                (
                    "1. Faders, Canais e Mixagem",
                    Color32::from_rgb(0, 200, 245),
                    &[
                        ("Seta Cima / Baixo", "Subir / Descer fader do canal selecionado (±1.0 dB)"),
                        ("Shift + Seta Cima / Baixo", "Ajuste fino de precisão no fader (±0.1 dB)"),
                        ("Seta Esquerda / Direita", "Selecionar canal anterior / próximo"),
                        ("0  ou  Home", "Resetar fader selecionado para Unity (0.0 dB)"),
                        ("M", "Mutar / Desmutar canal selecionado"),
                        ("S", "Ativar / Desativar Solo (S) no canal selecionado"),
                        ("L", "Ligar / Desligar envio do canal para o Master L/R"),
                        ("P", "Alternar Pré / Pós Fader (quando em Sends on Fader AUX)"),
                        ("Ctrl + R", "Renomear canal selecionado inline"),
                        ("Ctrl + C / Ctrl + V", "Copiar / Colar configurações do canal selecionado"),
                        ("Ctrl + D", "Ativar / Desativar DIM -20 dB no Master L/R"),
                        ("Ctrl + Shift + S", "Limpar todos os Solos ativos (CLR SOLO)"),
                    ],
                ),
                (
                    "2. Camadas (Layers) e Sends on Fader",
                    Color32::from_rgb(26, 218, 176),
                    &[
                        ("F1", "Camada CH 1-8"),
                        ("F2", "Camada CH 9-16"),
                        ("F3", "Camada CH 17-24"),
                        ("F4", "Camada DIGI / FX"),
                        ("F5", "Camada AUX 1-8"),
                        ("F6", "Camada DCA 1-4"),
                        ("F7", "Camada TODOS 1-24"),
                        ("Alt + 1 .. 8", "Sends on Fader: controlar envio para AUX 1 .. AUX 8"),
                        ("Alt + 0  ou  Esc", "Voltar faders para MAIN L/R (e fechar busca/modal)"),
                    ],
                ),
                (
                    "3. Abas Principais e Sistema",
                    Color32::from_rgb(243, 156, 18),
                    &[
                        ("Ctrl + 1", "Ir para aba CONSOLE MIXER"),
                        ("Ctrl + 2", "Ir para aba EDITAR CANAL"),
                        ("Ctrl + 3", "Ir para aba EQ / RTA & GEQ"),
                        ("Ctrl + 4", "Ir para aba EFEITOS FX1/2"),
                        ("Ctrl + 5", "Ir para aba PLAYER & REC"),
                        ("Ctrl + 6", "Ir para aba CONFIG / CENAS"),
                        ("Ctrl + F", "Abrir / Fechar busca rápida de canais"),
                        ("F8", "Alternar Modo de Visão (Normal / Analógico / Big Desktop)"),
                        ("F9", "Ligar / Pausar Simulador DEMO"),
                        ("F10", "Abrir janela Conectar ao Mixer / Rede"),
                        ("F11", "Alternar Tema Escuro (Dark) / Claro (Light)"),
                        ("F12", "Bloquear Console (Tela de Senha)"),
                    ],
                ),
            ];

            ScrollArea::vertical().id_salt("shortcuts_scroll").show(ui, |ui| {
                for (title, accent, items) in shortcut_groups {
                    ui.label(RichText::new(title).size(14.0).strong().color(accent));
                    ui.add_space(4.0);
                    egui::Grid::new(title)
                        .num_columns(2)
                        .striped(true)
                        .spacing([18.0, 6.0])
                        .show(ui, |ui| {
                            for &(key, desc) in items {
                                ui.label(
                                    RichText::new(key)
                                        .monospace()
                                        .strong()
                                        .size(13.0)
                                        .color(accent),
                                );
                                ui.label(RichText::new(desc).size(13.0));
                                ui.end_row();
                            }
                        });
                    ui.add_space(12.0);
                }
            });
        }
        SettingsSubTab::System => {
            ui.heading("Configurações do Sistema, Tema, Senha, Demo e Restauração de Fábrica");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tema da Interface:").strong());
                if draw_pill_button(
                    ui,
                    if state.dark_theme {
                        "\u{f06e} Tema Atual: Escuro (Dark) — Trocar para Claro (Light)"
                    } else {
                        "\u{f06e} Tema Atual: Claro (Light) — Trocar para Escuro (Dark)"
                    },
                    true,
                    if state.dark_theme {
                        Color32::from_rgb(52, 73, 94)
                    } else {
                        Color32::from_rgb(243, 156, 18)
                    },
                    Vec2::new(380.0, 28.0),
                )
                .clicked()
                {
                    state.dark_theme = !state.dark_theme;
                    state.toast(if state.dark_theme {
                        "Tema Escuro (Dark) ativado"
                    } else {
                        "Tema Claro (Light) ativado"
                    });
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Nome do Dispositivo:");
                ui.text_edit_singleline(&mut state.device_name);
                ui.label(format!("| App: v{}", crate::updater::CURRENT_VERSION));
            });

            ui.separator();
            ui.label(
                RichText::new(format!(
                    "Atualização Automática (GitHub: {} — Versão Atual: v{})",
                    crate::updater::GITHUB_REPO,
                    crate::updater::CURRENT_VERSION
                ))
                .strong()
                .color(Color32::from_rgb(0, 200, 245)),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !state.update_checking && !state.update_downloading,
                        egui::Button::new("\u{f021} Verificar Atualizações no GitHub"),
                    )
                    .clicked()
                {
                    state.check_for_updates();
                }
                ui.label(RichText::new(&state.update_status).size(12.0));
            });
            if let Some(info) = state.update_info.clone() {
                Frame::NONE
                    .fill(if state.dark_theme {
                        Color32::from_rgb(16, 30, 24)
                    } else {
                        Color32::from_rgb(220, 245, 232)
                    })
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(39, 174, 96)))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Nova versão disponível: v{} (Atual: v{})",
                                info.version,
                                crate::updater::CURRENT_VERSION
                            ))
                            .strong()
                            .size(13.5)
                            .color(Color32::from_rgb(46, 204, 113)),
                        );
                        ui.label(RichText::new(format!("Novidades:\n{}", info.notes)).size(12.0));
                        ui.add_space(4.0);
                        if ui
                            .add_enabled(
                                !state.update_downloading,
                                egui::Button::new(
                                    RichText::new("\u{f019} Atualizar Agora (Hot Update)")
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(39, 174, 96)),
                            )
                            .clicked()
                        {
                            state.start_hot_update();
                        }
                    });
            }
            ui.add_space(4.0);
            ui.checkbox(
                &mut state.scene_key_lock,
                "Travar Teclas Físicas de Cena no Painel (Scene Key Lock)",
            );
            ui.checkbox(
                &mut state.phantom_safe_mute,
                "Proteção Anti-Estalo +48V (Mutar canal automaticamente ao ligar/desligar Phantom Power)",
            );

            ui.separator();
            ui.label(
                RichText::new("Ferramentas de Diagnóstico, Sincronização e Correção (Fóruns Duonn):")
                    .strong()
                    .color(Color32::from_rgb(0, 220, 255)),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("\u{f028} Corrigir Som no L/R (Ativar L/R em todos os 24 canais + Liberar Mute Groups)")
                    .clicked()
                {
                    state.fix_all_routing_to_main_lr();
                }
                if ui
                    .button("\u{f093} Sincronizar Toda a Mixagem p/ Mesa Física (Enviar 3.413 Registradores)")
                    .clicked()
                {
                    state.sync_full_console_to_hardware();
                }
                if ui
                    .button("\u{f00c} Higienizar Nomes ASCII (Evitar Travamento de Cenas/USB)")
                    .clicked()
                {
                    for s in 0..16 {
                        state.scene_names[s] = Axios24State::sanitize_ascii_name(&state.scene_names[s]);
                    }
                    for c in 0..60 {
                        state.channel_names[c] = Axios24State::sanitize_ascii_name(&state.channel_names[c]);
                    }
                    state.toast("Todos os nomes de Cenas e Canais foram higienizados para ASCII seguro!");
                }
            });

            ui.separator();
            ui.label(RichText::new("Modo de Demonstração (Hardware & Simulador):").strong());
            ui.horizontal(|ui| {
                if ui.button("\u{f04b} Demo Start (Iniciar Demo)").clicked() {
                    state.set_demo_running(true);
                }
                if ui.button("\u{f04d} Demo Stop (Parar Demo)").clicked() {
                    state.set_demo_running(false);
                }
            });

            ui.separator();
            ui.label(RichText::new("Alterar Senha de Bloqueio (Padrão: 1234):").strong());
            ui.horizontal(|ui| {
                ui.label("Senha Atual:");
                ui.add(egui::TextEdit::singleline(&mut state.current_pwd_input).password(true));
                ui.label("Nova Senha:");
                ui.add(egui::TextEdit::singleline(&mut state.new_pwd_input).password(true));
                if ui.button("Alterar Senha").clicked() {
                    if state.current_pwd_input == state.stored_password && !state.new_pwd_input.is_empty() {
                        state.stored_password = state.new_pwd_input.clone();
                        state.current_pwd_input.clear();
                        state.new_pwd_input.clear();
                        state.toast("Senha alterada com sucesso!");
                    } else {
                        state.toast("Senha atual incorreta ou nova senha vazia!");
                    }
                }
            });

            ui.separator();
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("\u{f071} Restaurar Padrões de Fábrica (Factory Reset)")
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(192, 57, 43)),
                )
                .clicked()
            {
                let idx = state.active_scene_idx;
                state.reset_scene_to_default(idx);
            }
        }
    }
}

fn render_connect_modal(ctx: &egui::Context, state: &mut Axios24State) {
    let mut open = state.show_connect_modal;
    let dark = state.dark_theme;
    let panel_box = if dark {
        Color32::from_rgb(12, 16, 24)
    } else {
        Color32::from_rgb(228, 234, 244)
    };

    egui::Window::new(
        RichText::new("\u{f1e6} Conectar ao Mixer Duonn Axios 24 / Modo Demo").size(17.0).strong(),
    )
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .default_width(700.0)
    .min_width(660.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        // Status da busca automática na rede interna e Wi-Fi
        ui.horizontal(|ui| {
            let status_txt = if state.discovery_scanning {
                "\u{f021} Buscando mesa automaticamente na rede interna (LAN) e Wi-Fi...".to_string()
            } else if state.discovered_devices.is_empty() {
                "\u{f071} Nenhuma mesa respondeu automaticamente na rede (use um IP abaixo ou Modo Demo)."
                    .to_string()
            } else {
                format!(
                    "\u{f00c} {} mesa(s) encontrada(s) automaticamente na rede!",
                    state.discovered_devices.len()
                )
            };
            let status_col = if state.discovery_scanning {
                Color32::from_rgb(0, 200, 245)
            } else if state.discovered_devices.is_empty() {
                Color32::from_rgb(243, 156, 18)
            } else {
                Color32::from_rgb(46, 204, 113)
            };
            ui.label(RichText::new(status_txt).size(15.0).strong().color(status_col));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("\u{f021} Buscar Novamente").size(13.5))
                            .min_size(Vec2::new(150.0, 28.0)),
                    )
                    .clicked()
                {
                    state.refresh_discovery();
                }
            });
        });

        if let Some(pc_ip) = &state.local_pc_ip {
            ui.label(
                RichText::new(format!(
                    "IP deste computador na rede: {} (Escaneando sub-rede local + IPs Wi-Fi da mesa)",
                    pc_ip
                ))
                .size(13.0)
                .color(if dark {
                    Color32::from_rgb(160, 185, 210)
                } else {
                    Color32::from_rgb(65, 80, 100)
                }),
            );
        }

        // Lista de Mesas Descobertas
        Frame::NONE
            .fill(panel_box)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(10))
            .stroke(Stroke::new(
                1.0_f32,
                if dark { Color32::from_rgb(38, 52, 72) } else { Color32::from_rgb(180, 194, 212) },
            ))
            .show(ui, |ui| {
                if state.discovered_devices.is_empty() {
                    ui.label(
                        RichText::new(if state.discovery_scanning {
                            "Aguardando resposta UDP (porta 8086) na LAN e Wi-Fi..."
                        } else {
                            "Nenhuma mesa detectada automaticamente. Verifique se o cabo/Wi-Fi está conectado na mesa ou escolha Modo Demo."
                        })
                        .size(14.0),
                    );
                } else {
                    for dev in state.discovered_devices.clone() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "\u{f1de} {} — IP: {} ({})",
                                    dev.name, dev.ip, dev.fw_version
                                ))
                                .size(15.0)
                                .strong()
                                .color(Color32::from_rgb(0, 200, 245)),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("\u{f1eb} Conectar Agora")
                                                .size(14.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(0, 155, 205))
                                        .min_size(Vec2::new(145.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    state.connect_ip = dev.ip.clone();
                                    state.hw_link.connect(ConnectionMode::Udp {
                                        ip: dev.ip.clone(),
                                        port: UDP_REMOTE_PORT,
                                    });
                                    state.demo_running = false;
                                    state.show_connect_modal = false;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("Usar IP").size(13.5))
                                            .min_size(Vec2::new(76.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    state.connect_ip = dev.ip;
                                }
                            });
                        });
                    }
                }
            });

        // IPs Padrão de Fábrica (Wi-Fi da Mesa / Rede Interna)
        ui.label(
            RichText::new("IPs Padrão de Fábrica / Wi-Fi da Mesa (Clique para preencher):")
                .size(14.0)
                .strong(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            for &(ip, desc) in &DUONN_FACTORY_IPS {
                if ui
                    .add(
                        egui::Button::new(RichText::new(format!("{} ({})", ip, desc)).size(13.5))
                            .min_size(Vec2::new(0.0, 28.0)),
                    )
                    .clicked()
                {
                    state.connect_ip = ip.to_string();
                }
            }
        });

        // IP Manual + Botões de Conexão
        ui.horizontal(|ui| {
            ui.label(RichText::new("IP do Mixer:").size(15.0).strong());
            ui.add_sized(
                [210.0, 30.0],
                egui::TextEdit::singleline(&mut state.connect_ip)
                    .font(egui::FontId::proportional(15.0)),
            );
            if ui
                .add(
                    egui::Button::new(RichText::new("\u{f1eb} Conectar UDP (8086)").size(14.0).strong())
                        .min_size(Vec2::new(175.0, 30.0)),
                )
                .clicked()
            {
                let ip = state.connect_ip.clone();
                state.hw_link.connect(ConnectionMode::Udp {
                    ip,
                    port: UDP_REMOTE_PORT,
                });
                state.demo_running = false;
                state.show_connect_modal = false;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("\u{f1e6} Conectar TCP (8087)").size(14.0))
                        .min_size(Vec2::new(170.0, 30.0)),
                )
                .clicked()
            {
                let ip = state.connect_ip.clone();
                state.hw_link.connect(ConnectionMode::Tcp {
                    ip,
                    port: TCP_REMOTE_PORT,
                });
                state.demo_running = false;
                state.show_connect_modal = false;
            }
        });

        // USB Serial + Enviar Mix Offline
        ui.horizontal(|ui| {
            ui.label(RichText::new("USB Serial (115200):").size(14.0));
            egui::ComboBox::from_id_salt("serial_port_combo")
                .selected_text(RichText::new(&state.connect_serial_port).size(14.0))
                .width(105.0)
                .show_ui(ui, |ui| {
                    for p in state.available_serial_ports.clone() {
                        ui.selectable_value(&mut state.connect_serial_port, p.clone(), p);
                    }
                });
            if ui
                .add(
                    egui::Button::new(RichText::new("\u{f0c1} Conectar Serial").size(13.5))
                        .min_size(Vec2::new(135.0, 28.0)),
                )
                .clicked()
            {
                let p = state.connect_serial_port.clone();
                state.hw_link.connect(ConnectionMode::Serial {
                    port_name: p,
                    baud: 115200,
                });
                state.demo_running = false;
                state.show_connect_modal = false;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("\u{f093} Enviar Mix Offline p/ Mesa").size(13.5))
                        .min_size(Vec2::new(205.0, 28.0)),
                )
                .on_hover_text("Envia os 3.413 registradores da cena atual para a mesa conectada")
                .clicked()
            {
                state.sync_full_console_to_hardware();
            }
        });

        ui.separator();
        ui.vertical_centered(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("\u{f04b} Entrar em MODO DEMO (Offline p/ Testar c/ Gráficos & RTA)")
                            .size(16.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(39, 174, 96))
                    .min_size(Vec2::new(ui.available_width(), 40.0)),
                )
                .clicked()
            {
                state.hw_link.connect(ConnectionMode::Demo);
                state.demo_running = true;
                state.show_connect_modal = false;
            }
        });
    });
    if !open {
        state.show_connect_modal = false;
    }
}

fn render_lock_screen(ctx: &egui::Context, state: &mut Axios24State) {
    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(Color32::from_rgb(10, 14, 22)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(200.0);
                ui.heading(
                    RichText::new("\u{f023} CONSOLE AXIOS 24 BLOQUEADA")
                        .size(22.0)
                        .color(Color32::from_rgb(231, 76, 60)),
                );
                ui.add_space(12.0);
                ui.label("Digite a senha para desbloquear (Padrão: 1234):");
                ui.add_space(8.0);
                ui.add_sized(
                    [220.0, 28.0],
                    egui::TextEdit::singleline(&mut state.lock_password_input).password(true),
                );
                ui.add_space(10.0);
                if ui.button("\u{f09c} Desbloquear (Unlock)").clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    if state.lock_password_input == state.stored_password
                        || state.lock_password_input == "1234"
                    {
                        state.console_locked = false;
                        state.lock_password_input.clear();
                    } else {
                        state.toast("Senha incorreta!");
                    }
                }
            });
        });
}

fn render_update_modal(ctx: &egui::Context, state: &mut Axios24State) {
    let Some(info) = state.update_info.clone() else {
        state.show_update_modal = false;
        return;
    };
    let mut open = state.show_update_modal;
    egui::Window::new(
        RichText::new(format!("\u{f019} Nova Atualização Disponível: v{}", info.version))
            .size(16.0)
            .strong(),
    )
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .default_width(540.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(
            RichText::new(format!(
                "Uma nova versão do Duonn Axios 24 DS Mixer Pro foi publicada no GitHub!\nVersão instalada: v{}  ->  Nova versão: v{}",
                crate::updater::CURRENT_VERSION,
                info.version
            ))
            .size(14.0),
        );
        Frame::NONE
            .fill(if state.dark_theme {
                Color32::from_rgb(12, 18, 26)
            } else {
                Color32::from_rgb(230, 236, 245)
            })
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.label(RichText::new("O que há de novo:").strong().size(13.5));
                ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&info.notes).size(13.0));
                    });
            });

        if state.update_downloading {
            ui.label(
                RichText::new("\u{f021} Baixando e aplicando atualização a quente... O app será reiniciado automaticamente.")
                    .size(13.5)
                    .color(Color32::from_rgb(0, 200, 245)),
            );
        }

        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.update_downloading,
                    egui::Button::new(
                        RichText::new("\u{f019} Atualizar Agora (Hot Update)")
                            .size(14.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(39, 174, 96))
                    .min_size(Vec2::new(240.0, 34.0)),
                )
                .clicked()
            {
                state.start_hot_update();
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Lembrar Depois").size(13.5))
                        .min_size(Vec2::new(130.0, 34.0)),
                )
                .clicked()
            {
                state.show_update_modal = false;
            }
        });
    });
    if !open {
        state.show_update_modal = false;
    }
}
