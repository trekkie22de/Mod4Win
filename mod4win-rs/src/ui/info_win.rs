//! Module-Info-Fenster (`bm10.bmp` / `DETAILS.PAS`), Instrumenten-Fenster (`bm11.bmp`)
//! und Schnellauswahl (`Quick Selection`, `bm3.bmp` / `MULBOX.PAS`).

use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{
    Mod4WinApp, PressedButton, UiSharedState, WindowKind, INFO_WINDOW_WIDTH, INST_WINDOW_WIDTH,
};
use super::widgets::{
    draw_3d_frame_only, draw_3d_panel, draw_bmp_button_2state, draw_bmp_lcd_number,
    draw_bmp_text_16x20_marquee, draw_oem_text, draw_oem_text_centered, draw_oem_text_right,
    fill_rect,
};

/// Metadaten eines einzelnen Instruments für das Info- (`bm10`) und Instrumenten-Fenster (`bm11`).
#[derive(Clone, Debug, Default)]
pub struct InstrumentInfo {
    pub index: usize,
    pub name: String,
    pub length: i32,
    pub loop_start: i32,
    pub loop_length: i32,
    pub fine_tune: i8,
    pub volume: u8,
    pub is_16bit: bool,
    pub c4_freq: i32,
}

impl Mod4WinApp {
    /// Rendert das Module-Info-Fenster (`bm10.bmp`, 452 x 373) exakt nach `DETAILS.PAS` mit dem originalen `OEM.FNT` (`8x12`).
    pub(crate) fn render_module_info_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        // 1. Originales bm10.bmp als Basis
        self.assets.bm10_info.blit_rect_to(0, 0, dw, dh, frame, dw, dh, 0, 0);
        for y in 1..18 {
            for x in 1..(dw as i32 - 1) {
                let t = x as u32;
                let w_span = (dw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let idx = ((y as u32 * dw + x as u32) * 4) as usize;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 255;
                }
            }
        }
        fill_rect(frame, dw, dh, 1, 18, dw.saturating_sub(2), 1, [0, 0, 0, 255]);
        draw_oem_text(
            frame,
            dw,
            dh,
            22,
            4,
            self.locale.tr_or("win", "info_title", "MOD4WIN 3.10 Module Info"),
            [48, 56, 68, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 21, 2);

        // 2. Modultitel oben (18, 30, 26 Zeichen in FONT_16X20.bmp, deckend geblittet)
        let title_disp = if !state.title.trim().is_empty() {
            state.title.trim()
        } else {
            state.filename.as_str()
        };
        let info_scroll_ms = state.info_title_scroll_elapsed_ms();
        draw_bmp_text_16x20_marquee(
            frame,
            dw,
            dh,
            18,
            30,
            26,
            title_disp,
            info_scroll_ms,
            &self.assets.font_16x20,
        );

        // 3. [-]/[+] Scroll-Buttons (BOMINUS / BOPLUS 14x14, 2-State) bei (10, 60) und (208, 60)
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            10,
            60,
            self.pressed_btn == Some(PressedButton::InfoMinus),
            &self.assets.bominus,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            208,
            60,
            self.pressed_btn == Some(PressedButton::InfoPlus),
            &self.assets.boplus,
        );

        let col_green = [0, 255, 0, 255];
        let col_dim = [0, 168, 0, 255];

        // "Instruments"-Header zwischen [-] und [+] im originalen 8x12 OEM-Font (`DETAILS.PAS` Zeile 178)
        fill_rect(frame, dw, dh, 25, 60, 182, 14, [0, 0, 0, 255]);
        draw_oem_text(
            frame,
            dw,
            dh,
            72,
            61,
            self.locale.tr_or("info", "instruments_header", "Instruments"),
            col_green,
        );

        // 4. Beide Instrumenten-Spalten leeren und im originalen 8x12 `OEM.FNT` zeichnen (1..15 links, 16..31 rechts)
        fill_rect(frame, dw, dh, 11, 75, 210, 210, [0, 0, 0, 255]);
        fill_rect(frame, dw, dh, 231, 61, 210, 224, [0, 0, 0, 255]);

