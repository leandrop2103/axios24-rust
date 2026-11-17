use crate::dsp::{
    comp_transfer_db, eval_stages_db, freq_to_norm_x, gate_transfer_db, hpf_lpf_stages,
    norm_x_to_freq, peq_band_biquad, GEQ_FREQS,
};
use crate::model::{Axios24State, SelectedChannel};
use egui::{
    epaint::PathShape,
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2,
};

/// Palette of 32 channel strip colors (inspired by DS Mixer)
pub const STRIP_PALETTE: [Color32; 32] = [
    // Row 1: Cyans & Blues
    Color32::from_rgb(0, 188, 212),   // 0: Cyan
    Color32::from_rgb(0, 229, 255),   // 1: Electric Cyan
    Color32::from_rgb(26, 188, 156),  // 2: Teal
    Color32::from_rgb(0, 150, 136),   // 3: Deep Teal
    Color32::from_rgb(52, 152, 219),  // 4: Dodger Blue
    Color32::from_rgb(33, 150, 243),  // 5: Sky Blue
    Color32::from_rgb(41, 128, 185),  // 6: Ocean Blue
    Color32::from_rgb(63, 81, 181),   // 7: Indigo
    // Row 2: Greens & Limes
    Color32::from_rgb(46, 204, 113),  // 8: Emerald
    Color32::from_rgb(39, 174, 96),   // 9: Nephritis Green
    Color32::from_rgb(0, 230, 118),   // 10: Neon Mint
    Color32::from_rgb(76, 175, 80),   // 11: Leaf Green
    Color32::from_rgb(139, 195, 74),  // 12: Light Green
    Color32::from_rgb(163, 203, 56),  // 13: Lime
    Color32::from_rgb(205, 220, 57),  // 14: Acid Lime
    Color32::from_rgb(106, 176, 76),  // 15: Olive Green
    // Row 3: Yellows, Oranges & Reds
    Color32::from_rgb(241, 196, 15),  // 16: Sunflower Yellow
    Color32::from_rgb(255, 215, 0),   // 17: Gold
    Color32::from_rgb(243, 156, 18),  // 18: Amber
    Color32::from_rgb(230, 126, 34),  // 19: Carrot Orange
    Color32::from_rgb(255, 111, 0),   // 20: Deep Orange
    Color32::from_rgb(231, 76, 60),   // 21: Coral Red
    Color32::from_rgb(192, 57, 43),   // 22: Crimson
    Color32::from_rgb(255, 82, 82),   // 23: Bright Red
    // Row 4: Pinks, Purples & Neutrals
    Color32::from_rgb(233, 30, 99),   // 24: Magenta
    Color32::from_rgb(255, 64, 129),  // 25: Hot Pink
    Color32::from_rgb(240, 98, 146),  // 26: Rose Pink
    Color32::from_rgb(155, 89, 182),  // 27: Amethyst Purple
    Color32::from_rgb(142, 68, 173),  // 28: Deep Purple
    Color32::from_rgb(124, 77, 255),  // 29: Violet
    Color32::from_rgb(15, 17, 22),    // 30: Black
    Color32::from_rgb(245, 248, 252), // 31: White
];

/// Colors for each EQ band node & curve (HPF, Band 1..7, LPF)
pub const EQ_BAND_COLORS: [Color32; 9] = [
    Color32::from_rgb(180, 195, 210), // 0: HPF (Silver/Cyan)
    Color32::from_rgb(255, 99, 71),   // 1: Band 1 LOW (Tomato Red)
    Color32::from_rgb(255, 185, 0),   // 2: Band 2 L-MID (Gold/Amber)
    Color32::from_rgb(46, 213, 115),  // 3: Band 3 H-MID (Emerald Green)
    Color32::from_rgb(30, 144, 255),  // 4: Band 4 HIGH (Dodger Blue)
    Color32::from_rgb(186, 85, 211),  // 5: Band 5 (Orchid Purple)
    Color32::from_rgb(0, 229, 255),   // 6: Band 6 (Electric Cyan)
    Color32::from_rgb(255, 105, 180), // 7: Band 7 (Hot Pink)
    Color32::from_rgb(180, 195, 210), // 8: LPF (Silver/Cyan)
];

pub fn format_freq(freq: f32) -> String {
    if freq < 19.65 {
        "OFF".to_string()
    } else if freq >= 20500.0 {
        "OFF".to_string()
    } else if freq >= 1000.0 {
        format!("{:.2} kHz", freq / 1000.0)
    } else if freq < 100.0 {
        format!("{:.1} Hz", freq)
    } else {
        format!("{:.0} Hz", freq)
    }
}

pub fn format_fader_db(db: f32) -> String {
    if db <= -115.0 {
        "-∞ dB".to_string()
    } else {
        format!("{:+.1} dB", db)
    }
}

