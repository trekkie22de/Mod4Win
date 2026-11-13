//! Vollständige prozedurale Implementierung des 4. Skins **`Modern`** (`SkinMode::Modern`)
//! exakt nach der Referenz-Spezifikation in `mockup.html`.

use std::path::PathBuf;
use std::sync::Arc;
use super::winit::event::MouseButton;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{
    ConfirmAction, Mod4WinApp, PlaylistReorderDrag, PressedButton, SetupEditFocus, UiSharedState,
    WindowKind,
};
#[cfg(any(not(target_os = "windows"), feature = "retro-win32"))]
use super::app::FileSelectTarget;
use super::cpu_sim::CpuProfile;
use super::effects_win::{effect_id_from_cmd_op, effect_name_by_id};
use super::ini_config::{Mod4WinIniConfig, DEV_HDD, DEV_SOUNDCARD};
use super::theme_modern::SkinMode;
use super::widgets::{
    draw_modern_ui_text, draw_rect_outline, fill_rect, fill_rounded_rect_grad,
    modern_ui_text_width, rounded_corner_inset, stroke_rounded_rect,
};

pub const MODERN_MAIN_W: u32 = 580;
pub const MODERN_MAIN_H: u32 = 148;
pub const MODERN_MINI_W: u32 = 325;
pub const MODERN_MINI_H: u32 = 64;
pub const MODERN_INFO_W: u32 = 580;
pub const MODERN_INFO_H: u32 = 406;
pub const MODERN_PLAYLIST_W: u32 = 580;
pub const MODERN_PLAYLIST_H: u32 = 225;
pub const MODERN_SETUP_W: u32 = 530;
pub const MODERN_SETUP_H: u32 = 505;
pub const MODERN_ABOUT_W: u32 = 360;
pub const MODERN_ABOUT_H: u32 = 185;
pub const MODERN_SNAP_THRESHOLD: i32 = 12;

// Farbkonstanten aus `mockup.html`
const COL_CHASSIS_MID: [u8; 4] = [0x26, 0x30, 0x3d, 255];
const COL_CHASSIS_DARK: [u8; 4] = [0x1b, 0x22, 0x2c, 255];
const COL_CHASSIS_BORDER: [u8; 4] = [0x4a, 0x5c, 0x73, 255];
const COL_CYAN_BRAND: [u8; 4] = [0x00, 0xe5, 0xff, 255];
const COL_LCD_BG: [u8; 4] = [0x02, 0x0d, 0x06, 255];
const COL_LCD_BORDER: [u8; 4] = [0x11, 0x3d, 0x22, 255];
const COL_LCD_DIM: [u8; 4] = [0x09, 0x30, 0x16, 255];
const COL_LCD_DIAG: [u8; 4] = [0x0d, 0x4a, 0x1f, 255];
const COL_LCD_BRIGHT: [u8; 4] = [0x00, 0xff, 0x00, 255];
const COL_LCD_AMBER: [u8; 4] = [0xff, 0xea, 0x00, 255];
const COL_LCD_RED: [u8; 4] = [0xff, 0x2a, 0x00, 255];
const COL_TEXT_UI: [u8; 4] = [0xd0, 0xdc, 0xe8, 255];
const COL_TEXT_MUTED: [u8; 4] = [0x84, 0x96, 0xab, 255];
const COL_TITLE_TEXT: [u8; 4] = [0xa9, 0xbf, 0xd6, 255];

#[inline]
fn put_px(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, c: [u8; 4]) {
    if x >= 0 && y >= 0 && (x as u32) < dw && (y as u32) < dh {
        let idx = ((y as u32 * dw + x as u32) * 4) as usize;
        if idx + 3 < frame.len() {
            frame[idx] = c[0];
            frame[idx + 1] = c[1];
            frame[idx + 2] = c[2];
            frame[idx + 3] = c[3];
        }
    }
}

#[inline]
fn lerp_u8(a: u8, b: u8, num: u32, den: u32) -> u8 {
    if den == 0 {
        return a;
    }
    let va = a as i32;
    let vb = b as i32;
    (va + ((vb - va) * num as i32) / den as i32).clamp(0, 255) as u8
}

/// Zeichnet das Modern-Skin-Fensterchassis (`m4w-win`, `radius = 5px`) inkl. `24px` Titelleiste und optionalem `MOD 4 WIN` Brand-Pill.
pub fn draw_modern_window_shell(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    show_brand_pill: bool,
    title: &str,
    is_main: bool,
    is_mini: bool,
) {
    // Dunkler Eck-Hintergrund außerhalb des 5px-Radius
    fill_rect(frame, dw, dh, 0, 0, dw, dh, [0x0d, 0x11, 0x17, 255]);

    // 1. Vertikaler Chassis-Verlauf (#26303d -> #1b222c) mit radius = 5px
    fill_rounded_rect_grad(
        frame,
        dw,
        dh,
        0,
        0,
        dw,
        dh,
        5,
        COL_CHASSIS_MID,
        COL_CHASSIS_DARK,
    );

    // 2. Titelleiste (y = 1..24): Horizontaler Verlauf (#161d26 -> #283546 -> #161d26) unter Beachtung des oberen 5px-Radius
    let title_h = 23u32.min(dh.saturating_sub(1));
    let half_w = (dw / 2).max(1);
    for dy in 1..=title_h {
        let inset = rounded_corner_inset(dy, 5).max(1) as u32;
        for col in inset..dw.saturating_sub(inset) {
            let (r, g, b) = if col <= half_w {
                (
                    lerp_u8(0x16, 0x28, col, half_w),
                    lerp_u8(0x1d, 0x35, col, half_w),
                    lerp_u8(0x26, 0x46, col, half_w),
                )
            } else {
                let rem = col - half_w;
                (
                    lerp_u8(0x28, 0x16, rem, half_w),
                    lerp_u8(0x35, 0x1d, rem, half_w),
                    lerp_u8(0x46, 0x26, rem, half_w),
                )
            };
            put_px(frame, dw, dh, col as i32, dy as i32, [r, g, b, 255]);
        }
    }

    // Außen-Border (#4a5c73, radius = 5px) + heller 1px Inner-Highlight oben
    stroke_rounded_rect(frame, dw, dh, 0, 0, dw, dh, 5, COL_CHASSIS_BORDER);
    if dw > 10 {
        fill_rect(frame, dw, dh, 5, 1, dw - 10, 1, [0x60, 0x75, 0x8f, 255]);
    }
    // Untere Trennlinie der Titelleiste (y = 23)
    if dh > 24 && dw > 2 {
        fill_rect(frame, dw, dh, 1, 23, dw.saturating_sub(2), 1, [0x10, 0x15, 0x1c, 255]);
    }

    let mut title_x = 8i32;
    if show_brand_pill {
        // Türkises Brand-Badge `MOD 4 WIN` (radius = 2px, #00e5ff Text auf #08232c, Rahmen #00a2b8)
        draw_modern_brand_pill(frame, dw, dh, false);
        title_x = 76;
    }
    draw_modern_ui_text(frame, dw, dh, title_x, 9, title, COL_TITLE_TEXT);

    let w_i = dw as i32;
    if is_main && !is_mini {
        // Rechts in der Hauptfenster-Titelleiste: _ | □ | × (ohne Mixer/Setup/Help)
        draw_modern_win_ctrl_btn(frame, dw, dh, w_i - 61, 4, '_');
        draw_modern_win_ctrl_btn(frame, dw, dh, w_i - 42, 4, '#');
        draw_modern_win_ctrl_btn(frame, dw, dh, w_i - 23, 4, 'X');
    } else if is_main && is_mini {
        draw_modern_win_ctrl_btn(frame, dw, dh, w_i - 23, 4, '#');
    } else {
        draw_modern_win_ctrl_btn(frame, dw, dh, w_i - 23, 4, 'X');
    }
}

fn draw_modern_brand_pill(frame: &mut [u8], dw: u32, dh: u32, highlight: bool) {
    let (top_col, bot_col, border_col, text_col) = if highlight {
        (
            [0x14, 0x52, 0x66, 255],
            [0x0a, 0x32, 0x40, 255],
            [0x40, 0xf0, 0xff, 255],
            [0xb0, 0xfb, 0xff, 255],
        )
    } else {
        (
            [0x0a, 0x2c, 0x38, 255],
            [0x06, 0x1c, 0x24, 255],
            [0x00, 0xa2, 0xb8, 255],
            COL_CYAN_BRAND,
        )
    };
    fill_rounded_rect_grad(frame, dw, dh, 6, 4, 64, 16, 2, top_col, bot_col);
    stroke_rounded_rect(frame, dw, dh, 6, 4, 64, 16, 2, border_col);
    let bw = modern_ui_text_width("MOD 4 WIN");
    let bx = 6 + ((64 - bw) / 2).max(3);
    draw_modern_ui_text(frame, dw, dh, bx, 9, "MOD 4 WIN", text_col);
}

fn draw_modern_win_ctrl_btn(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, kind: char) {
    fill_rounded_rect_grad(
        frame,
        dw,
        dh,
        x,
        y,
        16,
        16,
        2,
        [0x2c, 0x3b, 0x4c, 255],
        [0x20, 0x2b, 0x38, 255],
    );
    stroke_rounded_rect(frame, dw, dh, x, y, 16, 16, 2, [0x45, 0x58, 0x70, 255]);
    let fg = [0xb8, 0xcd, 0xe3, 255];
    match kind {
        '_' => {
            fill_rect(frame, dw, dh, x + 4, y + 11, 8, 2, fg);
        }
        '#' => {
            draw_rect_outline(frame, dw, dh, x + 4, y + 4, 8, 8, fg);
            fill_rect(frame, dw, dh, x + 4, y + 4, 8, 2, fg);
        }
        _ => {
            // 'X'
            for i in 0..7 {
                put_px(frame, dw, dh, x + 4 + i, y + 4 + i, fg);
                put_px(frame, dw, dh, x + 5 + i, y + 4 + i, fg);
                put_px(frame, dw, dh, x + 10 - i, y + 4 + i, fg);
                put_px(frame, dw, dh, x + 9 - i, y + 4 + i, fg);
            }
        }
    }
}

/// Zeichnet eine LCD-Box (`lcd-box`, `radius = 3px`, `#020d06` Hintergrund, `#113d22` Rahmen, dunkler Inset-Schatten).
pub fn draw_modern_lcd_box(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, w: u32, h: u32) {
    fill_rounded_rect_grad(frame, dw, dh, x, y, w, h, 3, COL_LCD_BG, COL_LCD_BG);
    stroke_rounded_rect(frame, dw, dh, x, y, w, h, 3, COL_LCD_BORDER);
    if w > 6 && h > 6 {
        fill_rect(frame, dw, dh, x + 3, y + 1, w - 6, 1, [0x01, 0x05, 0x02, 255]);
        fill_rect(frame, dw, dh, x + 1, y + 3, 1, h - 6, [0x01, 0x05, 0x02, 255]);
    }
}

/// Zeichnet einen Hardware-Button (`hw-btn` / `sf-btn`, `radius = 2px` aus `mockup.html`) mit kompaktem 5x7 Font.
pub fn draw_modern_hw_btn(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    label: &str,
    active_on: bool,
) {
    if active_on {
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            x,
            y,
            w,
            h,
            2,
            [0x10, 0x3d, 0x22, 255],
            [0x07, 0x21, 0x12, 255],
        );
        stroke_rounded_rect(frame, dw, dh, x, y, w, h, 2, COL_LCD_BRIGHT);
    } else {
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            x,
            y,
            w,
            h,
            2,
            [0x33, 0x42, 0x54, 255],
            [0x1e, 0x27, 0x33, 255],
        );
        stroke_rounded_rect(frame, dw, dh, x, y, w, h, 2, [0x4c, 0x60, 0x78, 255]);
        if w > 4 {
            fill_rect(frame, dw, dh, x + 2, y + 1, w - 4, 1, [0x46, 0x59, 0x70, 255]);
        }
    }
    let fg = if active_on {
        COL_LCD_BRIGHT
    } else {
        [0xc7, 0xd8, 0xeb, 255]
    };
    let tw = modern_ui_text_width(label);
    let tx = x + ((w as i32 - tw) / 2).max(3);
    let ty = y + ((h as i32 - 7) / 2).max(2);
    draw_modern_ui_text(frame, dw, dh, tx, ty, label, fg);
}

/// Exakte `FONT_5X7`-Glyphentabelle aus `mockup.html` (`renderMod4WinMatrix20`).
fn font_5x7_glyph(ch: char) -> [&'static str; 7] {
    match ch.to_ascii_uppercase() {
        'A' | 'Ä' => ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
        'B' => ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
        'C' => ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
        'D' => ["11100", "10010", "10001", "10001", "10001", "10010", "11100"],
        'E' => ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
        'F' => ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
        'G' => ["01110", "10001", "10000", "10111", "10001", "10001", "01110"],
        'H' => ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
        'I' => ["01110", "00100", "00100", "00100", "00100", "00100", "01110"],
        'J' => ["00111", "00010", "00010", "00010", "10010", "10010", "01100"],
        'K' => ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
        'L' => ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
        'M' => ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
        'N' => ["10001", "11001", "10101", "10011", "10001", "10001", "10001"],
        'O' | 'Ö' => ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
        'P' => ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
        'Q' => ["01110", "10001", "10001", "10001", "10101", "10010", "01101"],
        'R' => ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
        'S' | 'ß' => ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
        'T' => ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
        'U' | 'Ü' => ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
        'V' => ["10001", "10001", "10001", "10001", "10001", "01010", "00100"],
        'W' => ["10001", "10001", "10001", "10101", "10101", "11011", "10001"],
        'X' => ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
        'Y' => ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
        'Z' => ["11111", "00001", "00010", "00100", "01000", "10000", "11111"],
        '!' => ["00100", "00100", "00100", "00100", "00100", "00000", "00100"],
        '0' => ["01110", "10011", "10101", "10101", "10101", "11001", "01110"],
        '1' => ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
        '2' => ["01110", "10001", "00001", "00110", "01000", "10000", "11111"],
        '3' => ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
        '4' => ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
        '5' => ["11111", "10000", "10000", "11110", "00001", "00001", "11110"],
        '6' => ["01110", "10000", "10000", "11110", "10001", "10001", "01110"],
        '7' => ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
        '8' => ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
        '9' => ["01110", "10001", "10001", "01111", "00001", "00001", "01110"],
        '-' | '—' | '–' => ["00000", "00000", "00000", "01110", "00000", "00000", "00000"],
        '.' => ["00000", "00000", "00000", "00000", "00000", "01100", "01100"],
        ',' => ["00000", "00000", "00000", "00000", "01100", "01100", "01000"],
        ';' => ["00000", "01100", "01100", "00000", "01100", "01100", "01000"],
        ':' => ["00000", "01100", "01100", "00000", "01100", "01100", "00000"],
        '\'' | '`' | '´' | '’' | '‘' => ["00100", "00100", "01000", "00000", "00000", "00000", "00000"],
        '"' | '“' | '”' | '„' => ["01010", "01010", "01010", "00000", "00000", "00000", "00000"],
        '?' => ["01110", "10001", "00001", "00110", "00100", "00000", "00100"],
        '&' => ["01100", "10010", "10100", "01000", "10101", "10010", "01101"],
        '#' => ["01010", "11111", "01010", "01010", "11111", "01010", "00000"],
        '$' => ["00100", "01111", "10100", "01110", "00101", "11110", "00100"],
        '%' => ["11001", "11010", "00100", "00100", "01011", "10011", "00000"],
        '*' => ["00000", "00100", "10101", "01110", "10101", "00100", "00000"],
        '=' => ["00000", "00000", "11111", "00000", "11111", "00000", "00000"],
        '<' | '«' => ["00010", "00100", "01000", "10000", "01000", "00100", "00010"],
        '>' | '»' => ["01000", "00100", "00010", "00001", "00010", "00100", "01000"],
        '/' => ["00001", "00010", "00100", "00100", "01000", "10000", "00000"],
        '\\' => ["10000", "01000", "00100", "00100", "00010", "00001", "00000"],
        '|' => ["00100", "00100", "00100", "00100", "00100", "00100", "00100"],
        '@' => ["01110", "10001", "10111", "10101", "10111", "10000", "01110"],
        '^' => ["00100", "01010", "10001", "00000", "00000", "00000", "00000"],
        '~' => ["00000", "00000", "01101", "10010", "00000", "00000", "00000"],
        '_' => ["00000", "00000", "00000", "00000", "00000", "00000", "11111"],
        '+' => ["00000", "00100", "00100", "11111", "00100", "00100", "00000"],
        '(' => ["00110", "01000", "01000", "01000", "01000", "01000", "00110"],
        ')' => ["01100", "00010", "00010", "00010", "00010", "00010", "01100"],
        '[' => ["01110", "01000", "01000", "01000", "01000", "01000", "01110"],
        ']' => ["01110", "00010", "00010", "00010", "00010", "00010", "01110"],
        '{' => ["00011", "00100", "00100", "01000", "00100", "00100", "00011"],
        '}' => ["11000", "00100", "00100", "00010", "00100", "00100", "11000"],
        _ => ["00000", "00000", "00000", "00000", "00000", "00000", "00000"],
    }
}

/// Rendert das ikonische **20-Zeichen (`119 × 7` Dots)** Dot-Matrix-Banner aus `mockup.html` (`renderMod4WinMatrix20`):
/// - Exakt `3px` Zellengröße (`119 * 3 = 357px` Breite, `7 * 3 - 1 = 20px` visuelle Punkthöhe) im Hauptfenster.
/// - Exakt vertikal zentriert im LCD-Rahmen (`start_y = box_y + (box_h - visual_h) / 2`), sodass oben und unten exakt gleich viel Abstand zum Rahmen bleibt.
/// - Gesetzter Dot (`1`): `2 × 2 px` grünes Quadrat (`#00ff00`).
/// - Inaktiver Dot (`0` sowie jede 6. Trennspalte): `2px` Diagonal-Strich (`\` von `(x, y)` nach `(x+1, y+1)`) in `#0d4a1f`!
pub fn render_mod4win_matrix20(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
    text: &str,
) {
    render_mod4win_matrix_custom(frame, dw, dh, box_x, box_y, box_w, box_h, text, 20, 0);
}

/// Rendert das 20-Zeichen Dot-Matrix-Banner inklusive originalgetreuem `BMPFont`-Marquee-Scrolling (`winunit.pas`).
#[allow(clippy::too_many_arguments)]
pub fn render_mod4win_matrix20_marquee(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
    text: &str,
    elapsed_ms: u64,
) {
    render_mod4win_matrix_custom(frame, dw, dh, box_x, box_y, box_w, box_h, text, 20, elapsed_ms);
}

/// Rendert das Dot-Matrix-Banner über die volle Innenbreite (`~555px`, 31 Zeichen / `185 × 7` Dots) im `580px` breiten Info-Fenster.
pub fn render_mod4win_matrix_info(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
    text: &str,
) {
    render_mod4win_matrix_custom(frame, dw, dh, box_x, box_y, box_w, box_h, text, 31, 0);
}

/// Rendert das 31-Zeichen Dot-Matrix-Banner im Info-Fenster inklusive originalgetreuem `BMPFont`-Marquee-Scrolling (`winunit.pas`).
#[allow(clippy::too_many_arguments)]
pub fn render_mod4win_matrix_info_marquee(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
    text: &str,
    elapsed_ms: u64,
) {
    render_mod4win_matrix_custom(frame, dw, dh, box_x, box_y, box_w, box_h, text, 31, elapsed_ms);
}

#[allow(clippy::too_many_arguments)]
fn render_mod4win_matrix_custom(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
    text: &str,
    total_chars: usize,
    elapsed_ms: u64,
) {
    draw_modern_lcd_box(frame, dw, dh, box_x, box_y, box_w, box_h);

    let total_cols = total_chars * 6 - 1;
    let cell_x: i32 = if box_w >= (total_cols as u32) * 3 { 3 } else { 2 };
    let cell_y: i32 = 3;
    let dot_w = if cell_x >= 3 { 2 } else { 1 };
    let dot_h = 2i32;

    // Visuelle Ausdehnung vom ersten gesetzten Pixel bis zum letzten gesetzten Pixel (ohne den nachfolgenden Zell-Gap)
    let visual_w = (total_cols as i32) * cell_x - (cell_x - dot_w);
    let visual_h = 7 * cell_y - (cell_y - dot_h); // 7 * 3 - 1 = 20px
    let start_x = box_x + ((box_w as i32 - visual_w) / 2).max(2);
    let start_y = box_y + ((box_h as i32 - visual_h) / 2).max(2);

    // Zentriert kürzere Texte auf `total_chars` Zeichen bzw. scrollt überlange Texte nach `BMPFont.TimerScroll` (`winunit.pas`)
    let formatted = super::widgets::format_bmp_marquee_field(text, total_chars, elapsed_ms);
    let mut fixed: Vec<char> = formatted.chars().collect();
    fixed.resize(total_chars, ' ');

    for col in 0..total_cols {
        let char_idx = col / 6;
        let sub_col = col % 6;
        let ch = fixed[char_idx];
        let glyph = font_5x7_glyph(ch);

        for row in 0..7usize {
            let is_lit = sub_col < 5 && glyph[row].as_bytes()[sub_col] == b'1';
            let px = start_x + (col as i32) * cell_x;
            let py = start_y + (row as i32) * cell_y;

            if is_lit {
                fill_rect(frame, dw, dh, px, py, dot_w as u32, dot_h as u32, COL_LCD_BRIGHT);
            } else {
                // 2px Diagonal-Strich (\) von (x, y) nach (x+1, y+1) in #0d4a1f
                put_px(frame, dw, dh, px, py, COL_LCD_DIAG);
                put_px(frame, dw, dh, px + 1, py + 1, COL_LCD_DIAG);
            }
        }
    }
}

