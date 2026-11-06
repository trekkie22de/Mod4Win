use super::bmp::BmpImage;

/// 5x7 Monospace-Bitmap-Font für Ziffern und ASCII-Zeichen.
/// Jedes Zeichen ist durch 5 Bytes definiert (je Spalte ein Byte, Bits 0..6 für Zeilen 0..6).
#[rustfmt::skip]
const FONT_5X7: [[u8; 5]; 96] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5F, 0x00, 0x00], // '!'
    [0x00, 0x07, 0x00, 0x07, 0x00], // '"'
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // '#'
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // '$'
    [0x23, 0x13, 0x08, 0x64, 0x62], // '%'
    [0x36, 0x49, 0x55, 0x22, 0x50], // '&'
    [0x00, 0x05, 0x03, 0x00, 0x00], // '\''
    [0x00, 0x1C, 0x22, 0x41, 0x00], // '('
    [0x00, 0x41, 0x22, 0x1C, 0x00], // ')'
    [0x08, 0x2A, 0x1C, 0x2A, 0x08], // '*'
    [0x08, 0x08, 0x3E, 0x08, 0x08], // '+'
    [0x00, 0x50, 0x30, 0x00, 0x00], // ','
    [0x08, 0x08, 0x08, 0x08, 0x08], // '-'
    [0x00, 0x60, 0x60, 0x00, 0x00], // '.'
    [0x20, 0x10, 0x08, 0x04, 0x02], // '/'
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // '0'
    [0x00, 0x42, 0x7F, 0x40, 0x00], // '1'
    [0x42, 0x61, 0x51, 0x49, 0x46], // '2'
    [0x21, 0x41, 0x45, 0x4B, 0x31], // '3'
    [0x18, 0x14, 0x12, 0x7F, 0x10], // '4'
    [0x27, 0x45, 0x45, 0x45, 0x39], // '5'
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // '6'
    [0x01, 0x71, 0x09, 0x05, 0x03], // '7'
    [0x36, 0x49, 0x49, 0x49, 0x36], // '8'
    [0x06, 0x49, 0x49, 0x29, 0x1E], // '9'
    [0x00, 0x36, 0x36, 0x00, 0x00], // ':'
    [0x00, 0x56, 0x36, 0x00, 0x00], // ';'
    [0x08, 0x14, 0x22, 0x41, 0x00], // '<'
    [0x14, 0x14, 0x14, 0x14, 0x14], // '='
    [0x00, 0x41, 0x22, 0x14, 0x08], // '>'
    [0x02, 0x01, 0x51, 0x09, 0x06], // '?'
    [0x32, 0x49, 0x79, 0x41, 0x3E], // '@'
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // 'A'
    [0x7F, 0x49, 0x49, 0x49, 0x36], // 'B'
    [0x3E, 0x41, 0x41, 0x41, 0x22], // 'C'
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // 'D'
    [0x7F, 0x49, 0x49, 0x49, 0x41], // 'E'
    [0x7F, 0x09, 0x09, 0x09, 0x01], // 'F'
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // 'G'
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // 'H'
    [0x00, 0x41, 0x7F, 0x41, 0x00], // 'I'
    [0x20, 0x40, 0x41, 0x3F, 0x01], // 'J'
    [0x7F, 0x08, 0x14, 0x22, 0x41], // 'K'
    [0x7F, 0x40, 0x40, 0x40, 0x40], // 'L'
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // 'M'
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // 'N'
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // 'O'
    [0x7F, 0x09, 0x09, 0x09, 0x06], // 'P'
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // 'Q'
    [0x7F, 0x09, 0x19, 0x29, 0x46], // 'R'
    [0x46, 0x49, 0x49, 0x49, 0x31], // 'S'
    [0x01, 0x01, 0x7F, 0x01, 0x01], // 'T'
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // 'U'
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // 'V'
    [0x7F, 0x20, 0x18, 0x20, 0x7F], // 'W'
    [0x63, 0x14, 0x08, 0x14, 0x63], // 'X'
    [0x07, 0x08, 0x70, 0x08, 0x07], // 'Y'
    [0x61, 0x51, 0x49, 0x45, 0x43], // 'Z'
    [0x00, 0x7F, 0x41, 0x41, 0x00], // '['
    [0x02, 0x04, 0x08, 0x10, 0x20], // '\'
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ']'
    [0x04, 0x02, 0x01, 0x02, 0x04], // '^'
    [0x40, 0x40, 0x40, 0x40, 0x40], // '_'
    [0x00, 0x01, 0x02, 0x04, 0x00], // '`'
    [0x20, 0x54, 0x54, 0x54, 0x78], // 'a'
    [0x7F, 0x48, 0x44, 0x44, 0x38], // 'b'
    [0x38, 0x44, 0x44, 0x44, 0x20], // 'c'
    [0x38, 0x44, 0x44, 0x48, 0x7F], // 'd'
    [0x38, 0x54, 0x54, 0x54, 0x18], // 'e'
    [0x08, 0x7E, 0x09, 0x01, 0x02], // 'f'
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // 'g'
    [0x7F, 0x08, 0x04, 0x04, 0x78], // 'h'
    [0x00, 0x44, 0x7D, 0x40, 0x00], // 'i'
    [0x20, 0x40, 0x44, 0x3D, 0x00], // 'j'
    [0x7F, 0x10, 0x28, 0x44, 0x00], // 'k'
    [0x00, 0x41, 0x7F, 0x40, 0x00], // 'l'
    [0x7C, 0x04, 0x18, 0x04, 0x78], // 'm'
    [0x7C, 0x08, 0x04, 0x04, 0x78], // 'n'
    [0x38, 0x44, 0x44, 0x44, 0x38], // 'o'
    [0x7C, 0x14, 0x14, 0x14, 0x08], // 'p'
    [0x08, 0x14, 0x14, 0x18, 0x7C], // 'q'
    [0x7C, 0x08, 0x04, 0x04, 0x08], // 'r'
    [0x48, 0x54, 0x54, 0x54, 0x20], // 's'
    [0x04, 0x3F, 0x44, 0x40, 0x20], // 't'
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // 'u'
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // 'v'
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // 'w'
    [0x44, 0x28, 0x10, 0x28, 0x44], // 'x'
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // 'y'
    [0x44, 0x64, 0x54, 0x4C, 0x44], // 'z'
    [0x00, 0x08, 0x36, 0x41, 0x00], // '{'
    [0x00, 0x00, 0x7F, 0x00, 0x00], // '|'
    [0x00, 0x41, 0x36, 0x08, 0x00], // '}'
    [0x08, 0x08, 0x2A, 0x1C, 0x08], // '~'
    [0x7F, 0x7F, 0x7F, 0x7F, 0x7F], // DEL (voll gefüllt)
];