        // Linke Spalte: 15 Zeilen ab y = 76 (je 14px hoch, 8x12 OEM-Font bei row_y + 1)
        for i in 0..15 {
            let inst_num = self.info_inst_offset + i + 1;
            let row_y = 76 + (i as i32 * 14);
            let is_sel = inst_num == self.selected_instrument;
            if is_sel {
                fill_rect(frame, dw, dh, 12, row_y, 208, 13, [0, 0, 128, 255]);
            }
            let num_str = format!("{:>2}", inst_num);
            draw_oem_text(
                frame,
                dw,
                dh,
                12,
                row_y + 1,
                &num_str,
                if is_sel { [255, 255, 255, 255] } else { col_dim },
            );

            if let Some(inst) = state.instruments.get(inst_num.saturating_sub(1)) {
                let disp: String = inst.name.chars().take(22).collect();
                draw_oem_text(
                    frame,
                    dw,
                    dh,
                    36,
                    row_y + 1,
                    &disp,
                    if is_sel { [255, 255, 0, 255] } else { col_green },
                );
            }
        }

        // Rechte Spalte: 16 Zeilen ab y = 62 (je 14px hoch, 8x12 OEM-Font bei row_y + 1)
        for i in 0..16 {
            let inst_num = self.info_inst_offset + 16 + i;
            let row_y = 62 + (i as i32 * 14);
            let is_sel = inst_num == self.selected_instrument;
            if is_sel {
                fill_rect(frame, dw, dh, 232, row_y, 208, 13, [0, 0, 128, 255]);
            }
            let num_str = format!("{:>2}", inst_num);
            draw_oem_text(
                frame,
                dw,
                dh,
                232,
                row_y + 1,
                &num_str,
                if is_sel { [255, 255, 255, 255] } else { col_dim },
            );

            if let Some(inst) = state.instruments.get(inst_num.saturating_sub(1)) {
                let disp: String = inst.name.chars().take(22).collect();
                draw_oem_text(
                    frame,
                    dw,
                    dh,
                    256,
                    row_y + 1,
                    &disp,
                    if is_sel { [255, 255, 0, 255] } else { col_green },
                );
            }
        }

        // 5. Zusammenfassungs-Box unten (y = 295..363) im originalen 8x12 OEM-Font
        fill_rect(frame, dw, dh, 12, 296, 428, 66, [0, 0, 0, 255]);
        let active_insts = state
            .instruments
            .iter()
            .filter(|ins| ins.length > 0 || !ins.name.trim().is_empty())
            .count()
            .max(state.instruments.len());

        let ch_str = state.num_channels.to_string();
        let inst_str = active_insts.to_string();
        let line1 = self.locale.tr_fmt(
            "info",
            "summary_line1",
            "{0} channel {1} module with {2} instruments",
            &[&ch_str, &state.format_str, &inst_str],
        );
        let mem_kb = (state.song_mem_bytes / 1024).max(1);
        let f_kb_str = state.file_size_kb.to_string();
        let m_kb_str = mem_kb.to_string();
        let line2 = self.locale.tr_fmt(
            "info",
            "summary_line2",
            "File: {0} KB - Mem: {1} KB",
            &[&f_kb_str, &m_kb_str],
        );
        let up_str = state.num_unique_patterns.to_string();
        let ord_str = state.num_orders.to_string();
        let line3 = self.locale.tr_fmt(
            "info",
            "summary_line3",
            "{0} of {1} patterns are different.",
            &[&up_str, &ord_str],
        );
        let mins = state.total_time_secs / 60;
        let secs = state.total_time_secs % 60;
        let m_str = format!("{:02}", mins);
        let s_str = format!("{:02}", secs);
        let line4 = self.locale.tr_fmt(
            "info",
            "summary_line4",
            "Total play time: {0}:{1} min.",
            &[&m_str, &s_str],
        );