/// Zeichnet eine 7-Segment-Zahl mit dunklen Geister-Segmenten (`#093016`) und leuchtendem Wert (`#00ff00`).
/// Dezimalpunkte (`.`) nutzen einen kompakten `3px`-Vorschub wie im Originalskin, damit zwischen Ziffern und Punkt keine Lücke entsteht.
pub fn draw_modern_seg_value(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    ghost: &str,
    val: &str,
    large: bool,
) {
    let base_step = if large { 14 } else { 8 };
    let dot_step = if large { 5 } else { 3 };
    let colon_step = if large { 7 } else { 8 };
    let minus_step = if large { 12 } else { 8 };
    let mut cx = x;
    for (idx, gch) in ghost.chars().enumerate() {
        draw_seg_char(frame, dw, dh, cx, y, gch, COL_LCD_DIM, large);
        cx += if gch == '.' {
            dot_step
        } else if gch == ':' {
            colon_step
        } else if idx == 0 && gch == '-' {
            minus_step
        } else {
            base_step
        };
    }
    let mut vx = x;
    let g_chars: Vec<char> = ghost.chars().collect();
    for (i, vch) in val.chars().enumerate() {
        if vch != ' ' {
            draw_seg_char(frame, dw, dh, vx, y, vch, COL_LCD_BRIGHT, large);
        }
        let gch = g_chars.get(i).copied().unwrap_or(vch);
        vx += if vch == '.' || gch == '.' {
            dot_step
        } else if vch == ':' || gch == ':' {
            colon_step
        } else if i == 0 && (vch == '-' || gch == '-') {
            minus_step
        } else {
            base_step
        };
    }
}

/// Zeichnet eine 4-stellige Song-/Total-Anzeige im Modern Skin:
/// - `0..=9_999`: `"8888"` (4 Segmente)
/// - `10_000..=99_999`: `"88.88"` (`10.00`..`99.99` + `"K"` direkt dahinter)
/// - `100_000..`: `"888.8"` (`100.0`..`999.9` + `"K"` direkt dahinter)
fn draw_modern_song_count(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, value: u32) {
    if value < 10_000 {
        draw_modern_seg_value(frame, dw, dh, x, y, "8888", &format!("{:>4}", value), false);
    } else if value < 100_000 {
        let hundredths = value / 10; // 1000 ..= 9999
        let val_str = format!("{:02}.{:02}", hundredths / 100, hundredths % 100);
        draw_modern_seg_value(frame, dw, dh, x, y, "88.88", &val_str, false);
        draw_modern_ui_text(frame, dw, dh, x + 35, y + 3, "K", COL_LCD_BRIGHT);
    } else {
        let tenths = (value / 100).min(9999); // 1000 ..= 9999
        let val_str = format!("{:03}.{}", tenths / 10, tenths % 10);
        draw_modern_seg_value(frame, dw, dh, x, y, "888.8", &val_str, false);
        draw_modern_ui_text(frame, dw, dh, x + 35, y + 3, "K", COL_LCD_BRIGHT);
    }
}

fn draw_seg_char(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    ch: char,
    color: [u8; 4],
    large: bool,
) {
    let w = if large { 11 } else { 6 };
    let h = if large { 20 } else { 11 };
    let mid = if large { 9 } else { 5 };
    let t = if large { 2 } else { 1 };

    match ch {
        ':' => {
            if large {
                // Exakt symmetrisch zur Mittelachse (y + 9..11): Punkte bei y + 5..7 und y + 13..15
                fill_rect(frame, dw, dh, x + 1, y + 5, 2, 2, color);
                fill_rect(frame, dw, dh, x + 1, y + 13, 2, 2, color);
            } else {
                fill_rect(frame, dw, dh, x + w / 2 - 1, y + h / 3, t as u32, t as u32, color);
                fill_rect(frame, dw, dh, x + w / 2 - 1, y + (2 * h) / 3, t as u32, t as u32, color);
            }
            return;
        }
        '.' => {
            fill_rect(frame, dw, dh, x, y + h - 2, t as u32, 2, color);
            return;
        }
        '-' => {
            let mw = if large { (w - 3) as u32 } else { (w - 2) as u32 };
            fill_rect(frame, dw, dh, x + 1, y + mid, mw, t as u32, color);
            return;
        }
        '+' => {
            fill_rect(frame, dw, dh, x + 1, y + mid, (w - 2) as u32, t as u32, color);
            fill_rect(frame, dw, dh, x + w / 2, y + mid - 2, t as u32, 5, color);
            return;
        }
        _ => {}
    }

    // 7 Segmente: [a(top), b(top-right), c(bot-right), d(bot), e(bot-left), f(top-left), g(mid)]
    let segs: [bool; 7] = match ch {
        '0' => [true, true, true, true, true, true, false],
        '1' => [false, true, true, false, false, false, false],
        '2' => [true, true, false, true, true, false, true],
        '3' => [true, true, true, true, false, false, true],
        '4' => [false, true, true, false, false, true, true],
        '5' => [true, false, true, true, false, true, true],
        '6' => [true, false, true, true, true, true, true],
        '7' => [true, true, true, false, false, false, false],
        '8' => [true, true, true, true, true, true, true],
        '9' => [true, true, true, true, false, true, true],
        _ => return,
    };

    // Für large (w=11, h=20, mid=9, t=2):
    // - Oberes Horizontalsegment a: y + 0..2
    // - Mittleres Horizontalsegment g: y + 9..11 (exakt 7px Innenfenster oben: 2..9, 7px Innenfenster unten: 11..18!)
    // - Unteres Horizontalsegment d: y + 18..20
    // - Obere Vertikalsegmente b/f: y + 1..9 (Höhe 8px)
    // - Untere Vertikalsegmente c/e: y + 11..19 (Höhe 8px)
    let v_len = (mid - 1) as u32;
    let bot_v_y = y + mid + t;

    if segs[0] {
        fill_rect(frame, dw, dh, x + 1, y, (w - 2) as u32, t as u32, color);
    }
    if segs[1] {
        fill_rect(frame, dw, dh, x + w - t, y + 1, t as u32, v_len, color);
    }
    if segs[2] {
        fill_rect(frame, dw, dh, x + w - t, bot_v_y, t as u32, v_len, color);
    }
    if segs[3] {
        fill_rect(frame, dw, dh, x + 1, y + h - t, (w - 2) as u32, t as u32, color);
    }
    if segs[4] {
        fill_rect(frame, dw, dh, x, bot_v_y, t as u32, v_len, color);
    }
    if segs[5] {
        fill_rect(frame, dw, dh, x, y + 1, t as u32, v_len, color);
    }
    if segs[6] {
        fill_rect(frame, dw, dh, x + 1, y + mid, (w - 2) as u32, t as u32, color);
    }
}

/// Zeichnet ein gefülltes Dreieck für die grünen Transport-Vektorsymbole.
fn draw_triangle_h(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x_base: i32,
    x_tip: i32,
    y_top: i32,
    y_bot: i32,
    color: [u8; 4],
) {
    let y_mid = (y_top + y_bot) / 2;
    let half_h = (y_bot - y_top).max(2) / 2;
    for y in y_top..=y_bot {
        let dist = (y - y_mid).abs();
        let frac_num = (half_h - dist).max(0);
        let x_end = x_base + ((x_tip - x_base) * frac_num) / half_h;
        let (x0, x1) = if x_base <= x_end {
            (x_base, x_end)
        } else {
            (x_end, x_base)
        };
        fill_rect(frame, dw, dh, x0, y, (x1 - x0 + 1) as u32, 1, color);
    }
}

/// Zeichnet einen der 7 Transport-Buttons (`t7-btn`, `radius = 3px`) mit grünen Vektor-Symbolen (`#00d438`, aktiv `#00ff00`).
fn draw_transport_btn(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    idx: usize,
    active: bool,
) {
    if active {
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            x,
            y,
            w,
            h,
            3,
            [0x18, 0x22, 0x2e, 255],
            [0x2a, 0x38, 0x4a, 255],
        );
        stroke_rounded_rect(frame, dw, dh, x, y, w, h, 3, COL_LCD_BRIGHT);
    } else {
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            x,
            y,
            w,
            h,
            3,
            [0x39, 0x49, 0x5e, 255],
            [0x22, 0x2c, 0x3a, 255],
        );
        stroke_rounded_rect(frame, dw, dh, x, y, w, h, 3, [0x53, 0x68, 0x82, 255]);
        if w > 6 {
            fill_rect(frame, dw, dh, x + 3, y + 1, w - 6, 1, [0x50, 0x65, 0x7e, 255]);
        }
    }
    let ic = if active {
        COL_LCD_BRIGHT
    } else {
        [0x00, 0xd4, 0x38, 255]
    };
    let cx = x + (w as i32) / 2;
    let cy = y + (h as i32) / 2;

    match idx {
        0 => {
            // 1. Stop (■)
            fill_rect(frame, dw, dh, cx - 5, cy - 5, 10, 10, ic);
        }
        1 => {
            // 2. Play (►)
            draw_triangle_h(frame, dw, dh, cx - 4, cx + 5, cy - 5, cy + 5, ic);
        }
        2 => {
            // 3. Pause (||)
            fill_rect(frame, dw, dh, cx - 4, cy - 5, 3, 10, ic);
            fill_rect(frame, dw, dh, cx + 1, cy - 5, 3, 10, ic);
        }
        3 => {
            // 4. Prev Song (|◄◄)
            fill_rect(frame, dw, dh, cx - 8, cy - 5, 2, 10, ic);
            draw_triangle_h(frame, dw, dh, cx + 1, cx - 5, cy - 5, cy + 5, ic);
            draw_triangle_h(frame, dw, dh, cx + 8, cx + 2, cy - 5, cy + 5, ic);
        }
        4 => {
            // 5. Rewind Pattern (◄◄)
            draw_triangle_h(frame, dw, dh, cx, cx - 7, cy - 5, cy + 5, ic);
            draw_triangle_h(frame, dw, dh, cx + 7, cx, cy - 5, cy + 5, ic);
        }
        5 => {
            // 6. Fast Forward Pattern (►►)
            draw_triangle_h(frame, dw, dh, cx - 7, cx, cy - 5, cy + 5, ic);
            draw_triangle_h(frame, dw, dh, cx, cx + 7, cy - 5, cy + 5, ic);
        }
        _ => {
            // 7. Next Song (►►|)
            draw_triangle_h(frame, dw, dh, cx - 8, cx - 2, cy - 5, cy + 5, ic);
            draw_triangle_h(frame, dw, dh, cx - 1, cx + 5, cy - 5, cy + 5, ic);
            fill_rect(frame, dw, dh, cx + 6, cy - 5, 2, 10, ic);
        }
    }
}

/// Blittet das originale Pixel-Art-`M_ST`-Bitmap (27x11 aus `M_ST.bmp`: `0` = Mono, `1` = Stereo, `2` = Surround)
/// passgenau auf das dunkelgrüne Status-LCD (`COL_LCD_BG`).
fn blit_original_mst_bitmap(
    m_st: &super::bmp::BmpImage,
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    mst_idx: u32,
) {
    let sy = mst_idx.min(2) * 11;
    for row in 0..11u32 {
        for col in 0..27u32 {
            let src_x = col;
            let src_y = sy + row;
            if src_x < m_st.width && src_y < m_st.height {
                let s_idx = ((src_y * m_st.width + src_x) * 4) as usize;
                let r = m_st.data[s_idx];
                let g = m_st.data[s_idx + 1];
                let b = m_st.data[s_idx + 2];
                let px_col = if r == 0 && g == 0 && b == 0 {
                    COL_LCD_BG
                } else {
                    [r, g, b, 255]
                };
                put_px(frame, dw, dh, x + col as i32, y + row as i32, px_col);
            }
        }
    }
}

/// Blittet das originale Pixel-Art-`iDO`-Bitmap (26x17 aus `SH_REP_SCAN.bmp`, y=66, alle 4 Stufen:
/// `Off` abgedunkelt, `LIN`, `CPU`, `FPU`) passgenau auf das dunkelgrüne Status-LCD (`COL_LCD_BG`).
fn blit_original_ido_bitmap(
    sh_rep_scan: &super::bmp::BmpImage,
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    oversamp: u8,
) {
    let sx = (oversamp.min(3) as u32) * 26;
    let sy = 66u32;
    for row in 0..17u32 {
        for col in 0..26u32 {
            let src_x = sx + col;
            let src_y = sy + row;
            if src_x < sh_rep_scan.width && src_y < sh_rep_scan.height {
                let s_idx = ((src_y * sh_rep_scan.width + src_x) * 4) as usize;
                let r = sh_rep_scan.data[s_idx];
                let g = sh_rep_scan.data[s_idx + 1];
                let b = sh_rep_scan.data[s_idx + 2];
                let px_col = if r == 0 && g == 0 && b == 0 {
                    COL_LCD_BG
                } else {
                    [r, g, b, 255]
                };
                put_px(frame, dw, dh, x + col as i32, y + row as i32, px_col);
            }
        }
    }
}

impl Mod4WinApp {
    /// Gibt zurück, ob aktuell `SkinMode::Modern` aktiv ist.
    pub(crate) fn is_modern_skin(&self, state: &UiSharedState) -> bool {
        state.skin_mode == SkinMode::Modern
    }

    /// Wendet den aktuellen Skin (`2.40` oder `Modern`) sofort live auf ALLE aktuell geöffneten Fenster an:
    /// 1. Berechnet für jedes offene Fenster die neue Zielgröße `window_base_size(kind, new_skin) * gui_scale`.
    /// 2. Führt für jedes offene Fenster sofort `window.request_inner_size(...)`, Win32-Resize sowie
    ///    `pixels.resize_buffer(...)` und `pixels.resize_surface(...)` auf die neue Basis-Auflösung aus.
    /// 3. Rendert jedes offene Fenster sofort im neuen Skin (`render_window` + `pixels.render()` + `window.request_redraw()`).
    pub(crate) fn apply_skin_to_all_open_windows(&mut self) {
        let st_snapshot = self.shared_state.lock().unwrap().clone();
        let active_skin = st_snapshot.skin_mode;

        // Falls beim Wechsel auf Modern sowohl QuickSelect als auch OpenDialog (bzw. ModuleInfo + InstrumentInfo)
        // gleichzeitig offen waren, deduplizieren wir die im Modern-Skin kombinierten Fenster:
        if active_skin == SkinMode::Modern {
            if self.kind_to_id.contains_key(&WindowKind::ModuleInfo)
                && self.kind_to_id.contains_key(&WindowKind::InstrumentInfo)
            {
                self.close_window(WindowKind::InstrumentInfo);
            }
            if self.kind_to_id.contains_key(&WindowKind::QuickSelect)
                && self.kind_to_id.contains_key(&WindowKind::OpenDialog)
            {
                self.close_window(WindowKind::OpenDialog);
            }
        }

        let scale = if cfg!(any(feature = "retro-win32", target_arch = "x86")) {
            1.0
        } else {
            self.scale_factor.clamp(0.5, 4.0)
        };
        let win_ids: Vec<(super::winit::window::WindowId, WindowKind)> =
            self.windows.iter().map(|(&id, s)| (id, s.kind)).collect();

        for (id, kind) in win_ids {
            let (expected_w, expected_h, title) = self.window_dimensions(kind, &st_snapshot);
            let mut render_buf = vec![0u8; (expected_w * expected_h * 4) as usize];
            self.render_window(kind, &mut render_buf, expected_w, expected_h, &st_snapshot);

            if let Some(surf) = self.windows.get_mut(&id) {
                surf.window.set_title(&title);
                let phys_w = ((expected_w as f64) * scale).round().max(32.0) as u32;
                let phys_h = ((expected_h as f64) * scale).round().max(32.0) as u32;
                surf.width = expected_w;
                surf.height = expected_h;
                surf.needs_aspect_snap = false;

                surf.window.set_min_inner_size(Some(super::winit::dpi::LogicalSize::new(
                    (expected_w as f64) * 0.25,
                    (expected_h as f64) * 0.25,
                )));
                let _ = surf.pixels.resize_buffer(expected_w, expected_h);
                let _ = surf
                    .window
                    .request_inner_size(super::winit::dpi::PhysicalSize::new(phys_w, phys_h));
                super::app::force_win32_window_size(&surf.window, phys_w, phys_h);
                let _ = surf.pixels.resize_surface(phys_w, phys_h);

                {
                    let frame = surf.pixels.frame_mut();
                    let copy_len = frame.len().min(render_buf.len());
                    frame[..copy_len].copy_from_slice(&render_buf[..copy_len]);
                }
                let _ = surf.pixels.render();
                surf.window.request_redraw();
            }
        }
    }

    /// Aktualisiert sofort alle offenen Fenster nach einem Skin-Wechsel (Größe, Puffer, Titel, Redraw).
    pub(crate) fn refresh_all_windows_for_skin(&mut self) {
        self.apply_skin_to_all_open_windows();
    }

    /// Magnetisches Andocken (`SNAP_THRESHOLD = 12px`) an andere offene Fenster und Bildschirmkanten
    /// (aktiv in ALLEN Skins: `2.30`, `2.40` und `Modern`) sowie synchrones Mitbewegen angedockter
    /// Unterfenster (`Info`, `Effects`, `Playlist/Open`), wenn das Hauptfenster (`Main`) gezogen wird.
    pub(crate) fn snap_window_position(
        &mut self,
        moved_id: super::winit::window::WindowId,
        pos_x: i32,
        pos_y: i32,
    ) -> (i32, i32) {
        if self.suppress_move_snap {
            if let Some(surf) = self.windows.get_mut(&moved_id) {
                surf.last_outer_pos = Some((pos_x, pos_y));
                surf.last_raw_outer_pos = Some((pos_x, pos_y));
            }
            return (pos_x, pos_y);
        }

        let Some((moved_kind, old_pos, old_raw_pos, dw, dh, monitor_rect)) =
            self.windows.get(&moved_id).map(|s| {
                let sz = s.window.outer_size();
                let mon = s.window.current_monitor().map(|m| {
                    let mp = m.position();
                    let ms = m.size();
                    (mp.x, mp.y, ms.width as i32, ms.height as i32)
                });
                (
                    s.kind,
                    s.last_outer_pos,
                    s.last_raw_outer_pos.or(s.last_outer_pos),
                    sz.width as i32,
                    sz.height as i32,
                    mon,
                )
            })
        else {
            return (pos_x, pos_y);
        };

        // Wenn das Hauptfenster (`Main`) bewegt wird: Alle aktuell direkt oder transitiv an `Main`
        // angedockten Unterfenster (`Info`, `InstrumentInfo`, `Effects`, `QuickSelect`, `OpenDialog`) ermitteln.
        let mut docked_followers: Vec<(super::winit::window::WindowId, WindowKind, i32, i32)> = Vec::new();
        if moved_kind == WindowKind::Main
            && let Some((old_mx, old_my)) = old_pos {
                let mut cluster_rects: Vec<(i32, i32, i32, i32)> = vec![(old_mx, old_my, dw, dh)];
                let candidates: Vec<(super::winit::window::WindowId, WindowKind, i32, i32, i32, i32)> =
                    self.windows
                        .iter()
                        .filter_map(|(&oid, osurf)| {
                            if oid == moved_id {
                                return None;
                            }
                            let is_dockable = matches!(
                                osurf.kind,
                                WindowKind::ModuleInfo
                                    | WindowKind::InstrumentInfo
                                    | WindowKind::Effects
                                    | WindowKind::QuickSelect
                                    | WindowKind::OpenDialog
                            );
                            if !is_dockable {
                                return None;
                            }
                            let (ox, oy) = osurf
                                .last_outer_pos
                                .or_else(|| osurf.window.outer_position().ok().map(|p| (p.x, p.y)))?;
                            let osz = osurf.window.outer_size();
                            Some((oid, osurf.kind, ox, oy, osz.width as i32, osz.height as i32))
                        })
                        .collect();

                let dock_tol = 6;
                let mut added = true;
                while added {
                    added = false;
                    for &(cid, ckind, cx, cy, cw, ch) in &candidates {
                        if docked_followers.iter().any(|(id, _, _, _)| *id == cid) {
                            continue;
                        }
                        let touches_cluster = cluster_rects.iter().any(|&(rx, ry, rw, rh)| {
                            let touch_h = ((cx - (rx + rw)).abs() <= dock_tol
                                || ((cx + cw) - rx).abs() <= dock_tol)
                                && (cy < ry + rh + dock_tol && cy + ch > ry - dock_tol);
                            let touch_v = ((cy - (ry + rh)).abs() <= dock_tol
                                || ((cy + ch) - ry).abs() <= dock_tol)
                                && (cx < rx + rw + dock_tol && cx + cw > rx - dock_tol);
                            touch_h || touch_v
                        });
                        if touches_cluster {
                            docked_followers.push((cid, ckind, cx, cy));
                            cluster_rects.push((cx, cy, cw, ch));
                            added = true;
                        }
                    }
                }
            }

        let (raw_dx, raw_dy) = old_raw_pos
            .map(|(rx, ry)| (pos_x - rx, pos_y - ry))
            .unwrap_or((0, 0));
        let (old_sx, old_sy) = old_pos.unwrap_or((pos_x, pos_y));

        // Richtungsabhängiges Snapping mit Break-Away-Hysterese:
        // - Zieht man von einer bereits gedockten Kante aktiv weg, wird NIEMALS zurückgesnappt
        //   (löst das Festkleben im „unteren Eck-Dreieck“ zuverlässig auf).
        // - Nur bei Annäherung an eine Kante (`SNAP_IN = 8px`) oder beim rein orthogonalen Entlanggleiten (`HOLD = 4px`)
        //   an einer überlappenden Kante rastet die Position ein.
        const SNAP_IN: i32 = 8;
        const SNAP_HOLD: i32 = 4;
        let can_snap_axis = |cur: i32, target: i32, old_snapped: i32, raw_delta: i32| -> Option<i32> {
            let dist = (cur - target).abs();
            if dist == 0 {
                return Some(0);
            }
            let was_docked_here = (old_snapped - target).abs() <= 1;
            if was_docked_here {
                let pulling_away = (cur > target && raw_delta > 0) || (cur < target && raw_delta < 0);
                if !pulling_away && dist <= SNAP_HOLD {
                    Some(dist)
                } else {
                    None
                }
            } else {
                let approaching = (cur > target && raw_delta < 0) || (cur < target && raw_delta > 0);
                if approaching && dist <= SNAP_IN {
                    Some(dist)
                } else {
                    None
                }
            }
        };

        let mut best_x: Option<(i32, i32)> = None; // (dist, target_x)
        let mut best_y: Option<(i32, i32)> = None; // (dist, target_y)
        let mut consider_x = |target_x: i32| {
            if let Some(d) = can_snap_axis(pos_x, target_x, old_sx, raw_dx)
                && best_x.map_or(true, |(bd, _)| d < bd)
            {
                best_x = Some((d, target_x));
            }
        };
        let mut consider_y = |target_y: i32| {
            if let Some(d) = can_snap_axis(pos_y, target_y, old_sy, raw_dy)
                && best_y.map_or(true, |(bd, _)| d < bd)
            {
                best_y = Some((d, target_y));
            }
        };

        // 1. Snapping an Bildschirmrändern (links/oben/rechts/unten)
        if let Some((mx, my, mw, mh)) = monitor_rect {
            consider_x(mx);
            consider_x(mx + mw - dw);
            consider_y(my);
            consider_y(my + mh - dh);
        } else {
            consider_x(0);
            consider_y(0);
        }

        // 2. Snapping an alle anderen geöffneten Player-Fenster (nur bei echter Kanten-Überlappung!)
        for (&other_id, other_surf) in &self.windows {
            if other_id == moved_id
                || docked_followers.iter().any(|(fid, _, _, _)| *fid == other_id)
            {
                continue;
            }
            if let Ok(opos) = other_surf.window.outer_position() {
                let osize = other_surf.window.outer_size();
                let ox = opos.x;
                let oy = opos.y;
                let ow = osize.width as i32;
                let oh = osize.height as i32;

                let vert_overlap = pos_y < oy + oh && pos_y + dh > oy;
                let horiz_overlap = pos_x < ox + ow && pos_x + dw > ox;

                if vert_overlap {
                    consider_x(ox + ow);
                    consider_x(ox - dw);
                    consider_x(ox);
                    consider_x(ox + ow - dw);
                }
                if horiz_overlap {
                    consider_y(oy + oh);
                    consider_y(oy - dh);
                    consider_y(oy);
                    consider_y(oy + oh - dh);
                }
            }
        }

        let nx = best_x.map(|(_, tx)| tx).unwrap_or(pos_x);
        let ny = best_y.map(|(_, ty)| ty).unwrap_or(pos_y);

        self.suppress_move_snap = true;
        if (nx != pos_x || ny != pos_y)
            && (nx - pos_x).abs() <= SNAP_IN
            && (ny - pos_y).abs() <= SNAP_IN
            && let Some(moved_surf) = self.windows.get(&moved_id) {
                moved_surf
                    .window
                    .set_outer_position(super::winit::dpi::PhysicalPosition::new(nx, ny));
            }
        if let Some(moved_surf) = self.windows.get_mut(&moved_id) {
            moved_surf.last_outer_pos = Some((nx, ny));
            moved_surf.last_raw_outer_pos = Some((pos_x, pos_y));
        }

        // 3. Wenn `Main` verschoben wurde: Alle angedockten Unterfenster synchron um `(dx, dy)` mitbewegen!
        if moved_kind == WindowKind::Main
            && let Some((old_mx, old_my)) = old_pos {
                let dx = nx - old_mx;
                let dy = ny - old_my;
                if dx != 0 || dy != 0 {
                    for (fid, fkind, fx, fy) in docked_followers {
                        let new_fx = fx + dx;
                        let new_fy = fy + dy;
                        if let Some(fsurf) = self.windows.get_mut(&fid) {
                            fsurf.last_outer_pos = Some((new_fx, new_fy));
                            fsurf.last_raw_outer_pos = Some((new_fx, new_fy));
                            fsurf
                                .window
                                .set_outer_position(super::winit::dpi::PhysicalPosition::new(new_fx, new_fy));
                        }
                        self.record_window_position(fkind, new_fx, new_fy);
                    }
                }
            }
        self.suppress_move_snap = false;

        (nx, ny)
    }

