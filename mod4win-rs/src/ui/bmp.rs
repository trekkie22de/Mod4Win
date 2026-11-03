use std::fs;
use std::path::Path;

/// Ein einfaches Bitmap-Bild im 32-Bit-RGBA-Format.
#[derive(Clone, Debug)]
pub struct BmpImage {
    pub width: u32,
    pub height: u32,
    /// RGBA-Pixeldaten (width * height * 4 Bytes, je Pixel: [R, G, B, A])
    pub data: Vec<u8>,
}

impl BmpImage {
    /// Erzeugt ein leeres Bild mit gegebener Auflösung.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }

    /// Lädt eine BMP-Datei von der Festplatte.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let bytes = fs::read(&path)
            .map_err(|e| format!("Fehler beim Lesen von {:?}: {}", path.as_ref(), e))?;
        Self::from_bytes(&bytes)
    }

    /// Dekodiert eine BMP-Datei aus einem Byte-Slice.
    /// Unterstützt 1-bpp, 4-bpp (16 Farben palettiert), 8-bpp (256 Farben palettiert),
    /// sowie 24-bpp und 32-bpp unkomprimiert (BI_RGB).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 54 {
            return Err("BMP-Daten zu kurz für Standard-Header".into());
        }

        if bytes[0] != b'B' || bytes[1] != b'M' {
            return Err("Keine gültige BMP-Signatur ('BM')".into());
        }

        let pixel_offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let dib_header_size = u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize;
        if dib_header_size < 40 {
            return Err(format!("Nicht unterstützte DIB-Header-Größe: {}", dib_header_size));
        }

        let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
        let height = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
        let planes = u16::from_le_bytes(bytes[26..28].try_into().unwrap());
        let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
        let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());
        let clr_used = u32::from_le_bytes(bytes[46..50].try_into().unwrap());

        if planes != 1 {
            return Err(format!("Nicht unterstützte Plane-Anzahl: {}", planes));
        }
        if compression != 0 {
            return Err(format!("Komprimierte BMPs (Typ {}) werden nicht unterstützt", compression));
        }
        if width <= 0 {
            return Err(format!("Ungültige Bildbreite: {}", width));
        }

        let img_width = width as u32;
        let is_top_down = height < 0;
        let img_height = height.unsigned_abs();

        // Farbpalette einlesen (falls bpp <= 8)
        let palette_entries = if bpp <= 8 {
            if clr_used != 0 {
                clr_used as usize
            } else {
                1usize << bpp
            }
        } else {
            0
        };

        let mut palette: Vec<[u8; 4]> = Vec::with_capacity(palette_entries);
        if palette_entries > 0 {
            let pal_offset = 14 + dib_header_size;
            if bytes.len() < pal_offset + palette_entries * 4 {
                return Err("BMP-Datei zu kurz für Farbpalette".into());
            }
            for i in 0..palette_entries {
                let entry_idx = pal_offset + i * 4;
                let b = bytes[entry_idx];
                let g = bytes[entry_idx + 1];
                let r = bytes[entry_idx + 2];
                palette.push([r, g, b, 255]);
            }
        }

        let mut rgba_data = vec![0u8; (img_width * img_height * 4) as usize];
        let row_stride = (img_width as usize * bpp as usize).div_ceil(32) * 4;

        if bytes.len() < pixel_offset {
            return Err("Pixel-Offset liegt außerhalb der Datei".into());
        }

        let raw_pixels = &bytes[pixel_offset..];

        for row in 0..img_height as usize {
            let src_row_offset = row * row_stride;
            if src_row_offset + row_stride > raw_pixels.len() && row < (img_height as usize - 1) {
                break;
            }
            let target_y = if is_top_down {
                row
            } else {
                img_height as usize - 1 - row
            };

            let row_slice = if src_row_offset < raw_pixels.len() {
                let end = (src_row_offset + row_stride).min(raw_pixels.len());
                &raw_pixels[src_row_offset..end]
            } else {
                &[]
            };

            for col in 0..img_width as usize {
                let dst_pixel_idx = (target_y * img_width as usize + col) * 4;
                let color = match bpp {
                    1 => {
                        let byte_idx = col / 8;
                        let bit_idx = 7 - (col % 8);
                        let pal_idx = if byte_idx < row_slice.len() {
                            ((row_slice[byte_idx] >> bit_idx) & 1) as usize
                        } else {
                            0
                        };
                        palette.get(pal_idx).copied().unwrap_or([0, 0, 0, 255])
                    }
                    4 => {
                        let byte_idx = col / 2;
                        let is_high = (col % 2) == 0;
                        let pal_idx = if byte_idx < row_slice.len() {
                            let b = row_slice[byte_idx];
                            if is_high {
                                (b >> 4) as usize
                            } else {
                                (b & 0x0F) as usize
                            }
                        } else {
                            0
                        };
                        palette.get(pal_idx).copied().unwrap_or([0, 0, 0, 255])
                    }
                    8 => {
                        let pal_idx = if col < row_slice.len() {
                            row_slice[col] as usize
                        } else {
                            0
                        };
                        palette.get(pal_idx).copied().unwrap_or([0, 0, 0, 255])
                    }
                    24 => {
                        let byte_idx = col * 3;
                        if byte_idx + 2 < row_slice.len() {
                            let b = row_slice[byte_idx];
                            let g = row_slice[byte_idx + 1];
                            let r = row_slice[byte_idx + 2];
                            [r, g, b, 255]
                        } else {
                            [0, 0, 0, 255]
                        }
                    }
                    32 => {
                        let byte_idx = col * 4;
                        if byte_idx + 3 < row_slice.len() {
                            let b = row_slice[byte_idx];
                            let g = row_slice[byte_idx + 1];
                            let r = row_slice[byte_idx + 2];
                            let a = row_slice[byte_idx + 3];
                            [r, g, b, if a == 0 { 255 } else { a }]
                        } else {
                            [0, 0, 0, 255]
                        }
                    }
                    _ => return Err(format!("Nicht unterstützte Farbtiefe: {} bpp", bpp)),
                };

                rgba_data[dst_pixel_idx..dst_pixel_idx + 4].copy_from_slice(&color);
            }
        }

        Ok(Self {
            width: img_width,
            height: img_height,
            data: rgba_data,
        })
    }

    /// Kopiert das gesamte Bild an die Zielposition `(dx, dy)` in einem Framebuffer.
    pub fn blit_to(&self, dst: &mut [u8], dst_width: u32, dst_height: u32, dx: i32, dy: i32) {
        self.blit_rect_to(
            0,
            0,
            self.width,
            self.height,
            dst,
            dst_width,
            dst_height,
            dx,
            dy,
        );
    }

    /// Kopiert einen Teilausschnitt `(sx, sy, w, h)` dieses Bildes an Position `(dx, dy)` in den Framebuffer.
    pub fn blit_rect_to(
        &self,
        sx: u32,
        sy: u32,
        w: u32,
        h: u32,
        dst: &mut [u8],
        dst_width: u32,
        dst_height: u32,
        dx: i32,
        dy: i32,
    ) {
        let sw = self.width as i32;
        let sh = self.height as i32;
        let dw = dst_width as i32;
        let dh = dst_height as i32;

        let w = w as i32;
        let h = h as i32;
        let sx = sx as i32;
        let sy = sy as i32;

        for y in 0..h {
            let src_y = sy + y;
            let target_y = dy + y;
            if src_y < 0 || src_y >= sh || target_y < 0 || target_y >= dh {
                continue;
            }

            for x in 0..w {
                let src_x = sx + x;
                let target_x = dx + x;
                if src_x < 0 || src_x >= sw || target_x < 0 || target_x >= dw {
                    continue;
                }

                let src_idx = ((src_y * sw + src_x) * 4) as usize;
                let dst_idx = ((target_y * dw + target_x) * 4) as usize;

                let alpha = self.data[src_idx + 3];
                if alpha == 255 {
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
                } else if alpha > 0 {
                    let a = alpha as u32;
                    let inv_a = 255 - a;
                    dst[dst_idx] = ((self.data[src_idx] as u32 * a
                        + dst[dst_idx] as u32 * inv_a)
                        / 255) as u8;
                    dst[dst_idx + 1] = ((self.data[src_idx + 1] as u32 * a
                        + dst[dst_idx + 1] as u32 * inv_a)
                        / 255) as u8;
                    dst[dst_idx + 2] = ((self.data[src_idx + 2] as u32 * a
                        + dst[dst_idx + 2] as u32 * inv_a)
                        / 255) as u8;
                    dst[dst_idx + 3] = 255;
                }
            }
        }
    }

    /// Kopiert einen Teilausschnitt mit Color-Keying (Transparenzfarbe wird übersprungen).
    pub fn blit_rect_key_to(
        &self,
        sx: u32,
        sy: u32,
        w: u32,
        h: u32,
        dst: &mut [u8],
        dst_width: u32,
        dst_height: u32,
        dx: i32,
        dy: i32,
        key_color: [u8; 3],
    ) {
        let sw = self.width as i32;
        let sh = self.height as i32;
        let dw = dst_width as i32;
        let dh = dst_height as i32;

        let w = w as i32;
        let h = h as i32;
        let sx = sx as i32;
        let sy = sy as i32;

        for y in 0..h {
            let src_y = sy + y;
            let target_y = dy + y;
            if src_y < 0 || src_y >= sh || target_y < 0 || target_y >= dh {
                continue;
            }

            for x in 0..w {
                let src_x = sx + x;
                let target_x = dx + x;
                if src_x < 0 || src_x >= sw || target_x < 0 || target_x >= dw {
                    continue;
                }

                let src_idx = ((src_y * sw + src_x) * 4) as usize;
                let r = self.data[src_idx];
                let g = self.data[src_idx + 1];
                let b = self.data[src_idx + 2];

                if r == key_color[0] && g == key_color[1] && b == key_color[2] {
                    continue;
                }

                let dst_idx = ((target_y * dw + target_x) * 4) as usize;
                dst[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_main_bmp() {
        let path = std::path::Path::new("assets/hlp_extracted/bm0.bmp");
        let img = BmpImage::from_file(path).expect("bm0.bmp konnte nicht geladen werden");
        assert_eq!(img.width, 542);
        assert_eq!(img.height, 135);
        assert_eq!(img.data.len(), 542 * 135 * 4);
    }

    #[test]
    fn test_load_setup_bmp() {
        let path = std::path::Path::new("assets/hlp_extracted/bm4.bmp");
        let img = BmpImage::from_file(path).expect("bm4.bmp konnte nicht geladen werden");
        assert!(img.width > 0 && img.height > 0);
        assert_eq!(img.data.len(), (img.width * img.height * 4) as usize);
    }

    #[test]
    fn test_inspect_effects_bmp() {
        let eff = BmpImage::from_file("assets/hlp_extracted/bm13.bmp")
            .expect("bm13.bmp konnte nicht geladen werden");
        assert_eq!(eff.width, 381);
        assert_eq!(eff.height, 281);
    }
}