/// Interactive Parametric EQ + HPF/LPF Frequency Response Graph (with RTA, per-band fills, total curve, draggable handles)
pub fn draw_interactive_peq_graph(
    ui: &mut Ui,
    state: &mut Axios24State,
    group: u8,
    ch: usize,
    desired_size: Vec2,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // Background panel
    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(12, 16, 23));
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, Color32::from_rgb(42, 54, 72)),
        StrokeKind::Inside,
    );

    let pad_left = 34.0;
    let pad_right = 14.0;
    let pad_top = 14.0;
    let pad_bottom = 22.0;
    let plot_rect = Rect::from_min_max(
        Pos2::new(rect.left() + pad_left, rect.top() + pad_top),
        Pos2::new(rect.right() - pad_right, rect.bottom() - pad_bottom),
    );

    if plot_rect.width() <= 20.0 || plot_rect.height() <= 20.0 {
        return response;
    }

    let db_min = -18.0_f32;
    let db_max = 18.0_f32;
    let db_to_y = |db: f32| -> f32 {
        let norm = (db.clamp(db_min, db_max) - db_min) / (db_max - db_min);
        plot_rect.bottom() - norm * plot_rect.height()
    };
    let y_to_db = |y: f32| -> f32 {
        let norm = ((plot_rect.bottom() - y) / plot_rect.height()).clamp(0.0, 1.0);
        db_min + norm * (db_max - db_min)
    };
    let freq_to_x = |f: f32| -> f32 {
        plot_rect.left() + freq_to_norm_x(f) * plot_rect.width()
    };
    let x_to_freq = |x: f32| -> f32 {
        let nx = ((x - plot_rect.left()) / plot_rect.width()).clamp(0.0, 1.0);
        norm_x_to_freq(nx)
    };

    // 1. Optional RTA Spectrum Analyzer Bars (Real 2048-pt FFT via WinMM USB/Mic OR Demo Simulator)
    if state.show_rta_on_eq {
        let is_real_fft = state.live_rta.active;
        let num_bins = state.rta_bins_db.len();
        let bar_w = (plot_rect.width() / num_bins as f32).max(2.0);
        for (i, &rta_db) in state.rta_bins_db.iter().enumerate() {
            let norm_h = ((rta_db + 80.0) / 80.0).clamp(0.0, 1.0);
            if norm_h > 0.02 {
                let x0 = plot_rect.left() + (i as f32) * (plot_rect.width() / num_bins as f32);
                let y_top = plot_rect.bottom() - norm_h * plot_rect.height() * 0.88;
                let bar_rect = Rect::from_min_max(
                    Pos2::new(x0 + 0.5, y_top),
                    Pos2::new((x0 + bar_w - 0.5).min(plot_rect.right()), plot_rect.bottom()),
                );
                let fill_col = if is_real_fft {
                    Color32::from_rgba_unmultiplied(46, 213, 115, 42)
                } else {
                    Color32::from_rgba_unmultiplied(0, 195, 255, 34)
                };
                let cap_col = if is_real_fft {
                    Color32::from_rgba_unmultiplied(80, 255, 150, 145)
                } else {
                    Color32::from_rgba_unmultiplied(0, 230, 255, 110)
                };
                painter.rect_filled(bar_rect, CornerRadius::same(1), fill_col);
                painter.line_segment(
                    [Pos2::new(bar_rect.left(), y_top), Pos2::new(bar_rect.right(), y_top)],
                    Stroke::new(1.2_f32, cap_col),
                );
            }
        }

        // Peak Frequency Readout Badge & Marker in Top-Left of EQ Graph
        if state.rta_peak_db > -78.0 {
            let pk_x = freq_to_x(state.rta_peak_freq_hz);
            painter.line_segment(
                [Pos2::new(pk_x, plot_rect.top()), Pos2::new(pk_x, plot_rect.bottom())],
                Stroke::new(
                    1.0_f32,
                    if is_real_fft {
                        Color32::from_rgba_unmultiplied(46, 213, 115, 95)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 200, 50, 65)
                    },
                ),
            );
            let mode_tag = if is_real_fft {
                "RTA REAL (USB/MIC)"
            } else {
                "RTA DEMO"
            };
            painter.text(
                Pos2::new(plot_rect.left() + 6.0, plot_rect.top() + 4.0),
                Align2::LEFT_TOP,
                format!(
                    "{} | PICO: {:.0} Hz ({:+.1} dB)",
                    mode_tag, state.rta_peak_freq_hz, state.rta_peak_db
                ),
                FontId::proportional(10.0),
                if is_real_fft {
                    Color32::from_rgb(80, 255, 155)
                } else {
                    Color32::from_rgb(160, 200, 230)
                },
            );
        }
    }

    // 2. Logarithmic Frequency Grid Lines (20Hz .. 20kHz)
    let grid_freqs: [(f32, &str); 10] = [
        (20.0, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (5000.0, "5k"),
        (10000.0, "10k"),
        (20000.0, "20k"),
    ];
    for (gf, label) in grid_freqs {
        let x = freq_to_x(gf);
        let is_major = gf == 100.0 || gf == 1000.0 || gf == 10000.0;
        let line_col = if is_major {
            Color32::from_rgb(45, 58, 78)
        } else {
            Color32::from_rgb(26, 34, 48)
        };
        painter.line_segment(
            [Pos2::new(x, plot_rect.top()), Pos2::new(x, plot_rect.bottom())],
            Stroke::new(1.0_f32, line_col),
        );
        painter.text(
            Pos2::new(x, plot_rect.bottom() + 4.0),
            Align2::CENTER_TOP,
            label,
            FontId::proportional(10.5),
            Color32::from_rgb(130, 148, 170),
        );
    }

    // 3. Horizontal dB Grid Lines (-18 .. +18 dB)
    for db in [-15, -12, -6, 0, 6, 12, 15] {
        let y = db_to_y(db as f32);
        let is_zero = db == 0;
        let col = if is_zero {
            Color32::from_rgb(80, 105, 135)
        } else {
            Color32::from_rgb(28, 37, 52)
        };
        painter.line_segment(
            [Pos2::new(plot_rect.left(), y), Pos2::new(plot_rect.right(), y)],
            Stroke::new(if is_zero { 1.5 } else { 1.0_f32 }, col),
        );
        if db % 6 == 0 {
            painter.text(
                Pos2::new(plot_rect.left() - 5.0, y),
                Align2::RIGHT_CENTER,
                format!("{:+}", db),
                FontId::proportional(10.0),
                if is_zero {
                    Color32::from_rgb(200, 220, 240)
                } else {
                    Color32::from_rgb(120, 136, 158)
                },
            );
        }
    }

    // 4. Evaluate Biquad Transfer Functions & Render Per-Band + Total Curves
    let eq_on = state.eq_enabled(group, ch);
    let band_count = Axios24State::peq_band_count(group);
    let hpf_freq = state.hpf_freq_hz(group, ch);
    let (hpf_fam, hpf_slope) = state.hpf_family_slope(group, ch);
    let hpf_stages = hpf_lpf_stages(hpf_freq, hpf_fam, hpf_slope, false);

    let lpf_freq = state.lpf_freq_hz(group, ch);
    let (lpf_fam, lpf_slope) = state.lpf_family_slope(group, ch);
    let lpf_stages = hpf_lpf_stages(lpf_freq, lpf_fam, lpf_slope, true);

    let mut band_biquads = Vec::with_capacity(band_count);
    let mut band_datas = Vec::with_capacity(band_count);
    for b in 0..band_count {
        let bd = state.peq_band(group, ch, b);
        band_datas.push(bd);
        band_biquads.push(peq_band_biquad(bd.mode, bd.freq_hz, bd.gain_db, bd.q));
    }

    let num_samples = 180usize;
    let zero_y = db_to_y(0.0);

    // Draw individual band curves & translucent fills when enabled
    if state.show_band_fills {
        for (b, bq) in band_biquads.iter().enumerate() {
            if band_datas[b].gain_db.abs() < 0.08 && state.selected_eq_band != b + 1 {
                continue;
            }
            let base_col = EQ_BAND_COLORS[(b + 1).min(7)];
            let is_sel = state.selected_eq_band == b + 1;
            let fill_alpha = if is_sel { 48 } else { 22 };
            let stroke_alpha = if is_sel { 200 } else { 110 };
            let fill_col = Color32::from_rgba_unmultiplied(
                base_col.r(),
                base_col.g(),
                base_col.b(),
                fill_alpha,
            );
            let stroke_col = Color32::from_rgba_unmultiplied(
                base_col.r(),
                base_col.g(),
                base_col.b(),
                stroke_alpha,
            );

            let mut pts = Vec::with_capacity(num_samples);
            for i in 0..num_samples {
                let nx = (i as f32) / ((num_samples - 1) as f32);
                let f = norm_x_to_freq(nx);
                let db = bq.eval_db(f as f64) as f32;
                let x = plot_rect.left() + nx * plot_rect.width();
                let y = db_to_y(db);
                pts.push(Pos2::new(x, y));
            }

            // Vertical quad strips for clean non-convex fill to 0 dB line
            for i in 0..(pts.len() - 1) {
                let p0 = pts[i];
                let p1 = pts[i + 1];
                if (p0.y - zero_y).abs() > 0.5 || (p1.y - zero_y).abs() > 0.5 {
                    painter.add(Shape::convex_polygon(
                        vec![
                            p0,
                            p1,
                            Pos2::new(p1.x, zero_y),
                            Pos2::new(p0.x, zero_y),
                        ],
                        fill_col,
                        Stroke::NONE,
                    ));
                }
            }
            painter.add(Shape::Path(PathShape::line(
                pts,
                Stroke::new(if is_sel { 1.8 } else { 1.1_f32 }, stroke_col),
            )));
        }
    }

    // Draw Total Combined EQ Response Curve (HPF + All PEQ Bands + LPF)
    let mut total_pts = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let nx = (i as f32) / ((num_samples - 1) as f32);
        let f = norm_x_to_freq(nx) as f64;
        let mut sum_db = 0.0_f32;
        if eq_on {
            sum_db += eval_stages_db(&hpf_stages, f);
            sum_db += eval_stages_db(&lpf_stages, f);
            for bq in &band_biquads {
                sum_db += bq.eval_db(f) as f32;
            }
        }
        let x = plot_rect.left() + nx * plot_rect.width();
        let y = db_to_y(sum_db);
        total_pts.push(Pos2::new(x, y));
    }

    let total_fill_col = if eq_on {
        Color32::from_rgba_unmultiplied(0, 215, 255, 32)
    } else {
        Color32::from_rgba_unmultiplied(120, 130, 145, 18)
    };
    for i in 0..(total_pts.len() - 1) {
        let p0 = total_pts[i];
        let p1 = total_pts[i + 1];
        if (p0.y - zero_y).abs() > 0.4 || (p1.y - zero_y).abs() > 0.4 {
            painter.add(Shape::convex_polygon(
                vec![
                    p0,
                    p1,
                    Pos2::new(p1.x, zero_y),
                    Pos2::new(p0.x, zero_y),
                ],
                total_fill_col,
                Stroke::NONE,
            ));
        }
    }
    let total_stroke_col = if eq_on {
        Color32::from_rgb(0, 235, 255)
    } else {
        Color32::from_rgb(110, 120, 135)
    };
    painter.add(Shape::Path(PathShape::line(
        total_pts,
        Stroke::new(2.6_f32, total_stroke_col),
    )));

    // 5. Interactive Draggable Control Nodes:
    //    Node 0 = HPF, Nodes 1..=band_count = PEQ 1..N, Node band_count+1 = LPF
    let mut node_positions: Vec<(usize, Pos2, String, Color32)> = Vec::with_capacity(band_count + 2);
    node_positions.push((
        0,
        Pos2::new(freq_to_x(hpf_freq.max(20.0)), zero_y),
        "H".to_string(),
        EQ_BAND_COLORS[0],
    ));
    for (b, bd) in band_datas.iter().enumerate() {
        node_positions.push((
            b + 1,
            Pos2::new(freq_to_x(bd.freq_hz), db_to_y(bd.gain_db)),
            format!("{}", b + 1),
            EQ_BAND_COLORS[(b + 1).min(7)],
        ));
    }
    node_positions.push((
        band_count + 1,
        Pos2::new(freq_to_x(lpf_freq.min(20000.0)), zero_y),
        "L".to_string(),
        EQ_BAND_COLORS[8],
    ));

    // Handle mouse click / drag / wheel on nodes
    if let Some(pointer_pos) = response.interact_pointer_pos() {
        if response.clicked() || response.drag_started() {
            // Find closest node within 18px
            let mut best_idx = None;
            let mut best_dist = 20.0_f32;
            for &(idx, pos, _, _) in &node_positions {
                let d = pos.distance(pointer_pos);
                if d < best_dist {
                    best_dist = d;
                    best_idx = Some(idx);
                }
            }
            if let Some(idx) = best_idx {
                state.selected_eq_band = idx;
            }
        }

        if response.dragged() {
            let sel = state.selected_eq_band;
            let new_freq = x_to_freq(pointer_pos.x);
            let new_db = y_to_db(pointer_pos.y).clamp(-12.0, 12.0);
            if sel == 0 {
                state.set_hpf_freq_hz(group, ch, new_freq.clamp(19.6, 20000.0));
                response.mark_changed();
            } else if sel == band_count + 1 {
                state.set_lpf_freq_hz(group, ch, new_freq.clamp(19.7, 20600.0));
                response.mark_changed();
            } else if sel >= 1 && sel <= band_count {
                let mut bd = state.peq_band(group, ch, sel - 1);
                bd.freq_hz = new_freq.clamp(20.0, 20000.0);
                bd.gain_db = (new_db * 10.0).round() / 10.0;
                state.set_peq_band(group, ch, sel - 1, bd);
                response.mark_changed();
            }
        }
    }

    // Double click resets selected PEQ band gain to 0 dB
    if response.double_clicked() {
        let sel = state.selected_eq_band;
        if sel >= 1 && sel <= band_count {
            let mut bd = state.peq_band(group, ch, sel - 1);
            bd.gain_db = 0.0;
            state.set_peq_band(group, ch, sel - 1, bd);
            response.mark_changed();
        }
    }

    // Scroll wheel over graph adjusts Q of selected PEQ band
    if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_y.abs() > 0.1 {
            let sel = state.selected_eq_band;
            if sel >= 1 && sel <= band_count {
                let mut bd = state.peq_band(group, ch, sel - 1);
                let factor = if scroll_y > 0.0 { 1.08 } else { 0.92 };
                bd.q = (bd.q * factor).clamp(0.60, 28.08);
                state.set_peq_band(group, ch, sel - 1, bd);
                response.mark_changed();
            }
        }
    }

    // Draw nodes and active tooltip
    for (idx, pos, label, col) in node_positions {
        let is_selected = state.selected_eq_band == idx;
        let radius = if is_selected { 11.0 } else { 8.5 };
        if is_selected {
            painter.circle_filled(
                pos,
                radius + 4.0,
                Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 65),
            );
        }
        painter.circle_filled(pos, radius, col);
        painter.circle_stroke(
            pos,
            radius,
            Stroke::new(
                if is_selected { 2.2 } else { 1.2_f32 },
                if is_selected {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(20, 25, 35)
                },
            ),
        );
        painter.text(
            pos,
            Align2::CENTER_CENTER,
            &label,
            FontId::proportional(if is_selected { 11.5 } else { 10.0 }),
            Color32::from_rgb(12, 16, 22),
        );

        // Info badge next to selected node (like fcn.00405100 in axios24new.exe & DS Mixer)
        if is_selected {
            let badge_text = if idx == 0 {
                format!("HPF: {} ({} {})", format_freq(hpf_freq), hpf_fam.short_label(), hpf_slope.label())
            } else if idx == band_count + 1 {
                format!("LPF: {} ({} {})", format_freq(lpf_freq), lpf_fam.short_label(), lpf_slope.label())
            } else {
                let bd = band_datas[idx - 1];
                format!(
                    "B{}: {} | {:+.1}dB | Q:{:.2}",
                    idx,
                    format_freq(bd.freq_hz),
                    bd.gain_db,
                    bd.q
                )
            };
            let badge_pos = Pos2::new(
                pos.x.clamp(plot_rect.left() + 75.0, plot_rect.right() - 75.0),
                (pos.y - 22.0).max(plot_rect.top() + 12.0),
            );
            let galley = painter.layout_no_wrap(
                badge_text,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
            let b_rect = Align2::CENTER_CENTER
                .anchor_size(badge_pos, galley.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(
                b_rect,
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(15, 22, 34, 225),
            );
            painter.rect_stroke(
                b_rect,
                CornerRadius::same(4),
                Stroke::new(1.0_f32, col),
                StrokeKind::Inside,
            );
            painter.galley(
                Pos2::new(b_rect.left() + 5.0, b_rect.top() + 2.0),
                galley,
                Color32::WHITE,
            );
        }
    }

    response
}