    // =========================================================================
    // A. HAUPTFENSTER ([1] Main Deck – 580 x 148 px)
    // =========================================================================
    pub(crate) fn is_brand_pill_highlighted(&self) -> bool {
        if self.is_window_open(WindowKind::HelpViewer) {
            return true;
        }
        self.kind_to_id
            .get(&WindowKind::Main)
            .and_then(|id| self.windows.get(id))
            .and_then(|surf| {
                surf.pixels
                    .window_pos_to_pixel((surf.mouse_pos.0 as f32, surf.mouse_pos.1 as f32))
                    .ok()
            })
            .map(|(px, py)| (6..=70).contains(&(px as i32)) && (4..=20).contains(&(py as i32)))
            .unwrap_or(false)
    }

    pub(crate) fn render_modern_main_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        draw_modern_window_shell(
            frame,
            dw,
            dh,
            true,
            "MOD4WIN 3.11",
            true,
            false,
        );
        if self.is_brand_pill_highlighted() {
            draw_modern_brand_pill(frame, dw, dh, true);
        }

        // Banner-Text (Peak-Search / Modultitel / Dateiname / Status-Banner)
        let peak_disp = state.active_peak_search_display();
        let banner_active = state.status_banner.as_ref().and_then(|(msg, ts)| {
            if ts.elapsed().as_secs_f32() < 2.2 {
                Some(msg.as_str())
            } else {
                None
            }
        });
        let display_name = if let Some((ref ps_text, _, _)) = peak_disp {
            ps_text.as_str()
        } else if let Some(banner_msg) = banner_active {
            banner_msg
        } else if state.show_filename && !state.filename.is_empty() {
            state.filename.as_str()
        } else if !state.title.trim().is_empty() {
            state.title.trim()
        } else if !state.filename.is_empty() {
            state.filename.as_str()
        } else {
            "GIANA SISTERS !"
        };

        // --- SPALTE 1 (LINKS, x = 6..378) ---
        // Zeile 1 (y = 30..58, 28px hoch): Volle 357x21 Dot-Matrix (119x7 * 3px) im 359x28 Rahmen + echter mechanischer Schiebeschalter (12x28)
        let scroll_ms = state.main_title_scroll_elapsed_ms();
        render_mod4win_matrix20_marquee(frame, dw, dh, 6, 30, 359, 28, display_name, scroll_ms);

        // Physisches Schalter-Gehäuse (`orig-title-switch`, exakt 28px hoch wie das Titel-LCD: x = 367, y = 30, w = 12, h = 28)
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            367,
            30,
            12,
            28,
            3,
            [0x2c, 0x39, 0x49, 255],
            [0x18, 0x20, 0x29, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 367, 30, 12, 28, 3, [0x4c, 0x60, 0x7a, 255]);
        // Vertiefter vertikaler Schacht im Gehäuse (x = 370, y = 33, w = 6, h = 22)
        fill_rect(frame, dw, dh, 370, 33, 6, 22, [0x09, 0x0d, 0x12, 255]);
        draw_rect_outline(frame, dw, dh, 370, 33, 6, 22, [0x1c, 0x25, 0x30, 255]);
        // Bewegliche Schalterkappe: Oben (y = 32) = Modultitel aktiv, Unten (y = 45) = Dateiname aktiv
        let (cap_y, led_col) = if state.show_filename {
            (45, COL_CYAN_BRAND)
        } else {
            (32, COL_LCD_BRIGHT)
        };
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            368,
            cap_y,
            10,
            11,
            2,
            [0x5c, 0x71, 0x8a, 255],
            [0x2d, 0x39, 0x47, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 368, cap_y, 10, 11, 2, [0x7c, 0x95, 0xb2, 255]);
        fill_rect(frame, dw, dh, 370, cap_y + 4, 6, 3, led_col);

        // Zeile 2 (y = 63..111, 48px hoch): 2 getrennte LCD-Boxen (Links: 6..188 = 182px, Rechts: 192..379 = 187px)
        draw_modern_lcd_box(frame, dw, dh, 6, 63, 182, 48);
        draw_modern_ui_text(frame, dw, dh, 21, 68, "MIN", COL_LCD_BRIGHT);
        draw_modern_ui_text(frame, dw, dh, 56, 68, "SEC", COL_LCD_BRIGHT);

        let disp_secs = if state.minus_mode && state.total_time_secs > 0 {
            state.total_time_secs.saturating_sub(state.elapsed_secs)
        } else {
            state.elapsed_secs
        };
        let mins = (disp_secs / 60).min(99);
        let secs = disp_secs % 60;
        let time_str = format!(
            "{}{:02}:{:02}",
            if state.minus_mode { '-' } else { ' ' },
            mins,
            secs
        );
        draw_modern_seg_value(frame, dw, dh, 10, 82, "-88:88", &time_str, true);

        // Rechts in Box 1: Song / Total (linksbündig mit der Anzeige darunter, ab 10.000 mit "K" hinter den 4 Ziffern)
        draw_modern_ui_text(frame, dw, dh, 91, 68, "Song", COL_LCD_BRIGHT);
        draw_modern_ui_text(frame, dw, dh, 145, 68, "Total", COL_LCD_BRIGHT);
        let curr_song = if state.playlist.is_empty() && state.current_file_path.is_none() {
            0
        } else {
            (state.playlist_idx + 1).max(1) as u32
        };
        let total_songs = if state.playlist.is_empty() && state.current_file_path.is_none() {
            0
        } else {
            state.playlist.len().max(1) as u32
        };
        draw_modern_song_count(frame, dw, dh, 91, 85, curr_song);
        let of_x = if curr_song >= 10_000 { 133 } else { 129 };
        draw_modern_ui_text(frame, dw, dh, of_x, 88, "of", COL_LCD_BRIGHT);
        draw_modern_song_count(frame, dw, dh, 145, 85, total_songs);

        // Rechtes LCD in Zeile 2 (x = 192, y = 63, w = 187, h = 48): Pattern + Shuffle/Repeat/Introscan + DeClick/Amiga/DeHiss
        let (disp_pos, disp_orders) = if let Some((_, scan_ord, max_ord)) = peak_disp {
            (scan_ord, max_ord)
        } else {
            (state.curr_pos, state.num_orders)
        };
        draw_modern_lcd_box(frame, dw, dh, 192, 63, 187, 48);
        draw_modern_ui_text(frame, dw, dh, 198, 68, "Pattern", COL_LCD_BRIGHT);
        draw_modern_seg_value(
            frame,
            dw,
            dh,
            198,
            85,
            "888",
            &format!("{:>3}", disp_pos.min(999)),
            false,
        );
        draw_modern_ui_text(frame, dw, dh, 225, 88, "of", COL_LCD_BRIGHT);
        draw_modern_seg_value(
            frame,
            dw,
            dh,
            240,
            85,
            "888",
            &format!("{:>3}", disp_orders.min(999)),
            false,
        );

        let shuf_col = if state.shuffle {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        let rep_col = if state.repeat_mode > 0 {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        let rep1_col = if state.repeat_mode == 2 {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        let scan_col = if state.intro_scan {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        draw_modern_ui_text(frame, dw, dh, 271, 68, "Shuffle", shuf_col);
        draw_modern_ui_text(frame, dw, dh, 271, 82, "Repeat", rep_col);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            271 + modern_ui_text_width("Repeat "),
            82,
            "1",
            rep1_col,
        );
        draw_modern_ui_text(frame, dw, dh, 271, 96, "Introscan", scan_col);

        // Neue 2. Spalte rechts neben Shuffle/Repeat/Introscan: DeClick | Amiga | DeHiss
        let declick_col = if state.sound_settings.declick() {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        let amiga_col = if state.sound_settings.amiga_filter() {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        let dehiss_col = if state.sound_settings.dehiss() {
            COL_LCD_BRIGHT
        } else {
            COL_LCD_DIM
        };
        draw_modern_ui_text(frame, dw, dh, 332, 68, "DeClick", declick_col);
        draw_modern_ui_text(frame, dw, dh, 332, 82, "Amiga", amiga_col);
        draw_modern_ui_text(frame, dw, dh, 332, 96, "DeHiss", dehiss_col);

        // Zeile 3 (y = 116..142, 26px hoch): 7er-Transportleiste (`transport-bar-7`, radius = 5px)
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            6,
            116,
            373,
            26,
            5,
            [0x12, 0x18, 0x20, 255],
            [0x12, 0x18, 0x20, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 6, 116, 373, 26, 5, [0x45, 0x57, 0x6e, 255]);
        let is_stopped = !state.is_playing && !state.is_paused;
        let is_play = state.is_playing && !state.is_paused;
        let is_pause = state.is_paused;
        for i in 0..7usize {
            let bx = 9 + (i as i32) * 53;
            let bw = if i == 6 { 49 } else { 50 };
            let active = (i == 0 && is_stopped) || (i == 1 && is_play) || (i == 2 && is_pause);
            draw_transport_btn(frame, dw, dh, bx, 118, bw, 22, i, active);
        }

        // --- SPALTE 2 (MITTE-RECHTS, x = 383..535, 152px breit) ---
        // Oben: Status-LCD (383, 30, 152, 66)
        draw_modern_lcd_box(frame, dw, dh, 383, 30, 152, 66);

        // Links im Status-LCD: 14-teilige vertikale CPU-Last-LED-Kette (gekoppelt an gedämpfte cpu_load 0%..100%)
        let cpu_pct_bar = state.cpu_telemetry.display_cpu_percent.min(100);
        let lit_cpu_segs = if cpu_pct_bar == 0 {
            0
        } else {
            (cpu_pct_bar * 14).div_ceil(100).clamp(1, 14)
        };
        for idx in 0..14u32 {
            let from_bottom = 13 - idx;
            let lit = from_bottom < lit_cpu_segs;
            let seg_col = if idx < 4 {
                if lit {
                    COL_LCD_RED
                } else {
                    [0x33, 0x08, 0x00, 255]
                }
            } else if idx < 9 {
                if lit {
                    COL_LCD_AMBER
                } else {
                    [0x33, 0x2c, 0x00, 255]
                }
            } else if lit {
                COL_LCD_BRIGHT
            } else {
                [0x05, 0x2b, 0x10, 255]
            };
            fill_rect(frame, dw, dh, 387, 34 + (idx as i32) * 4, 4, 3, seg_col);
        }

        // Zeile 1 im Status-LCD: 44 kHz | 16 Bit | Clip | ((OO)) mit sauberen Abständen
        let sr_khz = ((state.sound_settings.samplerate + 500) / 1000).clamp(4, 99) as u32;
        draw_modern_seg_value(frame, dw, dh, 395, 34, "88", &format!("{:>2}", sr_khz), false);
        draw_modern_ui_text(frame, dw, dh, 412, 37, "kHz", COL_LCD_BRIGHT);

        let bits = state.sound_settings.bits as u32;
        draw_modern_seg_value(frame, dw, dh, 434, 34, "88", &format!("{:>2}", bits), false);
        draw_modern_ui_text(frame, dw, dh, 451, 37, "Bit", COL_LCD_BRIGHT);

        let clip_on = (state.sound_settings.pre_amp & 0x80) != 0;
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            471,
            37,
            "Clip",
            if clip_on { COL_LCD_BRIGHT } else { COL_LCD_DIM },
        );

        let mst_idx = if state.sound_settings.stereo < 2 {
            0
        } else if state.sound_settings.surround {
            2
        } else {
            1
        };
        blit_original_mst_bitmap(&self.assets.m_st, frame, dw, dh, 498, 34, mst_idx);

        // Zeile 2 im Status-LCD: 04 Chn | BPM | PAN | Originales Pixel-Art-iDO-Logo
        draw_modern_seg_value(
            frame,
            dw,
            dh,
            395,
            55,
            "88",
            &format!("{:>2}", state.num_channels.min(99)),
            false,
        );
        draw_modern_ui_text(frame, dw, dh, 412, 58, "Chn", COL_LCD_BRIGHT);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            436,
            58,
            "BPM",
            if !state.sound_settings.no_bpm {
                COL_LCD_BRIGHT
            } else {
                COL_LCD_DIM
            },
        );
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            462,
            58,
            "PAN",
            if state.sound_settings.panning {
                COL_LCD_BRIGHT
            } else {
                COL_LCD_DIM
            },
        );
        let eff_oversamp = state.effective_oversamp();
        if eff_oversamp <= 3 {
            blit_original_ido_bitmap(
                &self.assets.sh_rep_scan,
                frame,
                dw,
                dh,
                498,
                51,
                eff_oversamp,
            );
        } else {
            super::main_win::draw_it_lcd_badge(
                frame,
                dw,
                dh,
                498,
                51,
                eff_oversamp,
                COL_LCD_BG,
            );
        }

        // Zeile 3 im Status-LCD: 0% CPU | 34.8s Buffer | Horizontales Stereo-VU-Meter (L / R)
        let cpu_pct = state.cpu_telemetry.display_cpu_percent.min(999);
        draw_modern_seg_value(
            frame,
            dw,
            dh,
            395,
            78,
            "888",
            &format!("{:>3}", cpu_pct),
            false,
        );
        draw_modern_ui_text(frame, dw, dh, 420, 81, "%", COL_LCD_BRIGHT);

        let (btime_sec, btime_tenth) = state.cpu_telemetry.buffer_time_digits();
        let buf_str = format!("{:>2}.{}", btime_sec.min(99), btime_tenth.min(9));
        draw_modern_seg_value(frame, dw, dh, 433, 78, "88.8", &buf_str, false);
        draw_modern_ui_text(frame, dw, dh, 461, 81, "s", COL_LCD_BRIGHT);

        // Horizontales Stereo-VU-Meter (L oben, R unten, je 11 LED-Segmente in Grün -> Gelb -> Rot)
        // Zeigt das tatsächlich gehörte Summensignal (inkl. Mono/Stereo/Surround, Panning & PreAmp)
        let (mut vu_l_val, mut vu_r_val) = if !state.is_playing || state.is_paused {
            (0u32, 0u32)
        } else if state.output_vu_lr != (0, 0) {
            (state.output_vu_lr.0 as u32, state.output_vu_lr.1 as u32)
        } else {
            // Fallback (z. B. für Offline-Tests ohne Live-Callback)
            let mut max_l = 0u32;
            let mut max_r = 0u32;
            let mut sum_l = 0u32;
            let mut sum_r = 0u32;
            for ch in 0..(state.num_channels.min(32)) {
                if !state.channel_muted[ch] {
                    let (vl, vr) = state.channel_vols[ch];
                    max_l = max_l.max(vl as u32);
                    max_r = max_r.max(vr as u32);
                    sum_l += vl as u32;
                    sum_r += vr as u32;
                }
            }
            ((max_l + sum_l / 6).min(64), (max_r + sum_r / 6).min(64))
        };
        // Bei Mono (`stereo < 2`) ist das Ausgangssignal auf beiden Kanälen identisch
        if state.sound_settings.stereo < 2 {
            let mono_val = (vu_l_val + vu_r_val).div_ceil(2);
            vu_l_val = mono_val;
            vu_r_val = mono_val;
        }
        let lit_vu_l = ((vu_l_val * 11 + 31) / 64).min(11);
        let lit_vu_r = ((vu_r_val * 11 + 31) / 64).min(11);

        // Mini-Labels 'L' (x=475, y=77, 3x5) und 'R' (x=475, y=84, 3x5)
        let lbl_col = [0x4d, 0xc4, 0x84, 255];
        fill_rect(frame, dw, dh, 475, 77, 1, 5, lbl_col);
        fill_rect(frame, dw, dh, 475, 81, 3, 1, lbl_col);
        fill_rect(frame, dw, dh, 475, 84, 1, 5, lbl_col);
        fill_rect(frame, dw, dh, 476, 84, 2, 1, lbl_col);
        put_px(frame, dw, dh, 477, 85, lbl_col);
        fill_rect(frame, dw, dh, 476, 86, 2, 1, lbl_col);
        put_px(frame, dw, dh, 477, 87, lbl_col);
        put_px(frame, dw, dh, 477, 88, lbl_col);

        for seg in 0..11u32 {
            let sx = 480 + (seg as i32) * 4;
            for (row_y, lit_cnt, val) in [(77i32, lit_vu_l, vu_l_val), (84i32, lit_vu_r, vu_r_val)] {
                let is_lit = seg < lit_cnt && val > 0;
                let col = if seg >= 9 {
                    if is_lit {
                        COL_LCD_RED
                    } else {
                        [0x33, 0x08, 0x00, 255]
                    }
                } else if seg >= 6 {
                    if is_lit {
                        COL_LCD_AMBER
                    } else {
                        [0x33, 0x2c, 0x00, 255]
                    }
                } else if is_lit {
                    COL_LCD_BRIGHT
                } else {
                    [0x05, 0x2b, 0x10, 255]
                };
                fill_rect(frame, dw, dh, sx, row_y, 3, 4, col);
            }
        }

        // Unten in Spalte 2: 2x4 Funktions-Buttons (383, 101, 152, 41)
        let btn_labels = [
            ("Shuffle", state.shuffle),
            ("Repeat", state.repeat_mode > 0),
            ("Scan", state.intro_scan),
            ("Time", state.minus_mode),
            (
                "List",
                self.is_window_open(WindowKind::QuickSelect)
                    || self.is_window_open(WindowKind::OpenDialog),
            ),
            ("Setup", self.is_window_open(WindowKind::SetupDialog)),
            (
                "Info",
                self.is_window_open(WindowKind::ModuleInfo)
                    || self.is_window_open(WindowKind::InstrumentInfo),
            ),
            ("Effects", self.is_window_open(WindowKind::Effects)),
        ];
        for (idx, (lbl, is_on)) in btn_labels.iter().enumerate() {
            let col = (idx % 4) as i32;
            let row = (idx / 4) as i32;
            let bx = 383 + col * 38;
            let by = 101 + row * 21;
            draw_modern_hw_btn(frame, dw, dh, bx, by, 37, 19, lbl, *is_on);
        }

        // --- SPALTE 3 (GANZ RECHTS, x = 539..575, 36px breit): VERTIKALER PREAMP-FADER ---
        draw_modern_lcd_box(frame, dw, dh, 539, 30, 35, 112);

        let raw_preamp = (state.sound_settings.pre_amp & 0x7F).min(40) as i32;
        let db_val = raw_preamp - 20; // -20 .. +20 dB
        let norm = ((20 - db_val) as f32 / 40.0).clamp(0.0, 1.0);
        let hw = (36 + ((32 * (20 - raw_preamp)) / 20)).clamp(0, 71);

        // Durchgehender vertikaler Farbverlauf-Balken wie im Original (`PREAMP.bmp`: Rot -> Orange/Gelb -> Grün um 0 dB -> Gelb/Orange -> Rot)
        let bar_x = 544i32;
        let bar_y = 34i32;
        let bar_w = 25u32;
        let bar_h = 72i32;
        draw_rect_outline(
            frame,
            dw,
            dh,
            bar_x - 1,
            bar_y - 1,
            bar_w + 2,
            (bar_h + 2) as u32,
            [0x0b, 0x26, 0x16, 255],
        );
        for ry in 0..bar_h {
            let dist = ((ry - 36).abs() as f32 / 36.0).clamp(0.0, 1.0);
            let (base_r, base_g, base_b) = if dist < 0.5 {
                let t = dist * 2.0; // Grün (#10e838) -> Gelb/Orange (#f5b810)
                (
                    (16.0 + (245.0 - 16.0) * t),
                    (232.0 + (184.0 - 232.0) * t),
                    (56.0 + (16.0 - 56.0) * t),
                )
            } else {
                let t = (dist - 0.5) * 2.0; // Gelb/Orange (#f5b810) -> Rot (#f02814)
                (
                    (245.0 + (240.0 - 245.0) * t),
                    (184.0 + (40.0 - 184.0) * t),
                    (16.0 + (20.0 - 16.0) * t),
                )
            };
            // Wie in DrawPreAmp: Bereich zwischen 0 dB (ry=36) und Fader-Stellung (hw) leuchtet voll, Rest dezent abgedunkelt
            let in_active_span = if hw <= 36 {
                ry >= hw && ry <= 36
            } else {
                ry >= 36 && ry <= hw
            };
            let mul = if ry % 3 == 2 {
                0.28
            } else if in_active_span {
                1.0
            } else {
                0.55
            };
            let col = [
                (base_r * mul).clamp(0.0, 255.0) as u8,
                (base_g * mul).clamp(0.0, 255.0) as u8,
                (base_b * mul).clamp(0.0, 255.0) as u8,
                255,
            ];
            fill_rect(frame, dw, dh, bar_x, bar_y + ry, bar_w, 1, col);
        }
        // 0-dB-Mittelmarkierung links und rechts am Farbbalken
        fill_rect(frame, dw, dh, bar_x - 2, bar_y + 36, 2, 1, COL_LCD_BRIGHT);
        fill_rect(
            frame,
            dw,
            dh,
            bar_x + bar_w as i32,
            bar_y + 36,
            2,
            1,
            COL_LCD_BRIGHT,
        );

        // Kontrastreich ausgestanzte Buchstaben P - R - E - A - M - P auf dem Farbbalken
        let pa_chars = ['P', 'R', 'E', 'A', 'M', 'P'];
        for (i, ch) in pa_chars.iter().enumerate() {
            let cy = 36 + (i as i32) * 11;
            let s = ch.to_string();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx != 0 || dy != 0 {
                        draw_modern_ui_text(
                            frame,
                            dw,
                            dh,
                            554 + dx,
                            cy + dy,
                            &s,
                            [0x03, 0x08, 0x05, 255],
                        );
                    }
                }
            }
            draw_modern_ui_text(frame, dw, dh, 554, cy, &s, [0xf2, 0xf8, 0xff, 255]);
        }

