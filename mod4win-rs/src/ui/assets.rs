//! Eingebettete Original-Bitmaps und Hotspot-Tabellen (via `include_bytes!` / `include_str!`).
//! Dadurch läuft die kompilierte `.exe` komplett standalone ohne externe Dateipfade.

use super::bmp::BmpImage;
use std::collections::HashMap;

/// Eingebettete JSON-Hotspot-Tabelle aus `M4W_GER.HLP` (`|bm0` bis `|bm15`).
pub const HOTSPOTS_JSON: &str = include_str!("../../assets/hlp_extracted/hotspots.json");

/// Gemeinsam genutzte Roh-Bytes der 16 Hilfe-Bitmaps (`bm0.bmp`..`bm15.bmp`).
pub const HLP_BM_RAW_BYTES: [(&str, &[u8]); 16] = [
    ("bm0.bmp", include_bytes!("../../assets/hlp_extracted/bm0.bmp")),
    ("bm1.bmp", include_bytes!("../../assets/hlp_extracted/bm1.bmp")),
    ("bm2.bmp", include_bytes!("../../assets/hlp_extracted/bm2.bmp")),
    ("bm3.bmp", include_bytes!("../../assets/hlp_extracted/bm3.bmp")),
    ("bm4.bmp", include_bytes!("../../assets/hlp_extracted/bm4.bmp")),
    ("bm5.bmp", include_bytes!("../../assets/hlp_extracted/bm5.bmp")),
    ("bm6.bmp", include_bytes!("../../assets/hlp_extracted/bm6.bmp")),
    ("bm7.bmp", include_bytes!("../../assets/hlp_extracted/bm7.bmp")),
    ("bm8.bmp", include_bytes!("../../assets/hlp_extracted/bm8.bmp")),
    ("bm9.bmp", include_bytes!("../../assets/hlp_extracted/bm9.bmp")),
    ("bm10.bmp", include_bytes!("../../assets/hlp_extracted/bm10.bmp")),
    ("bm11.bmp", include_bytes!("../../assets/hlp_extracted/bm11.bmp")),
    ("bm12.bmp", include_bytes!("../../assets/hlp_extracted/bm12.bmp")),
    ("bm13.bmp", include_bytes!("../../assets/hlp_extracted/bm13.bmp")),
    ("bm14.bmp", include_bytes!("../../assets/hlp_extracted/bm14.bmp")),
    ("bm15.bmp", include_bytes!("../../assets/hlp_extracted/bm15.bmp")),
];

/// Ein einzelnes Hotspot-Rechteck aus einer `.HLP` Segmented Hypergraphic (`|bm0`..`|bm15`).
#[derive(Clone, Debug)]
pub struct HotspotRect {
    pub name: String,
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl HotspotRect {
    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width as i32
            && py >= self.y
            && py < self.y + self.height as i32
    }
}

/// Alle Hotspots eines WinHelp-Bitmap-Blocks (`bm0`..`bm15`).
#[derive(Clone, Debug, Default)]
pub struct BmHotspotBlock {
    pub width: u32,
    pub height: u32,
    pub hotspots: Vec<HotspotRect>,
}

impl BmHotspotBlock {
    /// Findet das erste Hotspot-Rechteck mit passender `id` (Case-Insensitive).
    pub fn find_by_id(&self, id: &str) -> Option<&HotspotRect> {
        self.hotspots
            .iter()
            .find(|h| h.id.eq_ignore_ascii_case(id))
    }

    /// Findet das Hotspot-Rechteck unter den Koordinaten `(px, py)`.
    pub fn hit_test(&self, px: i32, py: i32) -> Option<&HotspotRect> {
        self.hotspots.iter().find(|h| h.contains(px, py))
    }
}