/// Renders a compact live EQ curve thumbnail inside a channel strip (like DS Mixer)
pub fn draw_mini_eq_thumbnail(
    ui: &mut Ui,
    state: &Axios24State,
    group: u8,
    ch: usize,
    size: Vec2,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(10, 14, 20));
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(
            1.0_f32,
            if response.hovered() {
                Color32::from_rgb(0, 210, 255)
            } else {
                Color32::from_rgb(38, 48, 65)
            },
        ),
        StrokeKind::Inside,
    );

    let mid_y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.left() + 2.0, mid_y), Pos2::new(rect.right() - 2.0, mid_y)],
        Stroke::new(0.8_f32, Color32::from_rgb(32, 42, 58)),
    );

    let eq_on = state.eq_enabled(group, ch);
    let band_count = Axios24State::peq_band_count(group);
    let hpf_freq = state.hpf_freq_hz(group, ch);
    let (hpf_fam, hpf_slope) = state.hpf_family_slope(group, ch);
    let hpf_stages = hpf_lpf_stages(hpf_freq, hpf_fam, hpf_slope, false);

    let lpf_freq = state.lpf_freq_hz(group, ch);
    let (lpf_fam, lpf_slope) = state.lpf_family_slope(group, ch);
    let lpf_stages = hpf_lpf_stages(lpf_freq, lpf_fam, lpf_slope, true);

    let mut bqs = Vec::with_capacity(band_count);
    for b in 0..band_count {
        let bd = state.peq_band(group, ch, b);
        bqs.push(peq_band_biquad(bd.mode, bd.freq_hz, bd.gain_db, bd.q));
    }

    let steps = 32usize;
    let mut pts = Vec::with_capacity(steps);
    let w = (rect.width() - 4.0).max(4.0);
    let h = (rect.height() - 4.0).max(4.0);
    for i in 0..steps {
        let nx = (i as f32) / ((steps - 1) as f32);
        let f = norm_x_to_freq(nx) as f64;
        let mut db = 0.0_f32;
        if eq_on {
            db += eval_stages_db(&hpf_stages, f);
            db += eval_stages_db(&lpf_stages, f);
            for bq in &bqs {
                db += bq.eval_db(f) as f32;
            }
        }
        let norm_y = (db.clamp(-15.0, 15.0) / 30.0) + 0.5;
        let x = rect.left() + 2.0 + nx * w;
        let y = rect.bottom() - 2.0 - norm_y * h;
        pts.push(Pos2::new(x, y));
    }

    let col = if eq_on {
        Color32::from_rgb(0, 225, 255)
    } else {
        Color32::from_rgb(90, 100, 115)
    };
    painter.add(Shape::Path(PathShape::line(pts, Stroke::new(1.5_f32, col))));
    response
}

/// Interactive 31-Band Graphic EQ Response Curve Graph
pub fn draw_geq_response_graph(
    ui: &mut Ui,
    gains_db: &[f32; 31],
    enabled: bool,
    desired_size: Vec2,
) {
    let (rect, _resp) = ui.allocate_exact_size(desired_size, Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(12, 16, 23));
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, Color32::from_rgb(42, 54, 72)),
        StrokeKind::Inside,
    );

    let pad_l = 32.0;
    let pad_r = 12.0;
    let pad_t = 10.0;
    let pad_b = 18.0;
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + pad_l, rect.top() + pad_t),
        Pos2::new(rect.right() - pad_r, rect.bottom() - pad_b),
    );
    let zero_y = plot.center().y;
    let db_to_y = |db: f32| -> f32 {
        let norm = (db.clamp(-15.0, 15.0) + 15.0) / 30.0;
        plot.bottom() - norm * plot.height()
    };

    for db in [-12, -6, 0, 6, 12] {
        let y = db_to_y(db as f32);
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(
                if db == 0 { 1.4 } else { 0.8_f32 },
                if db == 0 {
                    Color32::from_rgb(70, 95, 125)
                } else {
                    Color32::from_rgb(28, 36, 50)
                },
            ),
        );
        painter.text(
            Pos2::new(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            format!("{:+}", db),
            FontId::proportional(9.5),
            Color32::from_rgb(130, 145, 165),
        );
    }

    // Build active peaking biquads for non-zero 1/3-octave bands (Q = 4.318)
    let mut active_bqs = Vec::new();
    if enabled {
        for (i, &g) in gains_db.iter().enumerate() {
            if g.abs() > 0.05 {
                active_bqs.push(peq_band_biquad(
                    crate::dsp::PeqBandMode::Peaking,
                    GEQ_FREQS[i],
                    g,
                    4.318,
                ));
            }
        }
    }

    let steps = 160usize;
    let mut pts = Vec::with_capacity(steps);
    for i in 0..steps {
        let nx = (i as f32) / ((steps - 1) as f32);
        let f = norm_x_to_freq(nx) as f64;
        let mut db = 0.0_f32;
        for bq in &active_bqs {
            db += bq.eval_db(f) as f32;
        }
        let x = plot.left() + nx * plot.width();
        let y = db_to_y(db);
        pts.push(Pos2::new(x, y));
    }

    let fill_col = if enabled {
        Color32::from_rgba_unmultiplied(46, 213, 115, 34)
    } else {
        Color32::from_rgba_unmultiplied(110, 120, 135, 16)
    };
    for i in 0..(pts.len() - 1) {
        let p0 = pts[i];
        let p1 = pts[i + 1];
        if (p0.y - zero_y).abs() > 0.4 || (p1.y - zero_y).abs() > 0.4 {
            painter.add(Shape::convex_polygon(
                vec![p0, p1, Pos2::new(p1.x, zero_y), Pos2::new(p0.x, zero_y)],
                fill_col,
                Stroke::NONE,
            ));
        }
    }

    painter.add(Shape::Path(PathShape::line(
        pts,
        Stroke::new(
            2.2_f32,
            if enabled {
                Color32::from_rgb(46, 213, 115)
            } else {
                Color32::from_rgb(110, 120, 135)
            },
        ),
    )));

    // Draw 31 band dots along the curve
    for (i, &g) in gains_db.iter().enumerate() {
        let nx = freq_to_norm_x(GEQ_FREQS[i]);
        let x = plot.left() + nx * plot.width();
        let y = db_to_y(if enabled { g } else { 0.0 });
        painter.circle_filled(
            Pos2::new(x, y),
            3.2,
            if g.abs() > 0.1 && enabled {
                Color32::from_rgb(255, 210, 60)
            } else {
                Color32::from_rgb(110, 130, 150)
            },
        );
    }
}

/// Interactive Noise Gate Transfer Curve Graph (-80..0 dB)
pub fn draw_gate_curve_graph(
    ui: &mut Ui,
    threshold_db: &mut f32,
    enabled: bool,
    input_meter_db: f32,
    size: Vec2,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let mut changed = false;

    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(12, 16, 23));
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, Color32::from_rgb(42, 54, 72)),
        StrokeKind::Inside,
    );

    let pad = 22.0;
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + pad, rect.top() + 10.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - pad),
    );

    let db_to_x = |db: f32| plot.left() + ((db.clamp(-80.0, 0.0) + 80.0) / 80.0) * plot.width();
    let db_to_y = |db: f32| plot.bottom() - ((db.clamp(-80.0, 0.0) + 80.0) / 80.0) * plot.height();
    let x_to_db = |x: f32| (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * 80.0) - 80.0;

    // Grid lines (-60, -40, -20, 0 dB)
    for db in [-60, -40, -20, 0] {
        let x = db_to_x(db as f32);
        let y = db_to_y(db as f32);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(0.8_f32, Color32::from_rgb(26, 35, 48)),
        );
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.8_f32, Color32::from_rgb(26, 35, 48)),
        );
    }

    // Unity 1:1 reference line
    painter.line_segment(
        [Pos2::new(plot.left(), plot.bottom()), Pos2::new(plot.right(), plot.top())],
        Stroke::new(1.0_f32, Color32::from_rgb(45, 58, 75)),
    );

    // Dragging threshold knee
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            *threshold_db = x_to_db(pos.x).round().clamp(-80.0, 0.0);
            changed = true;
        }
    }

    let mut pts = Vec::with_capacity(81);
    for i in 0..=80 {
        let in_db = -80.0 + (i as f32);
        let out_db = gate_transfer_db(in_db, *threshold_db, enabled);
        pts.push(Pos2::new(db_to_x(in_db), db_to_y(out_db)));
    }

    let col = if enabled {
        Color32::from_rgb(46, 213, 115)
    } else {
        Color32::from_rgb(100, 110, 125)
    };
    painter.add(Shape::Path(PathShape::line(pts, Stroke::new(2.4_f32, col))));

    // Knee handle
    let knee_pos = Pos2::new(db_to_x(*threshold_db), db_to_y(*threshold_db));
    painter.circle_filled(knee_pos, 6.0, col);
    painter.circle_stroke(knee_pos, 6.0, Stroke::new(1.5_f32, Color32::WHITE));

    // Live signal dot on curve
    if input_meter_db > -80.0 {
        let out_db = gate_transfer_db(input_meter_db, *threshold_db, enabled);
        let sig_pos = Pos2::new(db_to_x(input_meter_db), db_to_y(out_db));
        painter.circle_filled(sig_pos, 4.5, Color32::from_rgb(255, 220, 60));
    }

    painter.text(
        Pos2::new(plot.left() + 4.0, plot.top() + 4.0),
        Align2::LEFT_TOP,
        format!("GATE TH: {:.0} dB", *threshold_db),
        FontId::proportional(10.5),
        col,
    );

    changed
}