        let center_x = |s: &str| -> i32 { (226 - (s.len() as i32 * 4)).max(16) };
        draw_oem_text(frame, dw, dh, center_x(&line1), 300, &line1, col_green);
        draw_oem_text(frame, dw, dh, center_x(&line2), 314, &line2, col_green);
        draw_oem_text(frame, dw, dh, center_x(&line3), 328, &line3, col_green);
        draw_oem_text(frame, dw, dh, center_x(&line4), 342, &line4, col_green);
    }

    /// Rendert das separate Instrumenten-Fenster (`204 x 182`) exakt nach `DETAILS.PAS:635-760`.
    pub(crate) fn render_instrument_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let is_de = self.locale.is_german() || state.ini_config.language == 1;
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];

        // 1. Äußerer 3D-Rahmen + Titelleiste (CapDY = 18)
        draw_3d_panel(frame, dw, dh, 0, 0, dw, dh, false, col_gray);
        for y in 1..18 {
            for x in 1..(dw as i32 - 1) {
                let t = x as u32;
                let w_span = (dw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let idx = ((y as u32 * dw + x as u32) * 4) as usize;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 255;
                }
            }
        }
        fill_rect(frame, dw, dh, 1, 18, dw.saturating_sub(2), 1, col_black);
        draw_oem_text(
            frame,
            dw,
            dh,
            22,
            4,
            self.locale.tr_or("Titles", "inst", "Instruments"),
            col_black,
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);

        let max_inst = state.instruments.len().max(1);
        let idx = self.selected_instrument.clamp(1, max_inst);
        let inst = state.instruments.get(idx - 1).cloned().unwrap_or_default();

        // 2. Oberer versenkter 3D-Rahmen für die 7 Wertezeilen (`DETAILS.PAS:748`: `frame3d(msg.wParam,4,CapDY+3,w-10,14+7*Fh,1,down)`)
        draw_3d_panel(frame, dw, dh, 4, 21, dw.saturating_sub(8), 98, true, col_gray);

        let hz = if inst.c4_freq > 0 { inst.c4_freq } else { 8363 };
        let bits_str = if inst.is_16bit { "16 bit" } else { "8 bit" };
        let rows: [(&str, String); 7] = if is_de {
            [
                ("Länge", format!("{}", inst.length)),
                ("Loopstart", format!("{}", inst.loop_start)),
                ("Looplänge", format!("{}", inst.loop_length)),
                ("Finetune", format!("{}", inst.fine_tune)),
                ("Def. Volume", format!("{}", inst.volume)),
                ("Auflösung", bits_str.to_string()),
                ("C2-Tuning", format!("{} Hz", hz)),
            ]
        } else {
            [
                ("Length", format!("{}", inst.length)),
                ("Loop Start", format!("{}", inst.loop_start)),
                ("Loop Length", format!("{}", inst.loop_length)),
                ("Finetune", format!("{}", inst.fine_tune)),
                ("Def. Volume", format!("{}", inst.volume)),
                ("Resolution", bits_str.to_string()),
                ("C2-Tuning", format!("{} Hz", hz)),
            ]
        };

        for (r, (lbl, val)) in rows.iter().enumerate() {
            let ry = 28 + (r as i32) * 12;
            draw_oem_text(frame, dw, dh, 8, ry, lbl, col_black);
            draw_oem_text(frame, dw, dh, 104, ry, ":", col_black);
            draw_oem_text_right(frame, dw, dh, dw as i32 - 10, ry, val, col_black);
        }

        // 3. Mittlere Zeile: Links vollständiges `M_LABEL.bmp` (`MOD 4 WIN`, 94x20 bei 8, 128),
        //    Rechts versenkter Rahmen (126, 126, 68, 22) mit `bominus`, 3-stelligem `LCD_08X13` und `boplus`.
        self.assets
            .m_label
            .blit_rect_to(0, 0, 94, 20, frame, dw, dh, 8, 128);

        draw_3d_panel(frame, dw, dh, 126, 126, 68, 22, true, col_gray);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            128,
            130,
            self.pressed_btn == Some(PressedButton::InstMinus),
            &self.assets.bominus,
        );
        fill_rect(frame, dw, dh, 143, 128, 32, 18, col_black);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            147,
            131,
            idx as u32,
            3,
            false,
            &self.assets.lcd_08x13,
            8,
            13,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            177,
            130,
            self.pressed_btn == Some(PressedButton::InstPlus),
            &self.assets.boplus,
        );

        // 4. Untere Zeile: Versenkter grauer 3D-Rahmen (`DETAILS.PAS:751`: `6, CapDY+140, w-14, Fh+4`) mit zentriertem Instrumentennamen
        draw_3d_panel(frame, dw, dh, 6, 156, dw.saturating_sub(12), 16, true, col_gray);
        let disp_name: String = inst.name.trim().chars().take(22).collect();
        draw_oem_text_centered(
            frame,
            dw,
            dh,
            8,
            158,
            dw.saturating_sub(16),
            &disp_name,
            col_black,
        );
    }

    /// Liefert die (ggf. nach Name oder Erweiterung sortierte) Liste `(orig_idx, display_name)` für die Quick Selection (`MULBOX.PAS`).
    pub fn sorted_quick_select_items(&self, state: &UiSharedState) -> Vec<(usize, String)> {
        let mut items: Vec<(usize, String)> = state
            .playlist
            .iter()
            .enumerate()
            .map(|(idx, p)| {
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("module.mod")
                    .to_lowercase();
                (idx, name)
            })
            .collect();

        match self.quick_select_sort {
            1 => items.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))),
            2 => items.sort_by(|a, b| {
                let ext_a = a.1.rsplit('.').next().unwrap_or("");
                let ext_b = b.1.rsplit('.').next().unwrap_or("");
                ext_a
                    .cmp(ext_b)
                    .then_with(|| a.1.cmp(&b.1))
                    .then_with(|| a.0.cmp(&b.0))
            }),
            _ => {}
        }
        items
    }

    /// Rendert die rahmenlose Playlist-Schnellauswahl (`tviewwin` in `MULBOX.PAS`: `capdy = 0`, dynamische Höhe).
    pub(crate) fn render_quick_select_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];

        // 1. Äußerer 1px schwarzer Rahmen + 2px erhabener 3D-Hauptrahmen (`MULBOX.PAS:201`)
        fill_rect(frame, dw, dh, 0, 0, dw, dh, col_black);
        draw_3d_panel(
            frame,
            dw,
            dh,
            1,
            1,
            dw.saturating_sub(2),
            dh.saturating_sub(2),
            false,
            col_gray,
        );

        // 2. Schwarzer Listenbereich (`7, 7, dw-14, dh-41`) mit 2px versenktem 3D-Rahmen (`MULBOX.PAS:202`)
        let lb_h = dh.saturating_sub(41).max(15);
        let vis_rows = ((lb_h.saturating_sub(2)) / 13).max(1) as usize;
        draw_3d_panel(
            frame,
            dw,
            dh,
            5,
            5,
            dw.saturating_sub(10),
            lb_h + 4,
            true,
            col_black,
        );

        let total_items = state.playlist.len();
        let max_scroll = total_items.saturating_sub(vis_rows);
        let scroll = self.quick_select_scroll.min(max_scroll);

        if self.quick_select_sort == 0 {
            for row in 0..vis_rows {
                let orig_idx = scroll + row;
                let Some(p) = state.playlist.get(orig_idx) else {
                    break;
                };
                let ry = 8 + (row as i32) * 13;
                let is_cursor =
                    self.quick_select_all_selected || orig_idx == self.quick_select_cursor;
                if is_cursor {
                    fill_rect(frame, dw, dh, 15, ry, dw.saturating_sub(30), 13, col_green);
                }
                let text_col = if is_cursor { col_black } else { col_green };
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("module.mod")
                    .to_lowercase();
                let line = format!("{:4}:{}", orig_idx + 1, name);
                let disp: String = line.chars().take(17).collect();
                draw_oem_text(frame, dw, dh, 15, ry + 1, &disp, text_col);
            }
        } else {
            let items = self.sorted_quick_select_items(state);
            for (row, (orig_idx, name)) in items.iter().skip(scroll).take(vis_rows).enumerate() {
                let ry = 8 + (row as i32) * 13;
                let is_cursor =
                    self.quick_select_all_selected || *orig_idx == self.quick_select_cursor;
                if is_cursor {
                    fill_rect(frame, dw, dh, 15, ry, dw.saturating_sub(30), 13, col_green);
                }
                let text_col = if is_cursor { col_black } else { col_green };
                let mut disp_name = name.clone();
                if let Some(first) = disp_name.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                let line = format!("{:4}:{}", orig_idx + 1, disp_name);
                let disp: String = line.chars().take(17).collect();
                draw_oem_text(frame, dw, dh, 15, ry + 1, &disp, text_col);
            }
        }

        // 3. Drei Sortier-Buttons unten bündig nebeneinander (`b_order`, `b_name`, `b_ext`, je 47x21)
        //    in einem 1px versenkten 3D-Rahmen (`MULBOX.PAS:158-160, 204`)
        let by = dh as i32 - 28;
        let bx = dw as i32 - 149;
        draw_3d_frame_only(frame, dw, dh, bx - 1, by - 1, 143, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            bx,
            by,
            self.quick_select_sort == 0,
            &self.assets.b_order,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            bx + 47,
            by,
            self.quick_select_sort == 1,
            &self.assets.b_name,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            bx + 94,
            by,
            self.quick_select_sort == 2,
            &self.assets.b_ext,
        );
    }

    pub(crate) fn handle_module_info_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        if vy <= 18 {
            if vx <= 20 || vx >= (INFO_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::ModuleInfo);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        // [-]/[+] Buttons (Info_selbtns: (10..24, 60..73) und (208..221, 60..73))
        if (58..=74).contains(&vy) {
            if (8..=26).contains(&vx) {
                self.pressed_btn = Some(PressedButton::InfoMinus);
                self.info_inst_offset = self.info_inst_offset.saturating_sub(31);
                return;
            } else if (206..=224).contains(&vx) {
                self.pressed_btn = Some(PressedButton::InfoPlus);
                let total = self.shared_state.lock().unwrap().instruments.len();
                if self.info_inst_offset + 31 < total {
                    self.info_inst_offset += 31;
                }
                return;
            }
        }

        // Klick auf ein Instrument in der linken Spalte (1..15)
        if (12..=220).contains(&vx) && (76..=285).contains(&vy) {
            let row = ((vy - 76) / 14) as usize;
            if row < 15 {
                self.selected_instrument = self.info_inst_offset + row + 1;
                self.open_window(event_loop, WindowKind::InstrumentInfo);
                return;
            }
        }

        // Klick auf ein Instrument in der rechten Spalte (16..31)
        if (232..=440).contains(&vx) && (62..=285).contains(&vy) {
            let row = ((vy - 62) / 14) as usize;
            if row < 16 {
                self.selected_instrument = self.info_inst_offset + 16 + row;
                self.open_window(event_loop, WindowKind::InstrumentInfo);
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn handle_instrument_click(&mut self, vx: i32, vy: i32, window: &Arc<Window>) {
        if vy <= 18 {
            if vx <= 20 || vx >= (INST_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::InstrumentInfo);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        // [-] Button bei (128..142, 128..146) und [+] Button bei (177..192, 128..146)
        if (126..=148).contains(&vy) {
            if (126..=144).contains(&vx) {
                self.pressed_btn = Some(PressedButton::InstMinus);
                self.selected_instrument = self.selected_instrument.saturating_sub(1).max(1);
                return;
            } else if (176..=194).contains(&vx) {
                self.pressed_btn = Some(PressedButton::InstPlus);
                let max_inst = self.shared_state.lock().unwrap().instruments.len().max(1);
                self.selected_instrument = (self.selected_instrument + 1).min(max_inst);
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn handle_quick_select_click(
        &mut self,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        let (dh, items, cur_pl_idx, classic_input) = {
            let st = self.shared_state.lock().unwrap();
            let (_, dh, _) = self.window_dimensions(WindowKind::QuickSelect, &st);
            let items = self.sorted_quick_select_items(&st);
            (dh, items, st.playlist_idx, st.classic_input)
        };
        let lb_h = dh.saturating_sub(41).max(15);
        let vis_rows = ((lb_h.saturating_sub(2)) / 13).max(1) as usize;
        let max_scroll = items.len().saturating_sub(vis_rows);
        self.quick_select_scroll = self.quick_select_scroll.min(max_scroll);

        // Sortierbuttons unten (`by = dh - 28`, je 47x21 bei x = 17..63, 64..110, 111..158)
        let by = dh as i32 - 28;
        if (by..=(by + 21)).contains(&vy) {
            if (17..=63).contains(&vx) {
                self.quick_select_sort = 0;
            } else if (64..=110).contains(&vx) {
                self.quick_select_sort = 1;
            } else if (111..=158).contains(&vx) {
                self.quick_select_sort = 2;
            }
            let new_items = {
                let st = self.shared_state.lock().unwrap();
                self.sorted_quick_select_items(&st)
            };
            if let Some(pos) = new_items
                .iter()
                .position(|(orig_idx, _)| *orig_idx == cur_pl_idx)
                && (pos < self.quick_select_scroll || pos >= self.quick_select_scroll + vis_rows) {
                    self.quick_select_scroll = pos
                        .saturating_sub(vis_rows / 2)
                        .min(new_items.len().saturating_sub(vis_rows));
                }
            return;
        }

        // Klick in die Dateiliste (`x = 7..158, y = 7..(7 + lb_h)`):
        if (7..=158).contains(&vx) && (7..=(7 + lb_h as i32)).contains(&vy) {
            let row = ((vy - 8).max(0) / 13) as usize;
            let item_idx = self.quick_select_scroll + row;
            if let Some(&(orig_idx, _)) = items.get(item_idx) {
                self.quick_select_cursor = orig_idx;
                if classic_input || is_double_click {
                    let mut state = self.shared_state.lock().unwrap();
                    if orig_idx < state.playlist.len() {
                        state.playlist_idx = orig_idx;
                        state.playing_track_detached = false;
                        if let Some(pos) = state.number_list.iter().position(|&i| i == orig_idx) {
                            state.playlist_pos = pos;
                        }
                        let path = state.playlist[orig_idx].clone();
                        state.pending_load_file = Some(path);
                        state.is_playing = true;
                        state.is_paused = false;
                    }
                    drop(state);
                    self.close_window(WindowKind::QuickSelect);
                }
            }
            return;
        }

        let _ = window.drag_window();
    }
}
