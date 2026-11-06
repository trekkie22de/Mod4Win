//! Wiederverwendbare UI-Widgets (3D-Rahmen, Scrollbars, Buttons, LCD-Anzeigen,
//! abgerundete Rechtecke mit Farbverlauf & kompakter 5x7 UI-Font)
//! für die MOD4WIN 2.40 Beta Multi-Window-Oberfläche.

pub use super::font::{
    char_to_oem_code, draw_3d_frame_only, draw_3d_panel, draw_bmp_button_2state,
    draw_bmp_button_3state, draw_bmp_lcd_number, draw_bmp_text_16x20_centered,
    draw_bmp_text_16x20_marquee, draw_frame_3d, draw_frame_3d_1px, draw_hkonoff_led,
    draw_lrswitch, draw_modern_ui_text, draw_num_7x5_note, draw_num_7x5_text, draw_oem_char,
    draw_oem_text, draw_oem_text_centered, draw_oem_text_mnemonic, draw_oem_text_right,
    draw_pegel_stereo_meter, draw_rect_outline, draw_text, draw_up_down_btn, draw_vswitch,
    fill_rect, format_bmp_marquee_field, modern_ui_glyph_6x10, modern_ui_text_width,
};

use super::app::Mod4WinApp;

#[inline]
fn lerp_chan(a: u8, b: u8, num: u32, den: u32) -> u8 {
    if den == 0 {
        return a;
    }
    let va = a as i32;
    let vb = b as i32;
    (va + ((vb - va) * num as i32) / den as i32).clamp(0, 255) as u8
}

/// Berechnet den horizontalen Scanline-Inset an den abgerundeten Ecken für Zeilenabstand `dist_y` vom Rand.
#[inline]
pub fn rounded_corner_inset(dist_y: u32, radius: u32) -> i32 {
    if radius == 0 || dist_y >= radius {
        return 0;
    }
    match radius {
        1 => 1,
        2 => match dist_y {
            0 => 2,
            _ => 1,
        },
        3 => match dist_y {
            0 => 3,
            1 => 1,
            _ => 1,
        },
        4 => match dist_y {
            0 => 4,
            1 => 2,
            2 => 1,
            _ => 1,
        },
        5 => match dist_y {
            0 => 5,
            1 => 3,
            2 => 2,
            3 => 1,
            _ => 1,
        },
        r => {
            let dy = (r as f32) - 0.5 - (dist_y as f32);
            let r_f = r as f32;
            let dx = (r_f * r_f - dy * dy).max(0.0).sqrt();
            ((r_f - dx).round() as i32).max(0)
        }
    }
}

/// Füllt ein Rechteck mit abgerundeten Ecken (`border-radius`) und vertikalem Farbverlauf (`color_top` -> `color_bottom`).
#[allow(clippy::too_many_arguments)]
pub fn fill_rounded_rect_grad(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: u32,
    rh: u32,
    radius: u32,
    color_top: [u8; 4],
    color_bottom: [u8; 4],
) {
    if rw == 0 || rh == 0 {
        return;
    }
    let eff_r = radius.min(rw / 2).min(rh / 2);
    let den = rh.saturating_sub(1).max(1);
    for dy in 0..rh {
        let dist_y = dy.min(rh - 1 - dy);
        let inset = rounded_corner_inset(dist_y, eff_r);
        let span_w = (rw as i32 - inset * 2).max(0) as u32;
        if span_w > 0 {
            let c = [
                lerp_chan(color_top[0], color_bottom[0], dy, den),
                lerp_chan(color_top[1], color_bottom[1], dy, den),
                lerp_chan(color_top[2], color_bottom[2], dy, den),
                lerp_chan(color_top[3], color_bottom[3], dy, den),
            ];
            fill_rect(buf, w, h, x + inset, y + dy as i32, span_w, 1, c);
        }
    }
}

/// Zeichnet eine geschlossene 1px-Umrisslinie (`border`) mit abgerundeten Ecken (`border-radius`).
#[allow(clippy::too_many_arguments)]
pub fn stroke_rounded_rect(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: u32,
    rh: u32,
    radius: u32,
    border_color: [u8; 4],
) {
    if rw == 0 || rh == 0 {
        return;
    }
    let eff_r = radius.min(rw / 2).min(rh / 2);
    for dy in 0..rh {
        let dist_y = dy.min(rh - 1 - dy);
        let inset = rounded_corner_inset(dist_y, eff_r);
        let py = y + dy as i32;
        if dy == 0 || dy == rh - 1 {
            let span_w = (rw as i32 - inset * 2).max(0) as u32;
            if span_w > 0 {
                fill_rect(buf, w, h, x + inset, py, span_w, 1, border_color);
            }
        } else {
            let prev_dist = dist_y.saturating_sub(1);
            let prev_inset = if dist_y < eff_r {
                rounded_corner_inset(prev_dist, eff_r)
            } else {
                inset
            };
            let stroke_w = (prev_inset - inset).max(1) as u32;
            fill_rect(buf, w, h, x + inset, py, stroke_w, 1, border_color);
            fill_rect(
                buf,
                w,
                h,
                x + (rw as i32) - inset - (stroke_w as i32),
                py,
                stroke_w,
                1,
                border_color,
            );
        }
    }
}

impl Mod4WinApp {
    /// Hilfsfunktion zum Zeichnen einer klassischen Windows-3.1/95-Scrollbar (`14px` breit) mit Pfeilen und proportionalem Thumb.
    pub(crate) fn draw_vertical_scrollbar(
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        scroll: usize,
        visible_rows: usize,
        total_rows: usize,
    ) {
        let btn_h = 14u32.min(h / 2);
        // Schiene
        fill_rect(frame, dw, dh, x, y, w, h, [160, 160, 160, 255]);
        // Oberer Pfeil-Button
        draw_3d_panel(frame, dw, dh, x, y, w, btn_h, false, [192, 192, 192, 255]);
        draw_text(frame, dw, dh, x + 4, y + 3, "^", [0, 0, 0, 255]);
        // Unterer Pfeil-Button
        let bot_y = y + h as i32 - btn_h as i32;
        draw_3d_panel(frame, dw, dh, x, bot_y, w, btn_h, false, [192, 192, 192, 255]);
        draw_text(frame, dw, dh, x + 4, bot_y + 3, "v", [0, 0, 0, 255]);

        // Scroll-Thumb auf der Schiene
        let track_y = y + btn_h as i32;
        let track_h = (h.saturating_sub(btn_h * 2)).max(8);
        if total_rows > visible_rows && track_h > 12 {
            let max_scroll = total_rows.saturating_sub(visible_rows).max(1);
            let thumb_h = ((track_h as usize * visible_rows) / total_rows.max(1))
                .clamp(12, track_h as usize) as u32;
            let avail_h = track_h.saturating_sub(thumb_h);
            let clamped_scroll = scroll.min(max_scroll);
            let thumb_y = track_y + ((avail_h as usize * clamped_scroll) / max_scroll) as i32;
            draw_3d_panel(frame, dw, dh, x, thumb_y, w, thumb_h, false, [192, 192, 192, 255]);
        } else {
            draw_3d_panel(
                frame,
                dw,
                dh,
                x,
                track_y,
                w,
                14.min(track_h),
                false,
                [192, 192, 192, 255],
            );
        }
    }
}