/// Interactive Compressor Transfer Curve Graph (-80..0 dB)
pub fn draw_comp_curve_graph(
    ui: &mut Ui,
    threshold_db: &mut f32,
    ratio: f32,
    gain_db: f32,
    enabled: bool,
    input_meter_db: f32,
    size: Vec2,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let mut changed = false;

    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(12, 16, 23));
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, Color32::from_rgb(42, 54, 72)),
        StrokeKind::Inside,
    );

    let pad = 22.0;
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + pad, rect.top() + 10.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - pad),
    );

    let db_to_x = |db: f32| plot.left() + ((db.clamp(-80.0, 0.0) + 80.0) / 80.0) * plot.width();
    let db_to_y = |db: f32| plot.bottom() - ((db.clamp(-80.0, 0.0) + 80.0) / 80.0) * plot.height();
    let x_to_db = |x: f32| (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * 80.0) - 80.0;

    for db in [-60, -40, -20, 0] {
        let x = db_to_x(db as f32);
        let y = db_to_y(db as f32);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(0.8_f32, Color32::from_rgb(26, 35, 48)),
        );
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.8_f32, Color32::from_rgb(26, 35, 48)),
        );
    }

    painter.line_segment(
        [Pos2::new(plot.left(), plot.bottom()), Pos2::new(plot.right(), plot.top())],
        Stroke::new(1.0_f32, Color32::from_rgb(45, 58, 75)),
    );

    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            *threshold_db = (x_to_db(pos.x) * 2.0).round() / 2.0;
            *threshold_db = threshold_db.clamp(-80.0, 0.0);
            changed = true;
        }
    }

    let mut pts = Vec::with_capacity(81);
    for i in 0..=80 {
        let in_db = -80.0 + (i as f32);
        let out_db = comp_transfer_db(in_db, *threshold_db, ratio, gain_db, enabled);
        pts.push(Pos2::new(db_to_x(in_db), db_to_y(out_db)));
    }

    let col = if enabled {
        Color32::from_rgb(255, 175, 0)
    } else {
        Color32::from_rgb(100, 110, 125)
    };
    painter.add(Shape::Path(PathShape::line(pts, Stroke::new(2.4_f32, col))));

    let knee_y = comp_transfer_db(*threshold_db, *threshold_db, ratio, gain_db, enabled);
    let knee_pos = Pos2::new(db_to_x(*threshold_db), db_to_y(knee_y));
    painter.circle_filled(knee_pos, 6.0, col);
    painter.circle_stroke(knee_pos, 6.0, Stroke::new(1.5_f32, Color32::WHITE));

    if input_meter_db > -80.0 {
        let out_db = comp_transfer_db(input_meter_db, *threshold_db, ratio, gain_db, enabled);
        let sig_pos = Pos2::new(db_to_x(input_meter_db), db_to_y(out_db));
        painter.circle_filled(sig_pos, 4.5, Color32::from_rgb(0, 235, 255));
    }

    let gr_db = if enabled && input_meter_db > *threshold_db {
        ((input_meter_db - *threshold_db) * (1.0 - 1.0 / ratio.max(1.0))).clamp(0.0, 20.0)
    } else {
        0.0
    };
    if gr_db > 0.05 {
        let gr_bar_w = 6.0;
        let gr_h = (gr_db / 20.0) * plot.height();
        let gr_rect = Rect::from_min_size(
            Pos2::new(plot.right() - gr_bar_w - 2.0, plot.top()),
            Vec2::new(gr_bar_w, gr_h),
        );
        painter.rect_filled(gr_rect, CornerRadius::same(2), Color32::from_rgb(231, 76, 60));
    }

    painter.text(
        Pos2::new(plot.left() + 4.0, plot.top() + 4.0),
        Align2::LEFT_TOP,
        if gr_db > 0.1 {
            format!("COMP {:.1}:1 | GR -{:.1}dB", ratio, gr_db)
        } else {
            format!("COMP {:.1}:1 | TH:{:.1}dB", ratio, *threshold_db)
        },
        FontId::proportional(10.5),
        col,
    );

    changed
}

/// Rotary Knob Widget with glowing arc, value readout, drag & scroll support, and double-click reset
pub fn draw_rotary_knob(
    ui: &mut Ui,
    label: &str,
    val: &mut f32,
    min: f32,
    max: f32,
    default_val: f32,
    value_text: &str,
    accent: Color32,
    diameter: f32,
) -> bool {
    let total_size = Vec2::new(diameter + 18.0, diameter + 30.0);
    let (rect, resp) = ui.allocate_exact_size(total_size, Sense::click_and_drag());
    let mut changed = false;

    if resp.double_clicked() {
        *val = default_val;
        changed = true;
    } else if resp.dragged() {
        let delta = -resp.drag_delta().y * 0.0065 * (max - min);
        if delta.abs() > 1e-5 {
            *val = (*val + delta).clamp(min, max);
            changed = true;
        }
    }

    if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let mult = if ui.input(|i| i.modifiers.shift) { 0.003 } else { 0.015 };
            let step = (max - min) * mult * scroll.signum();
            *val = (*val + step).clamp(min, max);
            changed = true;
        }
    }

    let painter = ui.painter_at(rect);
    let center = Pos2::new(rect.center().x, rect.top() + 13.0 + diameter * 0.5);
    let radius = diameter * 0.44;

    // Top label
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 1.0),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(10.0),
        Color32::from_rgb(165, 180, 200),
    );

    // Outer track arc (-135 deg .. +135 deg)
    let start_ang = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let norm = ((*val - min) / (max - min)).clamp(0.0, 1.0);

    let arc_steps = 24usize;
    let mut bg_arc = Vec::with_capacity(arc_steps + 1);
    for i in 0..=arc_steps {
        let a = start_ang + (i as f32 / arc_steps as f32) * sweep;
        bg_arc.push(Pos2::new(
            center.x + a.cos() * (radius + 3.0),
            center.y + a.sin() * (radius + 3.0),
        ));
    }
    painter.add(Shape::Path(PathShape::line(
        bg_arc,
        Stroke::new(2.5_f32, Color32::from_rgb(30, 38, 52)),
    )));

    let active_steps = ((norm * arc_steps as f32).ceil() as usize).max(1);
    let mut fg_arc = Vec::with_capacity(active_steps + 1);
    for i in 0..=active_steps {
        let t = (i as f32 / active_steps as f32) * norm;
        let a = start_ang + t * sweep;
        fg_arc.push(Pos2::new(
            center.x + a.cos() * (radius + 3.0),
            center.y + a.sin() * (radius + 3.0),
        ));
    }
    painter.add(Shape::Path(PathShape::line(fg_arc, Stroke::new(2.8_f32, accent))));

    // Knob body
    painter.circle_filled(center, radius, Color32::from_rgb(26, 33, 46));
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.2_f32, Color32::from_rgb(65, 80, 105)),
    );

    // Indicator pointer line
    let cur_ang = start_ang + norm * sweep;
    let p_in = Pos2::new(
        center.x + cur_ang.cos() * (radius * 0.28),
        center.y + cur_ang.sin() * (radius * 0.28),
    );
    let p_out = Pos2::new(
        center.x + cur_ang.cos() * (radius * 0.88),
        center.y + cur_ang.sin() * (radius * 0.88),
    );
    painter.line_segment([p_in, p_out], Stroke::new(2.2_f32, accent));

    // Bottom value readout
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 1.0),
        Align2::CENTER_BOTTOM,
        value_text,
        FontId::proportional(10.0),
        Color32::from_rgb(220, 230, 245),
    );

    changed
}

/// Non-linear fader law matching Axios 24 hardware taper (-120 dB .. +10 dB)
pub fn db_to_fader_norm(db: f32, max_db: f32) -> f32 {
    if db <= -119.0 {
        0.0
    } else if db >= -20.0 {
        // Upper 65% of fader travel covers -20 dB .. max_db (high resolution around 0 dB)
        0.35 + ((db - (-20.0)) / (max_db - (-20.0))) * 0.65
    } else {
        // Lower 35% covers -120 dB .. -20 dB
        ((db - (-120.0)) / 100.0) * 0.35
    }
    .clamp(0.0, 1.0)
}

pub fn fader_norm_to_db(norm: f32, max_db: f32) -> f32 {
    let n = norm.clamp(0.0, 1.0);
    if n <= 0.01 {
        -120.0
    } else if n >= 0.35 {
        -20.0 + ((n - 0.35) / 0.65) * (max_db - (-20.0))
    } else {
        -120.0 + (n / 0.35) * 100.0
    }
}