        // Physischer Fader-Cap (`preamp-thumb`, radius = 2px)
        let thumb_y = 33 + (norm * 64.0).round() as i32;
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            541,
            thumb_y,
            31,
            9,
            2,
            [0xd8, 0xe4, 0xf0, 255],
            [0x47, 0x58, 0x6b, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 541, thumb_y, 31, 9, 2, [0x10, 0x16, 0x1d, 255]);
        fill_rect(frame, dw, dh, 545, thumb_y + 4, 23, 2, [0x14, 0x1c, 0x24, 255]);

        // Unten im Schacht: 7-Segment dB-Anzeige (+20 bis -20 mit -88 Geistersegmenten) + dB
        let db_str = format!(
            "{}{:02}",
            if db_val >= 0 { '+' } else { '-' },
            db_val.unsigned_abs().min(99)
        );
        draw_modern_seg_value(frame, dw, dh, 545, 113, "-88", &db_str, false);
        draw_modern_ui_text(frame, dw, dh, 551, 128, "dB", COL_LCD_BRIGHT);

        // Bestätigungs-Overlay (nur F7 RemoveFromPlaylist; F5/F6/F8 & Rechtsklick-Kontextmenü im Modern Skin deaktiviert)
        let is_de = self.locale.is_german();
        if self.confirm_action == Some(ConfirmAction::RemoveFromPlaylist) {
            let ox = 110i32;
            let oy = 38i32;
            let ow = 360u32;
            let oh = 74u32;
            fill_rounded_rect_grad(
                frame,
                dw,
                dh,
                ox,
                oy,
                ow,
                oh,
                3,
                [0x1c, 0x25, 0x32, 255],
                [0x12, 0x19, 0x22, 255],
            );
            stroke_rounded_rect(frame, dw, dh, ox, oy, ow, oh, 3, COL_CYAN_BRAND);
            let msg = self.locale.tr_or(
                "confirm",
                "remove_playlist",
                "Remove current track from playlist? (F7)",
            );
            draw_modern_ui_text(frame, dw, dh, ox + 16, oy + 16, msg, COL_LCD_BRIGHT);
            let yes_lbl = if is_de { "JA" } else { "YES" };
            let no_lbl = if is_de { "NEIN" } else { "NO" };
            draw_modern_hw_btn(frame, dw, dh, ox + 70, oy + 42, 90, 22, yes_lbl, true);
            draw_modern_hw_btn(frame, dw, dh, ox + 200, oy + 42, 90, 22, no_lbl, false);
        }
    }

    pub(crate) fn handle_modern_main_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        button: MouseButton,
        window: &Arc<Window>,
    ) {
        if let Some(action) = self.confirm_action {
            if action == ConfirmAction::RemoveFromPlaylist && (80..=102).contains(&vy) {
                if (180..=270).contains(&vx) {
                    self.confirm_action = None;
                    self.execute_confirm_action(action);
                    return;
                } else if (310..=400).contains(&vx) {
                    self.confirm_action = None;
                    return;
                }
            }
            self.confirm_action = None;
            return;
        }

        // Rechtsklick-Interaktionen im Modern Main Window (ohne Classic-Datei-Kontextmenü)
        if button == MouseButton::Right {
            // Rechtsklick auf PreAmp-Schacht setzt PreAmp auf +00 dB zurück
            if (538..=574).contains(&vx) && (30..=142).contains(&vy) {
                self.reset_preamp_to_zero_db();
                return;
            }
            // Rechtsklick auf Clip / CL+DC im Status-LCD (Zeile 1) schaltet rückwärts
            if (468..=495).contains(&vx) && (30..=49).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.step_clip_declick(-1);
                self.setup_cfg.clipping = state.ini_config.clipping;
                self.setup_cfg.declick = state.ini_config.declick;
                return;
            }
            // Rechtsklick auf CPU-% im Status-LCD (Zeile 3 links) schaltet die Retro-CPU-Profile rückwärts durch
            if (380..=429).contains(&vx) && (72..=96).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                let prev_prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile).prev();
                state.ini_config.cpu_sim_profile = prev_prof.to_u8();
                self.setup_cfg.cpu_sim_profile = prev_prof.to_u8();
                let banner =
                    prev_prof.lcd_banner_with_stutter(state.ini_config.authentic_stutter);
                state.set_status_banner(banner);
                let _ = state
                    .ini_config
                    .save_to_file(&Mod4WinIniConfig::default_ini_path());
                return;
            }
            // Rechtsklick auf Pufferzeit im Status-LCD (Zeile 3 Mitte) toggelt Authentic Stutter
            if (430..=471).contains(&vx) && (72..=96).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.ini_config.authentic_stutter = !state.ini_config.authentic_stutter;
                self.setup_cfg.authentic_stutter = state.ini_config.authentic_stutter;
                let prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile);
                let banner = if state.ini_config.authentic_stutter {
                    format!("STUT:ON {}", prof.short_code())
                } else {
                    format!("STUT:OFF {}", prof.short_code())
                };
                state.set_status_banner(banner);
                let _ = state
                    .ini_config
                    .save_to_file(&Mod4WinIniConfig::default_ini_path());
                return;
            }
            // Rechtsklick auf Song/Total -> Voriger Song
            if (100..=210).contains(&vx) && (63..=111).contains(&vy) {
                self.prev_song();
                return;
            }
            // Rechtsklick auf Pattern -> Pattern -1
            if (215..=295).contains(&vx) && (63..=111).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta -= 1;
                return;
            }
            return;
        }

        // Titelleiste (y = 0..24)
        if vy < 24 {
            let w_i = MODERN_MAIN_W as i32;
            if (4..=20).contains(&vy) {
                if (6..=70).contains(&vx) {
                    if self.is_window_open(WindowKind::HelpViewer) {
                        self.close_window(WindowKind::HelpViewer);
                    } else {
                        self.open_help_topic(event_loop, "HauptDlg");
                    }
                    return;
                } else if (w_i - 61..=w_i - 45).contains(&vx) {
                    self.switch_mini_mode(event_loop, true);
                    return;
                } else if (w_i - 42..=w_i - 26).contains(&vx) {
                    self.toggle_window(event_loop, WindowKind::About);
                    return;
                } else if (w_i - 23..=w_i - 7).contains(&vx) {
                    self.save_ini_on_exit();
                    let mut state = self.shared_state.lock().unwrap();
                    state.is_playing = false;
                    state.is_paused = false;
                    state.is_finished = true;
                    event_loop.exit();
                    return;
                }
            }
            let _ = window.drag_window();
            return;
        }

        // Spalte 1, Zeile 1: Banner oder mechanischer Schiebeschalter (y = 30..58)
        if (30..=58).contains(&vy) && (6..=379).contains(&vx) {
            let mut state = self.shared_state.lock().unwrap();
            if vx >= 366 {
                // Klick auf den mechanischen Schiebeschalter: Oben (vy < 44) = Modultitel, Unten (vy >= 44) = Dateiname
                let want_filename = vy >= 44;
                if state.show_filename == want_filename {
                    state.show_filename = !state.show_filename;
                } else {
                    state.show_filename = want_filename;
                }
            } else {
                state.show_filename = !state.show_filename;
            }
            state.reset_title_scroll();
            state.sync_runtime_to_ini();
            return;
        }

        // Spalte 1, Zeile 2: Dual-LCDs (y = 63..111)
        if (63..=111).contains(&vy) {
            if (6..=88).contains(&vx) {
                // Klick auf Zeitanzeige toggelt Elapsed / Remaining
                let mut state = self.shared_state.lock().unwrap();
                state.minus_mode = !state.minus_mode;
                state.sync_runtime_to_ini();
                return;
            } else if (89..=188).contains(&vx) {
                self.next_song();
                return;
            } else if (192..=268).contains(&vx) {
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta += 1;
                return;
            } else if (269..=328).contains(&vx) {
                let mut state = self.shared_state.lock().unwrap();
                if vy < 79 {
                    state.shuffle = !state.shuffle;
                    state.rebuild_number_list();
                } else if vy < 93 {
                    state.repeat_mode = (state.repeat_mode + 1) % 3;
                } else {
                    state.intro_scan = !state.intro_scan;
                }
                state.sync_runtime_to_ini();
                return;
            } else if (329..=378).contains(&vx) {
                let mut state = self.shared_state.lock().unwrap();
                if vy < 79 {
                    state.toggle_declick();
                    self.setup_cfg.declick = state.ini_config.declick;
                } else if vy < 93 {
                    state.toggle_amiga_filter();
                    self.setup_cfg.amiga_filter = state.ini_config.amiga_filter;
                } else {
                    state.toggle_dehiss();
                    self.setup_cfg.dehiss = state.ini_config.dehiss;
                }
                let _ = state
                    .ini_config
                    .save_to_file(&Mod4WinIniConfig::default_ini_path());
                return;
            }
        }

        // Spalte 1, Zeile 3: 7er Transportleiste (y = 116..142, x = 6..374)
        if (116..=142).contains(&vy) && (6..=374).contains(&vx) {
            let idx = ((vx - 9).max(0) / 52).min(6);
            match idx {
                0 => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.intro_scan = false;
                    state.is_playing = false;
                    state.is_paused = false;
                    state.pending_seek_pattern = Some(1);
                }
                1 => self.trigger_play_or_hdd_record(event_loop),
                2 => {
                    let mut state = self.shared_state.lock().unwrap();
                    if state.is_playing || state.is_paused {
                        state.is_paused = !state.is_paused;
                        state.is_playing = !state.is_paused;
                    }
                }
                3 => self.prev_song(),
                4 => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.pending_seek_delta -= 1;
                }
                5 => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.pending_seek_delta += 1;
                }
                _ => self.next_song(),
            }
            return;
        }

        // Spalte 2, Oben: Status-LCD Hotspots (x = 380..532, y = 30..96)
        if (380..=532).contains(&vx) && (30..=96).contains(&vy) {
            if vy < 50 {
                // Zeile 1: 44kHz | 16Bit | Clip | ((OO))
                if vx < 432 {
                    const STD_RATES: [i32; 4] = [11025, 22050, 44100, 48000];
                    let mut state = self.shared_state.lock().unwrap();
                    let cur_khz = (state.sound_settings.samplerate + 500) / 1000;
                    let cur_idx = STD_RATES
                        .iter()
                        .position(|&r| (r + 500) / 1000 >= cur_khz)
                        .unwrap_or(2);
                    state.sound_settings.samplerate = STD_RATES[(cur_idx + 1) % STD_RATES.len()];
                    state.settings_dirty = true;
                    state.sync_runtime_to_ini();
                    self.setup_cfg.sample_rate = state.ini_config.sample_rate;
                } else if vx < 468 {
                    let mut state = self.shared_state.lock().unwrap();
                    state.sound_settings.bits = if state.sound_settings.bits == 8 { 16 } else { 8 };
                    state.settings_dirty = true;
                    state.sync_runtime_to_ini();
                    self.setup_cfg.bits_per_sample = state.ini_config.bits_per_sample;
                } else if vx < 496 {
                    let mut state = self.shared_state.lock().unwrap();
                    state.toggle_clipping();
                    self.setup_cfg.clipping = state.ini_config.clipping;
                } else {
                    let mut state = self.shared_state.lock().unwrap();
                    if state.sound_settings.stereo < 2 {
                        state.sound_settings.stereo = 2;
                        state.sound_settings.surround = false;
                    } else if !state.sound_settings.surround {
                        state.sound_settings.stereo = 2;
                        state.sound_settings.surround = true;
                    } else {
                        state.sound_settings.stereo = 1;
                        state.sound_settings.surround = false;
                    }
                    state.settings_dirty = true;
                    state.sync_runtime_to_ini();
                    self.setup_cfg.channels = state.ini_config.channels;
                    self.setup_cfg.surround = state.ini_config.surround;
                }
            } else if vy < 72 {
                // Zeile 2: Chn | BPM | PAN | IDO
                if (432..=462).contains(&vx) {
                    let mut state = self.shared_state.lock().unwrap();
                    state.sound_settings.no_bpm = !state.sound_settings.no_bpm;
                    state.settings_dirty = true;
                } else if (463..=490).contains(&vx) {
                    let mut state = self.shared_state.lock().unwrap();
                    state.sound_settings.panning = !state.sound_settings.panning;
                    state.settings_dirty = true;
                    state.sync_runtime_to_ini();
                    self.setup_cfg.enable_panning = state.ini_config.enable_panning;
                } else if vx >= 491 {
                    let mut state = self.shared_state.lock().unwrap();
                    state.step_oversamp(1);
                    self.setup_cfg.over_sampling = state.ini_config.over_sampling;
                }
            } else {
                // Zeile 3: CPU% | Buffer | TimerMode
                if vx < 430 {
                    let mut state = self.shared_state.lock().unwrap();
                    let next_prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile).next();
                    state.ini_config.cpu_sim_profile = next_prof.to_u8();
                    self.setup_cfg.cpu_sim_profile = next_prof.to_u8();
                    let banner =
                        next_prof.lcd_banner_with_stutter(state.ini_config.authentic_stutter);
                    state.set_status_banner(banner);
                    let _ = state
                        .ini_config
                        .save_to_file(&Mod4WinIniConfig::default_ini_path());
                } else if vx < 472 {
                    let mut state = self.shared_state.lock().unwrap();
                    state.ini_config.authentic_stutter = !state.ini_config.authentic_stutter;
                    self.setup_cfg.authentic_stutter = state.ini_config.authentic_stutter;
                    let prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile);
                    let banner = if state.ini_config.authentic_stutter {
                        format!("STUT:ON {}", prof.short_code())
                    } else {
                        format!("STUT:OFF {}", prof.short_code())
                    };
                    state.set_status_banner(banner);
                    let _ = state
                        .ini_config
                        .save_to_file(&Mod4WinIniConfig::default_ini_path());
                } else if vx >= 472 {
                    let mut state = self.shared_state.lock().unwrap();
                    let next_tm = (state.sound_settings.timer_mode() + 1) % 3;
                    state.sound_settings.set_timer_mode(next_tm);
                    state.settings_dirty = true;
                    state.sync_runtime_to_ini();
                    self.setup_cfg.timer_mode = state.ini_config.timer_mode;
                }
            }
            return;
        }

        // Spalte 2, Unten: 2x4 Funktions-Buttons (x = 383..535, y = 101..142)
        if (383..=535).contains(&vx) && (101..=142).contains(&vy) {
            let col = ((vx - 383) / 38).clamp(0, 3);
            let row = if vy < 121 { 0 } else { 1 };
            match (row, col) {
                (0, 0) => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.shuffle = !state.shuffle;
                    state.rebuild_number_list();
                    state.sync_runtime_to_ini();
                }
                (0, 1) => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.repeat_mode = (state.repeat_mode + 1) % 3;
                    state.sync_runtime_to_ini();
                }
                (0, 2) => {
                    let new_scan = {
                        let mut state = self.shared_state.lock().unwrap();
                        state.toggle_intro_scan();
                        state.sync_runtime_to_ini();
                        state.intro_scan
                    };
                    if new_scan {
                        self.trigger_play_or_hdd_record(event_loop);
                    }
                }
                (0, 3) => {
                    let mut state = self.shared_state.lock().unwrap();
                    state.minus_mode = !state.minus_mode;
                    state.sync_runtime_to_ini();
                }
                (1, 0) => {
                    // List toggelt das Playlist-Dock (`QuickSelect` / `OpenDialog`) im Modern Skin!
                    if self.is_window_open(WindowKind::QuickSelect)
                        || self.is_window_open(WindowKind::OpenDialog)
                    {
                        self.close_window(WindowKind::QuickSelect);
                        self.close_window(WindowKind::OpenDialog);
                    } else {
                        self.open_window(event_loop, WindowKind::QuickSelect);
                    }
                }
                (1, 1) => {
                    // Setup toggelt das Setup-Fenster!
                    if self.is_window_open(WindowKind::SetupDialog) {
                        self.close_window(WindowKind::SetupDialog);
                    } else {
                        self.open_window(event_loop, WindowKind::SetupDialog);
                    }
                }
                (1, 2) => {
                    // Info toggelt das Kombi-Fenster Module & Instrument Info!
                    if self.is_window_open(WindowKind::ModuleInfo)
                        || self.is_window_open(WindowKind::InstrumentInfo)
                    {
                        self.close_window(WindowKind::ModuleInfo);
                        self.close_window(WindowKind::InstrumentInfo);
                    } else {
                        self.open_window(event_loop, WindowKind::ModuleInfo);
                    }
                }
                _ => {
                    if self.is_window_open(WindowKind::Effects) {
                        self.close_window(WindowKind::Effects);
                    } else {
                        self.open_window(event_loop, WindowKind::Effects);
                    }
                }
            }
            return;
        }

        // Spalte 3: PreAmp-Fader (x = 538..574, y = 30..142)
        if (538..=574).contains(&vx) && (30..=142).contains(&vy) {
            if vy >= 110 {
                // Klick auf dB-Anzeige startet manuellen Peak-Search!
                let (mod_path_opt, ss_snap, mod_title) = {
                    let st = self.shared_state.lock().unwrap();
                    (
                        st.current_file_path.clone(),
                        st.sound_settings,
                        if !st.title.trim().is_empty() {
                            st.title.clone()
                        } else {
                            st.filename.clone()
                        },
                    )
                };
                if let Some(mod_path) = mod_path_opt
                    && let Ok((file_data, ext_opt)) =
                        super::archive_mol::read_module_bytes_universal(&mod_path)
                        && let Ok(m) = crate::engine::loader::load_module_with_ext(
                            &file_data,
                            ext_opt.as_deref(),
                        ) {
                            UiSharedState::start_or_resolve_peak_search(
                                &self.shared_state,
                                m,
                                ss_snap,
                                file_data.len() as u32,
                                mod_title,
                                true,
                            );
                        }
            } else {
                self.preamp_dragging = true;
                self.update_preamp_from_y(vy);
            }
            return;
        }

        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // B. MINI-STATUS ([2] WindowShade – 325 x 64 px)
    // =========================================================================
    pub(crate) fn render_modern_mini_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        draw_modern_window_shell(frame, dw, dh, true, "MINI", true, true);
        if self.is_brand_pill_highlighted() {
            draw_modern_brand_pill(frame, dw, dh, true);
        }
        let peak_disp = state.active_peak_search_display();
        let display_name = if let Some((ref ps_text, _, _)) = peak_disp {
            ps_text.as_str()
        } else if state.show_filename && !state.filename.is_empty() {
            state.filename.as_str()
        } else if !state.title.trim().is_empty() {
            state.title.trim()
        } else {
            "MOD4WIN MINI"
        };
        let scroll_ms = state.main_title_scroll_elapsed_ms();
        render_mod4win_matrix20_marquee(frame, dw, dh, 6, 29, 253, 28, display_name, scroll_ms);
        let is_play = state.is_playing && !state.is_paused;
        let is_stop = !state.is_playing && !state.is_paused;
        draw_transport_btn(frame, dw, dh, 264, 30, 26, 26, 1, is_play);
        draw_transport_btn(frame, dw, dh, 293, 30, 26, 26, 0, is_stop);
    }

    pub(crate) fn handle_modern_mini_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if vy < 24 {
            if (4..=20).contains(&vy) && (6..=70).contains(&vx) {
                if self.is_window_open(WindowKind::HelpViewer) {
                    self.close_window(WindowKind::HelpViewer);
                } else {
                    self.open_help_topic(event_loop, "HauptDlg");
                }
                return;
            }
            if vx >= (MODERN_MINI_W as i32 - 26) {
                self.switch_mini_mode(event_loop, false);
                return;
            }
            let _ = window.drag_window();
            return;
        }
        if is_double_click {
            self.switch_mini_mode(event_loop, false);
            return;
        }
        if (264..=290).contains(&vx) && (29..=57).contains(&vy) {
            self.trigger_play_or_hdd_record(event_loop);
            return;
        }
        if (293..=319).contains(&vx) && (29..=57).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.intro_scan = false;
            state.is_playing = false;
            state.is_paused = false;
            state.pending_seek_pattern = Some(1);
            return;
        }
        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // C. KOMBI-FENSTER: MODULE & INSTRUMENT INFO ([10 + 12] – 580 x 365 px)
    // =========================================================================
    pub(crate) fn render_modern_module_info_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        let is_de = self.locale.is_german();
        let win_title = if is_de {
            "[10 + 12] Modul- & Instrument-Info"
        } else {
            "[10 + 12] Module & Instrument Info"
        };
        draw_modern_window_shell(frame, dw, dh, false, win_title, false, false);

        let title = if !state.title.trim().is_empty() {
            state.title.trim()
        } else if !state.filename.is_empty() {
            state.filename.as_str()
        } else if is_de {
            "KEIN MODUL GELADEN"
        } else {
            "NO MODULE LOADED"
        };
        let info_scroll_ms = state.info_title_scroll_elapsed_ms();
        render_mod4win_matrix_info_marquee(frame, dw, dh, 6, 29, 568, 29, title, info_scroll_ms);

        // Zweispaltige Sample-Liste (x = 6, y = 63, w = 568, h = 215)
        draw_modern_lcd_box(frame, dw, dh, 6, 63, 568, 215);
        let max_rows = 16usize; // 2 Spalten x 16 Zeilen = 32 Samples pro Seite
        let base = self.info_inst_offset;
        let sel_1based = self.selected_instrument.max(1);
        for idx in 0..32usize {
            let inst_idx = base + idx;
            let col = (idx / max_rows) as i32;
            let row = (idx % max_rows) as i32;
            let rx = 12 + col * 280;
            let ry = 68 + row * 13;
            let is_sel = (inst_idx + 1) == sel_1based;
            let name = state
                .instruments
                .get(inst_idx)
                .map(|s| s.name.as_str())
                .unwrap_or("");
            if is_sel {
                fill_rounded_rect_grad(
                    frame,
                    dw,
                    dh,
                    rx - 2,
                    ry - 1,
                    274,
                    12,
                    2,
                    COL_LCD_BRIGHT,
                    COL_LCD_BRIGHT,
                );
            }
            let fg = if is_sel {
                [0x02, 0x12, 0x08, 255]
            } else {
                COL_LCD_BRIGHT
            };
            let idx_fg = if is_sel {
                [0x02, 0x12, 0x08, 255]
            } else {
                [0x00, 0xa8, 0x38, 255]
            };
            draw_modern_ui_text(
                frame,
                dw,
                dh,
                rx + 2,
                ry + 2,
                &format!("{:02}", inst_idx + 1),
                idx_fg,
            );
            let short_name: String = name.chars().take(42).collect();
            draw_modern_ui_text(frame, dw, dh, rx + 22, ry + 2, &short_name, fg);
        }

        // Unten integriert: [12] Instrument-Inspektor (2x4 Grid bei x = 6, y = 283, w = 568, h = 76)
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            6,
            283,
            568,
            76,
            3,
            [0x16, 0x1e, 0x28, 255],
            [0x12, 0x18, 0x20, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 6, 283, 568, 76, 3, [0x38, 0x48, 0x5c, 255]);

        let det = state
            .instruments
            .get(sel_1based.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        let inst_name = det.name.as_str();
        let fine_str = if det.fine_tune > 0 {
            format!("+{}", det.fine_tune)
        } else {
            format!("{}", det.fine_tune)
        };
        let bits_str = if det.is_16bit { "16 bit" } else { "8 bit" };

        let cells = [
            (
                "Instrument:",
                format!("{:02} {}", sel_1based, inst_name),
                COL_LCD_BRIGHT,
            ),
            (
                if is_de { "Länge:" } else { "Length:" },
                format!("{}", det.length),
                COL_LCD_BRIGHT,
            ),
            (
                if is_de { "Loopstart:" } else { "Loop Start:" },
                format!("{}", det.loop_start),
                COL_LCD_BRIGHT,
            ),
            (
                if is_de { "Looplänge:" } else { "Loop Length:" },
                format!("{}", det.loop_length),
                COL_LCD_BRIGHT,
            ),
            ("Finetune:", fine_str, COL_LCD_BRIGHT),
            ("C2-Tuning:", format!("{} Hz", det.c4_freq), COL_LCD_BRIGHT),
            ("Def.Volume:", format!("{}", det.volume), COL_LCD_BRIGHT),
            (
                if is_de { "Auflösung:" } else { "Resolution:" },
                bits_str.to_string(),
                COL_LCD_AMBER,
            ),
        ];

        for (i, (lbl, val, vcol)) in cells.iter().enumerate() {
            let col = (i % 4) as i32;
            let row = (i / 4) as i32;
            let cx = 12 + col * 140;
            let cy = 289 + row * 34;
            fill_rounded_rect_grad(frame, dw, dh, cx, cy, 134, 28, 3, COL_LCD_BG, COL_LCD_BG);
            stroke_rounded_rect(frame, dw, dh, cx, cy, 134, 28, 3, [0x0d, 0x36, 0x22, 255]);
            draw_modern_ui_text(frame, dw, dh, cx + 5, cy + 4, lbl, [0x65, 0x99, 0x7d, 255]);
            let val_short: String = val.chars().take(21).collect();
            draw_modern_ui_text(frame, dw, dh, cx + 5, cy + 16, &val_short, *vcol);
        }

        // Unten angeflanscht (2 Zeilen nebeneinander bei x = 6, y = 363, w = 568, h = 37):
        // Die 4 Modul-Statistik-Zeilen aus dem originalen Info-Fenster (`DETAILS.PAS:476-520`)
        draw_modern_lcd_box(frame, dw, dh, 6, 363, 568, 37);
        fill_rect(frame, dw, dh, 290, 368, 1, 27, [0x0f, 0x3d, 0x26, 255]);

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

        let left_center_x = |s: &str| -> i32 {
            10 + ((276 - modern_ui_text_width(s)) / 2).max(4)
        };
        let right_center_x = |s: &str| -> i32 {
            294 + ((276 - modern_ui_text_width(s)) / 2).max(4)
        };
        draw_modern_ui_text(frame, dw, dh, left_center_x(&line1), 370, &line1, COL_LCD_BRIGHT);
        draw_modern_ui_text(frame, dw, dh, right_center_x(&line2), 370, &line2, COL_LCD_BRIGHT);
        draw_modern_ui_text(frame, dw, dh, left_center_x(&line3), 385, &line3, COL_LCD_BRIGHT);
        draw_modern_ui_text(frame, dw, dh, right_center_x(&line4), 385, &line4, COL_LCD_BRIGHT);
    }

    pub(crate) fn handle_modern_module_info_click(
        &mut self,
        _event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        if vy < 24 {
            if vx >= (MODERN_INFO_W as i32 - 26) {
                self.close_window(WindowKind::ModuleInfo);
                self.close_window(WindowKind::InstrumentInfo);
                return;
            }
            let _ = window.drag_window();
            return;
        }
        if (68..=275).contains(&vy) && (10..=568).contains(&vx) {
            let col = if vx >= 292 { 1usize } else { 0usize };
            let row = ((vy - 68) / 13).clamp(0, 15) as usize;
            let idx = self.info_inst_offset + col * 16 + row;
            let total = self.shared_state.lock().unwrap().instruments.len().max(31);
            if idx < total {
                self.selected_instrument = idx + 1;
            }
            return;
        }
        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // D. EFFECTS ANALYZER ([11] – ab 350 x 270 px, dynamisch je Kanalzahl)
    // =========================================================================
    pub(crate) fn render_modern_effects_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        let num_ch = state.num_channels.clamp(4, 32);
        let title = format!("[11] Effects Analyzer (Channels 1-{})", num_ch);
        draw_modern_window_shell(frame, dw, dh, false, &title, false, false);

        draw_modern_lcd_box(frame, dw, dh, 6, 30, dw.saturating_sub(12), dh.saturating_sub(36));

        // Kopfzeile: EFFECT + Kanalnummern 1..N
        draw_modern_ui_text(frame, dw, dh, 14, 38, "EFFECT", [0x4d, 0xc4, 0x84, 255]);
        let col_w = ((dw as i32 - 114).max(num_ch as i32 * 14) / (num_ch as i32)).max(14);
        for ch in 0..num_ch {
            let cx = 104 + (ch as i32) * col_w;
            let muted = state.channel_muted[ch];
            let ch_col = if muted {
                COL_LCD_RED
            } else {
                [0x4d, 0xc4, 0x84, 255]
            };
            draw_modern_ui_text(frame, dw, dh, cx + 2, 38, &format!("{}", ch + 1), ch_col);
        }
        fill_rect(
            frame,
            dw,
            dh,
            12,
            49,
            dw.saturating_sub(24),
            1,
            [0x0f, 0x3d, 0x26, 255],
        );

        // Zeile direkt unter den Kanalnummern (y = 52..78): Getrennte L/R Kanal-Lautstärke-Anzeige (Stereo Volume-Bars)
        draw_modern_ui_text(frame, dw, dh, 14, 62, "VOL L/R", [0x4d, 0xc4, 0x84, 255]);
        for ch in 0..num_ch {
            let cx = 104 + (ch as i32) * col_w + 1;
            let (vol_l, vol_r) = if state.is_playing && !state.is_paused && !state.channel_muted[ch] {
                let (vl, vr) = state.channel_vols[ch];
                (vl.min(64) as u32, vr.min(64) as u32)
            } else {
                (0, 0)
            };
            let lit_l = ((vol_l * 8 + 7) / 64).min(8);
            let lit_r = ((vol_r * 8 + 7) / 64).min(8);
            for (bar_dx, lit_segs, vol) in [(0i32, lit_l, vol_l), (5i32, lit_r, vol_r)] {
                for seg in 0..8u32 {
                    // seg = 0 ist oben (Rot), seg = 7 ist unten (Grün)
                    let from_bottom = 7 - seg;
                    let is_lit = from_bottom < lit_segs && vol > 0;
                    let seg_col = if seg == 0 {
                        if is_lit {
                            COL_LCD_RED
                        } else {
                            [0x33, 0x08, 0x00, 255]
                        }
                    } else if seg <= 2 {
                        if is_lit {
                            COL_LCD_AMBER
                        } else {
                            [0x33, 0x2c, 0x00, 255]
                        }
                    } else if is_lit {
                        COL_LCD_BRIGHT
                    } else {
                        [0x08, 0x24, 0x14, 255]
                    };
                    let sy = 53 + (seg as i32) * 3;
                    fill_rect(frame, dw, dh, cx + bar_dx, sy, 4, 2, seg_col);
                }
            }
        }
        fill_rect(
            frame,
            dw,
            dh,
            12,
            79,
            dw.saturating_sub(24),
            1,
            [0x0f, 0x3d, 0x26, 255],
        );

        // Dynamische Liste der im geladenen Modul tatsächlich genutzten Effekte (`0..=67`, analog zu `ana.pas`)
        let active_cols = state.active_effect_columns();
        for (row_idx, &eff_id) in active_cols.iter().enumerate() {
            let ry = 83 + (row_idx as i32) * 15;
            let fx_name = effect_name_by_id(eff_id);
            draw_modern_ui_text(frame, dw, dh, 14, ry + 2, fx_name, [0x79, 0xd9, 0xa5, 255]);
            for ch in 0..num_ch {
                let cx = 104 + (ch as i32) * col_w + 1;
                let eff_word = state.channel_effects[ch];
                let cmd = (eff_word >> 8) as u8;
                let op = (eff_word & 0xFF) as u8;
                let active_id = if state.channel_muted[ch] {
                    None
                } else {
                    effect_id_from_cmd_op(cmd, op)
                };
                let is_lit = active_id == Some(eff_id);
                let dot_col = if is_lit {
                    if (row_idx + ch) % 5 == 0 {
                        COL_LCD_AMBER
                    } else {
                        COL_LCD_BRIGHT
                    }
                } else {
                    [0x08, 0x24, 0x14, 255]
                };
                fill_rounded_rect_grad(frame, dw, dh, cx, ry + 2, 9, 8, 1, dot_col, dot_col);
            }
        }
    }

    pub(crate) fn handle_modern_effects_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        button: MouseButton,
        window: &Arc<Window>,
    ) {
        let (num_ch, num_fx) = {
            let st = self.shared_state.lock().unwrap();
            (
                st.num_channels.clamp(4, 32),
                st.active_effect_columns().len().max(1) as u32,
            )
        };
        let dw = 350u32.max(115 + (num_ch as u32) * 20);
        let dh = 90 + num_fx * 15;
        if vy < 24 {
            if vx >= (dw as i32 - 26) {
                self.close_window(WindowKind::Effects);
                return;
            }
            let _ = window.drag_window();
            return;
        }
        let col_w = ((dw as i32 - 114).max(num_ch as i32 * 14) / (num_ch as i32)).max(14);
        if (32..=(dh as i32 - 8)).contains(&vy) && vx >= 100 {
            let ch_idx = ((vx - 104) / col_w) as usize;
            if ch_idx < num_ch {
                let mut state = self.shared_state.lock().unwrap();
                if button == MouseButton::Right {
                    // Rechtsklick = Solo
                    for m in state.channel_muted.iter_mut() {
                        *m = true;
                    }
                    state.channel_muted[ch_idx] = false;
                } else {
                    // Linksklick = Mute
                    state.channel_muted[ch_idx] = !state.channel_muted[ch_idx];
                }
                state.channel_muted_dirty = true;
                return;
            }
        }
        if button == MouseButton::Left && (12..100).contains(&vx) {
            if (32..=79).contains(&vy) {
                // Klick auf "EFFECT" / "VOL L/R": Alle Kanäle anschalten (analog zu "Chn" im Classic Skin)
                let mut state = self.shared_state.lock().unwrap();
                state.channel_muted = [false; 32];
                state.channel_muted_dirty = true;
                return;
            }
            if (80..=(dh as i32 - 8)).contains(&vy) {
                // Klick auf Effektname: Effekt-Erklärungsfenster öffnen (analog zu Classic Skin)
                self.open_window(event_loop, WindowKind::EffectsHelp);
                return;
            }
        }
        if button == MouseButton::Right {
            self.pending_right_click_close = Some(WindowKind::Effects);
            return;
        }
        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // E. PLAYLIST-DOCK (580 x [81 + rows * 16] px, Standard 9 Zeilen = 225 px)
    // =========================================================================
    pub(crate) fn render_modern_playlist_dock(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        draw_modern_window_shell(
            frame,
            dw,
            dh,
            false,
            "Playlist Dock (Drag & Drop / .ZIP / .MOL)",
            false,
            false,
        );

        let max_vis = ((dh.saturating_sub(81)) / 16).clamp(5, 36) as usize;
        let lcd_h = 12 + (max_vis as u32) * 16;
        draw_modern_lcd_box(frame, dw, dh, 6, 30, 568, lcd_h);

        let total_items = state.playlist.len();
        let max_scroll = total_items.saturating_sub(max_vis);
        let scroll = self.quick_select_scroll.min(max_scroll);

        // O(1) Virtual-Scrolling: Nur die sichtbaren Zeilen (`scroll..scroll+max_vis`) auswerten,
        // damit selbst bei 999.999 Songs in der Playlist 0 irrelevante Strings pro Frame allokiert werden!
        let sorted_fallback = if self.quick_select_sort == 0 {
            None
        } else {
            Some(self.sorted_quick_select_items(state))
        };

        for row in 0..max_vis {
            let (orig_idx, label) = if let Some(ref sorted) = sorted_fallback {
                let Some((idx, lbl)) = sorted.get(scroll + row) else {
                    break;
                };
                (*idx, lbl.clone())
            } else {
                let idx = scroll + row;
                let Some(p) = state.playlist.get(idx) else {
                    break;
                };
                let lbl = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("module.mod")
                    .to_lowercase();
                (idx, lbl)
            };
            let ry = 35 + (row as i32) * 16;
            let is_playing = !state.playing_track_detached && orig_idx == state.playlist_idx;
            let is_selected = self.quick_select_all_selected
                || self.playlist_selected.contains(&orig_idx)
                || (self.playlist_selected.is_empty() && orig_idx == self.quick_select_cursor);
            if is_selected && is_playing {
                fill_rounded_rect_grad(
                    frame,
                    dw,
                    dh,
                    10,
                    ry,
                    544,
                    15,
                    2,
                    [0x0f, 0x52, 0x28, 255],
                    [0x0a, 0x3b, 0x1c, 255],
                );
                stroke_rounded_rect(frame, dw, dh, 10, ry, 544, 15, 2, [0x2a, 0xa8, 0x5e, 255]);
            } else if is_selected {
                fill_rounded_rect_grad(
                    frame,
                    dw,
                    dh,
                    10,
                    ry,
                    544,
                    15,
                    2,
                    [0x14, 0x2c, 0x42, 255],
                    [0x0e, 0x20, 0x30, 255],
                );
                stroke_rounded_rect(frame, dw, dh, 10, ry, 544, 15, 2, [0x2e, 0x6b, 0x9e, 255]);
            } else if is_playing {
                fill_rounded_rect_grad(
                    frame,
                    dw,
                    dh,
                    10,
                    ry,
                    544,
                    15,
                    2,
                    [0x0b, 0x3d, 0x1c, 255],
                    [0x08, 0x2c, 0x14, 255],
                );
            }
            let fg = if is_playing {
                [255, 255, 255, 255]
            } else if is_selected {
                [0xd8, 0xf3, 0xff, 255]
            } else {
                [0x8c, 0xe6, 0xb5, 255]
            };
            draw_modern_ui_text(frame, dw, dh, 14, ry + 4, &format!("{:02}.", orig_idx + 1), fg);

            #[cfg(not(feature = "retro-win32"))]
            let cached_meta = state
                .playlist
                .get(orig_idx)
                .and_then(|p| self.playlist_meta_cache.get(&UiSharedState::playlist_path_key(p)))
                .and_then(|m| m.as_ref());

            let fname = label.as_str();
            let disp = if is_playing && !state.title.trim().is_empty() {
                format!("{} - {}", fname, state.title.trim())
            } else {
                #[cfg(not(feature = "retro-win32"))]
                {
                    if let Some(meta) = cached_meta
                        && !meta.title.trim().is_empty()
                    {
                        format!("{} - {}", fname, meta.title.trim())
                    } else {
                        fname.to_string()
                    }
                }
                #[cfg(feature = "retro-win32")]
                {
                    fname.to_string()
                }
            };
            let short_disp: String = disp.chars().take(54).collect();
            draw_modern_ui_text(frame, dw, dh, 42, ry + 4, &short_disp, fg);

            let ch_str = if is_playing {
                format!("{:02}CH", state.num_channels)
            } else {
                #[cfg(not(feature = "retro-win32"))]
                {
                    if let Some(meta) = cached_meta {
                        format!("{} {:02}C", meta.short_fmt, meta.num_channels)
                    } else {
                        fname
                            .rsplit('.')
                            .next()
                            .unwrap_or("MOD")
                            .to_ascii_uppercase()
                            .chars()
                            .take(4)
                            .collect()
                    }
                }
                #[cfg(feature = "retro-win32")]
                {
                    fname
                        .rsplit('.')
                        .next()
                        .unwrap_or("MOD")
                        .chars()
                        .take(4)
                        .collect()
                }
            };
            draw_modern_ui_text(frame, dw, dh, 434, ry + 4, &ch_str, fg);

            let dur_secs = if is_playing {
                state.total_time_secs
            } else {
                #[cfg(not(feature = "retro-win32"))]
                {
                    cached_meta.map(|m| m.duration_secs).unwrap_or(0)
                }
                #[cfg(feature = "retro-win32")]
                {
                    0
                }
            };
            let dur_str = if dur_secs > 0 {
                format!("{:02}:{:02}", dur_secs / 60, dur_secs % 60)
            } else {
                "--:--".to_string()
            };
            draw_modern_ui_text(frame, dw, dh, 492, ry + 4, &dur_str, fg);

            // Symmetrisches großes `X` (5x7) exakt mittig im 15x13 Button (x = 536..550, y = ry + 1..ry + 13)
            fill_rounded_rect_grad(
                frame,
                dw,
                dh,
                536,
                ry + 1,
                15,
                13,
                2,
                [0x2a, 0x1c, 0x22, 255],
                [0x1c, 0x12, 0x16, 255],
            );
            stroke_rounded_rect(frame, dw, dh, 536, ry + 1, 15, 13, 2, [0x5c, 0x34, 0x3e, 255]);
            draw_modern_ui_text(frame, dw, dh, 541, ry + 4, "X", [0xf0, 0x80, 0x80, 255]);
        }

        // Leuchtende Einfügelinie während einer laufenden Drag-&-Drop-Umsortierung
        if let Some(ref drag) = self.playlist_reorder_drag
            && drag.active
            && !self.quick_select_all_selected
            && drag.drop_slot >= scroll
            && drag.drop_slot <= scroll + max_vis
        {
            let rel_row = drag.drop_slot - scroll;
            let line_y = 34 + (rel_row as i32) * 16;
            fill_rect(frame, dw, dh, 10, line_y, 544, 2, [0x42, 0xd4, 0xff, 255]);
            fill_rect(frame, dw, dh, 8, line_y - 2, 4, 6, [0x7c, 0xe8, 0xff, 255]);
            fill_rect(frame, dw, dh, 552, line_y - 2, 4, 6, [0x7c, 0xe8, 0xff, 255]);
        }

        // Vertikaler Scrollbalken rechts in der Playlist-Box (x = 558..570, y = 34..34+sb_h)
        let sb_x: i32 = 558;
        let sb_y: i32 = 34;
        let sb_w: u32 = 12;
        let sb_h: u32 = 4 + (max_vis as u32) * 16;
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            sb_x,
            sb_y,
            sb_w,
            sb_h,
            2,
            [0x05, 0x09, 0x0e, 255],
            [0x07, 0x0d, 0x14, 255],
        );
        stroke_rounded_rect(
            frame,
            dw,
            dh,
            sb_x,
            sb_y,
            sb_w,
            sb_h,
            2,
            [0x1a, 0x28, 0x38, 255],
        );
        let track_h: i32 = (sb_h as i32) - 2;
        let (thumb_y, thumb_h) = if total_items <= max_vis || max_scroll == 0 {
            (sb_y + 1, track_h)
        } else {
            let th = (((max_vis as f64) / (total_items as f64)) * (track_h as f64)).round() as i32;
            let th = th.clamp(16, track_h);
            let avail = (track_h - th).max(1);
            let ty = sb_y
                + 1
                + (((scroll as f64) / (max_scroll as f64)) * (avail as f64)).round() as i32;
            (ty.clamp(sb_y + 1, sb_y + 1 + avail), th)
        };
        let (th_top, th_bot, th_border) = if self.playlist_scroll_dragging {
            (
                [0x2e, 0x6b, 0x9e, 255],
                [0x1e, 0x4b, 0x72, 255],
                [0x56, 0xa8, 0xe8, 255],
            )
        } else if max_scroll > 0 {
            (
                [0x22, 0x36, 0x4a, 255],
                [0x17, 0x26, 0x36, 255],
                [0x38, 0x54, 0x70, 255],
            )
        } else {
            (
                [0x12, 0x1b, 0x24, 255],
                [0x0e, 0x15, 0x1d, 255],
                [0x20, 0x2e, 0x3d, 255],
            )
        };
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            sb_x + 1,
            thumb_y,
            sb_w - 2,
            thumb_h as u32,
            2,
            th_top,
            th_bot,
        );
        stroke_rounded_rect(
            frame,
            dw,
            dh,
            sb_x + 1,
            thumb_y,
            sb_w - 2,
            thumb_h as u32,
            2,
            th_border,
        );
        if thumb_h >= 14 {
            let mid_y = thumb_y + thumb_h / 2;
            for dy in [-2, 0, 2] {
                fill_rect(
                    frame,
                    dw,
                    dh,
                    sb_x + 3,
                    mid_y + dy,
                    sb_w - 6,
                    1,
                    th_border,
                );
            }
        }

        // Fußzeile: + Add / .ZIP | Remove | Clear | Load .MOL | Save .MOL | Song X of Y (MM:SS)
        let btn_y = 48 + (max_vis as i32) * 16;
        let is_de = self.locale.is_german();
        let add_lbl = if is_de { "+ Datei / .ZIP" } else { "+ Add / .ZIP" };
        let rem_lbl = if is_de { "Entf." } else { "Remove" };
        let clr_lbl = if is_de { "Leeren" } else { "Clear" };
        draw_modern_hw_btn(frame, dw, dh, 8, btn_y, 96, 24, add_lbl, false);
        draw_modern_hw_btn(frame, dw, dh, 108, btn_y, 62, 24, rem_lbl, false);
        draw_modern_hw_btn(frame, dw, dh, 174, btn_y, 54, 24, clr_lbl, false);
        draw_modern_hw_btn(frame, dw, dh, 232, btn_y, 74, 24, "Load .MOL", false);
        draw_modern_hw_btn(frame, dw, dh, 310, btn_y, 74, 24, "Save .MOL", false);

        let cur_idx = if state.playlist.is_empty() || state.playing_track_detached {
            0
        } else {
            (state.playlist_idx + 1).min(state.playlist.len())
        };
        let tot = state.playlist.len();
        #[cfg(not(feature = "retro-win32"))]
        let total_pl_secs: u64 = state
            .playlist
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if !state.playing_track_detached && i == state.playlist_idx && state.total_time_secs > 0 {
                    state.total_time_secs as u64
                } else {
                    self.playlist_meta_cache
                        .get(&UiSharedState::playlist_path_key(p))
                        .and_then(|m| m.as_ref())
                        .map(|m| m.duration_secs as u64)
                        .unwrap_or(0)
                }
            })
            .sum();
        #[cfg(feature = "retro-win32")]
        let total_pl_secs: u64 = state.total_time_secs as u64;

        let dur_fmt = if total_pl_secs >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                total_pl_secs / 3600,
                (total_pl_secs % 3600) / 60,
                total_pl_secs % 60
            )
        } else {
            format!("{:02}:{:02}", total_pl_secs / 60, total_pl_secs % 60)
        };
        let sel_count = if self.quick_select_all_selected {
            tot
        } else {
            self.playlist_selected.len()
        };
        let summary = if self.quick_select_all_selected && tot > 0 {
            if is_de {
                format!("Alle {} markiert (Entf = Leeren)", tot)
            } else {
                format!("All {} selected (Del = Clear)", tot)
            }
        } else if sel_count > 1 {
            if is_de {
                format!("{} von {} markiert ({})", sel_count, tot, dur_fmt)
            } else {
                format!("{} of {} selected ({})", sel_count, tot, dur_fmt)
            }
        } else if is_de {
            format!("Song {} von {} ({})", cur_idx, tot, dur_fmt)
        } else {
            format!("Song {} of {} ({})", cur_idx, tot, dur_fmt)
        };
        draw_modern_ui_text(frame, dw, dh, 394, btn_y + 8, &summary, COL_TEXT_MUTED);

        // Dezenter Resize-Grip am unteren Fensterrand (signalisiert vertikale Höhenverstellbarkeit)
        let grip_y = (dh as i32) - 4;
        let grip_x = (dw as i32) / 2 - 14;
        fill_rect(frame, dw, dh, grip_x, grip_y, 28, 1, [0x4a, 0x5c, 0x73, 255]);
        fill_rect(frame, dw, dh, grip_x + 4, grip_y + 2, 20, 1, [0x36, 0x45, 0x57, 255]);
    }

    /// Aktualisiert `quick_select_scroll` anhand der vertikalen Mausposition `vy` auf dem Scrollbalken.
    pub(crate) fn update_modern_playlist_scroll_from_y(&mut self, vy: i32) {
        let items_len = {
            let st = self.shared_state.lock().unwrap();
            st.playlist.len()
        };
        let max_vis = self.modern_playlist_rows.clamp(5, 36);
        let max_scroll = items_len.saturating_sub(max_vis);
        if max_scroll == 0 {
            self.quick_select_scroll = 0;
            return;
        }
        let track_top = 35;
        let track_h = 2 + (max_vis as i32) * 16;
        let th = (((max_vis as f64) / (items_len as f64)) * (track_h as f64)).round() as i32;
        let th = th.clamp(16, track_h);
        let eff_top = track_top + th / 2;
        let eff_span = (track_h - th).max(1);
        let ratio = ((vy - eff_top) as f64 / (eff_span as f64)).clamp(0.0, 1.0);
        self.quick_select_scroll = ((ratio * (max_scroll as f64)).round() as usize).min(max_scroll);
    }

    pub(crate) fn handle_modern_playlist_dock_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if vy < 24 {
            if vx >= (MODERN_PLAYLIST_W as i32 - 26) {
                self.close_window(WindowKind::QuickSelect);
                self.close_window(WindowKind::OpenDialog);
                return;
            }
            let _ = window.drag_window();
            return;
        }
        let max_vis = self.modern_playlist_rows.clamp(5, 36);
        let sb_bottom = 42 + (max_vis as i32) * 16;
        let list_bottom = 36 + (max_vis as i32) * 16;
        let btn_y = 48 + (max_vis as i32) * 16;

        // Klick auf den vertikalen Scrollbalken rechts (x = 555..574, y = 30..sb_bottom)
        if (30..=sb_bottom).contains(&vy) && (555..=574).contains(&vx) {
            self.playlist_scroll_dragging = true;
            self.update_modern_playlist_scroll_from_y(vy);
            return;
        }
        if (35..=list_bottom).contains(&vy) && (10..=554).contains(&vx) {
            let row = ((vy - 35) / 16).clamp(0, max_vis.saturating_sub(1) as i32) as usize;
            let orig_idx_opt = if self.quick_select_sort == 0 {
                let total_len = self.shared_state.lock().unwrap().playlist.len();
                let max_scroll = total_len.saturating_sub(max_vis);
                let scroll = self.quick_select_scroll.min(max_scroll);
                let idx = scroll + row;
                (idx < total_len).then_some(idx)
            } else {
                let st_snap = self.shared_state.lock().unwrap();
                let items = self.sorted_quick_select_items(&st_snap);
                let max_scroll = items.len().saturating_sub(max_vis);
                let scroll = self.quick_select_scroll.min(max_scroll);
                items.get(scroll + row).map(|(orig_idx, _)| *orig_idx)
            };
            if let Some(orig_idx) = orig_idx_opt {
                if vx >= 534 {
                    // Klick auf das kleine `x` am Zeilenende -> Eintrag sofort entfernen
                    self.playlist_selected.remove(&orig_idx);
                    let shifted: std::collections::BTreeSet<usize> = self
                        .playlist_selected
                        .iter()
                        .map(|&i| if i > orig_idx { i - 1 } else { i })
                        .collect();
                    self.playlist_selected = shifted;
                    self.playlist_reorder_drag = None;
                    self.remove_playlist_item_and_update_cursor(orig_idx);
                    return;
                }
                let ctrl = self.modifiers.state().control_key();
                let shift = self.modifiers.state().shift_key();
                if self.quick_select_all_selected {
                    let len = self.shared_state.lock().unwrap().playlist.len();
                    self.playlist_selected = (0..len).collect();
                    self.quick_select_all_selected = false;
                }

                let mut defer_collapse_idx = None;
                if is_double_click {
                    self.playlist_selected.clear();
                    self.playlist_selected.insert(orig_idx);
                    self.playlist_anchor = orig_idx;
                    self.quick_select_cursor = orig_idx;
                    self.playlist_reorder_drag = None;
                    let mut state = self.shared_state.lock().unwrap();
                    if let Some(path) = state.playlist.get(orig_idx).cloned() {
                        state.playlist_idx = orig_idx;
                        state.playing_track_detached = false;
                        if state.shuffle {
                            state.rebuild_number_list();
                            if let Some(pos) = state.number_list.iter().position(|&i| i == orig_idx) {
                                state.playlist_pos = pos;
                            }
                        } else {
                            state.playlist_pos = orig_idx;
                        }
                        state.pending_load_file = Some(path);
                        state.is_playing = true;
                        state.is_paused = false;
                    }
                    return;
                } else if shift {
                    if !ctrl {
                        self.playlist_selected.clear();
                    }
                    if self.quick_select_sort == 0 {
                        let a = self.playlist_anchor.min(orig_idx);
                        let b = self.playlist_anchor.max(orig_idx);
                        let len = self.shared_state.lock().unwrap().playlist.len();
                        for i in a..=b.min(len.saturating_sub(1)) {
                            self.playlist_selected.insert(i);
                        }
                    } else {
                        let st_snap = self.shared_state.lock().unwrap();
                        let items = self.sorted_quick_select_items(&st_snap);
                        let anchor_pos = items
                            .iter()
                            .position(|(idx, _)| *idx == self.playlist_anchor)
                            .unwrap_or(0);
                        let target_pos = items
                            .iter()
                            .position(|(idx, _)| *idx == orig_idx)
                            .unwrap_or(0);
                        let a = anchor_pos.min(target_pos);
                        let b = anchor_pos.max(target_pos);
                        for i in a..=b {
                            if let Some((oidx, _)) = items.get(i) {
                                self.playlist_selected.insert(*oidx);
                            }
                        }
                    }
                    self.quick_select_cursor = orig_idx;
                } else if ctrl {
                    if self.playlist_selected.is_empty() {
                        let cur = self.quick_select_cursor;
                        let len = self.shared_state.lock().unwrap().playlist.len();
                        if cur < len {
                            self.playlist_selected.insert(cur);
                        }
                    }
                    if self.playlist_selected.contains(&orig_idx) {
                        self.playlist_selected.remove(&orig_idx);
                    } else {
                        self.playlist_selected.insert(orig_idx);
                    }
                    self.playlist_anchor = orig_idx;
                    self.quick_select_cursor = orig_idx;
                } else if self.playlist_selected.len() > 1
                    && self.playlist_selected.contains(&orig_idx)
                {
                    // Klick auf bereits mehrfach ausgewählten Eintrag ohne Modifikator:
                    // Mehrfachauswahl zunächst behalten, damit sie per Drag & Drop als Block gezogen werden kann!
                    // Wird ohne Ziehen losgelassen, reduziert `finish_modern_playlist_reorder_drag` auf `orig_idx`.
                    defer_collapse_idx = Some(orig_idx);
                    self.quick_select_cursor = orig_idx;
                } else {
                    self.playlist_selected.clear();
                    self.playlist_selected.insert(orig_idx);
                    self.playlist_anchor = orig_idx;
                    self.quick_select_cursor = orig_idx;
                }

                if !self.playlist_selected.is_empty() {
                    self.playlist_reorder_drag = Some(PlaylistReorderDrag {
                        press_vy: vy,
                        active: false,
                        drop_slot: orig_idx,
                        defer_collapse_idx,
                    });
                }
            }
            return;
        }
        if (btn_y..=btn_y + 24).contains(&vy) {
            if (8..=104).contains(&vx) {
                let start_dir = self
                    .shared_state
                    .lock()
                    .unwrap()
                    .ini_config
                    .last_dir
                    .clone();
                if let Some(selected_paths) = native_open_files_dialog(&start_dir) {
                    if !selected_paths.is_empty() {
                        let new_cursor = {
                            let mut state = self.shared_state.lock().unwrap();
                            if let Some(parent) = selected_paths[0].parent() {
                                state.ini_config.last_dir = parent.to_string_lossy().into_owned();
                            }
                            let was_empty = state.playlist.is_empty();
                            state.handle_dropped_paths(&selected_paths, was_empty);
                            let _ = state
                                .ini_config
                                .save_to_file(&Mod4WinIniConfig::default_ini_path());
                            state.playlist_idx
                        };
                        self.quick_select_all_selected = false;
                        self.playlist_selected.clear();
                        self.playlist_selected.insert(new_cursor);
                        self.playlist_anchor = new_cursor;
                        self.quick_select_cursor = new_cursor;
                    }
                    return;
                }
                #[cfg(not(target_os = "windows"))]
                {
                    self.fsel_dlg = super::setup_dlg::FileSelectState::new(
                        FileSelectTarget::PlaylistAdd,
                        &start_dir,
                    );
                    self.open_window(event_loop, WindowKind::FileSelectDialog);
                }
                #[cfg(target_os = "windows")]
                let _ = event_loop;
                return;
            } else if (108..=170).contains(&vx) {
                // Remove-Button: entfernt Alle / Mehrfachauswahl / Einzeleintrag ohne Wiedergabeabbruch
                self.remove_selected_modern_playlist_items();
                return;
            } else if (174..=228).contains(&vx) {
                // Clear-Button: gesamte Playlist leeren (laufender Song spielt normal zu Ende)
                {
                    let mut st = self.shared_state.lock().unwrap();
                    st.clear_playlist();
                    let _ = st
                        .ini_config
                        .save_to_file(&Mod4WinIniConfig::default_ini_path());
                }
                self.quick_select_all_selected = false;
                self.playlist_selected.clear();
                self.playlist_reorder_drag = None;
                self.playlist_anchor = 0;
                self.quick_select_cursor = 0;
                self.quick_select_scroll = 0;
                return;
            } else if (232..=306).contains(&vx) {
                let start_dir = {
                    let st = self.shared_state.lock().unwrap();
                    if st.ini_config.last_playlist_dir.is_empty() {
                        st.ini_config.last_dir.clone()
                    } else {
                        st.ini_config.last_playlist_dir.clone()
                    }
                };
                if let Some(mol_path) = native_open_mol_dialog(&start_dir) {
                    let loaded = UiSharedState::dedup_playlist_vec(
                        super::archive_mol::load_mol_file(&mol_path),
                    );
                    let mut st = self.shared_state.lock().unwrap();
                    if let Some(parent) = mol_path.parent() {
                        st.ini_config.last_playlist_dir = parent.to_string_lossy().into_owned();
                    }
                    if !loaded.is_empty() {
                        st.playlist = loaded;
                        st.playlist_idx = 0;
                        st.playing_track_detached = false;
                        st.rebuild_number_list();
                        if st.ini_config.play_on_load
                            && let Some(first) = st.playlist.first().cloned() {
                                st.pending_load_file = Some(first);
                                st.is_playing = true;
                                st.is_paused = false;
                            }
                    }
                    st.sync_runtime_to_ini();
                    let fname = mol_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("PLAYLIST.MOL");
                    st.set_status_banner(format!("LOADED {}", fname.to_ascii_uppercase()));
                    let _ = st
                        .ini_config
                        .save_to_file(&Mod4WinIniConfig::default_ini_path());
                    drop(st);
                    self.quick_select_all_selected = false;
                    self.playlist_selected.clear();
                    self.playlist_reorder_drag = None;
                    self.playlist_anchor = 0;
                    self.quick_select_cursor = 0;
                    self.quick_select_scroll = 0;
                }
                return;
            } else if (310..=384).contains(&vx) {
                let (start_dir, playlist_snap) = {
                    let st = self.shared_state.lock().unwrap();
                    let dir = if st.ini_config.last_playlist_dir.is_empty() {
                        st.ini_config.last_dir.clone()
                    } else {
                        st.ini_config.last_playlist_dir.clone()
                    };
                    (dir, st.playlist.clone())
                };
                if let Some(mol_path) = native_save_mol_dialog(&start_dir) {
                    if super::archive_mol::save_mol_file(&mol_path, &playlist_snap).is_ok() {
                        let mut st = self.shared_state.lock().unwrap();
                        if let Some(parent) = mol_path.parent() {
                            st.ini_config.last_playlist_dir = parent.to_string_lossy().into_owned();
                        }
                        let fname = mol_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("PLAYLIST.MOL");
                        st.set_status_banner(format!("SAVED {}", fname.to_ascii_uppercase()));
                        let _ = st
                            .ini_config
                            .save_to_file(&Mod4WinIniConfig::default_ini_path());
                    }
                    return;
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let mol_path = PathBuf::from(&start_dir).join("PLAYLIST.MOL");
                    let _ = super::archive_mol::save_mol_file(&mol_path, &playlist_snap);
                }
                return;
            }
        }
        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // F. SETUP-FENSTER ([13] – 530 x 505 px)
    // =========================================================================
    pub(crate) fn render_modern_setup_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        let is_it_mod = state.is_it_module();
        let is_de = self.setup_cfg.language == 1 || self.locale.is_german();
        draw_modern_window_shell(frame, dw, dh, false, "[13] MOD4WIN Setup", false, false);

        // Oberes Hardware- & Mixing-Panel (x = 6, y = 30, w = 518, h = 178, radius = 3px)
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            6,
            30,
            518,
            178,
            3,
            [0x16, 0x1e, 0x28, 255],
            [0x12, 0x19, 0x21, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 6, 30, 518, 178, 3, [0x39, 0x4b, 0x61, 255]);

        // Grüner LCD-Statusstreifen (x = 14, y = 36, w = 502, h = 26)
        draw_modern_lcd_box(frame, dw, dh, 14, 36, 502, 26);
        let dev_txt = if self.setup_cfg.device_id == DEV_HDD {
            if is_de {
                "GERÄT: DIRECT TO DISK"
            } else {
                "DEVICE: DIRECT TO DISK"
            }
        } else if is_de {
            "GERÄT: WAVE MAPPER"
        } else {
            "DEVICE: WAVE MAPPER"
        };
        draw_modern_ui_text(frame, dw, dh, 20, 46, dev_txt, COL_LCD_BRIGHT);

        let sr_khz = ((self.setup_cfg.sample_rate + 500) / 1000).clamp(4, 99);
        let ch_short = if self.setup_cfg.channels < 2 {
            "MONO"
        } else if self.setup_cfg.surround {
            "SURROUND"
        } else {
            "STEREO"
        };
        let fmt_txt = format!(
            "FORMAT: {} kHz / {} Bit / {}",
            sr_khz, self.setup_cfg.bits_per_sample, ch_short
        );
        draw_modern_ui_text(frame, dw, dh, 180, 46, &fmt_txt, COL_LCD_BRIGHT);

        let num_buf = if self.setup_cfg.device_id == DEV_HDD {
            self.setup_cfg.hdd_rec_buffers.clamp(2, 99)
        } else {
            self.setup_cfg.output_buffers.clamp(2, 99)
        };
        let kb = (num_buf as u32 * if self.setup_cfg.bits_per_sample == 16 { 46 } else { 23 }).min(9999);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            376,
            46,
            if is_de { "MIN. SPEICHER:" } else { "MIN. MEMORY:" },
            COL_LCD_BRIGHT,
        );
        draw_modern_seg_value(
            frame,
            dw,
            dh,
            454,
            44,
            "8888",
            &format!("{:04}", kb),
            false,
        );
        draw_modern_ui_text(frame, dw, dh, 490, 46, "KB", COL_LCD_BRIGHT);

        // 2x3 Kachel-Grid (`hw-card`, radius = 3px, x = 14, y = 68, 3 Spalten a 164px, 2 Zeilen a 48px)
        let cards = [
            (
                if is_de {
                    "Ausgabegerät"
                } else {
                    "Output Device"
                },
                if self.setup_cfg.device_id == DEV_HDD {
                    "Direct to Disk".to_string()
                } else {
                    "Wave Mapper".to_string()
                },
                false,
            ),
            (
                if is_de { "Samplerate" } else { "Sample Rate" },
                format!("{} kHz", sr_khz),
                true,
            ),
            (
                if is_de {
                    "Sampling-Auflösung"
                } else {
                    "Sampling Resolution"
                },
                format!("{} Bit", self.setup_cfg.bits_per_sample),
                false,
            ),
            (
                if is_de {
                    "Kanal-Modus"
                } else {
                    "Channel Mode"
                },
                if self.setup_cfg.channels < 2 {
                    "Mono (O)".to_string()
                } else if self.setup_cfg.surround {
                    "Surround ((OO))".to_string()
                } else {
                    "Stereo (OO)".to_string()
                },
                false,
            ),
            (
                if is_de {
                    "Puffer (1-99)"
                } else {
                    "Buffers (1-99)"
                },
                format!("{} Buf ({:04} KB)", num_buf, kb),
                true,
            ),
            (
                if is_it_mod {
                    "IDO / IT Resampling"
                } else {
                    "IDO Resampling"
                },
                match if is_it_mod {
                    self.setup_cfg.over_sampling
                } else {
                    self.setup_cfg.over_sampling.min(3)
                } {
                    0 => if is_de { "IDO 0: Aus" } else { "IDO 0: Off" }.to_string(),
                    1 => "IDO 1: Linear".to_string(),
                    2 => if is_de {
                        "IDO 2: Kub. CPU"
                    } else {
                        "IDO 2: Cub. CPU"
                    }
                    .to_string(),
                    3 => if is_de {
                        "IDO 3: Kub. FPU"
                    } else {
                        "IDO 3: Cub. FPU"
                    }
                    .to_string(),
                    4 => "IT: Non-Interp.".to_string(),
                    5 => "IT: Linear".to_string(),
                    _ => "IT: Cubic Spline".to_string(),
                },
                true,
            ),
        ];

        for (i, (lbl, val, has_step)) in cards.iter().enumerate() {
            let col = (i % 3) as i32;
            let row = (i / 3) as i32;
            let cx = 14 + col * 169;
            let cy = 68 + row * 52;
            fill_rounded_rect_grad(
                frame,
                dw,
                dh,
                cx,
                cy,
                164,
                48,
                3,
                [0x1c, 0x26, 0x33, 255],
                [0x17, 0x20, 0x2b, 255],
            );
            stroke_rounded_rect(frame, dw, dh, cx, cy, 164, 48, 3, [0x33, 0x43, 0x57, 255]);
            draw_modern_ui_text(frame, dw, dh, cx + 6, cy + 6, lbl, COL_TEXT_MUTED);
            if *has_step {
                let (minus_on, plus_on) = match i {
                    1 => (
                        self.pressed_btn == Some(PressedButton::SetupSrDown),
                        self.pressed_btn == Some(PressedButton::SetupSrUp),
                    ),
                    4 => (
                        self.pressed_btn == Some(PressedButton::SetupBufDown),
                        self.pressed_btn == Some(PressedButton::SetupBufUp),
                    ),
                    5 => (
                        self.pressed_btn == Some(PressedButton::SetupIdoDown),
                        self.pressed_btn == Some(PressedButton::SetupIdoUp),
                    ),
                    _ => (false, false),
                };
                draw_modern_hw_btn(frame, dw, dh, cx + 6, cy + 21, 20, 21, "-", minus_on);
                draw_modern_lcd_box(frame, dw, dh, cx + 29, cy + 21, 106, 21);
                let vw = modern_ui_text_width(val);
                let vx_txt = cx + 29 + ((106 - vw) / 2).max(4);
                draw_modern_ui_text(frame, dw, dh, vx_txt, cy + 28, val, COL_LCD_BRIGHT);
                draw_modern_hw_btn(frame, dw, dh, cx + 138, cy + 21, 20, 21, "+", plus_on);
            } else {
                draw_modern_lcd_box(frame, dw, dh, cx + 6, cy + 21, 152, 21);
                let vw = modern_ui_text_width(val);
                let vx_txt = cx + 6 + ((152 - vw) / 2).max(6);
                draw_modern_ui_text(frame, dw, dh, vx_txt, cy + 28, val, COL_LCD_BRIGHT);
            }
        }

        // 3 Direkt-Toggles darunter (y = 176, h = 24): PAN | Clip | Load State
        let toggles = [
            (
                if self.setup_cfg.enable_panning {
                    if is_de { "PAN: Ein" } else { "PAN: On" }
                } else if is_de {
                    "PAN: Aus"
                } else {
                    "PAN: Off"
                },
                self.setup_cfg.enable_panning,
            ),
            (
                if self.setup_cfg.declick {
                    if is_de { "CL+DC: Ein" } else { "CL+DC: On" }
                } else if self.setup_cfg.clipping {
                    if is_de { "Clip: Ein" } else { "Clip: On" }
                } else if is_de {
                    "Clip: Aus"
                } else {
                    "Clip: Off"
                },
                self.setup_cfg.clipping || self.setup_cfg.declick,
            ),
            (
                if self.setup_cfg.state_saver {
                    if is_de {
                        "Status laden: Ein"
                    } else {
                        "Load State: On"
                    }
                } else if is_de {
                    "Status laden: Aus"
                } else {
                    "Load State: Off"
                },
                self.setup_cfg.state_saver,
            ),
        ];
        for (col, (txt, on)) in toggles.iter().enumerate() {
            let tx = 14 + (col as i32) * 169;
            draw_modern_lcd_box(frame, dw, dh, tx, 176, 164, 24);
            let fg = if *on {
                COL_LCD_BRIGHT
            } else {
                [0x2b, 0x6e, 0x44, 255]
            };
            let tw = modern_ui_text_width(txt);
            let tx_lbl = tx + ((164 - tw) / 2).max(6);
            draw_modern_ui_text(frame, dw, dh, tx_lbl, 185, txt, fg);
        }

        // 7 Konfigurations-Zeilen (`setup-line`, radius = 3px, y = 208..374) inkl. expliziter .IT-Mixer-Charakteristik
        let on_txt = if is_de { "Ein" } else { "On" };
        let off_txt = if is_de { "Aus" } else { "Off" };
        let peak_txt = match (
            self.setup_cfg.pause_while_setup,
            self.setup_cfg.auto_preamp_peaksearch,
        ) {
            (false, true) => if is_de { "PeakS: Ein" } else { "PeakS: On" },
            (true, true) => "Pause+PeakS",
            _ => off_txt,
        };
        let it_mixer_txt = if !is_it_mod {
            if is_de {
                "Nur für .IT-Module"
            } else {
                "Only for .IT modules"
            }
        } else {
            match self.setup_cfg.over_sampling {
                0..=3 => "MOD4WIN IDO (0..3)",
                4 => "Native IT (Off)",
                5 => "Native IT (Linear)",
                _ => "Native IT (Cubic)",
            }
        };
        let cpu_prof = CpuProfile::from_u8(self.setup_cfg.cpu_sim_profile);
        let cpu_prof_txt = cpu_prof.display_name();
        let rows = [
            (
                if is_de {
                    "Engine-Modus (Clean / Full Emulation)"
                } else {
                    "Engine Mode (Clean / Full Emulation)"
                },
                if self.setup_cfg.clean_mode {
                    "Clean Mode"
                } else {
                    "Emulation Mode"
                },
                true,
            ),
            (
                if is_de {
                    ".IT Mixer-Charakteristik (MOD4WIN IDO vs. Native IT)"
                } else {
                    ".IT Mixer Mode (MOD4WIN IDO vs. Native IT)"
                },
                it_mixer_txt,
                is_it_mod,
            ),
            (
                if is_de {
                    "Retro-CPU-Auslastungssimulation (Links/Rechtsklick)"
                } else {
                    "Retro CPU Load Simulation (Left/Right click)"
                },
                cpu_prof_txt,
                self.setup_cfg.cpu_sim_profile != 0,
            ),
            (
                if is_de {
                    "Historisches 1995er DMA-Ringpuffer-Stottern"
                } else {
                    "Historic 1995 DMA Ring-Buffer Stutter"
                },
                if self.setup_cfg.authentic_stutter {
                    on_txt
                } else {
                    off_txt
                },
                self.setup_cfg.authentic_stutter,
            ),
            (
                if is_de {
                    "Samplerate in 1-kHz-Schritten ändern"
                } else {
                    "Change sample rate in 1 kHz steps"
                },
                if self.setup_cfg.override_rate_change {
                    on_txt
                } else {
                    off_txt
                },
                self.setup_cfg.override_rate_change,
            ),
            (
                if is_de {
                    "Pause im Setup / Auto-PreAmp PeakSearch"
                } else {
                    "Pause in Setup / Auto-PreAmp PeakSearch"
                },
                peak_txt,
                self.setup_cfg.auto_preamp_peaksearch || self.setup_cfg.pause_while_setup,
            ),
            (
                if is_de {
                    "Modul nach Open / beim Start sofort abspielen"
                } else {
                    "Play module immediately on load / startup"
                },
                if self.setup_cfg.play_on_load {
                    on_txt
                } else {
                    off_txt
                },
                self.setup_cfg.play_on_load,
            ),
            (
                if is_de {
                    "Fensterbewegung per Mausklick"
                } else {
                    "Move windows by mouse click"
                },
                if self.setup_cfg.move_on_click {
                    if is_de { "Überall" } else { "Anywhere" }
                } else if is_de {
                    "Nur Titelleiste"
                } else {
                    "Title bar only"
                },
                true,
            ),
            (
                if is_de {
                    "Immer nach WAVE-Dateinamen fragen"
                } else {
                    "Always prompt for WAVE filename"
                },
                if self.setup_cfg.ask_hd { on_txt } else { off_txt },
                self.setup_cfg.ask_hd,
            ),
        ];

        for (i, (lbl, val, on)) in rows.iter().enumerate() {
            let ry = 204 + (i as i32) * 20;
            fill_rounded_rect_grad(
                frame,
                dw,
                dh,
                6,
                ry,
                518,
                18,
                3,
                [0x1a, 0x23, 0x2e, 255],
                [0x16, 0x1e, 0x28, 255],
            );
            stroke_rounded_rect(frame, dw, dh, 6, ry, 518, 18, 3, [0x2d, 0x3c, 0x4f, 255]);
            draw_modern_ui_text(frame, dw, dh, 14, ry + 5, lbl, COL_TEXT_UI);
            draw_modern_lcd_box(frame, dw, dh, 366, ry + 1, 150, 16);
            let fg = if *on {
                COL_LCD_BRIGHT
            } else {
                [0x2b, 0x6e, 0x44, 255]
            };
            let vw = modern_ui_text_width(val);
            let vx_val = 366 + ((150 - vw) / 2).max(6);
            draw_modern_ui_text(frame, dw, dh, vx_val, ry + 5, val, fg);
        }

        // DTD Recording Path Box (x = 6, y = 386, w = 518, h = 58, radius = 3px)
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            6,
            386,
            518,
            58,
            3,
            [0x16, 0x1e, 0x28, 255],
            [0x12, 0x19, 0x22, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 6, 386, 518, 58, 3, [0x2d, 0x3c, 0x4f, 255]);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            14,
            394,
            if is_de {
                "DTD-Aufnahmepfad (Direct-to-Disk WAV-Ausgabe):"
            } else {
                "DTD Recording Path (Direct-to-Disk WAV Output):"
            },
            COL_TEXT_MUTED,
        );
        draw_modern_lcd_box(frame, dw, dh, 14, 410, 412, 26);
        let hd_disp: String = self.setup_cfg.hd_path.chars().take(65).collect();
        draw_modern_ui_text(frame, dw, dh, 20, 419, &hd_disp, COL_LCD_BRIGHT);
        draw_modern_hw_btn(
            frame,
            dw,
            dh,
            432,
            410,
            84,
            26,
            if is_de { "Ordner…" } else { "Browse…" },
            false,
        );

        // Fußzeile (y = 452, h = 30): Hotkeys | Lang | Skin ... SAVE | QUIT | OK
        draw_modern_hw_btn(frame, dw, dh, 6, 452, 74, 30, "Hotkeys", false);
        let lang_lbl = if is_de {
            "Lang: DEUTSCH"
        } else {
            "Lang: ENGLISH"
        };
        draw_modern_hw_btn(frame, dw, dh, 84, 452, 112, 30, lang_lbl, false);
        let skin_mode = SkinMode::from_u8(self.setup_cfg.skin_mode);
        let skin_lbl = format!("Skin: {}", skin_mode.short_setup_label());
        draw_modern_hw_btn(frame, dw, dh, 200, 452, 98, 30, &skin_lbl, true);

        draw_modern_hw_btn(frame, dw, dh, 336, 452, 60, 30, "SAVE", false);
        draw_modern_hw_btn(frame, dw, dh, 400, 452, 60, 30, "QUIT", false);
        draw_modern_hw_btn(frame, dw, dh, 464, 452, 60, 30, "OK", true);
    }

    pub(crate) fn handle_modern_setup_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        button: MouseButton,
        window: &Arc<Window>,
    ) {
        if vy < 24 {
            if button == MouseButton::Left {
                if vx >= (MODERN_SETUP_W as i32 - 26) {
                    self.cancel_setup_dialog();
                    return;
                }
                let _ = window.drag_window();
            }
            return;
        }

        // Rechtsklick ist im Setup-Dialog für die Direkt-Toggles (Clip/CL+DC rückwärts) und die Konfigurations-Zeilen relevant
        if button == MouseButton::Right {
            if (176..=200).contains(&vy) && (14..=516).contains(&vx) {
                let col = ((vx - 14) / 169).clamp(0, 2);
                if col == 1 {
                    match (self.setup_cfg.clipping, self.setup_cfg.declick) {
                        (false, _) => {
                            self.setup_cfg.clipping = true;
                            self.setup_cfg.declick = true;
                        }
                        (true, true) => {
                            self.setup_cfg.clipping = true;
                            self.setup_cfg.declick = false;
                        }
                        (true, false) => {
                            self.setup_cfg.clipping = false;
                            self.setup_cfg.declick = false;
                        }
                    }
                    self.sync_live_setup_to_state();
                }
                return;
            }
            if (204..=383).contains(&vy) && (6..=524).contains(&vx) {
                let row = ((vy - 204) / 20).clamp(0, 8);
                match row {
                    0 => self.setup_cfg.clean_mode = !self.setup_cfg.clean_mode,
                    1 => {
                        let is_it_mod = self.shared_state.lock().unwrap().is_it_module();
                        if is_it_mod {
                            self.setup_cfg.over_sampling = match self.setup_cfg.over_sampling {
                                6 => 3,
                                5 => 6,
                                4 => 5,
                                _ => 4,
                            };
                        }
                    }
                    2 => {
                        let prev_prof = CpuProfile::from_u8(self.setup_cfg.cpu_sim_profile).prev();
                        self.setup_cfg.cpu_sim_profile = prev_prof.to_u8();
                    }
                    3 => self.setup_cfg.authentic_stutter = !self.setup_cfg.authentic_stutter,
                    4 => self.setup_cfg.override_rate_change = !self.setup_cfg.override_rate_change,
                    5 => self.setup_cfg.auto_preamp_peaksearch = !self.setup_cfg.auto_preamp_peaksearch,
                    6 => self.setup_cfg.play_on_load = !self.setup_cfg.play_on_load,
                    7 => self.setup_cfg.move_on_click = !self.setup_cfg.move_on_click,
                    _ => self.setup_cfg.ask_hd = !self.setup_cfg.ask_hd,
                }
                self.sync_live_setup_to_state();
            }
            return;
        }

        // 2x3 Hardware-Kacheln (y = 68..172)
        if (68..=172).contains(&vy) && (14..=516).contains(&vx) {
            let col = ((vx - 14) / 169).clamp(0, 2);
            let row = if vy < 120 { 0 } else { 1 };
            let cx = 14 + col * 169;
            match (row, col) {
                (0, 0) => {
                    self.setup_cfg.device_id = if self.setup_cfg.device_id == DEV_HDD {
                        DEV_SOUNDCARD
                    } else {
                        DEV_HDD
                    };
                    self.sync_live_setup_to_state();
                }
                (0, 1) => {
                    if vx < cx + 28 {
                        self.pressed_btn = Some(PressedButton::SetupSrDown);
                        self.step_setup_samplerate(-1);
                    } else if vx >= cx + 136 {
                        self.pressed_btn = Some(PressedButton::SetupSrUp);
                        self.step_setup_samplerate(1);
                    } else {
                        self.step_setup_samplerate(1);
                    }
                    self.sync_live_setup_to_state();
                }
                (0, 2) => {
                    self.setup_cfg.bits_per_sample =
                        if self.setup_cfg.bits_per_sample == 8 { 16 } else { 8 };
                    self.sync_live_setup_to_state();
                }
                (1, 0) => {
                    if self.setup_cfg.channels < 2 {
                        self.setup_cfg.channels = 2;
                        self.setup_cfg.surround = false;
                    } else if !self.setup_cfg.surround {
                        self.setup_cfg.channels = 2;
                        self.setup_cfg.surround = true;
                    } else {
                        self.setup_cfg.channels = 1;
                        self.setup_cfg.surround = false;
                    }
                    self.sync_live_setup_to_state();
                }
                (1, 1) => {
                    let delta = if vx < cx + 28 {
                        self.pressed_btn = Some(PressedButton::SetupBufDown);
                        -1
                    } else {
                        if vx >= cx + 136 {
                            self.pressed_btn = Some(PressedButton::SetupBufUp);
                        }
                        1
                    };
                    if self.setup_cfg.device_id == DEV_HDD {
                        self.setup_cfg.hdd_rec_buffers =
                            (self.setup_cfg.hdd_rec_buffers as i32 + delta).clamp(1, 99) as u8;
                    } else {
                        self.setup_cfg.output_buffers =
                            (self.setup_cfg.output_buffers as i32 + delta).clamp(2, 99) as u8;
                    }
                    self.sync_live_setup_to_state();
                }
                _ => {
                    let is_it_mod = self.shared_state.lock().unwrap().is_it_module();
                    if vx < cx + 28 {
                        self.pressed_btn = Some(PressedButton::SetupIdoDown);
                    } else if vx >= cx + 136 {
                        self.pressed_btn = Some(PressedButton::SetupIdoUp);
                    }
                    if is_it_mod {
                        let cur = self.setup_cfg.over_sampling.min(6);
                        self.setup_cfg.over_sampling = if vx < cx + 28 {
                            (cur + 6) % 7
                        } else {
                            (cur + 1) % 7
                        };
                    } else {
                        let cur = self.setup_cfg.over_sampling.min(3);
                        self.setup_cfg.over_sampling = if vx < cx + 28 {
                            (cur + 3) % 4
                        } else {
                            (cur + 1) % 4
                        };
                    }
                    self.sync_live_setup_to_state();
                }
            }
            return;
        }

        // 3 Direkt-Toggles (y = 176..200)
        if (176..=200).contains(&vy) && (14..=516).contains(&vx) {
            let col = ((vx - 14) / 169).clamp(0, 2);
            match col {
                0 => self.setup_cfg.enable_panning = !self.setup_cfg.enable_panning,
                1 => {
                    match (self.setup_cfg.clipping, self.setup_cfg.declick) {
                        (false, _) => {
                            self.setup_cfg.clipping = true;
                            self.setup_cfg.declick = false;
                        }
                        (true, false) => {
                            self.setup_cfg.clipping = true;
                            self.setup_cfg.declick = true;
                        }
                        (true, true) => {
                            self.setup_cfg.clipping = false;
                            self.setup_cfg.declick = false;
                        }
                    }
                    if let Ok(mut st) = self.shared_state.lock() {
                        st.sound_settings.set_declick(self.setup_cfg.declick);
                        st.ini_config.declick = self.setup_cfg.declick;
                    }
                }
                _ => self.setup_cfg.state_saver = !self.setup_cfg.state_saver,
            }
            self.sync_live_setup_to_state();
            return;
        }

        // 9 Konfigurations-Zeilen (y = 204..383)
        if (204..=383).contains(&vy) && (6..=524).contains(&vx) {
            let row = ((vy - 204) / 20).clamp(0, 8);
            match row {
                0 => self.setup_cfg.clean_mode = !self.setup_cfg.clean_mode,
                1 => {
                    // Explizite Auswahl für .IT Mixer-Charakteristik (nur aktiv, wenn ein .IT-Modul geladen ist)
                    let is_it_mod = self.shared_state.lock().unwrap().is_it_module();
                    if is_it_mod {
                        self.setup_cfg.over_sampling = match self.setup_cfg.over_sampling {
                            0..=3 => 6, // Native IT (Cubic Spline)
                            6 => 5,     // Native IT (Linear)
                            5 => 4,     // Native IT (Non-Interpolated)
                            _ => 3,     // MOD4WIN IDO 3 (Kubisch FPU)
                        };
                    }
                }
                2 => {
                    let next_prof = CpuProfile::from_u8(self.setup_cfg.cpu_sim_profile).next();
                    self.setup_cfg.cpu_sim_profile = next_prof.to_u8();
                }
                3 => self.setup_cfg.authentic_stutter = !self.setup_cfg.authentic_stutter,
                4 => self.setup_cfg.override_rate_change = !self.setup_cfg.override_rate_change,
                5 => match (
                    self.setup_cfg.pause_while_setup,
                    self.setup_cfg.auto_preamp_peaksearch,
                ) {
                    (false, true) => {
                        self.setup_cfg.pause_while_setup = true;
                        self.setup_cfg.auto_preamp_peaksearch = true;
                    }
                    (true, true) => {
                        self.setup_cfg.pause_while_setup = false;
                        self.setup_cfg.auto_preamp_peaksearch = false;
                    }
                    _ => {
                        self.setup_cfg.pause_while_setup = false;
                        self.setup_cfg.auto_preamp_peaksearch = true;
                    }
                },
                6 => self.setup_cfg.play_on_load = !self.setup_cfg.play_on_load,
                7 => self.setup_cfg.move_on_click = !self.setup_cfg.move_on_click,
                _ => self.setup_cfg.ask_hd = !self.setup_cfg.ask_hd,
            }
            self.sync_live_setup_to_state();
            return;
        }

        // DTD Recording Path (y = 410..436)
        if (410..=436).contains(&vy) {
            if (14..=426).contains(&vx) {
                self.setup_edit_focus = SetupEditFocus::RecPath;
                return;
            } else if (432..=516).contains(&vx) {
                if let Some(chosen_dir) = native_browse_folder_dialog(&self.setup_cfg.hd_path) {
                    let mut p = chosen_dir.to_string_lossy().into_owned();
                    if !p.ends_with('\\') && !p.ends_with('/') {
                        p.push('\\');
                    }
                    self.setup_cfg.hd_path = p;
                    self.sync_live_setup_to_state();
                    return;
                }
                #[cfg(any(not(target_os = "windows"), feature = "retro-win32"))]
                {
                    self.fsel_dlg = super::setup_dlg::FileSelectState::new(
                        FileSelectTarget::SetupRecDir,
                        &self.setup_cfg.hd_path,
                    );
                    self.open_window(event_loop, WindowKind::FileSelectDialog);
                }
                #[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
                let _ = event_loop;
                return;
            }
        }

        // Fußzeile (y = 452..482)
        if (452..=482).contains(&vy) {
            if (6..=80).contains(&vx) {
                self.open_window(event_loop, WindowKind::HotkeyEditor);
                return;
            } else if (84..=196).contains(&vx) {
                self.setup_cfg.language = if self.setup_cfg.language == 1 { 0 } else { 1 };
                self.setup_backup_cfg.language = self.setup_cfg.language;
                self.sync_live_setup_to_state();
                return;
            } else if (200..=298).contains(&vx) {
                let next_skin = SkinMode::from_u8(self.setup_cfg.skin_mode).next();
                self.setup_cfg.skin_mode = next_skin.to_u8();
                self.setup_backup_cfg.skin_mode = self.setup_cfg.skin_mode;
                self.sync_live_setup_to_state();
                return;
            } else if (336..=396).contains(&vx) {
                self.commit_setup_dialog(true);
                self.close_window(WindowKind::SetupDialog);
                return;
            } else if (400..=460).contains(&vx) {
                self.setup_cfg = self.setup_backup_cfg.clone();
                self.sync_live_setup_to_state();
                self.close_window(WindowKind::SetupDialog);
                return;
            } else if (464..=524).contains(&vx) {
                self.commit_setup_dialog(false);
                self.close_window(WindowKind::SetupDialog);
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // G. ABOUT-FENSTER ([5] – 360 x 185 px)
    // =========================================================================
    pub(crate) fn render_modern_about_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        _state: &UiSharedState,
    ) {
        draw_modern_window_shell(frame, dw, dh, false, "[5] About MOD4WIN 3.11", false, false);

        // Obere Box mit 2 animierten Lautsprecher-Konussen links/rechts + MOD 4 WIN Pill
        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            10,
            34,
            340,
            88,
            3,
            [0x15, 0x1c, 0x26, 255],
            [0x10, 0x16, 0x1e, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 10, 34, 340, 88, 3, [0x32, 0x42, 0x54, 255]);

        let t = self.about_opened_at.elapsed().as_secs_f32();
        let pulse_r = (11.0 + (t * 8.0).sin() * 4.0).round() as i32;
        for &cx in &[44i32, 316i32] {
            let cy = 78i32;
            draw_circle_ring(frame, dw, dh, cx, cy, 18, [0x5c, 0x74, 0x8f, 255]);
            draw_circle_filled(frame, dw, dh, cx, cy, pulse_r, COL_CYAN_BRAND);
        }

        fill_rounded_rect_grad(
            frame,
            dw,
            dh,
            116,
            52,
            128,
            22,
            2,
            [0x0a, 0x2c, 0x38, 255],
            [0x06, 0x1c, 0x24, 255],
        );
        stroke_rounded_rect(frame, dw, dh, 116, 52, 128, 22, 2, [0x00, 0xa2, 0xb8, 255]);
        draw_modern_ui_text(frame, dw, dh, 146, 60, "MOD4WIN 3.11", COL_CYAN_BRAND);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            102,
            88,
            "Original (c) 1993-96 by Kay Bruns",
            [0x8f, 0xa6, 0xbf, 255],
        );

        // Untere LCD-Box: 2026 Rust Edition
        draw_modern_lcd_box(frame, dw, dh, 10, 130, 340, 42);
        draw_modern_ui_text(
            frame,
            dw,
            dh,
            78,
            148,
            "2026 Rust Edition (Win95 i586 / x86_64)",
            COL_LCD_BRIGHT,
        );
    }
}