/// Zeichnet ein einzelnes ASCII-Zeichen an Position `(x, y)` im Framebuffer.
pub fn draw_char(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    c: char,
    color: [u8; 4],
) {
    let ascii = c as usize;
    let glyph = if (32..128).contains(&ascii) {
        &FONT_5X7[ascii - 32]
    } else {
        &FONT_5X7[95] // Vollblock für unbekannte Zeichen
    };

    let dw = dst_width as i32;
    let dh = dst_height as i32;

    for col in 0..5 {
        let px = x + col;
        if px < 0 || px >= dw {
            continue;
        }
        let col_bits = glyph[col as usize];
        for row in 0..7 {
            let py = y + row;
            if py < 0 || py >= dh {
                continue;
            }
            if (col_bits & (1 << row)) != 0 {
                let idx = ((py * dw + px) * 4) as usize;
                dst[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Zeichnet einen Text-String an Position `(x, y)` im Framebuffer.
pub fn draw_text(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
) {
    for c in text.chars() {
        draw_char(dst, dst_width, dst_height, x, y, c, color);
        x += 6; // 5 Pixel Zeichenbreite + 1 Pixel Abstand
    }
}

/// Zeichnet einen Text mit dunklem Schatten für bessere Lesbarkeit auf Bitmap-Hintergründen.
pub fn draw_text_shadow(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
    shadow_color: [u8; 4],
) {
    draw_text(dst, dst_width, dst_height, x + 1, y + 1, text, shadow_color);
    draw_text(dst, dst_width, dst_height, x, y, text, color);
}

/// Füllt ein Rechteck mit einer einheitlichen Farbe.
pub fn fill_rect(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    let dw = dst_width as i32;
    let dh = dst_height as i32;
    let x_end = (x + w as i32).min(dw);
    let y_end = (y + h as i32).min(dh);
    let x_start = x.max(0);
    let y_start = y.max(0);

    for py in y_start..y_end {
        for px in x_start..x_end {
            let idx = ((py * dw + px) * 4) as usize;
            dst[idx..idx + 4].copy_from_slice(&color);
        }
    }
}

/// Zeichnet den Rahmen eines Rechtecks (1 Pixel breit).
pub fn draw_rect_outline(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    fill_rect(dst, dst_width, dst_height, x, y, w, 1, color);
    fill_rect(dst, dst_width, dst_height, x, y + h as i32 - 1, w, 1, color);
    fill_rect(dst, dst_width, dst_height, x, y, 1, h, color);
    fill_rect(dst, dst_width, dst_height, x + w as i32 - 1, y, 1, h, color);
}

/// Zeichnet ein 3D-Rechteck im Windows-3.1-Stil:
/// `sunken = false`: Erhaben (Raised: oben/links weiß, unten/rechts dunkel)
/// `sunken = true`:  Vertieft (Sunken: oben/links dunkel/schwarz, unten/rechts weiß)
pub fn draw_3d_panel(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    sunken: bool,
    fill_color: [u8; 4],
) {
    if w < 4 || h < 4 {
        fill_rect(dst, dw, dh, x, y, w, h, fill_color);
        return;
    }
    let col_white = [255, 255, 255, 255];
    let col_dark = [128, 128, 128, 255];
    let col_black = [0, 0, 0, 255];

    let (tl_outer, tl_inner, br_inner, br_outer) = if sunken {
        (col_dark, col_black, col_white, col_white)
    } else {
        (col_white, col_white, col_dark, col_black)
    };

    // Füllung
    fill_rect(dst, dw, dh, x + 2, y + 2, w - 4, h - 4, fill_color);

    // Äußere Kanten
    fill_rect(dst, dw, dh, x, y, w, 1, tl_outer);
    fill_rect(dst, dw, dh, x, y, 1, h, tl_outer);
    fill_rect(dst, dw, dh, x, y + h as i32 - 1, w, 1, br_outer);
    fill_rect(dst, dw, dh, x + w as i32 - 1, y, 1, h, br_outer);

    // Innere Kanten
    fill_rect(dst, dw, dh, x + 1, y + 1, w - 2, 1, tl_inner);
    fill_rect(dst, dw, dh, x + 1, y + 1, 1, h - 2, tl_inner);
    fill_rect(dst, dw, dh, x + 1, y + h as i32 - 2, w - 2, 1, br_inner);
    fill_rect(dst, dw, dh, x + w as i32 - 2, y + 1, 1, h - 2, br_inner);
}

// ==============================================================================
// Echte originale Bitmap-Font & Sprite-Blitter für MOD4WIN
// ==============================================================================

/// Zeichnet Text mit dem originalen 16x20 Dot-Matrix-Bitmap-Font (`FONT_16X20.bmp`).
/// Character width: 16, height: 20, 64 Zeichen (ASCII 32..95: Space, !, ..., 0..9, A..Z).
/// Blittet deckend (`blit_rect_to`), sodass die dunkle Punktmatrix im Font-Sprite alten Text überschreibt.
pub fn draw_bmp_text_16x20(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    font_bmp: &BmpImage,
) {
    for c in text.chars() {
        let mapped = match c {
            '~' => '-',
            '{' => '(',
            '}' => ')',
            '|' => '!',
            '`' | '´' | '’' | '‘' => '\'',
            '“' | '”' | '„' => '"',
            '«' => '<',
            '»' => '>',
            other => other,
        };
        let up = mapped.to_ascii_uppercase() as u8;
        let k = if (32..96).contains(&up) {
            (up - 32) as u32
        } else {
            0
        };
        let src_x = (k % 64) * 16;
        font_bmp.blit_rect_to(src_x, 0, 16, 20, dst, dst_width, dst_height, x, y);
        x += 16;
    }
}

/// Formatiert einen Text für ein `n`-stelliges Dot-Matrix-Feld exakt nach `BMPFont.setbmptext` und
/// `BMPFont.TimerScroll` in `winunit.pas` (Zeilen 2539–2624):
/// - Deutsche Umlaute werden wie im Original (`CharsInFont <= 64`) zu `AE`, `OE`, `UE`, `SS` expandiert.
/// - Ist die Zeichenkette `<= n` Zeichen lang, wird sie im `n`-Zeichen-Feld zentriert.
/// - Ist die Zeichenkette `> n` Zeichen lang (`L = chars.len()`), bleibt sie zunächst für `n * 100 ms`
///   (`2000 ms` bei `n = 20`, `2600 ms` bei `n = 26`, `3100 ms` bei `n = 31`) am linken Anschlag stehen
///   und scrollt danach im `120 ms`-Takt zeichenweise durch einen Ringpuffer der Länge `L + n`
///   (`L` Textzeichen gefolgt von `n` Leerzeichen).
pub fn format_bmp_marquee_field(text: &str, n: usize, elapsed_ms: u64) -> String {
    if n == 0 {
        return String::new();
    }
    let mut normalized = String::new();
    for ch in text.chars() {
        match ch {
            'ä' | 'Ä' => normalized.push_str("AE"),
            'ö' | 'Ö' => normalized.push_str("OE"),
            'ü' | 'Ü' => normalized.push_str("UE"),
            'ß' => normalized.push_str("SS"),
            _ => normalized.push(ch.to_ascii_uppercase()),
        }
    }
    let trimmed = normalized.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= n {
        let pad_left = (n.saturating_sub(chars.len())) / 2;
        let mut field = String::with_capacity(n);
        for _ in 0..pad_left {
            field.push(' ');
        }
        for c in chars {
            field.push(c);
        }
        while field.chars().count() < n {
            field.push(' ');
        }
        return field;
    }

    let hold_ms = (n as u64) * 100;
    let step = if elapsed_ms < hold_ms {
        0usize
    } else {
        1 + ((elapsed_ms - hold_ms) / 120) as usize
    };
    let period = chars.len() + n;
    let mut field = String::with_capacity(n);
    for i in 0..n {
        let idx = (step + i) % period;
        if idx < chars.len() {
            field.push(chars[idx]);
        } else {
            field.push(' ');
        }
    }
    field
}

/// Zeichnet ein Textfeld fester Breite `n` Zeichen mit `FONT_16X20.bmp` inklusive originalgetreuem
/// Marquee-Scrolling für überlange Titel (`BMPFont` in `winunit.pas`).
#[allow(clippy::too_many_arguments)]
pub fn draw_bmp_text_16x20_marquee(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    n: usize,
    text: &str,
    elapsed_ms: u64,
    font_bmp: &BmpImage,
) {
    let field = format_bmp_marquee_field(text, n, elapsed_ms);
    draw_bmp_text_16x20(dst, dst_width, dst_height, x, y, &field, font_bmp);
}

/// Zeichnet ein zentriertes Textfeld fester Breite `n` Zeichen mit `FONT_16X20.bmp` exakt wie `BMPText` in `winunit.pas`.
pub fn draw_bmp_text_16x20_centered(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    n: usize,
    text: &str,
    font_bmp: &BmpImage,
) {
    draw_bmp_text_16x20_marquee(dst, dst_width, dst_height, x, y, n, text, 0, font_bmp);
}

/// Zeichnet eine 7-Segment-LCD-Ziffer aus einem Sprite-Sheet (z.B. `LCD_15X24`, `LCD_13X20`, `LCD_08X13`).
/// Jedes Sprite-Sheet hat 11 Einträge: Index 0 = '/' (unbeleuchtetes '88'-Segment / Blank),
/// Index 1..10 = '0'..'9'. Unterstützt zusätzlich `'K'` auf der 13x20-Anzeige für >9999 Songs.
pub fn draw_bmp_lcd_digit(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    c: char,
    lcd_bmp: &BmpImage,
    digit_w: u32,
    digit_h: u32,
) {
    if (c == 'K' || c == 'k') && digit_w == 13 && digit_h == 20 {
        // 1. Unbeleuchtetes '8'-Hintergrundraster (Zelle 0) blitten
        lcd_bmp.blit_rect_to(0, 0, 13, 20, dst, dst_width, dst_height, x, y);
        // 2. Linke vertikale Segmente f & e (x_rel = 1..=2, y_rel = 2..=17) aus Zelle 1 ('0') übernehmen
        lcd_bmp.blit_rect_to(13 + 1, 2, 2, 16, dst, dst_width, dst_height, x + 1, y + 2);
        // 3. Mittelsteg links + obere/untere rechte Diagonale von 'K' in originalem LCD-Leuchtgrün [0, 255, 0]
        let lit = [0u8, 255, 0, 255];
        fill_rect(dst, dst_width, dst_height, x + 3, y + 9, 4, 2, lit);
        const K_DIAG: [(i32, i32); 7] = [
            (5, 8),
            (6, 7),
            (7, 6),
            (8, 5),
            (9, 4),
            (10, 3),
            (10, 2),
        ];
        for &(dx, dy) in &K_DIAG {
            fill_rect(dst, dst_width, dst_height, x + dx, y + dy, 2, 1, lit);
            fill_rect(dst, dst_width, dst_height, x + dx, y + (19 - dy), 2, 1, lit);
        }
        return;
    }
    if (c == 'K' || c == 'k') && digit_w == 8 && digit_h == 13 {
        lcd_bmp.blit_rect_to(0, 0, 8, 13, dst, dst_width, dst_height, x, y);
        lcd_bmp.blit_rect_to(8 + 1, 1, 1, 11, dst, dst_width, dst_height, x + 1, y + 1);
        let lit = [0u8, 255, 0, 255];
        fill_rect(dst, dst_width, dst_height, x + 2, y + 6, 2, 1, lit);
        for (i, &(dx, dy)) in [(4, 5), (5, 4), (6, 3), (6, 2)].iter().enumerate() {
            let _ = i;
            fill_rect(dst, dst_width, dst_height, x + dx, y + dy, 1, 1, lit);
            fill_rect(dst, dst_width, dst_height, x + dx, y + (12 - dy), 1, 1, lit);
        }
        return;
    }
    let idx = match c {
        '0'..='9' => (c as u8 - b'0' + 1) as u32,
        _ => 0, // '/' oder Space -> unbeleuchtetes '88' Segment
    };
    let src_x = idx * digit_w;
    lcd_bmp.blit_rect_to(src_x, 0, digit_w, digit_h, dst, dst_width, dst_height, x, y);
}

/// Zeichnet eine Zeichenkette mit einem 7-Segment-LCD-Sprite-Sheet.
pub fn draw_bmp_lcd_string(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    lcd_bmp: &BmpImage,
    digit_w: u32,
    digit_h: u32,
) {
    for c in text.chars() {
        draw_bmp_lcd_digit(dst, dst_width, dst_height, x, y, c, lcd_bmp, digit_w, digit_h);
        x += digit_w as i32;
    }
}

/// Zeichnet eine Zahl als 7-Segment-LCD-Anzeige mit fester Ziffernanzahl und wahlweise führenden Nullen.
/// Bei 4-stelligen Anzeigen (`num_digits == 4`) und Werten `>= 10_000` wird automatisch auf die
/// kompakte K-Notation (`10.0K` .. `99.9K` bzw. `100K` .. `999K`) umgeschaltet.
pub fn draw_bmp_lcd_number(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    value: u32,
    num_digits: usize,
    leading_zeros: bool,
    lcd_bmp: &BmpImage,
    digit_w: u32,
    digit_h: u32,
) {
    if num_digits == 4 && value >= 10_000 {
        if value < 100_000 {
            // 10_000 ..= 99_999 -> "12.5K":
            // Bei der 13x20-Hauptanzeige werden die hinteren beiden Segmente (Ziffer 3 + 'K') um 2 Pixel nach rechts
            // verschoben, damit der 2x2-Dezimalpunkt links und rechts je 1px schwarzen Freiraum hat.
            let hundreds = value / 100; // 100 ..= 999
            let s = format!("{:03}K", hundreds);
            let chars: Vec<char> = s.chars().collect();
            let shift = if digit_w == 13 && digit_h == 20 { 2i32 } else { 0i32 };
            draw_bmp_lcd_digit(dst, dst_width, dst_height, x, y, chars[0], lcd_bmp, digit_w, digit_h);
            draw_bmp_lcd_digit(
                dst,
                dst_width,
                dst_height,
                x + digit_w as i32,
                y,
                chars[1],
                lcd_bmp,
                digit_w,
                digit_h,
            );
            if shift > 0 {
                // Hintergrundsegmente aus MODMAINFRAME.bmp in der 2px-Lücke schwarz abdecken
                fill_rect(
                    dst,
                    dst_width,
                    dst_height,
                    x + (digit_w as i32) * 2,
                    y,
                    shift as u32,
                    digit_h,
                    [0, 0, 0, 255],
                );
            }
            draw_bmp_lcd_digit(
                dst,
                dst_width,
                dst_height,
                x + (digit_w as i32) * 2 + shift,
                y,
                chars[2],
                lcd_bmp,
                digit_w,
                digit_h,
            );
            draw_bmp_lcd_digit(
                dst,
                dst_width,
                dst_height,
                x + (digit_w as i32) * 3 + shift,
                y,
                chars[3],
                lcd_bmp,
                digit_w,
                digit_h,
            );
            // Dezimalpunkt unten bündig zwischen Ziffer 2 und Ziffer 3 in LCD-Leuchtgrün
            let dot_x = x + (digit_w as i32) * 2;
            let dot_y = y + (digit_h as i32) - 2;
            let dot_sz = if digit_w >= 13 { 2 } else { 1 };
            fill_rect(dst, dst_width, dst_height, dot_x, dot_y, dot_sz, dot_sz, [0, 255, 0, 255]);
        } else {
            // 100_000+ -> "100K" .. "999K"
            let thousands = (value / 1000).min(999);
            let s = format!("{:03}K", thousands);
            draw_bmp_lcd_string(dst, dst_width, dst_height, x, y, &s, lcd_bmp, digit_w, digit_h);
        }
        return;
    }

    let num_str = value.to_string();
    let pad_char = if leading_zeros { '0' } else { '/' };
    let mut padded = String::new();
    if num_str.len() < num_digits {
        for _ in 0..(num_digits - num_str.len()) {
            padded.push(pad_char);
        }
    }
    padded.push_str(&num_str);
    if padded.len() > num_digits {
        padded = padded[padded.len() - num_digits..].to_string();
    }
    draw_bmp_lcd_string(dst, dst_width, dst_height, x, y, &padded, lcd_bmp, digit_w, digit_h);
}

/// Zeichnet eine Tracker-Notenbezeichnung (z.B. "C-4", "D#5", "---") aus `NUM_7X5.bmp`.
pub fn draw_num_7x5_note(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    note_val: u8,
    num_bmp: &BmpImage,
) {
    if note_val == 0 || note_val > 108 {
        // "---"
        num_bmp.blit_rect_to(55, 0, 5, 7, dst, dst_width, dst_height, x, y);
        num_bmp.blit_rect_to(55, 0, 5, 7, dst, dst_width, dst_height, x + 5, y);
        num_bmp.blit_rect_to(55, 0, 5, 7, dst, dst_width, dst_height, x + 10, y);
    } else {
        let n = (note_val - 1) as usize;
        let semitone = n % 12;
        let octave = n / 12;
        // Notenname (10 px breit, z.B. "C-", "C#")
        num_bmp.blit_rect_to(60 + (semitone as u32 * 10), 0, 10, 7, dst, dst_width, dst_height, x, y);
        // Oktave (5 px breit)
        num_bmp.blit_rect_to(5 + (octave as u32 * 5), 0, 5, 7, dst, dst_width, dst_height, x + 10, y);
    }
}

/// Zeichnet Ziffern / Text aus `NUM_7X5.bmp` (5x7 Pixel pro Zeichen).
pub fn draw_num_7x5_text(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    num_bmp: &BmpImage,
) {
    for c in text.chars() {
        let src_x = match c {
            '0'..='9' => 5 + (c as u32 - '0' as u32) * 5,
            '-' | ':' => 55,
            _ => 0,
        };
        num_bmp.blit_rect_to(src_x, 0, 5, 7, dst, dst_width, dst_height, x, y);
        x += 5;
    }
}

/// Zeichnet eine Effekt-Aktivitäts-LED (14x9) aus `HKONOFF.bmp`.
pub fn draw_hkonoff_led(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    is_active: bool,
    hk_bmp: &BmpImage,
) {
    let src_y = if is_active { 19 } else { 10 };
    hk_bmp.blit_rect_to(0, src_y, 14, 9, dst, dst_width, dst_height, x, y);
}

/// Zeichnet ein Stereo-VU-Meter (37x9) aus `PEGEL.bmp` exakt nach `ana.pas`.
pub fn draw_pegel_stereo_meter(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    vol_l: u8,
    vol_r: u8,
    pegel_bmp: &BmpImage,
) {
    // 1. Dunkler unbeleuchteter Hintergrund (y=9..18 in PEGEL.bmp)
    pegel_bmp.blit_rect_to(0, 9, 37, 9, dst, dst_width, dst_height, x, y);

    // 2. Exakter Ausschnitt nach ana.pas:
    // l_nib in 0..=8, r_nib in 0..=8
    let l_nib = (vol_l / 8).min(8) as i32;
    let r_nib = (vol_r / 8).min(8) as i32;
    let dx = x + 1 + (16 - (l_nib << 1));
    let sx = (1 + (16 - (l_nib << 1))) as u32;
    let bw = ((r_nib << 1) + 3 + (l_nib << 1)) as u32;
    pegel_bmp.blit_rect_to(sx, 0, bw, 9, dst, dst_width, dst_height, dx, y);
}

/// Eingebettete Original-Schriftart `SWEBUZOEM` (`OEM.FNT`, 8x12 Pixel Monospace) aus `legacy_src/INTL/FONTS/ALL/OEM.FNT`.
pub const OEM_FNT: &[u8] = include_bytes!("../../assets/OEM.FNT");

/// Zeichnet ein einzelnes Zeichen mit der originalen `OEM.FNT` (`8x12` Pixel, `Fw = 8, Fh = 12` wie in `DETAILS.PAS`).
pub fn draw_oem_char(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    ch_code: u8,
    fg_color: [u8; 4],
    bg_color: Option<[u8; 4]>,
) {
    if OEM_FNT.len() < 118 + 254 * 4 {
        return;
    }
    let first_char = OEM_FNT[95];
    let last_char = OEM_FNT[96];
    let code = if ch_code >= first_char && ch_code <= last_char {
        ch_code
    } else {
        b' '
    };
    let table_idx = 118 + ((code - first_char) as usize) * 4;
    let glyph_ofs = u16::from_le_bytes([OEM_FNT[table_idx + 2], OEM_FNT[table_idx + 3]]) as usize;
    if glyph_ofs + 12 > OEM_FNT.len() {
        return;
    }

    let dw = dst_width as i32;
    let dh = dst_height as i32;

    for row in 0..12i32 {
        let py = y + row;
        if py < 0 || py >= dh {
            continue;
        }
        let bits = OEM_FNT[glyph_ofs + row as usize];
        for col in 0..8i32 {
            let px = x + col;
            if px < 0 || px >= dw {
                continue;
            }
            let is_set = (bits & (0x80 >> col)) != 0;
            if is_set {
                let idx = ((py * dw + px) * 4) as usize;
                dst[idx..idx + 4].copy_from_slice(&fg_color);
            } else if let Some(bg) = bg_color {
                let idx = ((py * dw + px) * 4) as usize;
                dst[idx..idx + 4].copy_from_slice(&bg);
            }
        }
    }
}

#[inline]
pub fn char_to_oem_code(c: char) -> u8 {
    match c {
        '☺' => 0x01,
        '«' => 0xAE,
        '»' => 0xAF,
        'ä' => 0x84,
        'Ä' => 0x8E,
        'ö' => 0x94,
        'Ö' => 0x99,
        'ü' => 0x81,
        'Ü' => 0x9A,
        'ß' => 0xE1,
        _ if (c as u32) <= 0xFF => c as u8,
        _ => b'?',
    }
}

/// Zeichnet eine Zeichenkette mit der originalen `OEM.FNT` (`8x12` Pixel pro Zeichen, `Fw = 8, Fh = 12`).
pub fn draw_oem_text(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    fg_color: [u8; 4],
) {
    for c in text.chars() {
        let code = char_to_oem_code(c);
        draw_oem_char(dst, dst_width, dst_height, x, y, code, fg_color, None);
        x += 8;
    }
}

/// Zeichnet eine Zeichenkette mit `OEM.FNT` (`8x12`), wobei `&` den nachfolgenden Buchstaben unterstreicht (`DT_SINGLELINE`).
pub fn draw_oem_text_mnemonic(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    fg_color: [u8; 4],
) {
    let mut underline_next = false;
    for c in text.chars() {
        if c == '&' && !underline_next {
            underline_next = true;
            continue;
        }
        let code = char_to_oem_code(c);
        draw_oem_char(dst, dst_width, dst_height, x, y, code, fg_color, None);
        if underline_next {
            fill_rect(dst, dst_width, dst_height, x, y + 11, 8, 1, fg_color);
            underline_next = false;
        }
        x += 8;
    }
}

/// Zeichnet eine Zeichenkette mit `OEM.FNT` (`8x12`) horizontal zentriert in einem Bereich der Breite `w`.
pub fn draw_oem_text_centered(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    w: u32,
    text: &str,
    fg_color: [u8; 4],
) {
    let char_count = text.chars().count() as i32;
    let text_w = char_count * 8;
    let tx = x + ((w as i32 - text_w) / 2).max(0);
    draw_oem_text(dst, dst_width, dst_height, tx, y, text, fg_color);
}

/// Zeichnet eine Zeichenkette mit `OEM.FNT` (`8x12`) rechtsbündig an `right_x`.
pub fn draw_oem_text_right(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    right_x: i32,
    y: i32,
    text: &str,
    fg_color: [u8; 4],
) {
    let char_count = text.chars().count() as i32;
    let tx = right_x - char_count * 8;
    draw_oem_text(dst, dst_width, dst_height, tx, y, text, fg_color);
}

/// Zeichnet einen 2-Stufen-Bitmap-Button (`BMPButton` in `winunit.pas`):
/// Obere Hälfte (`y = 0`) = Normalzustand, untere Hälfte (`y = h`) = Gedrückt (`pressed = true`).
pub fn draw_bmp_button_2state(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    pressed: bool,
    btn_bmp: &BmpImage,
) {
    let w = btn_bmp.width;
    let h = btn_bmp.height / 2;
    let src_y = if pressed { h } else { 0 };
    btn_bmp.blit_rect_to(0, src_y, w, h, dst, dst_width, dst_height, x, y);
}

/// Zeichnet einen 3-Stufen-Bitmap-Button (`BMP3Button` in `winunit.pas`, z.B. `BSTOP`, `BPLAY`, `BPAUSE`):
/// Drittel 0 (`y = 0`) = Normal/Aus, Drittel 1 (`y = h`) = Gedrückt (`pressed`), Drittel 2 (`y = 2*h`) = Aktiv/Ein (`active`).
pub fn draw_bmp_button_3state(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    x: i32,
    y: i32,
    pressed: bool,
    active: bool,
    btn_bmp: &BmpImage,
) {
    let w = btn_bmp.width;
    let h = btn_bmp.height / 3;
    let src_y = if pressed {
        h
    } else if active {
        h * 2
    } else {
        0
    };
    btn_bmp.blit_rect_to(0, src_y, w, h, dst, dst_width, dst_height, x, y);
}

/// Zeichnet einen `size`-Pixel-breiten 3D-Rahmen ohne Füllung exakt wie `Frame3D(dc, x, y, w, h, size, up/down)` in `winunit.pas`.
pub fn draw_frame_3d(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    size: u32,
    sunken: bool,
) {
    for i in 0..size {
        let off = i as i32;
        let sub = i * 2;
        if w <= sub + 1 || h <= sub + 1 {
            break;
        }
        draw_frame_3d_1px(dst, dw, dh, x + off, y + off, w - sub, h - sub, sunken);
    }
}

/// Zeichnet einen 2-Pixel-breiten 3D-Rahmen ohne Füllung.
pub fn draw_3d_frame_only(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    sunken: bool,
) {
    draw_frame_3d(dst, dw, dh, x, y, w, h, 2, sunken);
}

/// Zeichnet einen 1-Pixel-breiten 3D-Rahmen ohne Füllung exakt wie `Frame3D(dc, x, y, w, h, 1, up/down)` in `winunit.pas`.
pub fn draw_frame_3d_1px(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    sunken: bool,
) {
    if w < 2 || h < 2 {
        return;
    }
    let col_white = [255, 255, 255, 255];
    let col_dark = [128, 128, 128, 255];
    let (tl, br) = if sunken {
        (col_dark, col_white)
    } else {
        (col_white, col_dark)
    };
    fill_rect(dst, dw, dh, x, y, w, 1, tl);
    fill_rect(dst, dw, dh, x, y, 1, h, tl);
    fill_rect(dst, dw, dh, x, y + h as i32 - 1, w, 1, br);
    fill_rect(dst, dw, dh, x + w as i32 - 1, y, 1, h, br);
}

/// Zeichnet einen vertikalen Schiebeschalter (`TSwitch` in `winunit.pas`, `SWITCH.bmp`, `18 x 11` pro Stufe, `cur_pos` ist 1-basiert)
/// inklusive dem originalen vertieften 3D-Rahmen und schwarzem Innenrand.
pub fn draw_vswitch(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    num_pos: u32,
    cur_pos: u32,
    switch_bmp: &BmpImage,
) {
    let total_h = num_pos * 11;
    draw_frame_3d_1px(dst, dw, dh, x - 2, y - 2, 22, total_h + 4, true);
    draw_rect_outline(dst, dw, dh, x - 1, y - 1, 20, total_h + 2, [0, 0, 0, 255]);
    for i in 1..=num_pos {
        let sy = if i == cur_pos { 0 } else { 11 };
        let dy = y + ((i - 1) * 11) as i32;
        switch_bmp.blit_rect_to(0, sy, 18, 11, dst, dw, dh, x, dy);
    }
}

/// Zeichnet einen horizontalen Schiebeschalter (`TLRSwitch` in `winunit.pas`, `LRSWITCH.bmp`, `12 x 14` pro Stufe, `cur_pos` ist 1-basiert)
/// inklusive dem originalen vertieften 3D-Rahmen (`h + 4 = 18 px`) und schwarzem Innenrand (`h + 2 = 16 px`).
pub fn draw_lrswitch(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    num_pos: u32,
    cur_pos: u32,
    lrswitch_bmp: &BmpImage,
) {
    let step_w = (lrswitch_bmp.width / 2).max(1);
    let h = lrswitch_bmp.height.max(1);
    let total_w = num_pos * step_w;
    draw_frame_3d_1px(dst, dw, dh, x - 2, y - 2, total_w + 4, h + 4, true);
    draw_rect_outline(dst, dw, dh, x - 1, y - 1, total_w + 2, h + 2, [0, 0, 0, 255]);
    for i in 1..=num_pos {
        let sx = if i == cur_pos { 0 } else { step_w };
        let dx = x + ((i - 1) * step_w) as i32;
        lrswitch_bmp.blit_rect_to(sx, 0, step_w, h, dst, dw, dh, dx, y);
    }
}

/// Zeichnet einen doppelten Up/Down-Button (`TBmpUDButton` in `winunit.pas`, `B_UP_DOWN_1.bmp`: `38 x 39` -> `19 x 26` gesamt, mit Rahmen `21 x 28`).
pub fn draw_up_down_btn(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    up_pressed: bool,
    down_pressed: bool,
    ud_bmp: &BmpImage,
) {
    let w = (ud_bmp.width / 2).max(1);
    let h = (ud_bmp.height / 3).max(1);
    draw_frame_3d_1px(dst, dw, dh, x - 1, y - 1, w + 2, h * 2 + 2, true);
    let up_sy = if up_pressed { h } else { h * 2 };
    let dn_sy = if down_pressed { h } else { h * 2 };
    ud_bmp.blit_rect_to(0, up_sy, w, h, dst, dw, dh, x, y);
    ud_bmp.blit_rect_to(w, dn_sy, w, h, dst, dw, dh, x, y + h as i32);
}

// ==============================================================================
// Kompakter 6x10 Modern-UI-Font mit echten Unterlängen (g, j, p, q, y)
// ==============================================================================

/// Liefert das `6×10`-Bitmap-Glyph (Breite in Pixeln + 10 Zeilen-Bitmasken) für den Modern-Skin-UI-Font:
/// - Zeilen `0..=6` (1-basiert Y=1..7): Oberlängen & Großbuchstaben (`7px` hoch), normale Kleinbuchstaben (`a, c, e, m, n, o, r, s, u, v, w, x, z`) sind `5px` hoch (`Zeilen 2..=6`) und stehen exakt auf der Baseline (`Zeile 6` / `Y=7`).
/// - Zeilen `7..=9` (1-basiert Y=8..10): Echte Unterlängen (`g, j, p, q, y` sowie `,`, `;`), die `3px` UNTER die Baseline ragen!
pub fn modern_ui_glyph_6x10(ch: char) -> (u8, [u8; 10]) {
    match ch {
        ' ' => (3, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        '!' => (2, [0b10, 0b10, 0b10, 0b10, 0b10, 0b00, 0b10, 0, 0, 0]),
        '.' => (2, [0, 0, 0, 0, 0, 0b11, 0b11, 0, 0, 0]),
        ',' => (2, [0, 0, 0, 0, 0, 0b11, 0b11, 0b01, 0b10, 0]),
        ';' => (2, [0, 0, 0b11, 0b11, 0, 0b11, 0b11, 0b01, 0b10, 0]),
        ':' => (2, [0, 0b11, 0b11, 0, 0b11, 0b11, 0, 0, 0, 0]),
        '-' | '—' | '–' => (4, [0, 0, 0, 0b1111, 0, 0, 0, 0, 0, 0]),
        '+' => (5, [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0, 0, 0, 0]),
        '/' => (4, [0b0001, 0b0001, 0b0010, 0b0100, 0b1000, 0b1000, 0, 0, 0, 0]),
        '\\' => (4, [0b1000, 0b1000, 0b0100, 0b0010, 0b0001, 0b0001, 0, 0, 0, 0]),
        '<' | '«' => (4, [0b0001, 0b0010, 0b0100, 0b1000, 0b0100, 0b0010, 0b0001, 0, 0, 0]),
        '>' | '»' => (4, [0b1000, 0b0100, 0b0010, 0b0001, 0b0010, 0b0100, 0b1000, 0, 0, 0]),
        '=' => (4, [0, 0, 0b1111, 0, 0b1111, 0, 0, 0, 0, 0]),
        '*' => (5, [0, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0, 0, 0, 0]),
        '?' => (4, [0b0110, 0b1001, 0b0010, 0b0100, 0b0100, 0, 0b0100, 0, 0, 0]),
        '#' => (5, [0, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0, 0, 0, 0]),
        '$' => (5, [0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100, 0, 0, 0]),
        '\'' | '`' | '´' | '’' | '‘' => (2, [0b10, 0b10, 0b01, 0, 0, 0, 0, 0, 0, 0]),
        '"' | '“' | '”' | '„' => (4, [0b1001, 0b1001, 0b1001, 0, 0, 0, 0, 0, 0, 0]),
        '_' => (5, [0, 0, 0, 0, 0, 0, 0b11111, 0, 0, 0]),
        '(' => (3, [0b001, 0b010, 0b100, 0b100, 0b100, 0b010, 0b001, 0, 0, 0]),
        ')' => (3, [0b100, 0b010, 0b001, 0b001, 0b001, 0b010, 0b100, 0, 0, 0]),
        '[' => (3, [0b111, 0b100, 0b100, 0b100, 0b100, 0b100, 0b111, 0, 0, 0]),
        ']' => (3, [0b111, 0b001, 0b001, 0b001, 0b001, 0b001, 0b111, 0, 0, 0]),
        '{' => (3, [0b011, 0b010, 0b010, 0b100, 0b010, 0b010, 0b011, 0, 0, 0]),
        '}' => (3, [0b110, 0b010, 0b010, 0b001, 0b010, 0b010, 0b110, 0, 0, 0]),
        '|' => (2, [0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0, 0]),
        '@' => (5, [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110, 0, 0, 0]),
        '^' => (5, [0b00100, 0b01010, 0b10001, 0, 0, 0, 0, 0, 0, 0]),
        '~' => (5, [0, 0, 0b01101, 0b10010, 0, 0, 0, 0, 0, 0]),
        '%' => (5, [0b11001, 0b11010, 0b00100, 0b00100, 0b01011, 0b10011, 0, 0, 0, 0]),
        '&' => (5, [0b01100, 0b10010, 0b01100, 0b10101, 0b10010, 0b10010, 0b01101, 0, 0, 0]),
        '0' => (5, [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110, 0, 0, 0]),
        '1' => (3, [0b010, 0b110, 0b010, 0b010, 0b010, 0b010, 0b111, 0, 0, 0]),
        '2' => (5, [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111, 0, 0, 0]),
        '3' => (5, [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110, 0, 0, 0]),
        '4' => (5, [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010, 0, 0, 0]),
        '5' => (5, [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110, 0, 0, 0]),
        '6' => (5, [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        '7' => (5, [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0, 0, 0]),
        '8' => (5, [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        '9' => (5, [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110, 0, 0, 0]),
        'A' | 'Ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Α' | 'Ά' => (5, [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0, 0, 0]),
        'B' | 'ß' | 'Β' => (5, [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110, 0, 0, 0]),
        'C' | 'Ç' => (5, [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110, 0, 0, 0]),
        'D' => (5, [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100, 0, 0, 0]),
        'E' | 'É' | 'È' | 'Ê' | 'Ë' | 'Ε' | 'Έ' => (4, [0b1111, 0b1000, 0b1000, 0b1110, 0b1000, 0b1000, 0b1111, 0, 0, 0]),
        'F' => (4, [0b1111, 0b1000, 0b1000, 0b1110, 0b1000, 0b1000, 0b1000, 0, 0, 0]),
        'G' => (5, [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'H' | 'Η' | 'Ή' => (5, [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0, 0, 0]),
        'I' | 'Í' | 'Ì' | 'Î' | 'Ï' | 'Ι' | 'Ί' | 'Ϊ' => (3, [0b111, 0b010, 0b010, 0b010, 0b010, 0b010, 0b111, 0, 0, 0]),
        'J' => (4, [0b0111, 0b0010, 0b0010, 0b0010, 0b1010, 0b1010, 0b0100, 0, 0, 0]),
        'K' | 'Κ' => (5, [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001, 0, 0, 0]),
        'L' => (4, [0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1111, 0, 0, 0]),
        'M' | 'Μ' => (5, [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001, 0, 0, 0]),
        'N' | 'Ñ' | 'Ν' => (5, [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001, 0, 0, 0]),
        'O' | 'Ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ο' | 'Ό' => (5, [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'P' | 'Ρ' => (5, [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000, 0, 0, 0]),
        'Q' => (5, [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101, 0, 0, 0]),
        'R' => (5, [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001, 0, 0, 0]),
        'S' => (5, [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110, 0, 0, 0]),
        'T' | 'Τ' => (5, [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0, 0]),
        'U' | 'Ü' | 'Ú' | 'Ù' | 'Û' => (5, [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'V' => (5, [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0, 0, 0]),
        'W' => (5, [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001, 0, 0, 0]),
        'X' | 'Χ' => (5, [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001, 0, 0, 0]),
        'Y' | 'Υ' | 'Ύ' | 'Ϋ' => (5, [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0, 0]),
        'Z' | 'Ζ' => (5, [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0, 0, 0]),
        'Γ' => (4, [0b1111, 0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0, 0, 0]),
        'Δ' | 'Λ' => (5, [0b00100, 0b01010, 0b01010, 0b10001, 0b10001, 0b10001, 0b11111, 0, 0, 0]),
        'Θ' => (5, [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'Ξ' => (5, [0b11111, 0, 0, 0b01110, 0, 0, 0b11111, 0, 0, 0]),
        'Π' => (5, [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0, 0, 0]),
        'Σ' => (5, [0b11111, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b11111, 0, 0, 0]),
        'Φ' => (5, [0b00100, 0b01110, 0b10101, 0b10101, 0b01110, 0b00100, 0b00100, 0, 0, 0]),
        'Ψ' => (5, [0b10101, 0b10101, 0b10101, 0b01110, 0b00100, 0b00100, 0b00100, 0, 0, 0]),
        'Ω' | 'Ώ' => (5, [0b01110, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b11011, 0, 0, 0]),
        // Normale Kleinbuchstaben (5px hoch in Zeilen 2..=6 auf der Baseline Y=7)
        'a' => (4, [0, 0, 0b0110, 0b0001, 0b0111, 0b1001, 0b0111, 0, 0, 0]),
        'ä' => (4, [0b1010, 0, 0b0110, 0b0001, 0b0111, 0b1001, 0b0111, 0, 0, 0]),
        'á' => (4, [0b0010, 0b0100, 0b0110, 0b0001, 0b0111, 0b1001, 0b0111, 0, 0, 0]),
        'à' => (4, [0b0100, 0b0010, 0b0110, 0b0001, 0b0111, 0b1001, 0b0111, 0, 0, 0]),
        'â' | 'ã' => (4, [0b0110, 0b1001, 0b0110, 0b0001, 0b0111, 0b1001, 0b0111, 0, 0, 0]),
        'b' => (4, [0b1000, 0b1000, 0b1110, 0b1001, 0b1001, 0b1001, 0b1110, 0, 0, 0]),
        'c' => (4, [0, 0, 0b0111, 0b1000, 0b1000, 0b1000, 0b0111, 0, 0, 0]),
        'ç' => (4, [0, 0, 0b0111, 0b1000, 0b1000, 0b1000, 0b0111, 0b0010, 0b0100, 0]),
        'd' => (4, [0b0001, 0b0001, 0b0111, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'e' => (4, [0, 0, 0b0110, 0b1001, 0b1111, 0b1000, 0b0111, 0, 0, 0]),
        'é' => (4, [0b0010, 0b0100, 0b0110, 0b1001, 0b1111, 0b1000, 0b0111, 0, 0, 0]),
        'è' => (4, [0b0100, 0b0010, 0b0110, 0b1001, 0b1111, 0b1000, 0b0111, 0, 0, 0]),
        'ê' => (4, [0b0110, 0b1001, 0b0110, 0b1001, 0b1111, 0b1000, 0b0111, 0, 0, 0]),
        'ë' => (4, [0b1010, 0, 0b0110, 0b1001, 0b1111, 0b1000, 0b0111, 0, 0, 0]),
        'f' => (3, [0b011, 0b100, 0b111, 0b100, 0b100, 0b100, 0b100, 0, 0, 0]),
        // 'g' mit echter 3px Unterlänge (Zeilen 7..=9 unterhalb der Baseline!)
        'g' => (4, [0, 0, 0b0111, 0b1001, 0b1001, 0b1001, 0b0111, 0b0001, 0b1001, 0b0110]),
        'h' => (4, [0b1000, 0b1000, 0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0, 0, 0]),
        'i' | 'í' | 'ì' | 'î' | 'ï' => (2, [0b10, 0, 0b10, 0b10, 0b10, 0b10, 0b11, 0, 0, 0]),
        // 'j' mit echter 3px Unterlänge (Zeilen 7..=9 unterhalb der Baseline!)
        'j' => (3, [0b001, 0, 0b001, 0b001, 0b001, 0b001, 0b001, 0b001, 0b101, 0b010]),
        'k' => (4, [0b1000, 0b1000, 0b1001, 0b1010, 0b1100, 0b1010, 0b1001, 0, 0, 0]),
        'l' => (2, [0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b11, 0, 0, 0]),
        'm' => (5, [0, 0, 0b11010, 0b10101, 0b10101, 0b10101, 0b10001, 0, 0, 0]),
        'n' => (4, [0, 0, 0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0, 0, 0]),
        'ñ' => (4, [0b0101, 0b1010, 0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0, 0, 0]),
        'o' => (4, [0, 0, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ö' => (4, [0b1010, 0, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ó' => (4, [0b0010, 0b0100, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ò' => (4, [0b0100, 0b0010, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ô' | 'õ' => (4, [0b0110, 0b1001, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        // 'p' mit echter 3px Unterlänge (Bauch auf Zeilen 2..=6, Schaft auf Zeilen 7..=9 unterhalb der Baseline!)
        'p' => (4, [0, 0, 0b1110, 0b1001, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000, 0b1000]),
        // 'q' mit echter 3px Unterlänge (Bauch auf Zeilen 2..=6, Schaft auf Zeilen 7..=9 unterhalb der Baseline!)
        'q' => (4, [0, 0, 0b0111, 0b1001, 0b1001, 0b1001, 0b0111, 0b0001, 0b0001, 0b0001]),
        'r' => (3, [0, 0, 0b101, 0b110, 0b100, 0b100, 0b100, 0, 0, 0]),
        's' => (4, [0, 0, 0b0111, 0b1000, 0b0110, 0b0001, 0b1110, 0, 0, 0]),
        't' => (3, [0b010, 0b010, 0b111, 0b010, 0b010, 0b010, 0b011, 0, 0, 0]),
        'u' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'ü' => (4, [0b1010, 0, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'ú' => (4, [0b0010, 0b0100, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'ù' => (4, [0b0100, 0b0010, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'û' => (4, [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0, 0, 0]),
        'v' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b0110, 0b0110, 0, 0, 0]),
        'w' => (5, [0, 0, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010, 0, 0, 0]),
        'x' => (4, [0, 0, 0b1001, 0b0110, 0b0110, 0b1001, 0b1001, 0, 0, 0]),
        // 'y' mit echter 3px Unterlänge (Zeilen 7..=9 unterhalb der Baseline!)
        'y' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b1001, 0b0111, 0b0001, 0b1001, 0b0110]),
        'z' => (4, [0, 0, 0b1111, 0b0010, 0b0100, 0b1000, 0b1111, 0, 0, 0]),
        // Griechische Kleinbuchstaben (α..ω)
        'α' | 'ά' => (5, [0, 0, 0b01101, 0b10010, 0b10010, 0b10010, 0b01101, 0, 0, 0]),
        'β' => (4, [0b0110, 0b1001, 0b1110, 0b1001, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000, 0]),
        'γ' => (4, [0, 0, 0b1001, 0b1001, 0b0110, 0b0010, 0b0010, 0b0010, 0b0010, 0]),
        'δ' => (4, [0b0111, 0b0100, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ε' | 'έ' => (4, [0, 0, 0b0111, 0b1000, 0b0110, 0b1000, 0b0111, 0, 0, 0]),
        'ζ' => (4, [0b1111, 0b0010, 0b0100, 0b1000, 0b1000, 0b0110, 0b0001, 0b0110, 0, 0]),
        'η' | 'ή' => (4, [0, 0, 0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0001, 0b0001, 0]),
        'θ' => (4, [0b0110, 0b1001, 0b1001, 0b1111, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'ι' | 'ί' | 'ϊ' | 'ΐ' => (2, [0, 0, 0b10, 0b10, 0b10, 0b10, 0b11, 0, 0, 0]),
        'κ' => (4, [0, 0, 0b1001, 0b1010, 0b1100, 0b1010, 0b1001, 0, 0, 0]),
        'λ' => (4, [0b1000, 0b0100, 0b0110, 0b1010, 0b1001, 0b1001, 0b1001, 0, 0, 0]),
        'μ' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b1001, 0b1111, 0b1000, 0b1000, 0]),
        'ν' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b0110, 0b0010, 0, 0, 0]),
        'ξ' => (4, [0b1111, 0b0100, 0b0110, 0b0100, 0b1000, 0b0110, 0b0001, 0b0110, 0, 0]),
        'ο' | 'ό' => (4, [0, 0, 0b0110, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'π' => (5, [0, 0, 0b11111, 0b01010, 0b01010, 0b01010, 0b01010, 0, 0, 0]),
        'ρ' => (4, [0, 0, 0b0110, 0b1001, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000, 0]),
        'σ' | 'ς' => (5, [0, 0, 0b01111, 0b10010, 0b10010, 0b10010, 0b01100, 0, 0, 0]),
        'τ' => (4, [0, 0, 0b1111, 0b0100, 0b0100, 0b0100, 0b0011, 0, 0, 0]),
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => (4, [0, 0, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110, 0, 0, 0]),
        'φ' => (5, [0, 0, 0b01110, 0b10101, 0b10101, 0b10101, 0b01110, 0b00100, 0b00100, 0]),
        'χ' => (4, [0, 0, 0b1001, 0b0110, 0b0110, 0b1001, 0b1001, 0b1001, 0, 0]),
        'ψ' => (5, [0, 0, 0b10101, 0b10101, 0b10101, 0b01110, 0b00100, 0b00100, 0b00100, 0]),
        'ω' | 'ώ' => (5, [0, 0, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010, 0, 0, 0]),
        '…' => (5, [0, 0, 0, 0, 0, 0, 0b10101, 0, 0, 0]),
        _ => (4, [0b0110, 0b1001, 0b0010, 0b0100, 0b0100, 0, 0b0100, 0, 0, 0]),
    }
}

/// Berechnet die exakte Pixelbreite eines Strings im `6×10` Modern-UI-Font.
pub fn modern_ui_text_width(text: &str) -> i32 {
    let mut w = 0i32;
    let mut first = true;
    for ch in text.chars() {
        if !first {
            w += 1;
        }
        first = false;
        let (gw, _) = modern_ui_glyph_6x10(ch);
        w += gw as i32;
    }
    w
}

/// Zeichnet einen String mit dem `6×10` Modern-UI-Font inkl. echter Unterlängen (`g, j, p, q, y`) unterhalb der Baseline (`y + 6`).
pub fn draw_modern_ui_text(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
) {
    let dw = w as i32;
    let dh = h as i32;
    let mut cur_x = x;
    for ch in text.chars() {
        let (gw, rows) = modern_ui_glyph_6x10(ch);
        for (ry, &mask) in rows.iter().enumerate() {
            if mask == 0 {
                continue;
            }
            let py = y + ry as i32;
            if py < 0 || py >= dh {
                continue;
            }
            for rx in 0..(gw as i32) {
                let px = cur_x + rx;
                if px < 0 || px >= dw {
                    continue;
                }
                let bit = (gw as i32) - 1 - rx;
                if (mask & (1 << bit)) != 0 {
                    let idx = ((py as u32 * w + px as u32) * 4) as usize;
                    if idx + 3 < buf.len() {
                        buf[idx] = color[0];
                        buf[idx + 1] = color[1];
                        buf[idx + 2] = color[2];
                        buf[idx + 3] = color[3];
                    }
                }
            }
        }
        cur_x += (gw as i32) + 1;
    }
}