/// Long-throw vertical console fader + 24-segment LED peak meter (DS Mixer style)
pub fn draw_console_fader_and_meter(
    ui: &mut Ui,
    db: &mut f32,
    max_db: f32,
    meter_db: f32,
    accent: Color32,
    width: f32,
    height: f32,
) -> bool {
    let w = width.max(38.0);
    let h = height.max(36.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
    let mut changed = false;

    let pad_t = 10.0;
    let pad_b = 10.0;
    let track_h = (h - pad_t - pad_b).max(20.0);
    let track_top = rect.top() + pad_t;
    let track_bot = track_top + track_h;

    if resp.double_clicked() {
        *db = 0.0;
        changed = true;
    } else if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let norm = ((track_bot - pos.y) / track_h).clamp(0.0, 1.0);
            *db = (fader_norm_to_db(norm, max_db) * 10.0).round() / 10.0;
            changed = true;
        }
    }

    if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let shift = ui.input(|i| i.modifiers.shift);
            if shift {
                *db = ((*db + scroll.signum() * 0.1).clamp(-120.0, max_db) * 10.0).round() / 10.0;
            } else {
                let norm = (db_to_fader_norm(*db, max_db) + scroll.signum() * 0.012).clamp(0.0, 1.0);
                *db = (fader_norm_to_db(norm, max_db) * 10.0).round() / 10.0;
            }
            changed = true;
        }
    }

    let painter = ui.painter_at(rect);
    let center_x = rect.center().x;
    let fader_x = center_x - (w * 0.16).clamp(7.0, 16.0);
    let meter_x = center_x + (w * 0.28).clamp(13.0, 26.0);
    let meter_w = (w * 0.12).clamp(6.0, 10.0);

    // dB scale tick marks between fader and meter
    let ticks: [(f32, &str); 8] = [
        (10.0, "+10"),
        (5.0, "+5"),
        (0.0, "0"),
        (-5.0, "-5"),
        (-10.0, "-10"),
        (-20.0, "-20"),
        (-40.0, "-40"),
        (-120.0, "∞"),
    ];
    let tick_half = (w * 0.15).clamp(7.0, 13.0);
    for (t_db, t_lbl) in ticks {
        if t_db > max_db {
            continue;
        }
        let n = db_to_fader_norm(t_db, max_db);
        let y = track_bot - n * track_h;
        let is_zero = (t_db - 0.0).abs() < 0.1;
        painter.line_segment(
            [Pos2::new(fader_x - tick_half, y), Pos2::new(fader_x + tick_half, y)],
            Stroke::new(
                if is_zero { 1.5 } else { 0.8_f32 },
                if is_zero {
                    Color32::from_rgb(0, 210, 255)
                } else {
                    Color32::from_rgb(55, 68, 88)
                },
            ),
        );
        if w >= 48.0 {
            painter.text(
                Pos2::new((fader_x + meter_x - meter_w * 0.5) * 0.5 + 2.0, y),
                Align2::CENTER_CENTER,
                t_lbl,
                FontId::proportional(8.5),
                if is_zero {
                    Color32::from_rgb(200, 230, 255)
                } else {
                    Color32::from_rgb(105, 120, 142)
                },
            );
        }
    }

    // Fader slot groove
    let groove_rect = Rect::from_min_max(
        Pos2::new(fader_x - 2.5, track_top),
        Pos2::new(fader_x + 2.5, track_bot),
    );
    painter.rect_filled(groove_rect, CornerRadius::same(2), Color32::from_rgb(8, 11, 16));
    painter.rect_stroke(
        groove_rect,
        CornerRadius::same(2),
        Stroke::new(1.0_f32, Color32::from_rgb(38, 48, 65)),
        StrokeKind::Inside,
    );

    // Glowing fill up to fader cap
    let norm = db_to_fader_norm(*db, max_db);
    let cap_y = track_bot - norm * track_h;
    if norm > 0.01 {
        let active_groove = Rect::from_min_max(
            Pos2::new(fader_x - 1.5, cap_y),
            Pos2::new(fader_x + 1.5, track_bot),
        );
        painter.rect_filled(active_groove, CornerRadius::same(1), accent);
    }

    // Realistic hardware fader cap (3D metallic cap with center line)
    let cap_w = (w * 0.38).clamp(20.0, 30.0);
    let cap_h = (track_h * 0.16).clamp(18.0, 30.0);
    let cap_rect = Rect::from_center_size(Pos2::new(fader_x, cap_y), Vec2::new(cap_w, cap_h));
    painter.rect_filled(cap_rect, CornerRadius::same(4), Color32::from_rgb(48, 58, 74));
    let inner_cap = cap_rect.shrink2(Vec2::new(2.0, 3.0));
    painter.rect_filled(inner_cap, CornerRadius::same(2), Color32::from_rgb(32, 40, 54));
    painter.rect_stroke(
        cap_rect,
        CornerRadius::same(4),
        Stroke::new(
            1.2_f32,
            if resp.hovered() || resp.dragged() {
                accent
            } else {
                Color32::from_rgb(110, 125, 148)
            },
        ),
        StrokeKind::Inside,
    );
    // Grip ridges + bright center line
    for dy in [-5.0_f32, -2.5, 2.5, 5.0] {
        if dy.abs() < cap_h * 0.4 {
            painter.line_segment(
                [
                    Pos2::new(cap_rect.left() + 4.0, cap_y + dy),
                    Pos2::new(cap_rect.right() - 4.0, cap_y + dy),
                ],
                Stroke::new(1.0_f32, Color32::from_rgb(72, 85, 105)),
            );
        }
    }
    painter.line_segment(
        [
            Pos2::new(cap_rect.left() + 2.0, cap_y),
            Pos2::new(cap_rect.right() - 2.0, cap_y),
        ],
        Stroke::new(2.2_f32, Color32::WHITE),
    );

    // 24-segment LED Peak Meter
    let num_segs = if track_h < 90.0 { 16usize } else { 24usize };
    let seg_gap = 1.4;
    let seg_h = ((track_h - (num_segs as f32 - 1.0) * seg_gap) / num_segs as f32).max(1.5);
    let meter_norm = ((meter_db + 65.0) / 68.0).clamp(0.0, 1.0);
    let lit_segs = (meter_norm * num_segs as f32).round() as usize;

    let red_thresh = (num_segs * 22) / 24;
    let yel_thresh = (num_segs * 17) / 24;

    for s in 0..num_segs {
        let seg_top = track_bot - ((s + 1) as f32) * seg_h - (s as f32) * seg_gap;
        let seg_rect = Rect::from_min_size(
            Pos2::new(meter_x - meter_w * 0.5, seg_top),
            Vec2::new(meter_w, seg_h),
        );
        let is_lit = s < lit_segs;
        let col = if s >= red_thresh {
            if is_lit {
                Color32::from_rgb(255, 56, 56)
            } else {
                Color32::from_rgb(48, 18, 22)
            }
        } else if s >= yel_thresh {
            if is_lit {
                Color32::from_rgb(255, 205, 35)
            } else {
                Color32::from_rgb(46, 40, 16)
            }
        } else if is_lit {
            Color32::from_rgb(38, 222, 129)
        } else {
            Color32::from_rgb(16, 38, 28)
        };
        painter.rect_filled(seg_rect, CornerRadius::same(1), col);
    }

    changed
}

/// Compact pill toggle button (used for 48V, Ø, HI-Z, GATE, COMP, EQ, SOLO, MUTE, LINK)
pub fn draw_pill_button(
    ui: &mut Ui,
    label: &str,
    active: bool,
    active_bg: Color32,
    size: Vec2,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);

    let bg = if active {
        active_bg
    } else if resp.hovered() {
        Color32::from_rgb(42, 54, 72)
    } else {
        Color32::from_rgb(24, 31, 44)
    };
    let border = if active {
        Color32::WHITE
    } else if resp.hovered() {
        active_bg
    } else {
        Color32::from_rgb(55, 68, 88)
    };
    let text_col = if active {
        Color32::WHITE
    } else {
        Color32::from_rgb(185, 198, 215)
    };

    painter.rect_filled(rect, CornerRadius::same(4), bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        text_col,
    );
    resp
}