/// Sämtliche per `include_bytes!` fest in die `.exe` eingebetteten UI-Bitmaps und Hotspots.
#[derive(Clone, Debug)]
pub struct EmbeddedAssets {
    // Aus M4W_GER.HLP extrahierte Dialog-Bitmaps (bm0..bm15)
    pub bm0_main: BmpImage,
    pub bm1_mini: BmpImage,
    pub bm2_fileman: BmpImage,
    pub bm3_selbox: BmpImage,
    pub bm4_setup: BmpImage,
    pub bm5_arcopt: BmpImage,
    pub bm6_arcparm: BmpImage,
    pub bm7_keysel: BmpImage,
    pub bm8_fsel: BmpImage,
    pub bm9_open: BmpImage,
    pub bm10_info: BmpImage,
    pub bm11_ins: BmpImage,
    pub bm12_effwnd: BmpImage,
    pub bm13_effects_help: BmpImage,
    pub bm14_regdlg: BmpImage,
    pub bm15_recname: BmpImage,

    // Aus MOD4WIN.RES / WINUNIT.RES extrahierte Original-4bpp-Sprites
    pub modmainframe: BmpImage,
    pub about: BmpImage,
    pub font_16x20: BmpImage,
    pub lcd_15x24: BmpImage,
    pub lcd_13x20: BmpImage,
    pub lcd_08x13: BmpImage,
    pub num_7x5: BmpImage,
    pub pegel: BmpImage,
    pub hkonoff: BmpImage,
    pub effbmp: BmpImage,
    pub effcomm: BmpImage,
    pub preamp: BmpImage,
    pub minus_mode: BmpImage,
    pub sh_rep_scan: BmpImage,
    pub m_st: BmpImage,
    pub tmode: BmpImage,
    pub switch_bmp: BmpImage,
    pub m_label: BmpImage,
    pub boxes: BmpImage,
    pub funbmp: BmpImage,
    pub sysbuttons_w95: BmpImage,
    pub boplus: BmpImage,
    pub bominus: BmpImage,
    pub bstop: BmpImage,
    pub bplay: BmpImage,
    pub bpause: BmpImage,
    pub bprev: BmpImage,
    pub bfr: BmpImage,
    pub bff: BmpImage,
    pub bnext: BmpImage,
    pub bopen: BmpImage,
    pub bdatei: BmpImage,
    pub binfo: BmpImage,
    pub beffects: BmpImage,
    pub bshuffle: BmpImage,
    pub brepeat: BmpImage,
    pub bintroscan: BmpImage,
    pub btime: BmpImage,
    pub bmixer95: BmpImage,
    pub bsetup95: BmpImage,
    pub bhelp95: BmpImage,

    // Open-Dialog & Quick-Selection Bitmaps (aus MOD4WIN.RES / NEWOPEN.RES)
    pub boadd: BmpImage,
    pub bodel: BmpImage,
    pub bomal: BmpImage,
    pub bsok: BmpImage,
    pub bssave: BmpImage,
    pub bscancel: BmpImage,
    pub bshelp: BmpImage,
    pub b_order: BmpImage,
    pub b_name: BmpImage,
    pub b_ext: BmpImage,
    pub bmp_drives: BmpImage,
    pub arc_bmp: BmpImage,
    pub ft_mod: BmpImage,
    pub ft_nst: BmpImage,
    pub ft_okt: BmpImage,
    pub ft_669: BmpImage,
    pub ft_wow: BmpImage,
    pub ft_stm: BmpImage,
    pub ft_mtm: BmpImage,
    pub ft_far: BmpImage,
    pub ft_ult: BmpImage,
    pub ft_s3m: BmpImage,
    pub ft_dmf: BmpImage,
    pub ft_xm: BmpImage,
    pub ft_it: BmpImage,

    // Setup-Dialog & AskWin Bitmaps (aus MOD4WIN.RES / DSETUP.PAS)
    pub sgraypieces: BmpImage,
    pub b_up_down_1: BmpImage,
    pub lrswitch: BmpImage,
    pub bhotkeys: BmpImage,
    pub bpacker: BmpImage,
    pub b_lang: BmpImage,
    pub b_browse: BmpImage,
    pub b_more: BmpImage,
    pub b_default: BmpImage,
    pub mb_iconquestion: BmpImage,

