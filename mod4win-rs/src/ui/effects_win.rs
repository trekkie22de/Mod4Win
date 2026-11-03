//! Dynamisches Effekt-Analyse-Fenster (`ana.pas` / `bm12.bmp`) und Effekt-Hilfe (`bm13.bmp`).

use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{Mod4WinApp, UiSharedState, WindowKind};
use super::widgets::{
    draw_frame_3d_1px, draw_hkonoff_led, draw_num_7x5_note, draw_num_7x5_text,
    draw_pegel_stereo_meter, draw_rect_outline, fill_rect,
};

/// Liefert das 8 Pixel hohe (`rows 0..=7`) Bitmap-Glyph `(Breite, [u8; 8])` für die
/// rechtsbündigen Zahlen (`Speed`, `BPM`, `Note`, `Time`) im Effekt-Fenster, exakt passend
/// zur 8px-Versalhöhe (`dy + 1 ..= dy + 8`) und Baseline (`dy + 8`) von `EFFCOMM.bmp`.
#[inline]
fn eff_stat_glyph_8px(ch: char) -> (u8, [u8; 8]) {
    match ch {
        '0' => (5, [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
        '1' => (5, [0b00100, 0b01100, 0b10100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
        '2' => (5, [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
        '3' => (5, [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b00001, 0b10001, 0b01110]),
        '4' => (5, [0b00010, 0b00110, 0b01010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010]),
        '5' => (5, [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
        '6' => (5, [0b01110, 0b10001, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110]),
        '7' => (5, [0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b01000, 0b01000]),
        '8' => (5, [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110]),
        '9' => (5, [0b01110, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b10001, 0b01110]),
        ':' => (3, [0b000, 0b000, 0b010, 0b000, 0b000, 0b000, 0b000, 0b010]),
        '-' => (5, [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000, 0b00000]),
        _ => (5, [0; 8]),
    }
}

/// Zeichnet einen Zahlen-/Zeit-String rechtsbündig an `right_x` (inklusiver rechter Rand des letzten Zeichens)
/// mit der 8px hohen Schrift passend zu `EFFCOMM.bmp` (`Speed:`, `BPM  :`, `Note :`, `Time :`).
fn draw_eff_stat_text_right(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    right_x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut total_w = 0i32;
    for (idx, ch) in trimmed.chars().enumerate() {
        if idx > 0 {
            total_w += 1;
        }
        let (gw, _) = eff_stat_glyph_8px(ch);
        total_w += gw as i32;
    }
    let mut cur_x = right_x - total_w + 1;
    let w_i = dw as i32;
    let h_i = dh as i32;

    for ch in trimmed.chars() {
        let (gw, rows) = eff_stat_glyph_8px(ch);
        for (ry, &mask) in rows.iter().enumerate() {
            let py = y + ry as i32;
            if py < 0 || py >= h_i || mask == 0 {
                continue;
            }
            for rx in 0..(gw as i32) {
                let px = cur_x + rx;
                if px < 0 || px >= w_i {
                    continue;
                }
                let bit = (gw as i32) - 1 - rx;
                if (mask & (1 << bit)) != 0 {
                    let idx = ((py * w_i + px) * 4) as usize;
                    dst[idx..idx + 4].copy_from_slice(&color);
                }
            }
        }
        cur_x += (gw as i32) + 1;
    }
}

/// Konvertiert eine Tracker-Notennummer in einen 3-Zeichen-String (z.B. "C-4", "F#5", "---").
pub fn note_to_str(note: u8) -> &'static str {
    if note == 0 || note > 108 {
        return "---";
    }
    let n = (note - 1) as usize;
    let semitone = n % 12;
    let octave = n / 12;

    match (semitone, octave) {
        (0, 0) => "C-0", (1, 0) => "C#0", (2, 0) => "D-0", (3, 0) => "D#0", (4, 0) => "E-0",
        (5, 0) => "F-0", (6, 0) => "F#0", (7, 0) => "G-0", (8, 0) => "G#0", (9, 0) => "A-0",
        (10, 0) => "A#0", (11, 0) => "B-0",
        (0, 1) => "C-1", (1, 1) => "C#1", (2, 1) => "D-1", (3, 1) => "D#1", (4, 1) => "E-1",
        (5, 1) => "F-1", (6, 1) => "F#1", (7, 1) => "G-1", (8, 1) => "G#1", (9, 1) => "A-1",
        (10, 1) => "A#1", (11, 1) => "B-1",
        (0, 2) => "C-2", (1, 2) => "C#2", (2, 2) => "D-2", (3, 2) => "D#2", (4, 2) => "E-2",
        (5, 2) => "F-2", (6, 2) => "F#2", (7, 2) => "G-2", (8, 2) => "G#2", (9, 2) => "A-2",
        (10, 2) => "A#2", (11, 2) => "B-2",
        (0, 3) => "C-3", (1, 3) => "C#3", (2, 3) => "D-3", (3, 3) => "D#3", (4, 3) => "E-3",
        (5, 3) => "F-3", (6, 3) => "F#3", (7, 3) => "G-3", (8, 3) => "G#3", (9, 3) => "A-3",
        (10, 3) => "A#3", (11, 3) => "B-3",
        (0, 4) => "C-4", (1, 4) => "C#4", (2, 4) => "D-4", (3, 4) => "D#4", (4, 4) => "E-4",
        (5, 4) => "F-4", (6, 4) => "F#4", (7, 4) => "G-4", (8, 4) => "G#4", (9, 4) => "A-4",
        (10, 4) => "A#4", (11, 4) => "B-4",
        (0, 5) => "C-5", (1, 5) => "C#5", (2, 5) => "D-5", (3, 5) => "D#5", (4, 5) => "E-5",
        (5, 5) => "F-5", (6, 5) => "F#5", (7, 5) => "G-5", (8, 5) => "G#5", (9, 5) => "A-5",
        (10, 5) => "A#5", (11, 5) => "B-5",
        (0, 6) => "C-6", (1, 6) => "C#6", (2, 6) => "D-6", (3, 6) => "D#6", (4, 6) => "E-6",
        (5, 6) => "F-6", (6, 6) => "F#6", (7, 6) => "G-6", (8, 6) => "G#6", (9, 6) => "A-6",
        (10, 6) => "A#6", (11, 6) => "B-6",
        (0, 7) => "C-7", (1, 7) => "C#7", (2, 7) => "D-7", (3, 7) => "D#7", (4, 7) => "E-7",
        (5, 7) => "F-7", (6, 7) => "F#7", (7, 7) => "G-7", (8, 7) => "G#7", (9, 7) => "A-7",
        (10, 7) => "A#7", (11, 7) => "B-7",
        _ => "---",
    }
}

/// Berechnet die Effekt-Spalten-ID (`0..=67`) aus Effektbefehl `effkt` und Operand `opnd` nach `ana.pas`.
pub fn effect_id_from_cmd_op(cmd: u8, op: u8) -> Option<u8> {
    if cmd == 0xFF || (cmd == 0 && op == 0) {
        return None;
    }
    let id = match cmd {
        14 => 16 + (op >> 4),
        0..=67 => cmd,
        _ => return None,
    };
    if id <= 67 && id != 14 {
        Some(id)
    } else {
        None
    }
}

/// Prüft für Legacy-Aufrufe, ob ein Effekt in Spalte `col` aktiv ist.
pub fn check_effect_active(eff: u16, col: usize) -> bool {
    let cmd = (eff >> 8) as u8;
    let op = (eff & 0xFF) as u8;
    effect_id_from_cmd_op(cmd, op) == Some(col as u8)
}

impl Mod4WinApp {
    /// Baut das Effekt-Fenster (`bm12` / `ana.pas`) prozedural exakt wie im Pascal-Code auf.
    pub(crate) fn render_effects_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let active_cols = state.active_effect_columns();
        let num_ch = state.num_channels.max(4).min(32);
        let w = dw as i32;
        let col_grey = [192, 192, 192, 255];
        let col_white = [255, 255, 255, 255];
        let col_dark = [128, 128, 128, 255];
        let col_black = [0, 0, 0, 255];

        // 1. Äußerer 1px schwarzer Fensterrahmen (`ws_border`) + 2px erhabener 3D-Rahmen (`framefilled3d(..., 2, cForeGnd, up)`)
        fill_rect(frame, dw, dh, 0, 0, dw, dh, col_grey);
        draw_rect_outline(frame, dw, dh, 0, 0, dw, dh, col_black);
        if dw >= 6 && dh >= 6 {
            fill_rect(frame, dw, dh, 1, 1, dw - 2, 2, col_white);
            fill_rect(frame, dw, dh, 1, 1, 2, dh - 2, col_white);
            fill_rect(frame, dw, dh, 1, dh as i32 - 3, dw - 2, 2, col_dark);
            fill_rect(frame, dw, dh, dw as i32 - 3, 1, 2, dh - 2, col_dark);
            fill_rect(frame, dw, dh, 1, dh as i32 - 2, 1, 1, col_white);
            fill_rect(frame, dw, dh, dw as i32 - 2, 1, 1, 1, col_white);
        }

        // Alle Client-Koordinaten aus `ana.pas` liegen innerhalb des 1px `ws_border` um (+1, +1) versetzt:
        let box_h = (num_ch as u32 * 9) + 4;
        let eff_box_w = dw.saturating_sub(120);

        // 2. Obere Sektion: Sprites blitten + 1px vertiefte 3D-Rahmen (`frame3d(..., 1, down)`)
        // Oben links: "Effects" vertikal (Client 6, 6, 19, 55 -> Window 7, 7, 19, 55)
        self.assets
            .effbmp
            .blit_rect_to(0, 0, 14, 52, frame, dw, dh, 8, 9);
        draw_frame_3d_1px(frame, dw, dh, 7, 7, 19, 55, true);

        // Darunter: "Chn" Header (Client 6, 64, 19, 11 -> Window 7, 65, 19, 11)
        self.assets
            .effcomm
            .blit_rect_to(64, 1, 17, 9, frame, dw, dh, 8, 66);
        draw_frame_3d_1px(frame, dw, dh, 7, 65, 19, 11, true);

        // Effekt-Spaltenköpfe (Client 27, 6, dw - 120, 69 -> Window 28, 7, dw - 120, 69)
        for (col_idx, &eff_id) in active_cols.iter().enumerate() {
            let sx = 14 + (eff_id as u32) * 14;
            let dx = 30 + (col_idx as i32) * 14;
            self.assets
                .effbmp
                .blit_rect_to(sx, 0, 14, 65, frame, dw, dh, dx, 9);
        }
        draw_frame_3d_1px(frame, dw, dh, 28, 7, eff_box_w, 69, true);

        // Rechts oben: Speed, BPM, Note (Row), Time (Client w - 91, 6, 83, 55 -> Window w - 90, 7, 83, 55)
        let right_x = w - 90;
        let lbl_x = w - 87;
        self.assets
            .effcomm
            .blit_rect_to(0, 0, 32, 10, frame, dw, dh, lbl_x, 10); // Speed
        self.assets
            .effcomm
            .blit_rect_to(32, 0, 32, 10, frame, dw, dh, lbl_x, 23); // BPM
        self.assets
            .effcomm
            .blit_rect_to(198, 0, 32, 10, frame, dw, dh, lbl_x, 36); // Note
        self.assets
            .effcomm
            .blit_rect_to(165, 0, 32, 10, frame, dw, dh, lbl_x, 49); // Time
        draw_frame_3d_1px(frame, dw, dh, right_x, 7, 83, 55, true);

        // Rechtsbündige Zahlen auf exakt derselben 8px-Versalhöhe und Baseline wie `EFFCOMM.bmp` (`y = 11, 24, 37, 50`)
        let val_right_x = w - 13;
        draw_eff_stat_text_right(frame, dw, dh, val_right_x, 11, &format!("{}", state.speed), col_black);
        draw_eff_stat_text_right(frame, dw, dh, val_right_x, 24, &format!("{}", state.bpm), col_black);
        draw_eff_stat_text_right(frame, dw, dh, val_right_x, 37, &format!("{}", state.curr_row), col_black);

        let mins = (state.elapsed_secs / 60).min(99);
        let secs = state.elapsed_secs % 60;
        draw_eff_stat_text_right(
            frame,
            dw,
            dh,
            val_right_x,
            50,
            &format!("{}:{:02}", mins, secs),
            col_black,
        );

        // Rechts darunter: Header "Note  L Vol R  Instr." (Client w - 91, 64, 83, 11 -> Window w - 90, 65, 83, 11)
        self.assets
            .effcomm
            .blit_rect_to(81, 1, 81, 9, frame, dw, dh, w - 89, 66);
        draw_frame_3d_1px(frame, dw, dh, right_x, 65, 83, 11, true);

        // 3. 1px vertiefte 3D-Rahmen für die 5 Kanal-Spalten auf grauem Grund (`frame3d(..., 1, down)`, KEINE schwarzen Ränder!)
        draw_frame_3d_1px(frame, dw, dh, 7, 78, 19, box_h, true);
        draw_frame_3d_1px(frame, dw, dh, 28, 78, eff_box_w, box_h, true);
        draw_frame_3d_1px(frame, dw, dh, right_x, 78, 19, box_h, true);
        draw_frame_3d_1px(frame, dw, dh, w - 69, 78, 41, box_h, true);
        draw_frame_3d_1px(frame, dw, dh, w - 26, 78, 19, box_h, true);

        // 4. Kanalzeilen i = 0..num_ch zeichnen
        for i in 0..num_ch {
            let led_y = 80 + (i as i32) * 9;
            let txt_y = 81 + (i as i32) * 9;
            let muted = state.channel_muted[i];

            // Kanalnummer bei (9, txt_y): "---" wenn stumm, sonst rechtsbündig mit '/' gepaddet
            let ch_str = if muted {
                ":::".to_string()
            } else {
                let n = i + 1;
                if n < 10 {
                    format!("//{}", n)
                } else {
                    format!("/{}", n)
                }
            };
            draw_num_7x5_text(frame, dw, dh, 9, txt_y, &ch_str, &self.assets.num_7x5);

            // Effekt-LEDs (HKONOFF.bmp 14x9)
            let eff_word = state.channel_effects[i];
            let cmd = (eff_word >> 8) as u8;
            let op = (eff_word & 0xFF) as u8;
            let active_eff_id = if muted {
                None
            } else {
                effect_id_from_cmd_op(cmd, op)
            };

            for (col_idx, &col_eff_id) in active_cols.iter().enumerate() {
                let led_x = 30 + (col_idx as i32) * 14;
                let is_lit = active_eff_id == Some(col_eff_id);
                draw_hkonoff_led(frame, dw, dh, led_x, led_y, is_lit, &self.assets.hkonoff);
            }

            // Gespielte Note bei (w - 88, txt_y)
            let note_val = if muted { 0 } else { state.channel_notes[i] };
            draw_num_7x5_note(frame, dw, dh, w - 88, txt_y, note_val, &self.assets.num_7x5);

            // Horizontale L/R-Lautstärke/Panning-Anzeige (PEGEL.bmp) bei (w - 68, led_y)
            let (vol_l, vol_r) = if muted { (0, 0) } else { state.channel_vols[i] };
            draw_pegel_stereo_meter(frame, dw, dh, w - 68, led_y, vol_l, vol_r, &self.assets.pegel);

            // Instrumentennummer bei (w - 24, txt_y)
            let inst_val = if muted { 0 } else { state.channel_insts[i] };
            let inst_str = if inst_val == 0 {
                ":::".to_string()
            } else if inst_val < 10 {
                format!("//{}", inst_val)
            } else if inst_val < 100 {
                format!("/{}", inst_val)
            } else {
                format!("{}", inst_val)
            };
            draw_num_7x5_text(frame, dw, dh, w - 24, txt_y, &inst_str, &self.assets.num_7x5);
        }
    }

    pub(crate) fn handle_effects_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        let (num_ch, dey) = {
            let state = self.shared_state.lock().unwrap();
            (
                state.num_channels.max(4).min(32),
                state.active_effect_columns().len() as i32,
            )
        };
        let eff_end_x = 29 + dey * 14;
        let ch_end_y = 80 + (num_ch as i32) * 9;

        // Klick auf die vertikalen Effekt-Spaltenköpfe (eff_effon) -> Effekt-Erklärungsfenster (bm13)
        if (29..eff_end_x).contains(&vx) && (8..=73).contains(&vy) {
            self.open_window(event_loop, WindowKind::EffectsHelp);
            return;
        }

        // Klick auf "Chn" Header (8..23, 65..75): Alle Kanäle anschalten
        if (8..=23).contains(&vx) && (65..=75).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.channel_muted = [false; 32];
            state.channel_muted_dirty = true;
            return;
        }

        // Klick auf Kanalnummer (eff_chn: 8..23, 80..ch_end_y): Kanal stumm/an
        if (8..=23).contains(&vx) && (80..ch_end_y).contains(&vy) {
            let ch_idx = ((vy - 80) / 9) as usize;
            if ch_idx < 32 {
                let mut state = self.shared_state.lock().unwrap();
                state.channel_muted[ch_idx] = !state.channel_muted[ch_idx];
                state.channel_muted_dirty = true;
            }
            return;
        }

        let _ = window.drag_window();
    }
}