/// Draws a single complete Channel Strip for the console view (supporting BigDesktop compact strip, Normal strip, and Analog knob strip)
pub fn draw_channel_strip(
    ui: &mut Ui,
    state: &mut Axios24State,
    sel: SelectedChannel,
    strip_w: f32,
    strip_height: f32,
    is_master_section: bool,
) {
    let is_selected = state.selected_channel == sel;
    let strip_color = STRIP_PALETTE[(state.channel_color_idx(sel) as usize) % STRIP_PALETTE.len()];
    let (group, ch) = sel.group_and_index();
    let flat_id = sel.flat_id();
    let view_mode = state.view_mode;

    let strip_w = strip_w.max(44.0);
    let strip_height = strip_height.max(120.0);

    let (rect, strip_resp) = ui.allocate_exact_size(Vec2::new(strip_w, strip_height), Sense::click());
    if strip_resp.clicked() {
        if ui.input(|i| i.modifiers.command) {
            state.ctrl_link_channel(sel);
        } else {
            state.selected_channel = sel;
        }
    }

    let painter = ui.painter_at(rect);
    let dark = state.dark_theme;
    let base_bg = if is_selected {
        if dark { Color32::from_rgb(22, 30, 44) } else { Color32::from_rgb(218, 232, 248) }
    } else if is_master_section {
        if dark { Color32::from_rgb(16, 24, 36) } else { Color32::from_rgb(225, 233, 245) }
    } else if dark {
        Color32::from_rgb(16, 21, 30)
    } else {
        Color32::from_rgb(236, 240, 246)
    };
    let tint_alpha = if is_master_section {
        0.16_f32
    } else if is_selected {
        if dark { 0.34 } else { 0.38 }
    } else if dark {
        0.24
    } else {
        0.28
    };
    let blend_ch = |a: u8, b: u8, t: f32| -> u8 {
        ((a as f32) * (1.0 - t) + (b as f32) * t).round().clamp(0.0, 255.0) as u8
    };
    let bg_col = Color32::from_rgb(
        blend_ch(base_bg.r(), strip_color.r(), tint_alpha),
        blend_ch(base_bg.g(), strip_color.g(), tint_alpha),
        blend_ch(base_bg.b(), strip_color.b(), tint_alpha),
    );
    let bg_luma = (bg_col.r() as u16 * 299 + bg_col.g() as u16 * 587 + bg_col.b() as u16 * 114) / 1000;
    let text_is_light = bg_luma < 145;
    let border_col = if is_selected {
        Color32::from_rgb(0, 225, 255)
    } else if is_master_section {
        Color32::from_rgb(0, 140, 180)
    } else if dark {
        Color32::from_rgb(48, 60, 78)
    } else {
        Color32::from_rgb(175, 188, 206)
    };
    painter.rect_filled(rect, CornerRadius::same(6), bg_col);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(if is_selected { 2.0 } else { 1.0_f32 }, border_col),
        StrokeKind::Inside,
    );

    // Top color bar (right-click header to cycle strip color)
    let top_bar = Rect::from_min_size(rect.min, Vec2::new(strip_w, 6.0));
    painter.rect_filled(top_bar, CornerRadius::same(4), strip_color);

    let scribble_top = rect.bottom() - 23.0;
    let inner_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 3.0, rect.top() + 8.0),
        Pos2::new(rect.right() - 3.0, scribble_top - 2.0),
    );

    let mut child_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child_ui.set_clip_rect(inner_rect);
    child_ui.spacing_mut().item_spacing = Vec2::new(2.0, 3.0);

    let inner_w = (strip_w - 8.0).max(36.0);

    // Channel ID badge (shown at bottom) & Custom Name (shown at top)
    let badge_lbl = match sel {
        SelectedChannel::Input(i) if i < 24 => format!("CH {:02}", i + 1),
        SelectedChannel::Input(_) => "DIGI".to_string(),
        SelectedChannel::Fx(i) => format!("FX {}", i + 1),
        SelectedChannel::Output(i) if i < 8 => format!("AUX {}", i + 1),
        SelectedChannel::Output(8) => "MAIN L".to_string(),
        SelectedChannel::Output(_) => "MAIN R".to_string(),
        SelectedChannel::Dca(i) => format!("DCA {}", i + 1),
    };

    // Top Header: Channel Custom Name (Double-click to rename inline, Right-click for color/menu)
    if state.renaming_channel == Some(sel) {
        let te_resp = child_ui.add_sized(
            Vec2::new(inner_w, 20.0),
            egui::TextEdit::singleline(&mut state.rename_buffer)
                .horizontal_align(egui::Align::Center)
                .font(FontId::proportional(11.0)),
        );
        te_resp.request_focus();
        let enter_pressed = child_ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc_pressed = child_ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed {
            state.renaming_channel = None;
        } else if enter_pressed || (te_resp.lost_focus() && !te_resp.has_focus()) {
            let new_name = state.rename_buffer.trim().to_string();
            if !new_name.is_empty() {
                state.set_channel_name(sel, new_name);
            }
            state.renaming_channel = None;
        }
    } else {
        let ch_name = state.channel_name(sel).to_string();
        let hdr_resp = child_ui
            .add(
                egui::Button::new(
                    egui::RichText::new(&ch_name)
                        .size(11.0)
                        .strong()
                        .color(if is_selected {
                            Color32::from_rgb(10, 14, 20)
                        } else if dark {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(20, 28, 40)
                        }),
                )
                .fill(if is_selected {
                    Color32::from_rgb(0, 220, 255)
                } else if dark {
                    Color32::from_rgb(30, 39, 54)
                } else {
                    Color32::from_rgb(206, 215, 228)
                })
                .min_size(Vec2::new(inner_w, 20.0)),
            )
            .on_hover_text("Duplo clique para renomear | Botão direito para mudar cor e opções");
        if hdr_resp.double_clicked() {
            state.renaming_channel = Some(sel);
            state.rename_buffer = ch_name;
        } else if hdr_resp.clicked() {
            if child_ui.input(|i| i.modifiers.command) {
                state.ctrl_link_channel(sel);
            } else {
                state.selected_channel = sel;
                if state.clipboard_channel.is_some()
                    && state.settings_tab == crate::model::SettingsSubTab::CopyPaste
                {
                    if let Some(src) = state.clipboard_channel {
                        state.paste_channel_from(src, sel);
                    }
                }
            }
        }
        show_channel_context_menu(&hdr_resp, state, sel);
    }

    // For Master Dock strips, always keep full fader throw clean and uncluttered;
    // for channel bank strips, render horizontal Gain bar + mode-specific upper controls.
    if !is_master_section {
        if group == 1 && ch < 24 && strip_height >= 160.0 {
            let mut gain_db = state.input_preamp_gain_db(ch);
            if draw_horizontal_gain_bar(&mut child_ui, &mut gain_db, inner_w, text_is_light) {
                state.set_input_preamp_gain_db(ch, gain_db);
                state.selected_channel = sel;
            }
        }
        match view_mode {
            crate::model::ViewMode::Normal => {
                if group == 1 && ch < 24 && strip_height >= 330.0 {
                    child_ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let btn_w = (inner_w - 2.0) * 0.5;
                        let p48 = state.input_phantom_48v(ch);
                        if draw_pill_button(
                            ui,
                            "48V",
                            p48,
                            Color32::from_rgb(231, 76, 60),
                            Vec2::new(btn_w, 16.0),
                        )
                        .on_hover_text("Phantom Power +48V: Alimenta microfones condensadores e Direct Box ativos (com proteção anti-estalo)")
                        .clicked()
                        {
                            state.set_input_phantom_48v(ch, !p48);
                        }
                        let inv = state.input_phase_invert(ch);
                        if draw_pill_button(
                            ui,
                            "Ø",
                            inv,
                            Color32::from_rgb(243, 156, 18),
                            Vec2::new(btn_w, 16.0),
                        )
                        .on_hover_text("Inversão de Fase (180°): Corrige cancelamento de áudio entre microfones próximos")
                        .clicked()
                        {
                            state.set_input_phase_invert(ch, !inv);
                        }
                    });
                } else if group == 3 && strip_height >= 330.0 {
                    let inv = state.output_phase_invert(ch);
                    if draw_pill_button(
                        &mut child_ui,
                        "Ø FASE",
                        inv,
                        Color32::from_rgb(243, 156, 18),
                        Vec2::new(inner_w, 16.0),
                    )
                    .on_hover_text("Inversão de Fase (180°) desta saída")
                    .clicked()
                    {
                        state.set_output_phase_invert(ch, !inv);
                    }
                }

                // Mini EQ Curve Thumbnail (for Inputs, FX, Outputs)
                if group <= 3 && strip_height >= 320.0 {
                    if draw_mini_eq_thumbnail(
                        &mut child_ui,
                        state,
                        group,
                        ch,
                        Vec2::new(inner_w, 28.0),
                    )
                    .on_hover_text("Mini Curva de Equalização: Clique para abrir o Equalizador completo (PEQ / RTA)")
                    .clicked()
                    {
                        state.selected_channel = sel;
                        state.top_tab = crate::model::TopNavTab::PeqGeq;
                    }
                }

                // Gate / Comp / EQ status pills
                if group == 1 && strip_height >= 330.0 {
                    child_ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let w3 = (inner_w - 4.0) / 3.0;
                        let g_on = state.input_gate_enabled(ch);
                        if draw_pill_button(
                            ui,
                            "GT",
                            g_on,
                            Color32::from_rgb(46, 204, 113),
                            Vec2::new(w3, 16.0),
                        )
                        .on_hover_text("Noise Gate: Corta ruídos e vazamentos abaixo do Threshold")
                        .clicked()
                        {
                            state.set_input_gate_enabled(ch, !g_on);
                            state.selected_channel = sel;
                        }
                        let c_on = state.comp_enabled(1, ch);
                        if draw_pill_button(
                            ui,
                            "CP",
                            c_on,
                            Color32::from_rgb(243, 156, 18),
                            Vec2::new(w3, 16.0),
                        )
                        .on_hover_text("Compressor Dinâmico: Controla picos e mantém o volume mais estável")
                        .clicked()
                        {
                            state.set_comp_enabled(1, ch, !c_on);
                            state.selected_channel = sel;
                        }
                        let e_on = state.eq_enabled(1, ch);
                        if draw_pill_button(
                            ui,
                            "EQ",
                            e_on,
                            Color32::from_rgb(0, 188, 212),
                            Vec2::new(w3, 16.0),
                        )
                        .on_hover_text("Equalizador Paramétrico (PEQ): Liga ou desliga o EQ deste canal")
                        .clicked()
                        {
                            state.set_eq_enabled(1, ch, !e_on);
                            state.selected_channel = sel;
                        }
                    });
                } else if group == 3 && strip_height >= 330.0 {
                    child_ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let w2 = (inner_w - 2.0) * 0.5;
                        let c_on = state.comp_enabled(3, ch);
                        if draw_pill_button(
                            ui,
                            "COMP",
                            c_on,
                            Color32::from_rgb(243, 156, 18),
                            Vec2::new(w2, 16.0),
                        )
                        .on_hover_text("Compressor da Saída: Controla picos e protege caixas/fones")
                        .clicked()
                        {
                            state.set_comp_enabled(3, ch, !c_on);
                            state.selected_channel = sel;
                        }
                        let e_on = state.eq_enabled(3, ch);
                        if draw_pill_button(
                            ui,
                            "PEQ",
                            e_on,
                            Color32::from_rgb(0, 188, 212),
                            Vec2::new(w2, 16.0),
                        )
                        .on_hover_text("Equalizador Paramétrico (PEQ) desta saída")
                        .clicked()
                        {
                            state.set_eq_enabled(3, ch, !e_on);
                            state.selected_channel = sel;
                        }
                    });
                }

                // Pan control on Inputs
                if group == 1 && strip_height >= 350.0 {
                    let mut pan = state.input_pan(ch) as f32;
                    let pan_txt = if pan.abs() < 1.0 {
                        "L=R".to_string()
                    } else if pan < 0.0 {
                        format!("L{:.0}", -pan)
                    } else {
                        format!("R{:.0}", pan)
                    };
                    if draw_rotary_knob(
                        &mut child_ui,
                        "PAN",
                        &mut pan,
                        -100.0,
                        100.0,
                        0.0,
                        &pan_txt,
                        strip_color,
                        26.0,
                    ) {
                        state.set_input_pan(ch, pan.round() as i32);
                    }
                }
            }
            crate::model::ViewMode::Analog => {
                let knob_d = if strip_height >= 520.0 { 26.0 } else { 22.0 };
                if group == 1 {
                    if strip_height >= 280.0 {
                        let mut gain = state.input_preamp_gain_db(ch);
                        let g_txt = format!("+{:.0}dB", gain);
                        if draw_rotary_knob(
                            &mut child_ui,
                            "GAIN",
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
                    }

                    if strip_height >= 360.0 {
                        let mut comp_th = state.comp_threshold_db(1, ch);
                        let c_txt = format!("{:.0}dB", comp_th);
                        if draw_rotary_knob(
                            &mut child_ui,
                            "COMP",
                            &mut comp_th,
                            -60.0,
                            0.0,
                            0.0,
                            &c_txt,
                            Color32::from_rgb(243, 156, 18),
                            knob_d,
                        ) {
                            state.set_comp_enabled(1, ch, true);
                            state.set_comp_threshold_db(1, ch, comp_th);
                        }
                    }

                    if strip_height >= 460.0 {
                        let mut b_high = state.peq_band(1, ch, 3);
                        let hi_txt = format!("{:+.1}", b_high.gain_db);
                        if draw_rotary_knob(
                            &mut child_ui,
                            "EQ HI",
                            &mut b_high.gain_db,
                            -12.0,
                            12.0,
                            0.0,
                            &hi_txt,
                            Color32::from_rgb(30, 144, 255),
                            knob_d,
                        ) {
                            state.set_peq_band(1, ch, 3, b_high);
                        }

                        let mut b_low = state.peq_band(1, ch, 0);
                        let lo_txt = format!("{:+.1}", b_low.gain_db);
                        if draw_rotary_knob(
                            &mut child_ui,
                            "EQ LO",
                            &mut b_low.gain_db,
                            -12.0,
                            12.0,
                            0.0,
                            &lo_txt,
                            Color32::from_rgb(255, 99, 71),
                            knob_d,
                        ) {
                            state.set_peq_band(1, ch, 0, b_low);
                        }
                    }

                    if strip_height >= 520.0 {
                        let mut pan = state.input_pan(ch) as f32;
                        let pan_txt = if pan.abs() < 1.0 {
                            "C".to_string()
                        } else if pan < 0.0 {
                            format!("L{:.0}", -pan)
                        } else {
                            format!("R{:.0}", pan)
                        };
                        if draw_rotary_knob(
                            &mut child_ui,
                            "PAN",
                            &mut pan,
                            -100.0,
                            100.0,
                            0.0,
                            &pan_txt,
                            strip_color,
                            knob_d,
                        ) {
                            state.set_input_pan(ch, pan.round() as i32);
                        }
                    }
                } else if group <= 3 && strip_height >= 220.0 {
                    if draw_mini_eq_thumbnail(
                        &mut child_ui,
                        state,
                        group,
                        ch,
                        Vec2::new(inner_w, 30.0),
                    )
                    .clicked()
                    {
                        state.selected_channel = sel;
                    }
                }
            }
        }
    } else if group <= 3 && strip_height >= 220.0 {
        // Master L & R mini EQ curve thumbnail at the top of the right dock
        if draw_mini_eq_thumbnail(&mut child_ui, state, group, ch, Vec2::new(inner_w, 24.0))
            .on_hover_text("Clique para abrir o Equalizador (PEQ 7 bandas + GEQ 31 bandas) do MAIN L/R")
            .clicked()
        {
            state.selected_channel = sel;
            state.top_tab = crate::model::TopNavTab::PeqGeq;
        }
    }

    // Direct L/R Routing Indicator + Safe Cable Unplug (or Pre/Post Fader when in AUX Sends-on-Fader mode)
    if group == 1 && !is_master_section && strip_height >= 200.0 {
        if let crate::model::SendsOnFaderTarget::Aux(aux_idx) = state.sends_on_fader {
            let pre = state.input_aux_pre_fader(ch, aux_idx);
            if draw_pill_button(
                &mut child_ui,
                if pre { "PRE FADER" } else { "POST FADER" },
                true,
                if pre {
                    Color32::from_rgb(26, 188, 156)
                } else {
                    Color32::from_rgb(230, 126, 34)
                },
                Vec2::new(inner_w, 16.0),
            )
            .on_hover_text("Alterna envio do canal para este AUX entre Pré-Fader (independente do fader principal) e Pós-Fader")
            .clicked()
            {
                state.set_input_aux_pre_fader(ch, aux_idx, !pre);
            }
        } else {
            child_ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let to_lr = state.input_to_main_lr(ch);
                let unplugged = state.is_safe_unplugged(ch);
                let lr_w = (inner_w - 2.0) * 0.54;
                let unp_w = (inner_w - 2.0) * 0.46;
                if draw_pill_button(
                    ui,
                    if to_lr { "L/R" } else { "!L/R" },
                    to_lr,
                    Color32::from_rgb(0, 165, 210),
                    Vec2::new(lr_w, 16.0),
                )
                .on_hover_text("Envia este canal para o Master L/R (Se desligado, o som não sai no PA)")
                .clicked()
                {
                    state.set_input_to_main_lr(ch, !to_lr);
                }
                if draw_pill_button(
                    ui,
                    if unplugged { "OFF!" } else { "\u{f1e6}" },
                    unplugged,
                    Color32::from_rgb(230, 126, 34),
                    Vec2::new(unp_w, 16.0),
                )
                .on_hover_text("Modo Desplugar Cabo: Muta e baixa o volume sem estalo no PA para trocar instrumento/microfone")
                .clicked()
                {
                    state.toggle_safe_unplug(ch);
                }
            });
        }
    }

    // Solo & Mute buttons
    let is_muted = state.channel_muted(sel);
    let is_solo = state.channel_solo(sel);
    child_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if group != 4 {
            if draw_pill_button(
                ui,
                "S",
                is_solo,
                Color32::from_rgb(243, 156, 18),
                Vec2::new((inner_w - 2.0) * 0.34, 20.0),
            )
            .on_hover_text("Solo (PFL): Ouve apenas este canal nos fones de monitoração sem afetar o PA (Atalho: tecla S)")
            .clicked()
            {
                state.set_channel_solo(sel, !is_solo);
            }
        }
        let mute_w = if group == 4 {
            inner_w
        } else {
            (inner_w - 2.0) * 0.66
        };
        if draw_pill_button(
            ui,
            "MUTE",
            is_muted,
            Color32::from_rgb(231, 76, 60),
            Vec2::new(mute_w, 20.0),
        )
        .on_hover_text("Mute: Silencia imediatamente este canal (Atalho: tecla M)")
        .clicked()
        {
            state.set_channel_muted(sel, !is_muted);
        }
    });

    // Fader dB numeric badge (supports Sends on Fader mode!)
    let mut fader_db = if is_master_section {
        state.channel_fader_db(sel)
    } else {
        state.effective_fader_db(sel)
    };
    let max_db = if is_master_section {
        10.0
    } else {
        state.effective_fader_max_db(sel)
    };
    let fader_accent = if !is_master_section
        && state.sends_on_fader != crate::model::SendsOnFaderTarget::MainLR
    {
        match state.sends_on_fader {
            crate::model::SendsOnFaderTarget::Aux(_) => Color32::from_rgb(26, 218, 176),
            crate::model::SendsOnFaderTarget::Fx(_) => Color32::from_rgb(245, 60, 130),
            crate::model::SendsOnFaderTarget::MainLR => strip_color,
        }
    } else {
        strip_color
    };

    if group == 4 && !is_master_section {
        let spilled = state.dca_spill == Some(ch);
        if draw_pill_button(
            &mut child_ui,
            if spilled { "SPILL ON" } else { "SPILL" },
            spilled,
            Color32::from_rgb(241, 196, 15),
            Vec2::new(inner_w, 16.0),
        )
        .on_hover_text("DCA Spill: Exibe na tela apenas os canais que fazem parte deste DCA")
        .clicked()
        {
            state.dca_spill = if spilled { None } else { Some(ch) };
        }
    }

    if state.channel_clip_hold[flat_id] {
        if draw_pill_button(
            &mut child_ui,
            "CLIP!",
            true,
            Color32::from_rgb(220, 38, 38),
            Vec2::new(inner_w, 15.0),
        )
        .on_hover_text("Pico acima de 0 dBFS detectado! Clique para limpar o alerta de CLIP")
        .clicked()
        {
            state.channel_clip_hold[flat_id] = false;
        }
    }

    child_ui.label(
        egui::RichText::new(format_fader_db(fader_db))
            .size(10.0)
            .monospace()
            .color(if text_is_light {
                Color32::from_rgb(215, 228, 245)
            } else {
                Color32::from_rgb(20, 30, 48)
            }),
    );

    // Strictly bounded remaining vertical space for Fader + LED Meter (never overflows bottom scribble strip)
    let cursor_y = child_ui.cursor().top();
    let remaining_h = (scribble_top - cursor_y - 2.0).max(36.0);
    let meter_db = state.channel_meters_db[flat_id];
    if draw_console_fader_and_meter(
        &mut child_ui,
        &mut fader_db,
        max_db,
        meter_db,
        fader_accent,
        inner_w,
        remaining_h,
    ) {
        if is_master_section {
            state.set_channel_fader_db(sel, fader_db);
        } else {
            state.set_effective_fader_db(sel, fader_db);
        }
        state.selected_channel = sel;
    }

    // Bottom Scribble Strip (Channel Number / Bus ID)
    let scribble_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 3.0, scribble_top),
        Pos2::new(rect.right() - 3.0, rect.bottom() - 3.0),
    );
    painter.rect_filled(
        scribble_rect,
        CornerRadius::same(3),
        Color32::from_rgba_unmultiplied(strip_color.r(), strip_color.g(), strip_color.b(), if dark { 45 } else { 70 }),
    );
    painter.rect_stroke(
        scribble_rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, strip_color),
        StrokeKind::Inside,
    );
    let bottom_txt = if let Some(lnk) = state.channel_link_badge(sel) {
        if lnk == "LNK" {
            format!("\u{f0c1} {}", badge_lbl)
        } else {
            format!("\u{f0c1}{} {}", badge_lbl, lnk)
        }
    } else {
        badge_lbl.clone()
    };
    let bottom_font = if bottom_txt.chars().count() > 8 {
        FontId::proportional(9.0)
    } else {
        FontId::proportional(10.0)
    };
    painter.with_clip_rect(scribble_rect.shrink(2.0)).text(
        scribble_rect.center(),
        Align2::CENTER_CENTER,
        &bottom_txt,
        bottom_font,
        if text_is_light { Color32::WHITE } else { Color32::from_rgb(15, 22, 34) },
    );
    let scribble_resp = ui.interact(
        scribble_rect,
        ui.id().with(("scribble", flat_id)),
        Sense::click(),
    )
    .on_hover_text("Duplo clique para renomear | Botão direito para mudar cor e opções");
    if scribble_resp.double_clicked() {
        state.renaming_channel = Some(sel);
        state.rename_buffer = state.channel_name(sel).to_string();
    } else if scribble_resp.clicked() {
        if ui.input(|i| i.modifiers.command) {
            state.ctrl_link_channel(sel);
        } else {
            state.selected_channel = sel;
        }
    }
    show_channel_context_menu(&scribble_resp, state, sel);
}