    // Geparste Hotspot-Tabellen für bm0..bm15
    pub hotspots: HashMap<String, BmHotspotBlock>,
}

impl EmbeddedAssets {
    pub fn load() -> Self {
        let must = |bytes: &[u8], name: &str| -> BmpImage {
            BmpImage::from_bytes(bytes)
                .unwrap_or_else(|e| panic!("Fehler beim Dekodieren von {}: {}", name, e))
        };

        Self {
            bm0_main: must(HLP_BM_RAW_BYTES[0].1, HLP_BM_RAW_BYTES[0].0),
            bm1_mini: must(HLP_BM_RAW_BYTES[1].1, HLP_BM_RAW_BYTES[1].0),
            bm2_fileman: must(HLP_BM_RAW_BYTES[2].1, HLP_BM_RAW_BYTES[2].0),
            bm3_selbox: must(HLP_BM_RAW_BYTES[3].1, HLP_BM_RAW_BYTES[3].0),
            bm4_setup: must(HLP_BM_RAW_BYTES[4].1, HLP_BM_RAW_BYTES[4].0),
            bm5_arcopt: must(HLP_BM_RAW_BYTES[5].1, HLP_BM_RAW_BYTES[5].0),
            bm6_arcparm: must(HLP_BM_RAW_BYTES[6].1, HLP_BM_RAW_BYTES[6].0),
            bm7_keysel: must(HLP_BM_RAW_BYTES[7].1, HLP_BM_RAW_BYTES[7].0),
            bm8_fsel: must(HLP_BM_RAW_BYTES[8].1, HLP_BM_RAW_BYTES[8].0),
            bm9_open: must(HLP_BM_RAW_BYTES[9].1, HLP_BM_RAW_BYTES[9].0),
            bm10_info: must(HLP_BM_RAW_BYTES[10].1, HLP_BM_RAW_BYTES[10].0),
            bm11_ins: must(HLP_BM_RAW_BYTES[11].1, HLP_BM_RAW_BYTES[11].0),
            bm12_effwnd: must(HLP_BM_RAW_BYTES[12].1, HLP_BM_RAW_BYTES[12].0),
            bm13_effects_help: must(HLP_BM_RAW_BYTES[13].1, HLP_BM_RAW_BYTES[13].0),
            bm14_regdlg: must(HLP_BM_RAW_BYTES[14].1, HLP_BM_RAW_BYTES[14].0),
            bm15_recname: must(HLP_BM_RAW_BYTES[15].1, HLP_BM_RAW_BYTES[15].0),

            modmainframe: must(include_bytes!("../../assets/MODMAINFRAME.bmp"), "MODMAINFRAME.bmp"),
            about: must(include_bytes!("../../assets/ABOUT.bmp"), "ABOUT.bmp"),
            font_16x20: must(include_bytes!("../../assets/FONT_16X20.bmp"), "FONT_16X20.bmp"),
            lcd_15x24: must(include_bytes!("../../assets/LCD_15X24.bmp"), "LCD_15X24.bmp"),
            lcd_13x20: must(include_bytes!("../../assets/LCD_13X20.bmp"), "LCD_13X20.bmp"),
            lcd_08x13: must(include_bytes!("../../assets/LCD_08X13.bmp"), "LCD_08X13.bmp"),
            num_7x5: must(include_bytes!("../../assets/NUM_7X5.bmp"), "NUM_7X5.bmp"),
            pegel: must(include_bytes!("../../assets/PEGEL.bmp"), "PEGEL.bmp"),
            hkonoff: must(include_bytes!("../../assets/HKONOFF.bmp"), "HKONOFF.bmp"),
            effbmp: must(include_bytes!("../../assets/EFFBMP.bmp"), "EFFBMP.bmp"),
            effcomm: must(include_bytes!("../../assets/EFFCOMM.bmp"), "EFFCOMM.bmp"),
            preamp: must(include_bytes!("../../assets/PREAMP.bmp"), "PREAMP.bmp"),
            minus_mode: must(include_bytes!("../../assets/MINUS_MODE.bmp"), "MINUS_MODE.bmp"),
            sh_rep_scan: must(include_bytes!("../../assets/SH_REP_SCAN.bmp"), "SH_REP_SCAN.bmp"),
            m_st: must(include_bytes!("../../assets/M_ST.bmp"), "M_ST.bmp"),
            tmode: must(include_bytes!("../../assets/TMODE.bmp"), "TMODE.bmp"),
            switch_bmp: must(include_bytes!("../../assets/SWITCH.bmp"), "SWITCH.bmp"),
            m_label: must(include_bytes!("../../assets/M_LABEL.bmp"), "M_LABEL.bmp"),
            boxes: must(include_bytes!("../../assets/BOXES.bmp"), "BOXES.bmp"),
            funbmp: must(include_bytes!("../../assets/FUNBMP.bmp"), "FUNBMP.bmp"),
            sysbuttons_w95: must(
                include_bytes!("../../assets/SYSBUTTONS_W95.bmp"),
                "SYSBUTTONS_W95.bmp",
            ),
            boplus: must(include_bytes!("../../assets/BOPLUS.bmp"), "BOPLUS.bmp"),
            bominus: must(include_bytes!("../../assets/BOMINUS.bmp"), "BOMINUS.bmp"),
            bstop: must(include_bytes!("../../assets/BSTOP.bmp"), "BSTOP.bmp"),
            bplay: must(include_bytes!("../../assets/BPLAY.bmp"), "BPLAY.bmp"),
            bpause: must(include_bytes!("../../assets/BPAUSE.bmp"), "BPAUSE.bmp"),
            bprev: must(include_bytes!("../../assets/BPREV.bmp"), "BPREV.bmp"),
            bfr: must(include_bytes!("../../assets/BFR.bmp"), "BFR.bmp"),
            bff: must(include_bytes!("../../assets/BFF.bmp"), "BFF.bmp"),
            bnext: must(include_bytes!("../../assets/BNEXT.bmp"), "BNEXT.bmp"),
            bopen: must(include_bytes!("../../assets/BOPEN.bmp"), "BOPEN.bmp"),
            bdatei: must(include_bytes!("../../assets/BDATEI.bmp"), "BDATEI.bmp"),
            binfo: must(include_bytes!("../../assets/BINFO.bmp"), "BINFO.bmp"),
            beffects: must(include_bytes!("../../assets/BEFFECTS.bmp"), "BEFFECTS.bmp"),
            bshuffle: must(include_bytes!("../../assets/BSHUFFLE.bmp"), "BSHUFFLE.bmp"),
            brepeat: must(include_bytes!("../../assets/BREPEAT.bmp"), "BREPEAT.bmp"),
            bintroscan: must(include_bytes!("../../assets/BINTROSCAN.bmp"), "BINTROSCAN.bmp"),
            btime: must(include_bytes!("../../assets/BTIME.bmp"), "BTIME.bmp"),
            bmixer95: must(include_bytes!("../../assets/BMIXER95.bmp"), "BMIXER95.bmp"),
            bsetup95: must(include_bytes!("../../assets/BSETUP95.bmp"), "BSETUP95.bmp"),
            bhelp95: must(include_bytes!("../../assets/BHELP95.bmp"), "BHELP95.bmp"),

            boadd: must(include_bytes!("../../assets/BOADD.bmp"), "BOADD.bmp"),
            bodel: must(include_bytes!("../../assets/BODEL.bmp"), "BODEL.bmp"),
            bomal: must(include_bytes!("../../assets/BOMAL.bmp"), "BOMAL.bmp"),
            bsok: must(include_bytes!("../../assets/BSOK.bmp"), "BSOK.bmp"),
            bssave: must(include_bytes!("../../assets/BSSAVE.bmp"), "BSSAVE.bmp"),
            bscancel: must(include_bytes!("../../assets/BSCANCEL.bmp"), "BSCANCEL.bmp"),
            bshelp: must(include_bytes!("../../assets/BSHELP.bmp"), "BSHELP.bmp"),
            b_order: must(include_bytes!("../../assets/B_ORDER.bmp"), "B_ORDER.bmp"),
            b_name: must(include_bytes!("../../assets/B_NAME.bmp"), "B_NAME.bmp"),
            b_ext: must(include_bytes!("../../assets/B_EXT.bmp"), "B_EXT.bmp"),
            bmp_drives: must(include_bytes!("../../assets/BMP_DRIVES.bmp"), "BMP_DRIVES.bmp"),
            arc_bmp: must(include_bytes!("../../assets/ARC.bmp"), "ARC.bmp"),
            ft_mod: must(include_bytes!("../../assets/RT_BITMAP.bmp"), "RT_BITMAP.bmp"),
            ft_nst: must(include_bytes!("../../assets/RT_ICON.bmp"), "RT_ICON.bmp"),
            ft_okt: must(include_bytes!("../../assets/RT_MENU.bmp"), "RT_MENU.bmp"),
            ft_669: must(include_bytes!("../../assets/RT_DIALOG.bmp"), "RT_DIALOG.bmp"),
            ft_wow: must(include_bytes!("../../assets/RT_STRING.bmp"), "RT_STRING.bmp"),
            ft_stm: must(include_bytes!("../../assets/7.bmp"), "7.bmp"),
            ft_mtm: must(include_bytes!("../../assets/8.bmp"), "8.bmp"),
            ft_far: must(include_bytes!("../../assets/9.bmp"), "9.bmp"),
            ft_ult: must(include_bytes!("../../assets/10.bmp"), "10.bmp"),
            ft_s3m: must(include_bytes!("../../assets/11.bmp"), "11.bmp"),
            ft_dmf: must(include_bytes!("../../assets/12.bmp"), "12.bmp"),
            ft_xm: must(include_bytes!("../../assets/13.bmp"), "13.bmp"),
            ft_it: must(include_bytes!("../../assets/17.bmp"), "17.bmp"),

            sgraypieces: must(
                include_bytes!("../../assets/SGRAYPIECES.bmp"),
                "SGRAYPIECES.bmp",
            ),
            b_up_down_1: must(
                include_bytes!("../../assets/B_UP_DOWN_1.bmp"),
                "B_UP_DOWN_1.bmp",
            ),
            lrswitch: must(include_bytes!("../../assets/LRSWITCH.bmp"), "LRSWITCH.bmp"),
            bhotkeys: must(include_bytes!("../../assets/BHOTKEYS.bmp"), "BHOTKEYS.bmp"),
            bpacker: must(include_bytes!("../../assets/BPACKER.bmp"), "BPACKER.bmp"),
            b_lang: must(include_bytes!("../../assets/B_LANG.bmp"), "B_LANG.bmp"),
            b_browse: must(include_bytes!("../../assets/B_BROWSE.bmp"), "B_BROWSE.bmp"),
            b_more: must(include_bytes!("../../assets/B_MORE.bmp"), "B_MORE.bmp"),
            b_default: must(include_bytes!("../../assets/B_DEFAULT.bmp"), "B_DEFAULT.bmp"),
            mb_iconquestion: must(
                include_bytes!("../../assets/MB_ICONQUESTION.bmp"),
                "MB_ICONQUESTION.bmp",
            ),

            hotspots: parse_hotspots_json(HOTSPOTS_JSON),
        }
    }