fn draw_circle_filled(frame: &mut [u8], dw: u32, dh: u32, cx: i32, cy: i32, r: i32, col: [u8; 4]) {
    let r2 = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r2 {
                put_px(frame, dw, dh, cx + dx, cy + dy, col);
            }
        }
    }
}

fn draw_circle_ring(frame: &mut [u8], dw: u32, dh: u32, cx: i32, cy: i32, r: i32, col: [u8; 4]) {
    let r_in2 = (r - 2).max(0) * (r - 2).max(0);
    let r_out2 = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 >= r_in2 && d2 <= r_out2 {
                put_px(frame, dw, dh, cx + dx, cy + dy, col);
            }
        }
    }
}

// =============================================================================
// NATIVE WINDOWS SYSTEM-DIALOGE (GetOpenFileNameW, GetSaveFileNameW, SHBrowseForFolderW)
// =============================================================================

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: usize,
    h_instance: usize,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: usize,
    lp_template_name: *const u16,
    pv_reserved: *mut core::ffi::c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
#[repr(C)]
struct BrowseInfoW {
    hwnd_owner: usize,
    pidl_root: *const core::ffi::c_void,
    psz_display_name: *mut u16,
    lpsz_title: *const u16,
    ul_flags: u32,
    lpfn: usize,
    l_param: isize,
    i_image: i32,
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(lpofn: *mut OpenFileNameW) -> i32;
    fn GetSaveFileNameW(lpofn: *mut OpenFileNameW) -> i32;
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHBrowseForFolderW(lpbi: *mut BrowseInfoW) -> *mut core::ffi::c_void;
    fn SHGetPathFromIDListW(pidl: *const core::ffi::c_void, psz_path: *mut u16) -> i32;
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(pv: *mut core::ffi::c_void);
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
fn to_wide_null(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
#[repr(C)]
struct OpenFileNameA {
    l_struct_size: u32,
    hwnd_owner: usize,
    h_instance: usize,
    lpstr_filter: *const u8,
    lpstr_custom_filter: *mut u8,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u8,
    n_max_file: u32,
    lpstr_file_title: *mut u8,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u8,
    lpstr_title: *const u8,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u8,
    l_cust_data: isize,
    lpfn_hook: usize,
    lp_template_name: *const u8,
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameA(lpofn: *mut OpenFileNameA) -> i32;
    fn GetSaveFileNameA(lpofn: *mut OpenFileNameA) -> i32;
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
fn to_ansi_null(s: &str) -> Vec<u8> {
    crate::win32s_shim::utf8_to_ansi_z(s)
}

/// Öffnet den nativen Dateidialog (`IFileOpenDialog` unter Windows / XDG-Portal unter Linux via `rfd`)
/// mit unbegrenzter Mehrfachauswahl (`IShellItemArray` ohne `32768`-Zeichen-Pufferlimit!) für Module, Archive und `.MOL`.
#[cfg(all(any(target_os = "windows", target_os = "linux"), not(feature = "retro-win32")))]
fn native_open_files_dialog(initial_dir: &str) -> Option<Vec<PathBuf>> {
    let mut dlg = rfd::FileDialog::new()
        .set_title("Add Tracker Modules / Archives / .MOL Playlists")
        .add_filter(
            "Tracker Modules, Archives & Playlists",
            &[
                "mod", "xm", "s3m", "mtm", "669", "stm", "far", "okt", "wow", "ult", "it", "umx",
                "zip", "arj", "lha", "lzh", "mol", "MOD", "XM", "S3M", "MTM", "669", "STM", "FAR",
                "OKT", "WOW", "ULT", "IT", "UMX", "ZIP", "ARJ", "LHA", "LZH", "MOL",
            ],
        )
        .add_filter("All Files", &["*"]);
    if !initial_dir.is_empty() {
        let p = std::path::Path::new(initial_dir);
        if p.is_dir() {
            dlg = dlg.set_directory(p);
        }
    }
    #[cfg(target_os = "windows")]
    {
        Some(dlg.pick_files().unwrap_or_default())
    }
    #[cfg(not(target_os = "windows"))]
    {
        dlg.pick_files()
    }
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
fn native_open_files_dialog(initial_dir: &str) -> Option<Vec<PathBuf>> {
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;

    let filter = b"Tracker Modules (*.mod;*.xm;*.s3m;*.it;*.stm;*.mtm;*.669;*.ult;*.far;*.wow;*.okt;*.zip;*.arj;*.lha;*.lzh;*.mol)\0*.mod;*.xm;*.s3m;*.it;*.umx;*.stm;*.mtm;*.669;*.ult;*.far;*.wow;*.okt;*.zip;*.arj;*.lha;*.lzh;*.mol\0All Files (*.*)\0*.*\0\0";
    let init_dir_a = to_ansi_null(initial_dir);
    let title_a = to_ansi_null("Open Tracker Module / Archive / Playlist");
    let mut file_buf = vec![0u8; 4096];

    let mut ofn = OpenFileNameA {
        l_struct_size: std::mem::size_of::<OpenFileNameA>() as u32,
        hwnd_owner: 0,
        h_instance: 0,
        lpstr_filter: filter.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buf.as_mut_ptr(),
        n_max_file: file_buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: if initial_dir.is_empty() {
            std::ptr::null()
        } else {
            init_dir_a.as_ptr()
        },
        lpstr_title: title_a.as_ptr(),
        flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: std::ptr::null(),
        l_cust_data: 0,
        lpfn_hook: 0,
        lp_template_name: std::ptr::null(),
    };

    let ok = unsafe { GetOpenFileNameA(&mut ofn) };
    if ok != 0 {
        let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
        if len > 0 {
            return Some(vec![PathBuf::from(crate::win32s_shim::ansi_bytes_to_string(
                &file_buf[..len],
            ))]);
        }
    }
    Some(Vec::new())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
fn native_open_files_dialog(_initial_dir: &str) -> Option<Vec<PathBuf>> {
    None
}

/// Öffnet den nativen Windows-Datei-Öffnen-Dialog (`GetOpenFileNameW`) für `.MOL`-Playlists.
#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
fn native_open_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
    const OFN_EXPLORER: u32 = 0x0008_0000;

    let filter_str = "MOD4WIN Playlist (*.mol)\0*.mol\0All Files (*.*)\0*.*\0\0";
    let filter_w: Vec<u16> = filter_str.encode_utf16().collect();
    let init_dir_w = to_wide_null(initial_dir);
    let title_w = to_wide_null("Load MOD4WIN Playlist (.MOL)");
    let def_ext_w = to_wide_null("mol");
    let mut file_buf = vec![0u16; 1024];

    let mut ofn = OpenFileNameW {
        l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
        hwnd_owner: 0,
        h_instance: 0,
        lpstr_filter: filter_w.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buf.as_mut_ptr(),
        n_max_file: file_buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: if initial_dir.is_empty() {
            std::ptr::null()
        } else {
            init_dir_w.as_ptr()
        },
        lpstr_title: title_w.as_ptr(),
        flags: OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_HIDEREADONLY
            | OFN_NOCHANGEDIR,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: def_ext_w.as_ptr(),
        l_cust_data: 0,
        lpfn_hook: 0,
        lp_template_name: std::ptr::null(),
        pv_reserved: std::ptr::null_mut(),
        dw_reserved: 0,
        flags_ex: 0,
    };

    let ok = unsafe { GetOpenFileNameW(&mut ofn) };
    if ok != 0 {
        let len = file_buf.iter().position(|&c| c == 0).unwrap_or(0);
        if len > 0 {
            return Some(PathBuf::from(String::from_utf16_lossy(&file_buf[..len])));
        }
    }
    None
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
fn native_open_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;

    let filter = b"MOD4WIN Playlist (*.mol)\0*.mol\0All Files (*.*)\0*.*\0\0";
    let init_dir_a = to_ansi_null(initial_dir);
    let title_a = to_ansi_null("Load MOD4WIN Playlist (.MOL)");
    let def_ext_a = to_ansi_null("mol");
    let mut file_buf = vec![0u8; 1024];

    let mut ofn = OpenFileNameA {
        l_struct_size: std::mem::size_of::<OpenFileNameA>() as u32,
        hwnd_owner: 0,
        h_instance: 0,
        lpstr_filter: filter.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buf.as_mut_ptr(),
        n_max_file: file_buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: if initial_dir.is_empty() {
            std::ptr::null()
        } else {
            init_dir_a.as_ptr()
        },
        lpstr_title: title_a.as_ptr(),
        flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: def_ext_a.as_ptr(),
        l_cust_data: 0,
        lpfn_hook: 0,
        lp_template_name: std::ptr::null(),
    };

    let ok = unsafe { GetOpenFileNameA(&mut ofn) };
    if ok != 0 {
        let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
        if len > 0 {
            return Some(PathBuf::from(crate::win32s_shim::ansi_bytes_to_string(
                &file_buf[..len],
            )));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn native_open_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new()
        .set_title("Load MOD4WIN Playlist (.MOL)")
        .add_filter("MOD4WIN Playlist (*.mol)", &["mol", "MOL"])
        .add_filter("All Files", &["*"]);
    if !initial_dir.is_empty() {
        let p = std::path::Path::new(initial_dir);
        if p.is_dir() {
            dlg = dlg.set_directory(p);
        }
    }
    dlg.pick_file()
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
fn native_open_mol_dialog(_initial_dir: &str) -> Option<PathBuf> {
    None
}

/// Öffnet den nativen Windows-Speicherndialog (`GetSaveFileNameW`) für `.MOL`-Playlists.
#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
fn native_save_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_EXPLORER: u32 = 0x0008_0000;

    let filter_str = "MOD4WIN Playlist (*.mol)\0*.mol\0All Files (*.*)\0*.*\0\0";
    let filter_w: Vec<u16> = filter_str.encode_utf16().collect();
    let init_dir_w = to_wide_null(initial_dir);
    let title_w = to_wide_null("Save MOD4WIN Playlist (.MOL)");
    let def_ext_w = to_wide_null("mol");
    let mut file_buf = vec![0u16; 1024];
    for (i, ch) in "PLAYLIST.MOL".encode_utf16().enumerate() {
        file_buf[i] = ch;
    }

    let mut ofn = OpenFileNameW {
        l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
        hwnd_owner: 0,
        h_instance: 0,
        lpstr_filter: filter_w.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buf.as_mut_ptr(),
        n_max_file: file_buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: if initial_dir.is_empty() {
            std::ptr::null()
        } else {
            init_dir_w.as_ptr()
        },
        lpstr_title: title_w.as_ptr(),
        flags: OFN_EXPLORER
            | OFN_OVERWRITEPROMPT
            | OFN_PATHMUSTEXIST
            | OFN_HIDEREADONLY
            | OFN_NOCHANGEDIR,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: def_ext_w.as_ptr(),
        l_cust_data: 0,
        lpfn_hook: 0,
        lp_template_name: std::ptr::null(),
        pv_reserved: std::ptr::null_mut(),
        dw_reserved: 0,
        flags_ex: 0,
    };

    let ok = unsafe { GetSaveFileNameW(&mut ofn) };
    if ok != 0 {
        let len = file_buf.iter().position(|&c| c == 0).unwrap_or(0);
        if len > 0 {
            return Some(PathBuf::from(String::from_utf16_lossy(&file_buf[..len])));
        }
    }
    None
}

#[cfg(all(target_os = "windows", feature = "retro-win32"))]
fn native_save_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;

    let filter = b"MOD4WIN Playlist (*.mol)\0*.mol\0All Files (*.*)\0*.*\0\0";
    let init_dir_a = to_ansi_null(initial_dir);
    let title_a = to_ansi_null("Save MOD4WIN Playlist (.MOL)");
    let def_ext_a = to_ansi_null("mol");
    let mut file_buf = vec![0u8; 1024];
    for (i, &b) in b"PLAYLIST.MOL".iter().enumerate() {
        file_buf[i] = b;
    }

    let mut ofn = OpenFileNameA {
        l_struct_size: std::mem::size_of::<OpenFileNameA>() as u32,
        hwnd_owner: 0,
        h_instance: 0,
        lpstr_filter: filter.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buf.as_mut_ptr(),
        n_max_file: file_buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: if initial_dir.is_empty() {
            std::ptr::null()
        } else {
            init_dir_a.as_ptr()
        },
        lpstr_title: title_a.as_ptr(),
        flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: def_ext_a.as_ptr(),
        l_cust_data: 0,
        lpfn_hook: 0,
        lp_template_name: std::ptr::null(),
    };

    let ok = unsafe { GetSaveFileNameA(&mut ofn) };
    if ok != 0 {
        let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
        if len > 0 {
            return Some(PathBuf::from(crate::win32s_shim::ansi_bytes_to_string(
                &file_buf[..len],
            )));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn native_save_mol_dialog(initial_dir: &str) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new()
        .set_title("Save MOD4WIN Playlist (.MOL)")
        .set_file_name("PLAYLIST.MOL")
        .add_filter("MOD4WIN Playlist (*.mol)", &["mol", "MOL"])
        .add_filter("All Files", &["*"]);
    if !initial_dir.is_empty() {
        let p = std::path::Path::new(initial_dir);
        if p.is_dir() {
            dlg = dlg.set_directory(p);
        }
    }
    dlg.save_file()
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
fn native_save_mol_dialog(_initial_dir: &str) -> Option<PathBuf> {
    None
}

/// Öffnet den nativen Windows-Ordnerauswahldialog (`SHBrowseForFolderW`) für den DTD-Ausgabeordner.
#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
fn native_browse_folder_dialog(_initial_dir: &str) -> Option<PathBuf> {
    const BIF_RETURNONLYFSDIRS: u32 = 0x0000_0001;
    const BIF_NEWDIALOGSTYLE: u32 = 0x0000_0040;

    let mut display_buf = [0u16; 260];
    let title_w = to_wide_null("DTD Recording Path (WAV Output Folder) auswählen:");
    let mut bi = BrowseInfoW {
        hwnd_owner: 0,
        pidl_root: std::ptr::null(),
        psz_display_name: display_buf.as_mut_ptr(),
        lpsz_title: title_w.as_ptr(),
        ul_flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: 0,
        l_param: 0,
        i_image: 0,
    };

    let pidl = unsafe { SHBrowseForFolderW(&mut bi) };
    if pidl.is_null() {
        return None;
    }
    let mut path_buf = [0u16; 520];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) };
    unsafe {
        CoTaskMemFree(pidl);
    }
    if ok != 0 {
        let len = path_buf.iter().position(|&c| c == 0).unwrap_or(0);
        if len > 0 {
            return Some(PathBuf::from(String::from_utf16_lossy(&path_buf[..len])));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn native_browse_folder_dialog(initial_dir: &str) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new()
        .set_title("DTD Recording Path (WAV Output Folder) auswählen:");
    if !initial_dir.is_empty() {
        let p = std::path::Path::new(initial_dir);
        if p.is_dir() {
            dlg = dlg.set_directory(p);
        }
    }
    dlg.pick_folder()
}

#[cfg(any(all(not(target_os = "windows"), not(target_os = "linux")), feature = "retro-win32"))]
fn native_browse_folder_dialog(_initial_dir: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix20_renders_lit_green_and_diagonal_inactive_dots() {
        let mut frame = vec![0u8; 360 * 30 * 4];
        render_mod4win_matrix20(&mut frame, 360, 30, 0, 0, 360, 28, "GIANA SISTERS !");
        let mut has_bright_green = false;
        let mut has_diag_dim = false;
        for px in frame.as_chunks::<4>().0 {
            if px[0] == 0x00 && px[1] == 0xff && px[2] == 0x00 {
                has_bright_green = true;
            }
            if px[0] == 0x0d && px[1] == 0x4a && px[2] == 0x1f {
                has_diag_dim = true;
            }
        }
        assert!(has_bright_green, "Matrix20 muss leuchtend grüne Dots (#00ff00) enthalten");
        assert!(
            has_diag_dim,
            "Matrix20 muss dunkle Diagonal-Striche (#0d4a1f) für ungesetzte Dots enthalten"
        );
    }

    #[test]
    fn test_font_5x7_special_chars_and_marquee_scrolling() {
        for ch in [
            ',', ';', ':', '.', '!', '?', '\'', '"', '&', '#', '$', '%', '*', '=', '<', '>', '/',
            '\\', '|', '@', '^', '~', '_', '+', '-', '(', ')', '[', ']', '{', '}',
        ] {
            let glyph = font_5x7_glyph(ch);
            let has_lit_dot = glyph.iter().any(|row| row.as_bytes().contains(&b'1'));
            assert!(
                has_lit_dot,
                "Sonderzeichen {:?} muss in font_5x7_glyph gesetzte Dots besitzen",
                ch
            );
        }

        let title = "Heo, Spann dan Wagen an"; // 23 Zeichen (> 20 im Hauptfenster)
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 0),
            "HEO, SPANN DAN WAGEN"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 1999),
            "HEO, SPANN DAN WAGEN"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 2000),
            "EO, SPANN DAN WAGEN "
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 2120),
            "O, SPANN DAN WAGEN A"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 2240),
            ", SPANN DAN WAGEN AN"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 20, 2360),
            " SPANN DAN WAGEN AN "
        );

        // Im Info-Fenster (n = 26 Classic, n = 31 Modern) passt ein 23-Zeichen-Titel zentriert hinein:
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(title, 26, 5000),
            " HEO, SPANN DAN WAGEN AN  "
        );
        // Überlanger Titel (> 31 Zeichen) scrollt auch im Info-Fenster nach n * 100 ms (2600 ms / 3100 ms):
        let long_info_title = "CAPTAIN FUTURE - MAIN THEME (REMIX 1995)";
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(long_info_title, 26, 2599),
            "CAPTAIN FUTURE - MAIN THEM"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(long_info_title, 26, 2600),
            "APTAIN FUTURE - MAIN THEME"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(long_info_title, 31, 3099),
            "CAPTAIN FUTURE - MAIN THEME (RE"
        );
        assert_eq!(
            super::super::widgets::format_bmp_marquee_field(long_info_title, 31, 3100),
            "APTAIN FUTURE - MAIN THEME (REM"
        );
    }
}