fn draw_horizontal_gain_bar(
    ui: &mut Ui,
    gain_db: &mut f32,
    width: f32,
    text_is_light: bool,
) -> bool {
    let size = Vec2::new(width.max(32.0), 14.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let resp = resp.on_hover_text(
        "Ganho do Pré-Amplificador (0 a +60 dB): Arraste na horizontal para ajustar a sensibilidade. Duplo clique zera (0 dB)",
    );
    let mut changed = false;

    if resp.double_clicked() {
        *gain_db = 0.0;
        changed = true;
    } else if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let norm = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            *gain_db = (norm * 60.0).round();
            changed = true;
        }
    } else if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            *gain_db = (*gain_db + scroll.signum()).clamp(0.0, 60.0);
            changed = true;
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(12, 16, 24));
    let norm = (*gain_db / 60.0).clamp(0.0, 1.0);
    if norm > 0.005 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width() * norm, rect.height()));
        let fill_col = if *gain_db >= 48.0 {
            Color32::from_rgb(231, 76, 60)
        } else if *gain_db >= 32.0 {
            Color32::from_rgb(230, 126, 34)
        } else {
            Color32::from_rgb(211, 84, 0)
        };
        painter.rect_filled(fill_rect, CornerRadius::same(3), fill_col);
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(
            1.0_f32,
            if resp.hovered() {
                Color32::from_rgb(255, 160, 60)
            } else {
                Color32::from_rgb(48, 60, 78)
            },
        ),
        StrokeKind::Inside,
    );
    let label = if width < 62.0 {
        format!("G +{:.0}", *gain_db)
    } else {
        format!("GAIN +{:.0}dB", *gain_db)
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        if text_is_light || norm > 0.35 {
            Color32::WHITE
        } else {
            Color32::from_rgb(220, 230, 245)
        },
    );

    changed
}