    pub fn block(&self, bm_key: &str) -> Option<&BmHotspotBlock> {
        self.hotspots.get(bm_key)
    }

    /// Gibt das originale `28x12`-Dateityp-Badge (`MOD`, `XM`, `S3M`, `MTM`, etc.) für eine Dateiendung zurück.
    pub fn filetype_badge(&self, ext: &str) -> &BmpImage {
        match ext.to_ascii_lowercase().as_str() {
            "xm" => &self.ft_xm,
            "s3m" => &self.ft_s3m,
            "mtm" => &self.ft_mtm,
            "669" => &self.ft_669,
            "stm" => &self.ft_stm,
            "far" => &self.ft_far,
            "okt" => &self.ft_okt,
            "wow" => &self.ft_wow,
            "nst" => &self.ft_nst,
            "ult" => &self.ft_ult,
            "dmf" => &self.ft_dmf,
            "it" | "umx" => &self.ft_it,
            _ => &self.ft_mod,
        }
    }
}

/// Kompakter Parser für `hotspots.json` ohne externe JSON-Crate-Abhängigkeiten.
pub fn parse_hotspots_json(json: &str) -> HashMap<String, BmHotspotBlock> {
    let mut map = HashMap::new();
    for bm_idx in 0..=15 {
        let key = format!("bm{}", bm_idx);
        let pat = format!("\"{}\":", key);
        let Some(start_pos) = json.find(&pat) else {
            continue;
        };
        let rest = &json[start_pos + pat.len()..];
        let next_bm = (0..=15)
            .filter(|&i| i != bm_idx)
            .filter_map(|i| rest.find(&format!("\"bm{}\":", i)))
            .min()
            .unwrap_or(rest.len());
        let block_str = &rest[..next_bm];

        let width = extract_int_field(block_str, "\"width\":").unwrap_or(0) as u32;
        let height = extract_int_field(block_str, "\"height\":").unwrap_or(0) as u32;

        let mut hotspots = Vec::new();
        if let Some(hs_pos) = block_str.find("\"hotspots\":") {
            let hs_slice = &block_str[hs_pos..];
            for item in hs_slice.split('{').skip(1) {
                let Some(end_brace) = item.find('}') else {
                    continue;
                };
                let obj = &item[..end_brace];
                let name = extract_str_field(obj, "\"name\":").unwrap_or_default();
                let id = extract_str_field(obj, "\"id\":").unwrap_or_default();
                let x = extract_int_field(obj, "\"x\":").unwrap_or(0);
                let y = extract_int_field(obj, "\"y\":").unwrap_or(0);
                let w = extract_int_field(obj, "\"width\":").unwrap_or(0) as u32;
                let h = extract_int_field(obj, "\"height\":").unwrap_or(0) as u32;
                if !id.is_empty() {
                    hotspots.push(HotspotRect {
                        name,
                        id,
                        x,
                        y,
                        width: w,
                        height: h,
                    });
                }
            }
        }

        map.insert(
            key,
            BmHotspotBlock {
                width,
                height,
                hotspots,
            },
        );
    }
    map
}

fn extract_int_field(s: &str, key: &str) -> Option<i32> {
    let idx = s.find(key)?;
    let after = s[idx + key.len()..].trim_start();
    let num_str: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num_str.parse().ok()
}

fn extract_str_field(s: &str, key: &str) -> Option<String> {
    let idx = s.find(key)?;
    let after = s[idx + key.len()..].trim_start();
    let after_quote = after.strip_prefix('"')?;
    let end_quote = after_quote.find('"')?;
    Some(after_quote[..end_quote].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_assets_and_hotspots() {
        let assets = EmbeddedAssets::load();
        assert_eq!(assets.bm0_main.width, 542);
        assert_eq!(assets.bm0_main.height, 135);
        assert_eq!(assets.bm1_mini.width, 309);
        assert_eq!(assets.bm1_mini.height, 58);
        assert_eq!(assets.hotspots.len(), 16);

        let bm0 = assets.block("bm0").unwrap();
        assert_eq!(bm0.hotspots.len(), 43);
        let mod_time = bm0.find_by_id("Main_ModTime").unwrap();
        assert_eq!((mod_time.x, mod_time.y, mod_time.width, mod_time.height), (10, 59, 77, 36));
    }
}