pub fn draw_color_palette_grid(
    ui: &mut Ui,
    state: &mut Axios24State,
    sel: SelectedChannel,
    cols: usize,
    swatch_size: f32,
) {
    let dark = state.dark_theme;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for row in STRIP_PALETTE.chunks(cols.max(1)) {
            let row_idx_base =
                (row.as_ptr() as usize - STRIP_PALETTE.as_ptr() as usize) / std::mem::size_of::<Color32>();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (offset, &col) in row.iter().enumerate() {
                    let c_idx = row_idx_base + offset;
                    let is_cur = state.channel_color_idx(sel) as usize == c_idx;
                    let (c_rect, c_resp) =
                        ui.allocate_exact_size(Vec2::new(swatch_size, swatch_size), Sense::click());
                    ui.painter().rect_filled(c_rect, CornerRadius::same(3), col);
                    if is_cur {
                        ui.painter().rect_stroke(
                            c_rect,
                            CornerRadius::same(3),
                            Stroke::new(2.0_f32, if dark { Color32::WHITE } else { Color32::BLACK }),
                            StrokeKind::Inside,
                        );
                    }
                    if c_resp.clicked() {
                        state.set_channel_color_idx(sel, c_idx as u8);
                    }
                }
            });
        }
    });
}

fn show_channel_context_menu(resp: &Response, state: &mut Axios24State, sel: SelectedChannel) {
    resp.context_menu(|ui| {
        ui.set_min_width(195.0);
        ui.label(
            egui::RichText::new(format!("Canal: {}", state.channel_name(sel)))
                .strong()
                .size(12.0),
        );
        ui.separator();

        // Inline Rename inside menu
        ui.label(egui::RichText::new("Renomear Canal:").size(11.0));
        let mut cur_name = state.channel_name(sel).to_string();
        if ui.text_edit_singleline(&mut cur_name).changed() {
            state.set_channel_name(sel, cur_name);
        }

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Cor do Canal (32 cores):").size(11.0));
        draw_color_palette_grid(ui, state, sel, 8, 17.0);

        let (group, ch) = sel.group_and_index();
        if (group == 1 && ch < 24) || (group == 3 && ch < 10) {
            ui.separator();
            ui.label(egui::RichText::new("Link de Canais (Sem mexer no Pan):").size(11.0).strong());
            let pair_a = (ch / 2) * 2;
            let is_st = if group == 1 {
                state.is_input_stereo_linked(ch)
            } else {
                state.is_output_stereo_linked(ch)
            };
            let pair_lbl = if group == 1 {
                format!("\u{f0c1} Link Par CH{}+{} ({})", pair_a + 1, pair_a + 2, if is_st { "ON" } else { "OFF" })
            } else if ch >= 8 {
                format!("\u{f0c1} Link MAIN L & R ({})", if is_st { "ON" } else { "OFF" })
            } else {
                format!("\u{f0c1} Link AUX{}+{} ({})", pair_a + 1, pair_a + 2, if is_st { "ON" } else { "OFF" })
            };
            if ui.button(pair_lbl).clicked() {
                if group == 1 {
                    state.set_input_stereo_linked(ch, !is_st);
                } else {
                    state.set_output_stereo_linked(ch, !is_st);
                }
            }
            if group == 1 && ch < 24 {
                let cur_g = state.input_custom_link_group(ch);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Juntar:").size(10.5));
                    for g in 0..=4u8 {
                        let lbl = if g == 0 {
                            "Off".to_string()
                        } else {
                            format!("{}", (b'A' + g - 1) as char)
                        };
                        if ui.selectable_label(cur_g == g, lbl).clicked() {
                            state.set_input_custom_link_group(ch, g);
                        }
                    }
                });
            }
        }
        if group == 1 {
            if let crate::model::SendsOnFaderTarget::Aux(aux_idx) = state.sends_on_fader {
                ui.separator();
                let pre = state.input_aux_pre_fader(ch, aux_idx);
                if ui
                    .button(if pre {
                        format!("AUX {} — Mudar p/ POST Fader", aux_idx + 1)
                    } else {
                        format!("AUX {} — Mudar p/ PRE Fader", aux_idx + 1)
                    })
                    .clicked()
                {
                    state.set_input_aux_pre_fader(ch, aux_idx, !pre);
                    ui.close_menu();
                }
            }
        }

        if group != 4 {
            let flat_id = sel.flat_id();
            if flat_id < 38 {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("DCA:").size(11.0).strong());
                    for dca in 0..4 {
                        let mem = state.is_in_dca_group(dca, flat_id);
                        if ui.selectable_label(mem, format!("D{}", dca + 1)).clicked() {
                            state.set_in_dca_group(dca, flat_id, !mem);
                        }
                    }
                });
            }
        } else {
            ui.separator();
            if ui.button("\u{f085} Configurar Canais do DCA").clicked() {
                state.top_tab = crate::model::TopNavTab::Settings;
                state.settings_tab = crate::model::SettingsSubTab::DcaGroup;
                ui.close_menu();
            }
        }

        ui.separator();
        let flat_id = sel.flat_id();
        if flat_id < 42 {
            let safe = state.recall_safe_channels[flat_id];
                if ui
                    .button(if safe {
                        "\u{f023} Recall Safe: ATIVO (Destravar)"
                    } else {
                        "\u{f09c} Recall Safe (Travar na troca de cena)"
                    })
                    .clicked()
                {
                    state.recall_safe_channels[flat_id] = !safe;
                }
        }
        if group == 1 && ch < 24 {
            let am = state.automix_channels[ch];
            if ui
                .button(if am {
                    "\u{f00c} Auto-Mixer (Dugan): ATIVO"
                } else {
                    "Auto-Mixer (Dugan): Adicionar"
                })
                .clicked()
            {
                state.automix_channels[ch] = !am;
                if !am {
                    state.automix_enabled = true;
                }
            }
        }
        if ui.button("\u{f0c5} Copiar Canal").clicked() {
            state.clipboard_channel = Some(sel);
            state.toast(format!("Canal {} copiado!", state.channel_name(sel)));
            ui.close_menu();
        }
        if let Some(src) = state.clipboard_channel {
            if src != sel && ui.button("\u{f0ea} Colar Canal Aqui").clicked() {
                state.paste_channel_from(src, sel);
                ui.close_menu();
            }
        }
    });
}
