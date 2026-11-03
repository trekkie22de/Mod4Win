//! Integriertes HTML-Hilfesystem (`help/help_de.html` & `help/help_en.html`) für MOD4WIN 2.40.
//!
//! Unterstützt:
//! - Laden externer HTML-Hilfedateien (`help/help_de.html`, `help/help_en.html`) von der Festplatte
//!   mit eingebettetem Fallback im Binary
//! - Leichtgewichtiger HTML-Subset-Renderer (`<h1>`-`<h3>`, `<p>`, `<b>`, `<ul>`/`<li>`, `<pre>`,
//!   `<a href="#topic">` und `<img src="...">` mit anklickbaren Hotspot-Overlays `bm0`..`bm15`)
//! - Direkter „Browser“-Button zum Öffnen der HTML-Hilfe im System-Webbrowser
//! - Garantiert 0 `#$K+`-Steuerzeichen-Artefakte.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::archive_mol::inflate_raw_deflate;
use super::assets::{EmbeddedAssets, HLP_BM_RAW_BYTES};
use super::bmp::BmpImage;
use super::font::{
    draw_3d_panel, draw_frame_3d_1px, draw_oem_text, draw_rect_outline, fill_rect,
    modern_ui_glyph_6x10,
};
use super::i18n::LocaleCatalog;

/// Per `build.rs` (RFC-1951 Deflate) komprimiert eingebettete HTML-Hilfedateien für alle 7 Sprachen.
const EMBEDDED_HELP_DE_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_de.deflate"));
const EMBEDDED_HELP_EN_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_en.deflate"));
const EMBEDDED_HELP_FR_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_fr.deflate"));
const EMBEDDED_HELP_ES_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_es.deflate"));
const EMBEDDED_HELP_IT_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_it.deflate"));
const EMBEDDED_HELP_NL_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_nl.deflate"));
const EMBEDDED_HELP_GRK_DEFLATE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/help_grk.deflate"));

/// Eingebettete Original-Hilfequelle Deutsch (`MODHELP.RTF`, nur für Tests/Initial-Generierung).
#[cfg(test)]
pub const MODHELP_GER_RTF: &str = include_str!("../../assets/hlp_extracted/modhelp_ger.rtf");

/// Eingebettete Original-Hilfequelle Englisch (`MODUSHLP.RTF`, nur für Tests/Initial-Generierung).
#[cfg(test)]
pub const MODHELP_USA_RTF: &str = include_str!("../../assets/hlp_extracted/modhelp_usa.rtf");

/// Ein klickbarer Hyperlink im Hilfetext (`<a href="#target_id" data-popup="true|false">`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpLink {
    pub target_id: String,
    /// `true` = grünes Popup-Fenster, `false` = Sprung zur Hauptseite.
    pub is_popup: bool,
}

/// Ein formatierter Textabschnitt innerhalb eines Absatzes (`<b>`, `<a href="...">` oder Normaltext).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpSpan {
    pub text: String,
    pub bold: bool,
    pub link: Option<HelpLink>,
}

/// Ein Block innerhalb eines HTML-Hilfethemas (`<h1>`-`<h3>`, `<p>`, `<ul>/<li>`, `<pre>`, `<img src="bm0..bm15">`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelpBlock {
    Heading(String),
    Paragraph {
        indent: i32,
        bullet: bool,
        spans: Vec<HelpSpan>,
    },
    Preformatted(Vec<String>),
    TableRow(Vec<Vec<HelpSpan>>),
    Bitmap {
        bm_key: String,
    },
}

/// Ein einzelnes Hilfethema (`<section id="...">`).
#[derive(Clone, Debug)]
pub struct HelpTopic {
    /// Primäre Context-ID (z. B. `"main_index"`, `"HauptDlg"`, `"Main_Play"`, `"setup_IDO"`).
    pub id: String,
    /// Alle Context-IDs dieses Themas.
    pub all_ids: Vec<String>,
    /// Seitentitel. Wenn `Some`, ist es eine Hauptseite; wenn `None`, ein Popup-Thema!
    pub title: Option<String>,
    /// Sortierschlüssel für die `<<` / `>>` Browse-Buttons.
    pub browse_seq: Option<String>,
    /// Inhalt des Themas (`<h1>`-`<h3>`, `<p>`, `<ul>`/`<li>`, `<pre>`, `<img src="bm0..bm15">`).
    pub blocks: Vec<HelpBlock>,
}

impl HelpTopic {
    /// Gibt zurück, ob dieses Thema als eigenständige Hauptseite (statt als grünes Popup) dargestellt wird.
    pub fn is_full_page(&self) -> bool {
        if self.title.is_some() {
            return true;
        }
        self.blocks
            .iter()
            .any(|b| matches!(b, HelpBlock::Bitmap { .. }))
    }

    /// Anzeigename für die Titelleiste.
    pub fn display_title(&self) -> String {
        if let Some(t) = &self.title
            && !t.trim().is_empty() {
                return t.trim().to_string();
            }
        for b in &self.blocks {
            if let HelpBlock::Heading(h) = b
                && !h.trim().is_empty() {
                    return h.trim().to_string();
                }
        }
        self.id.clone()
    }
}

/// Vollständiges geparstes HTML-/WinHelp-Handbuch mit Index-, Browse- und Kontext-Suche.
#[derive(Clone, Debug)]
pub struct WinHelpBook {
    pub language_id: u8,
    pub topics: Vec<HelpTopic>,
    pub id_to_idx: HashMap<String, usize>,
    pub browse_order: Vec<usize>,
}

impl WinHelpBook {
    /// Lädt das deutsche Hilfesystem (bevorzugt `help/help_de.html` von der Festplatte, sonst Fallback).
    pub fn load_embedded() -> Self {
        Self::load_for_language(1)
    }

    /// Liefert den Sprachcode (`"de"`, `"en"`, `"fr"`, `"es"`, `"it"`, `"nl"`, `"grk"`) zur numerischen `language_id`.
    pub fn lang_code_for_id(lang_id: u8) -> &'static str {
        match lang_id {
            1 => "de",
            2 => "fr",
            3 => "es",
            4 => "it",
            5 => "nl",
            6 => "grk",
            _ => "en",
        }
    }

    /// Lädt das Hilfesystem für die angegebene Sprache (`0 = English`, `1 = Deutsch`, `2 = Français`, `3 = Español`, `4 = Italiano`, `5 = Nederlands`, `6 = Ελληνικά`).
    /// Vorrangig wird `help/help_<code>.html` von der Festplatte gelesen;
    /// fehlt der `help/`-Ordner (z. B. bei Weitergabe als einzelne `.exe`), wird das per RFC-1951 Deflate
    /// komprimiert in der `.exe` eingebettete HTML direkt im RAM entpackt.
    pub fn load_for_language(lang_id: u8) -> Self {
        let lang_code = Self::lang_code_for_id(lang_id);
        Self::load_for_lang_code(lang_id, lang_code)
    }

    /// Lädt das Hilfesystem anhand eines Sprachcodes (`"de"`, `"en"`, `"fr"`, `"es"`, `"it"`, `"nl"`, `"grk"`).
    pub fn load_for_lang_code(lang_id: u8, lang_code: &str) -> Self {
        let code = lang_code.trim().to_ascii_lowercase();
        let is_ger = code == "de";
        let html_filename = format!("help_{}.html", code);

        let mut topics = if let Some(html_path) = Self::find_help_html_on_disk(&html_filename) {
            if let Ok(html_content) = fs::read_to_string(&html_path) {
                let parsed = parse_help_html(&html_content);
                if !parsed.is_empty() {
                    parsed
                } else {
                    Self::load_topics_from_embedded_deflate(&code)
                }
            } else {
                Self::load_topics_from_embedded_deflate(&code)
            }
        } else {
            Self::load_topics_from_embedded_deflate(&code)
        };

        Self::ensure_supplemental_topics(&mut topics, is_ger);
        Self::from_topics(lang_id, topics)
    }

    /// Entpackt das per `build.rs` komprimiert im Binary eingebettete HTML (`help_<code>.deflate`) im Speicher.
    pub fn decompress_embedded_help_html(lang_code: &str) -> Option<String> {
        let blob: &[u8] = match lang_code.trim().to_ascii_lowercase().as_str() {
            "de" => EMBEDDED_HELP_DE_DEFLATE,
            "en" => EMBEDDED_HELP_EN_DEFLATE,
            "fr" => EMBEDDED_HELP_FR_DEFLATE,
            "es" => EMBEDDED_HELP_ES_DEFLATE,
            "it" => EMBEDDED_HELP_IT_DEFLATE,
            "nl" => EMBEDDED_HELP_NL_DEFLATE,
            "grk" => EMBEDDED_HELP_GRK_DEFLATE,
            _ => EMBEDDED_HELP_EN_DEFLATE,
        };
        if blob.len() <= 4 {
            return None;
        }
        let uncomp_len = u32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize;
        let raw_bytes = inflate_raw_deflate(&blob[4..], uncomp_len)?;
        String::from_utf8(raw_bytes).ok()
    }

    fn load_topics_from_embedded_deflate(lang_code: &str) -> Vec<HelpTopic> {
        if let Some(html) = Self::decompress_embedded_help_html(lang_code) {
            let parsed = parse_help_html(&html);
            if !parsed.is_empty() {
                return parsed;
            }
        }
        if lang_code != "de"
            && let Some(html) = Self::decompress_embedded_help_html("de") {
                return parse_help_html(&html);
            }
        Vec::new()
    }

    /// Baut die bereinigten Hilfethemen aus den Original-RTF-Quellen auf (nur für Tests/Initial-Generierung).
    #[cfg(test)]
    pub fn build_clean_topics_from_rtf(is_ger: bool) -> Vec<HelpTopic> {
        if is_ger {
            let mut topics = parse_modhelp_rtf(MODHELP_GER_RTF);
            Self::ensure_supplemental_topics(&mut topics, true);
            topics
        } else {
            let mut usa_topics = parse_modhelp_rtf(MODHELP_USA_RTF);
            let ger_topics = parse_modhelp_rtf(MODHELP_GER_RTF);
            // Falls einzelne Hotspot-IDs nur im deutschen RTF existierten, übernehme deren Struktur als Fallback
            let mut known_ids = std::collections::HashSet::new();
            for t in &usa_topics {
                for id in &t.all_ids {
                    known_ids.insert(id.to_ascii_lowercase());
                }
            }
            for gt in ger_topics {
                if !gt
                    .all_ids
                    .iter()
                    .any(|id| known_ids.contains(&id.to_ascii_lowercase()))
                {
                    for id in &gt.all_ids {
                        known_ids.insert(id.to_ascii_lowercase());
                    }
                    usa_topics.push(gt);
                }
            }
            Self::ensure_supplemental_topics(&mut usa_topics, false);
            usa_topics
        }
    }

    fn ensure_supplemental_topics(topics: &mut Vec<HelpTopic>, is_ger: bool) {
        let has_setup_fpu = topics.iter().any(|t| {
            t.all_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case("setup_fpu"))
        });
        if !has_setup_fpu {
            topics.push(HelpTopic {
                id: "setup_fpu".to_string(),
                all_ids: vec!["setup_fpu".to_string(), "main_fpu".to_string()],
                title: None,
                browse_seq: None,
                blocks: vec![
                    HelpBlock::Heading(if is_ger {
                        "IDO-FPU (Kubisches FPU-Oversampling)".to_string()
                    } else {
                        "IDO-FPU (Cubic FPU Oversampling)".to_string()
                    }),
                    HelpBlock::Paragraph {
                        indent: 0,
                        bullet: false,
                        spans: vec![HelpSpan {
                            text: if is_ger {
                                "In MOD4WIN 3.0 Beta berechnet die 4. IDO-Stufe (IDO-FPU, OverSamp = 3) die kubische 4-Punkt-Spline-Interpolation direkt ueber den mathematischen Coprozessor (80x87 FPU) bzw. die FPU-Spline-Tabelle.".to_string()
                            } else {
                                "In MOD4WIN 3.0 Beta, the 4th IDO stage (IDO-FPU, OverSamp = 3) computes 4-point cubic spline interpolation using the math coprocessor (80x87 FPU) spline table.".to_string()
                            },
                            bold: false,
                            link: None,
                        }],
                    },
                ],
            });
        }

        // Ergänze die Neuerungen von MOD4WIN 3.0 Beta in `main_index`, `Features` und `Revisions`
        for t in topics.iter_mut() {
            if t.id.eq_ignore_ascii_case("main_index") {
                let already_has_v30 = t.blocks.iter().any(|b| match b {
                    HelpBlock::Heading(h) => h.contains("3.0"),
                    HelpBlock::Paragraph { spans, .. } => {
                        spans.iter().any(|s| s.text.contains("3.0"))
                    }
                    _ => false,
                });
                if !already_has_v30 {
                    t.blocks.extend(Self::build_v30_new_features_blocks(is_ger));
                }
            } else if t.id.eq_ignore_ascii_case("Features") {
                let already_has_v30 = t.blocks.iter().any(|b| match b {
                    HelpBlock::Heading(h) => h.contains("3.0"),
                    HelpBlock::Paragraph { spans, .. } => {
                        spans.iter().any(|s| s.text.contains("3.0"))
                    }
                    _ => false,
                });
                if !already_has_v30 {
                    t.blocks.extend(Self::build_v30_new_features_blocks(is_ger));
                }
            } else if t.id.eq_ignore_ascii_case("Revisions") {
                let already_has_v30 = t.blocks.iter().any(|b| match b {
                    HelpBlock::Paragraph { spans, .. } => {
                        spans.iter().any(|s| s.text.contains("Version 3.0"))
                    }
                    _ => false,
                });
                if !already_has_v30 {
                    t.blocks.push(HelpBlock::Paragraph {
                        indent: 0,
                        bullet: false,
                        spans: vec![HelpSpan {
                            text: if is_ger {
                                "Version 3.00 Beta       2026  Native 64-Bit-Edition".to_string()
                            } else {
                                "Version 3.00 Beta       2026  Native 64-Bit Edition".to_string()
                            },
                            bold: true,
                            link: None,
                        }],
                    });
                    for item in if is_ger {
                        [
                            "Komplette native 64-Bit-Neuimplementierung (Windows 10/11 & Linux) ohne 16-Bit-DLLs",
                            "Neue Modulformate: Impulse Tracker (*.IT inkl. Resonanzfiltern & NNAs), Unreal Music (*.UMX) und UltraTracker (*.ULT)",
                            "Direkte Archiv-Wiedergabe fuer *.ZIP, *.LHA/*.LZH und *.ARJ Archive (in-memory) sowie *.MOL-Listen",
                            "4-stufiges IDO inkl. IDO-FPU (kubisches 4-Punkt-Spline-Oversampling) sowie 3 native IT-Filterstufen",
                            "Umschaltbarer Classic-2.40-Skin und neuer Modern-Skin sowie frei skalierbare Fenster",
                            "Integriertes 7-sprachiges HTML 3.2 Hilfesystem im Windows 3.1 WINHELP.EXE-Design",
                        ]
                    } else {
                        [
                            "Complete native 64-bit reimplementation (Windows 10/11 & Linux) without 16-bit DLLs",
                            "New module formats: Impulse Tracker (*.IT with resonant filters & NNAs), Unreal Music (*.UMX), and UltraTracker (*.ULT)",
                            "Direct in-memory archive playback for *.ZIP, *.LHA/*.LZH, and *.ARJ archives as well as *.MOL lists",
                            "4-stage IDO including IDO-FPU (cubic 4-point spline oversampling) plus 3 native IT filter stages",
                            "Switchable Classic 2.40 skin and new Modern skin plus freely scalable windows",
                            "Integrated 7-language HTML 3.2 Help System in Windows 3.1 WINHELP.EXE design",
                        ]
                    } {
                        t.blocks.push(HelpBlock::Paragraph {
                            indent: 0,
                            bullet: true,
                            spans: vec![HelpSpan {
                                text: item.to_string(),
                                bold: false,
                                link: None,
                            }],
                        });
                    }
                }
            }
        }
    }

    fn build_v30_new_features_blocks(is_ger: bool) -> Vec<HelpBlock> {
        let heading = if is_ger {
            "NEU IN MOD4WIN 3.0 BETA:"
        } else {
            "NEW IN MOD4WIN 3.0 BETA:"
        };
        #[allow(clippy::type_complexity)]
        let bullets: [(&str, &str, Option<(&str, bool)>); 6] = if is_ger {
            [
                (
                    "Native 64-Bit-Architektur: ",
                    "Laeuft nativ unter Windows 10/11 & Linux ganz ohne 16-Bit-DLLs (PLAYER32.DLL).",
                    None,
                ),
                (
                    "Erweiterte Tracker-Formate: ",
                    "Impulse Tracker (*.IT inkl. Resonanzfiltern & NNAs), Unreal Music (*.UMX) und UltraTracker (*.ULT) bis 64 Kanaele.",
                    None,
                ),
                (
                    "Integrierte Archiv-Engine: ",
                    "Direktes Browsen und Abspielen aus *.ZIP-, *.LHA/*.LZH- und *.ARJ-Archiven im Speicher.",
                    Some(("OpenDlg", false)),
                ),
                (
                    "4-stufiges IDO inkl. IDO-FPU: ",
                    "Kubische 4-Punkt-Spline-Interpolation fuer kristallklare Hoehen.",
                    Some(("setup_fpu", true)),
                ),
                (
                    "Classic-2.40 & Modern Skin + HiDPI: ",
                    "Pixelgenaue Original-Oberflaeche umschaltbar auf den neuen Modern Skin sowie frei skalierbare Fenster.",
                    Some(("SetupDlg", false)),
                ),
                (
                    "HTML 3.2 WinHelp (7 Sprachen): ",
                    "Komprimiert in der .exe integriert und per Browser-Button direkt im Webbrowser oeffenbar.",
                    Some(("Revisions", false)),
                ),
            ]
        } else {
            [
                (
                    "Native 64-Bit Architecture: ",
                    "Runs natively on Windows 10/11 & Linux without any 16-bit DLLs (PLAYER32.DLL).",
                    None,
                ),
                (
                    "Extended Tracker Formats: ",
                    "Impulse Tracker (*.IT with resonant filters & NNAs), Unreal Music (*.UMX), and UltraTracker (*.ULT) up to 64 channels.",
                    None,
                ),
                (
                    "Integrated Archive Engine: ",
                    "Direct in-memory browsing and playback of *.ZIP, *.LHA/*.LZH, and *.ARJ archives.",
                    Some(("OpenDlg", false)),
                ),
                (
                    "4-Stage IDO incl. IDO-FPU: ",
                    "Cubic 4-point spline interpolation for crystal-clear high frequencies.",
                    Some(("setup_fpu", true)),
                ),
                (
                    "Classic 2.40 & Modern Skin + HiDPI: ",
                    "Pixel-accurate original UI switchable to the new Modern skin plus freely scalable windows.",
                    Some(("SetupDlg", false)),
                ),
                (
                    "HTML 3.2 WinHelp (7 Languages): ",
                    "Deflate-compressed inside the .exe and viewable in your system web browser via the Browser button.",
                    Some(("Revisions", false)),
                ),
            ]
        };

        let mut blocks = Vec::with_capacity(8);
        blocks.push(HelpBlock::Heading(heading.to_string()));
        for (lead, body, opt_link) in bullets {
            let mut spans = vec![HelpSpan {
                text: lead.to_string(),
                bold: true,
                link: opt_link.map(|(tid, pop)| HelpLink {
                    target_id: tid.to_string(),
                    is_popup: pop,
                }),
            }];
            spans.push(HelpSpan {
                text: body.to_string(),
                bold: false,
                link: None,
            });
            blocks.push(HelpBlock::Paragraph {
                indent: 0,
                bullet: true,
                spans,
            });
        }
        blocks
    }

    pub fn from_topics(language_id: u8, topics: Vec<HelpTopic>) -> Self {
        let mut id_to_idx = HashMap::new();
        for (idx, t) in topics.iter().enumerate() {
            for raw_id in &t.all_ids {
                id_to_idx.insert(raw_id.to_ascii_lowercase(), idx);
            }
        }

        let mut browse_pairs: Vec<(String, usize)> = topics
            .iter()
            .enumerate()
            .filter_map(|(idx, t)| {
                if t.is_full_page() {
                    let seq = t
                        .browse_seq
                        .clone()
                        .unwrap_or_else(|| format!("ZZZ_{:04}", idx));
                    Some((seq, idx))
                } else {
                    None
                }
            })
            .collect();
        browse_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let browse_order = browse_pairs.into_iter().map(|(_, idx)| idx).collect();

        Self {
            language_id,
            topics,
            id_to_idx,
            browse_order,
        }
    }

    /// Sucht `help/<filename>` in den Arbeits- und Programmverzeichnissen auf der Festplatte.
    pub fn find_help_html_on_disk(filename: &str) -> Option<PathBuf> {
        for dir in Self::candidate_help_dirs() {
            let p = dir.join(filename);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    pub fn candidate_help_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        // 1. Hauptverzeichnis `help/` im Workspace-Root bzw. aktuellen Verzeichnis hat höchste Priorität
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd.join("help"));
            if let Some(parent) = cwd.parent() {
                dirs.push(parent.join("help"));
            }
            dirs.push(cwd.join("mod4win-rs").join("help"));
        }
        if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            dirs.push(root.join("help"));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent() {
                dirs.push(exe_dir.join("help"));
                if let Some(parent) = exe_dir.parent() {
                    dirs.push(parent.join("help"));
                }
            }
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("help"));
        dirs
    }

    /// Sucht ein Thema anhand seiner Context-ID (Case-Insensitive).
    pub fn get_topic(&self, id: &str) -> Option<&HelpTopic> {
        let key = id.trim().to_ascii_lowercase();
        let &idx = self.id_to_idx.get(&key)?;
        self.topics.get(idx)
    }

    /// Gibt das nächste oder vorherige Hauptthema in der Browse-Sequenz (`<<` / `>>`) zurück.
    pub fn step_browse(&self, current_id: &str, delta: isize) -> Option<&HelpTopic> {
        let key = current_id.trim().to_ascii_lowercase();
        let cur_topic_idx = *self.id_to_idx.get(&key)?;
        let pos = self
            .browse_order
            .iter()
            .position(|&i| i == cur_topic_idx)
            .unwrap_or(0);
        let new_pos =
            (pos as isize + delta).clamp(0, self.browse_order.len() as isize - 1) as usize;
        let target_idx = *self.browse_order.get(new_pos)?;
        self.topics.get(target_idx)
    }
}

// =========================================================================
// HTML-Generator & Leichtgewichtiger HTML-Subset-Parser (`<h1>`-`<h3>`, `<p>`, `<b>`, `<ul>`/`<li>`, `<pre>`, `<a>`, `<img>`)
// =========================================================================

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Erzeugt ein eigenständiges, reines HTML 3.2-Dokument im klassischen Windows 3.1 WinHelp-Design
/// (`help_de.html` / `help_en.html`) ganz ohne CSS (`<style>`), ohne HTML5 und ohne JavaScript.
pub fn export_topics_to_html(topics: &[HelpTopic], is_ger: bool) -> String {
    let lang_code = if is_ger { "de" } else { "en" };
    let assets = EmbeddedAssets::load();
    export_topics_to_html_for_lang(topics, lang_code, &assets.hotspots)
}

/// Erzeugt ein reines HTML 3.2-Dokument im Windows 3.1 WinHelp-Design für den angegebenen Sprachcode
/// (`"de"`, `"en"`, `"fr"`, `"es"`, `"it"`, `"nl"`, `"grk"`).
pub fn export_topics_to_html_for_lang(
    topics: &[HelpTopic],
    lang_code: &str,
    hotspots: &HashMap<String, super::assets::BmHotspotBlock>,
) -> String {
    let (doc_title, lbl_contents, lbl_dialogs, lbl_keys, lbl_effects, lbl_faq, lbl_history, menu_bar) =
        match lang_code {
            "de" => (
                "MOD4WIN 3.00 - Hilfesystem (Deutsch)",
                "Inhalt",
                "Dialoge",
                "Tasten",
                "Effekte",
                "FAQ",
                "Historie",
                "&nbsp;<U>D</U>atei &nbsp;&nbsp;<U>B</U>earbeiten &nbsp;&nbsp;<U>L</U>esezeichen &nbsp;&nbsp;<U>H</U>ilfe",
            ),
            "fr" => (
                "MOD4WIN 3.00 - Aide (Francais)",
                "Contenu",
                "Dialogues",
                "Touches",
                "Effets",
                "FAQ",
                "Historique",
                "&nbsp;<U>F</U>ichier &nbsp;&nbsp;<U>E</U>dition &nbsp;&nbsp;<U>S</U>ignets &nbsp;&nbsp;<U>A</U>ide",
            ),
            "es" => (
                "MOD4WIN 3.00 - Ayuda (Espanol)",
                "Indice",
                "Dialogos",
                "Teclas",
                "Efectos",
                "FAQ",
                "Historia",
                "&nbsp;<U>A</U>rchivo &nbsp;&nbsp;<U>E</U>dicion &nbsp;&nbsp;<U>M</U>arcador &nbsp;&nbsp;A<U>y</U>uda",
            ),
            "grk" => (
                "MOD4WIN 3.00 - Help System (Greek)",
                "Contents",
                "Dialogs",
                "Hotkeys",
                "Effects",
                "FAQ",
                "History",
                "&nbsp;<U>F</U>ile &nbsp;&nbsp;<U>E</U>dit &nbsp;&nbsp;<U>B</U>ookmark &nbsp;&nbsp;<U>H</U>elp",
            ),
            _ => (
                "MOD4WIN 3.00 - Help System",
                "Contents",
                "Dialogs",
                "Hotkeys",
                "Effects",
                "FAQ",
                "History",
                "&nbsp;<U>F</U>ile &nbsp;&nbsp;<U>E</U>dit &nbsp;&nbsp;<U>B</U>ookmark &nbsp;&nbsp;<U>H</U>elp",
            ),
        };

    let img_prefix = if lang_code == "de" {
        "img".to_string()
    } else {
        format!("img/{}", lang_code)
    };

    // Berechne die Browse-Reihenfolge (`<<` und `>>`) für alle Vollseiten-Themen
    let mut browse_pairs: Vec<(String, usize)> = topics
        .iter()
        .enumerate()
        .filter_map(|(idx, t)| {
            if t.is_full_page() {
                let seq = t
                    .browse_seq
                    .clone()
                    .unwrap_or_else(|| format!("ZZZ_{:04}", idx));
                Some((seq, idx))
            } else {
                None
            }
        })
        .collect();
    browse_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let browse_order: Vec<usize> = browse_pairs.into_iter().map(|(_, idx)| idx).collect();
    let mut prev_next_map: HashMap<usize, (String, String)> = HashMap::new();
    for (pos, &topic_idx) in browse_order.iter().enumerate() {
        let prev_idx = browse_order[pos.saturating_sub(1)];
        let next_idx = browse_order[(pos + 1).min(browse_order.len().saturating_sub(1))];
        prev_next_map.insert(
            topic_idx,
            (
                topics[prev_idx].id.clone(),
                topics[next_idx].id.clone(),
            ),
        );
    }

    // Zuordnung von Hotspot-Ziel-IDs zu ihrem übergeordneten Dialog (für Rücksprung-Links in Popups)
    let bm_parent_dlg: [(&str, &str); 16] = [
        ("bm0", "HauptDlg"),
        ("bm1", "MiniState"),
        ("bm2", "FMDlg"),
        ("bm3", "QuickSelBox"),
        ("bm4", "SetupDlg"),
        ("bm5", "ArcOptDlg"),
        ("bm6", "ArcParm"),
        ("bm7", "KeySelDlg"),
        ("bm8", "FSelDlg"),
        ("bm9", "OpenDlg"),
        ("bm10", "InfoDlg"),
        ("bm11", "InstrWindow"),
        ("bm12", "EffWnd"),
        ("bm13", "Effects"),
        ("bm14", "RegisterDlg"),
        ("bm15", "RecordingDlg"),
    ];
    let mut popup_parent_map: HashMap<String, String> = HashMap::new();
    for (bm_key, dlg_id) in bm_parent_dlg {
        if let Some(block) = hotspots.get(bm_key) {
            for hs in &block.hotspots {
                popup_parent_map
                    .entry(hs.id.to_ascii_lowercase())
                    .or_insert_with(|| dlg_id.to_string());
            }
        }
    }

    let mut html = String::with_capacity(240_000);
    html.push_str("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2 Final//EN\">\n");
    html.push_str("<HTML>\n<HEAD>\n");
    html.push_str("  <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=utf-8\">\n");
    html.push_str(&format!("  <TITLE>{}</TITLE>\n", html_escape(doc_title)));
    html.push_str("</HEAD>\n");
    html.push_str("<BODY BGCOLOR=\"#C0C0C0\" TEXT=\"#000000\" LINK=\"#008000\" VLINK=\"#008000\" ALINK=\"#008000\">\n");
    html.push_str("<BASEFONT FACE=\"Times New Roman, Times, Georgia, serif\" SIZE=\"3\">\n\n");

    for (idx, t) in topics.iter().enumerate() {
        let all_ids_str = t.all_ids.join(",");
        let title_attr = t.title.as_deref().unwrap_or("");
        let browse_attr = t.browse_seq.as_deref().unwrap_or("");
        let is_full = t.is_full_page();
        let popup_attr = if is_full { "false" } else { "true" };

        // Sprungmarken (<A NAME="...">) für die primäre ID und alle Alias-IDs (plus Kleinbuchstaben-Variante für Case-Sensitivity in Browsern)
        let mut emitted_anchors = std::collections::HashSet::new();
        for aid in std::iter::once(&t.id).chain(t.all_ids.iter()) {
            if emitted_anchors.insert(aid.clone()) {
                html.push_str(&format!(
                    "<A NAME=\"{}\" ID=\"{}\"></A>\n",
                    html_escape(aid),
                    html_escape(aid)
                ));
            }
            let low = aid.to_ascii_lowercase();
            if emitted_anchors.insert(low.clone()) {
                html.push_str(&format!(
                    "<A NAME=\"{}\" ID=\"{}\"></A>\n",
                    html_escape(&low),
                    html_escape(&low)
                ));
            }
        }

        if is_full {
            let win_title = format!("{} - {}", doc_title, t.display_title());
            let (prev_id, next_id) = prev_next_map
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| ("main_index".to_string(), "main_index".to_string()));

            html.push_str("<TABLE WIDTH=\"100%\" BORDER=\"2\" CELLPADDING=\"0\" CELLSPACING=\"0\" BGCOLOR=\"#C0C0C0\">\n");
            // Windows 3.1 Titelleiste (#000080 Dunkelblau mit Systemmenü-Strich links und Min/Max-Pfeilen rechts)
            html.push_str("  <TR BGCOLOR=\"#000080\">\n    <TD>\n");
            html.push_str("      <TABLE WIDTH=\"100%\" BORDER=\"1\" CELLPADDING=\"2\" CELLSPACING=\"0\" BGCOLOR=\"#000080\">\n        <TR>\n");
            html.push_str("          <TD WIDTH=\"18\" ALIGN=\"CENTER\" BGCOLOR=\"#C0C0C0\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"2\" COLOR=\"#000000\"><B>-</B></FONT></TD>\n");
            html.push_str(&format!(
                "          <TD ALIGN=\"CENTER\" BGCOLOR=\"#000080\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"2\" COLOR=\"#FFFFFF\"><B>{}</B></FONT></TD>\n",
                html_escape(&win_title)
            ));
            html.push_str("          <TD WIDTH=\"18\" ALIGN=\"CENTER\" BGCOLOR=\"#C0C0C0\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"1\" COLOR=\"#000000\"><B>&#9660;</B></FONT></TD>\n");
            html.push_str("          <TD WIDTH=\"18\" ALIGN=\"CENTER\" BGCOLOR=\"#C0C0C0\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"1\" COLOR=\"#000000\"><B>&#9650;</B></FONT></TD>\n");
            html.push_str("        </TR>\n      </TABLE>\n    </TD>\n  </TR>\n");
            // Windows 3.1 Menüleiste
            html.push_str(&format!(
                "  <TR BGCOLOR=\"#FFFFFF\">\n    <TD><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"1\" COLOR=\"#000000\">{}</FONT></TD>\n  </TR>\n",
                menu_bar
            ));
            // Windows 3.1 WinHelp Symbolleiste (#C0C0C0) mit 3D-Buttons
            html.push_str("  <TR BGCOLOR=\"#C0C0C0\">\n    <TD>\n");
            html.push_str("      <TABLE BORDER=\"1\" CELLPADDING=\"2\" CELLSPACING=\"2\" BGCOLOR=\"#C0C0C0\">\n        <TR>\n");
            for (btn_lbl, btn_href) in [
                (lbl_contents, "main_index"),
                (lbl_dialogs, "KnopfIndex"),
                ("&lt;&lt;", prev_id.as_str()),
                ("&gt;&gt;", next_id.as_str()),
                (lbl_keys, "KeyIndex"),
                (lbl_effects, "Effects"),
                (lbl_faq, "FAQ"),
                (lbl_history, "Revisions"),
            ] {
                html.push_str(&format!(
                    "          <TD BGCOLOR=\"#C0C0C0\" ALIGN=\"CENTER\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"1\">&nbsp;<A HREF=\"#{}\"><FONT COLOR=\"#000000\"><B>{}</B></FONT></A>&nbsp;</FONT></TD>\n",
                    html_escape(btn_href),
                    btn_lbl
                ));
            }
            html.push_str("        </TR>\n      </TABLE>\n    </TD>\n  </TR>\n");
            // Weißer WinHelp-Dokumentbereich (#FFFFFF) mit einmaliger Basis-Schriftdefinition
            html.push_str("  <TR BGCOLOR=\"#FFFFFF\">\n    <TD>\n");
            html.push_str("      <TABLE WIDTH=\"100%\" BORDER=\"0\" CELLPADDING=\"10\" CELLSPACING=\"0\" BGCOLOR=\"#FFFFFF\">\n        <TR>\n          <TD><FONT FACE=\"Times New Roman, Times, Georgia, serif\" SIZE=\"3\">\n");
        } else {
            // Grünes/Gelbliches Windows 3.1 Popup-Fenster (#FFFFF0) mit einmaliger Basis-Schriftdefinition
            html.push_str("<TABLE WIDTH=\"94%\" ALIGN=\"CENTER\" BORDER=\"1\" CELLPADDING=\"8\" CELLSPACING=\"0\" BGCOLOR=\"#FFFFF0\">\n  <TR>\n    <TD><FONT FACE=\"Times New Roman, Times, Georgia, serif\" SIZE=\"3\">\n");
        }

        html.push_str(&format!(
            "<!-- TOPIC id=\"{}\" ids=\"{}\" title=\"{}\" browse=\"{}\" popup=\"{}\" -->\n",
            html_escape(&t.id),
            html_escape(&all_ids_str),
            html_escape(title_attr),
            html_escape(browse_attr),
            popup_attr
        ));

        let mut in_ul = false;
        let mut in_dl = false;
        let mut in_table = false;
        let mut in_p = false;

        for (b_idx, b) in t.blocks.iter().enumerate() {
            let is_bullet = matches!(b, HelpBlock::Paragraph { bullet: true, .. });
            let is_indented = matches!(
                b,
                HelpBlock::Paragraph {
                    bullet: false,
                    indent,
                    ..
                } if *indent > 0
            );
            let is_normal_p = matches!(
                b,
                HelpBlock::Paragraph {
                    bullet: false,
                    indent: 0,
                    ..
                }
            );
            let is_trow = matches!(b, HelpBlock::TableRow(_));

            if !is_normal_p && in_p {
                html.push_str("\n    </P>\n");
                in_p = false;
            }
            if !is_bullet && in_ul {
                html.push_str("    </UL>\n");
                in_ul = false;
            }
            if !is_indented && in_dl {
                html.push_str("    </DL>\n");
                in_dl = false;
            }
            if !is_trow && in_table {
                html.push_str("    </TABLE>\n");
                in_table = false;
            }

            if is_bullet && !in_ul {
                html.push_str("    <UL TYPE=\"DISC\">\n");
                in_ul = true;
            }
            if is_indented && !in_dl {
                html.push_str("    <DL>\n");
                in_dl = true;
            }
            if is_trow && !in_table {
                html.push_str("    <TABLE BORDER=\"1\" CELLPADDING=\"3\" CELLSPACING=\"0\" BGCOLOR=\"#FFFFFF\">\n");
                in_table = true;
            }

            match b {
                HelpBlock::Heading(h) => {
                    html.push_str(&format!(
                        "    <H2 ALIGN=\"CENTER\"><FONT COLOR=\"#0000FF\">{}</FONT></H2>\n",
                        html_escape(h)
                    ));
                }
                HelpBlock::Bitmap { bm_key } => {
                    html.push_str(&format!(
                        "    <P ALIGN=\"CENTER\"><IMG SRC=\"{}/{}.bmp\" USEMAP=\"#map_{}\" BORDER=\"0\" ALT=\"{}\"></P>\n",
                        html_escape(&img_prefix),
                        html_escape(bm_key),
                        html_escape(bm_key),
                        html_escape(bm_key)
                    ));
                }
                HelpBlock::Preformatted(lines) => {
                    html.push_str("    <PRE><FONT FACE=\"Courier New, Courier\" SIZE=\"2\">");
                    for (l_idx, line) in lines.iter().enumerate() {
                        if l_idx > 0 {
                            html.push('\n');
                        }
                        html.push_str(&html_escape(line));
                    }
                    html.push_str("</FONT></PRE>\n");
                }
                HelpBlock::TableRow(cells) => {
                    html.push_str("      <TR VALIGN=\"TOP\">");
                    for cell_spans in cells {
                        html.push_str("<TD>");
                        if cell_spans.is_empty() {
                            html.push_str("&nbsp;");
                        } else {
                            append_inline_spans_html(&mut html, cell_spans);
                        }
                        html.push_str("</TD>");
                    }
                    html.push_str("</TR>\n");
                }
                HelpBlock::Paragraph {
                    indent,
                    bullet,
                    spans,
                } => {
                    if *bullet {
                        html.push_str("      <LI>");
                        append_inline_spans_html(&mut html, spans);
                        html.push_str("</LI>\n");
                    } else if *indent > 0 {
                        html.push_str("      <DD>");
                        append_inline_spans_html(&mut html, spans);
                        html.push_str("</DD>\n");
                    } else {
                        let next_is_normal_p = matches!(
                            t.blocks.get(b_idx + 1),
                            Some(HelpBlock::Paragraph {
                                bullet: false,
                                indent: 0,
                                ..
                            })
                        );
                        if !in_p {
                            if next_is_normal_p {
                                html.push_str("    <P>\n      ");
                                in_p = true;
                                append_inline_spans_html(&mut html, spans);
                            } else {
                                html.push_str("    <P>");
                                append_inline_spans_html(&mut html, spans);
                                html.push_str("</P>\n");
                            }
                        } else {
                            html.push_str("<BR>\n      ");
                            append_inline_spans_html(&mut html, spans);
                        }
                    }
                }
            }
        }
        if in_p {
            html.push_str("\n    </P>\n");
        }
        if in_ul {
            html.push_str("    </UL>\n");
        }
        if in_dl {
            html.push_str("    </DL>\n");
        }
        if in_table {
            html.push_str("    </TABLE>\n");
        }

        html.push_str("<!-- /TOPIC -->\n");

        if is_full {
            html.push_str("          </FONT></TD>\n        </TR>\n      </TABLE>\n    </TD>\n  </TR>\n</TABLE>\n<BR>\n");
        } else {
            let parent_dlg = popup_parent_map
                .get(&t.id.to_ascii_lowercase())
                .map(|s| s.as_str())
                .unwrap_or("main_index");
            html.push_str(&format!(
                "      <P ALIGN=\"RIGHT\"><FONT FACE=\"MS Sans Serif, Arial, Helvetica\" SIZE=\"1\"><A HREF=\"#{}\">[ ^ Dialog ]</A> &nbsp; <A HREF=\"#main_index\">[ ^ {} ]</A></FONT></P>\n",
                html_escape(parent_dlg),
                lbl_contents
            ));
            html.push_str("    </FONT></TD>\n  </TR>\n</TABLE>\n<BR>\n");
        }
    }

    // HTML 3.2 Client-Side Image-Maps (<MAP NAME="map_bm0">..15) für alle klickbaren Dialog-Hotspots
    for bm_idx in 0..=15 {
        let bm_key = format!("bm{}", bm_idx);
        if let Some(block) = hotspots.get(&bm_key) {
            html.push_str(&format!("<MAP NAME=\"map_{}\">\n", bm_key));
            for hs in &block.hotspots {
                let x1 = hs.x;
                let y1 = hs.y;
                let x2 = hs.x + hs.width as i32;
                let y2 = hs.y + hs.height as i32;
                html.push_str(&format!(
                    "  <AREA SHAPE=\"RECT\" COORDS=\"{},{},{},{}\" HREF=\"#{}\" ALT=\"{}\">\n",
                    x1,
                    y1,
                    x2,
                    y2,
                    html_escape(&hs.id),
                    html_escape(&hs.id)
                ));
            }
            html.push_str("</MAP>\n");
        }
    }

    html.push_str("\n</BODY>\n</HTML>\n");
    html
}

fn append_inline_spans_html(html: &mut String, spans: &[HelpSpan]) {
    for sp in spans {
        let escaped = html_escape(&sp.text);
        match (&sp.link, sp.bold) {
            (Some(lnk), true) => {
                let rel = if lnk.is_popup { " REL=\"popup\"" } else { "" };
                html.push_str(&format!(
                    "<A HREF=\"#{}\"{}><B>{}</B></A>",
                    html_escape(&lnk.target_id),
                    rel,
                    escaped
                ));
            }
            (Some(lnk), false) => {
                let rel = if lnk.is_popup { " REL=\"popup\"" } else { "" };
                html.push_str(&format!(
                    "<A HREF=\"#{}\"{}>{}</A>",
                    html_escape(&lnk.target_id),
                    rel,
                    escaped
                ));
            }
            (None, true) => {
                html.push_str(&format!("<B>{}</B>", escaped));
            }
            (None, false) => {
                html.push_str(&escaped);
            }
        }
    }
}

/// Zerlegt den Inhalt eines `<P>...</P>`, `<DD>...</DD>` oder `<LI>...</LI>` an `<BR>`, `<BR/>` oder `<BR />`
/// (case-insensitive) in einzelne Zeilen und normalisiert mehrzeilige Editor-Einrückungen innerhalb einer Zeile.
fn split_html_br_lines(inner: &str) -> Vec<String> {
    let mut raw_parts = Vec::new();
    let mut line_start = 0usize;
    let mut scan = 0usize;

    while let Some(lt_rel) = inner[scan..].find('<') {
        let tag_start = scan + lt_rel;
        let Some(gt_rel) = inner[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + gt_rel;
        let tag_inner = inner[tag_start + 1..tag_end].trim().to_ascii_lowercase();
        let is_br = tag_inner == "br"
            || tag_inner
                .strip_prefix("br")
                .map(|rest| {
                    let r = rest.trim_start();
                    r == "/" || r.starts_with('/') || rest.starts_with(char::is_whitespace)
                })
                .unwrap_or(false);
        if is_br {
            raw_parts.push(&inner[line_start..tag_start]);
            line_start = tag_end + 1;
        }
        scan = tag_end + 1;
    }
    raw_parts.push(&inner[line_start..]);

    let mut out = Vec::with_capacity(raw_parts.len());
    for part in raw_parts {
        let trimmed = part.trim_matches(|c: char| c == '\r' || c == '\n' || c == ' ' || c == '\t');
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains('\n') {
            let mut collapsed = String::with_capacity(trimmed.len());
            for (idx, seg) in trimmed.lines().enumerate() {
                let s = seg.trim_matches(|c: char| c == ' ' || c == '\t');
                if s.is_empty() {
                    continue;
                }
                if idx > 0 && !collapsed.is_empty() && !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
                collapsed.push_str(s);
            }
            if !collapsed.is_empty() {
                out.push(collapsed);
            }
        } else {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let tag_low = tag.to_ascii_lowercase();
    let pat = format!("{}=\"", attr_name.to_ascii_lowercase());
    let start = tag_low.find(&pat)? + pat.len();
    let end = tag[start..].find('"')? + start;
    Some(html_unescape(&tag[start..end]))
}

fn find_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle_lower)
}

/// Parst eine HTML-Hilfedatei (`help_*.html`) in `HelpTopic`-Strukturen.
/// Unterstützt sowohl reines HTML 3.2 (`<!-- TOPIC ... --> ... <!-- /TOPIC -->`) als auch `<section id="...">`.
pub fn parse_help_html(html: &str) -> Vec<HelpTopic> {
    let mut topics = Vec::new();
    let mut pos = 0usize;

    // 1. Reines HTML 3.2 Format (`<!-- TOPIC id="..." ... --> ... <!-- /TOPIC -->`)
    while let Some(rel) = html[pos..].find("<!-- TOPIC ") {
        let sec_start = pos + rel;
        let Some(tag_end_rel) = html[sec_start..].find("-->") else {
            break;
        };
        let tag_end = sec_start + tag_end_rel + 3;
        let open_tag = &html[sec_start..tag_end];

        let Some(end_sec_rel) = html[tag_end..].find("<!-- /TOPIC -->") else {
            break;
        };
        let sec_body_end = tag_end + end_sec_rel;
        let sec_body = &html[tag_end..sec_body_end];
        pos = sec_body_end + "<!-- /TOPIC -->".len();

        let id = extract_attr(open_tag, "id").unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let all_ids: Vec<String> = extract_attr(open_tag, "ids")
            .or_else(|| extract_attr(open_tag, "data-ids"))
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec![id.clone()]);
        let title = extract_attr(open_tag, "title")
            .or_else(|| extract_attr(open_tag, "data-title"))
            .filter(|t| !t.trim().is_empty());
        let browse_seq = extract_attr(open_tag, "browse")
            .or_else(|| extract_attr(open_tag, "data-browse"))
            .filter(|b| !b.trim().is_empty());

        let blocks = parse_html_section_blocks(sec_body);
        topics.push(HelpTopic {
            id,
            all_ids,
            title,
            browse_seq,
            blocks,
        });
    }

    if !topics.is_empty() {
        return topics;
    }

    // 2. Fallback für älteres `<section>`-Format
    pos = 0;
    while let Some(sec_rel) = find_ci(&html[pos..], "<section") {
        let sec_start = pos + sec_rel;
        let Some(tag_end_rel) = html[sec_start..].find('>') else {
            break;
        };
        let tag_end = sec_start + tag_end_rel;
        let open_tag = &html[sec_start..=tag_end];

        let Some(end_sec_rel) = find_ci(&html[tag_end + 1..], "</section>") else {
            break;
        };
        let sec_body_end = tag_end + 1 + end_sec_rel;
        let sec_body = &html[tag_end + 1..sec_body_end];
        pos = sec_body_end + "</section>".len();

        let id = extract_attr(open_tag, "id").unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let all_ids: Vec<String> = extract_attr(open_tag, "data-ids")
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec![id.clone()]);
        let title = extract_attr(open_tag, "data-title").filter(|t| !t.trim().is_empty());
        let browse_seq = extract_attr(open_tag, "data-browse").filter(|b| !b.trim().is_empty());

        let blocks = parse_html_section_blocks(sec_body);
        topics.push(HelpTopic {
            id,
            all_ids,
            title,
            browse_seq,
            blocks,
        });
    }

    topics
}

fn parse_html_section_blocks(body: &str) -> Vec<HelpBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let bytes = body.as_bytes();
    let mut ol_counter: Option<usize> = None;
    let mut dd_depth: i32 = 0;

    while i < bytes.len() {
        let Some(lt_rel) = body[i..].find('<') else {
            break;
        };
        let tag_start = i + lt_rel;
        let Some(gt_rel) = body[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + gt_rel;
        let tag_str = &body[tag_start..=tag_end];
        let tag_lower = tag_str.to_ascii_lowercase();

        if tag_lower.starts_with("<ol") {
            let start_num = extract_attr(tag_str, "start")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(1);
            ol_counter = Some(start_num);
            i = tag_end + 1;
            continue;
        } else if tag_lower.starts_with("</ol") || tag_lower.starts_with("<ul") {
            ol_counter = None;
            i = tag_end + 1;
            continue;
        } else if tag_lower.starts_with("</dd") {
            dd_depth = (dd_depth - 1).max(0);
            i = tag_end + 1;
            continue;
        } else if tag_lower == "<br>" || tag_lower == "<br/>" || tag_lower == "<br />" {
            blocks.push(HelpBlock::Paragraph {
                indent: 0,
                bullet: false,
                spans: vec![HelpSpan {
                    text: " ".to_string(),
                    bold: false,
                    link: None,
                }],
            });
            i = tag_end + 1;
            continue;
        } else if tag_lower.starts_with("<h1")
            || tag_lower.starts_with("<h2")
            || tag_lower.starts_with("<h3")
        {
            let close_tag = if tag_lower.starts_with("<h1") {
                "</h1>"
            } else if tag_lower.starts_with("<h2") {
                "</h2>"
            } else {
                "</h3>"
            };
            if let Some(end_rel) = find_ci(&body[tag_end + 1..], close_tag) {
                let inner = &body[tag_end + 1..tag_end + 1 + end_rel];
                let clean = sanitize_control_artifacts(&strip_html_tags(inner));
                if !clean.is_empty() {
                    blocks.push(HelpBlock::Heading(clean));
                }
                i = tag_end + 1 + end_rel + close_tag.len();
                continue;
            }
        } else if tag_lower.starts_with("<pre") {
            if let Some(end_rel) = find_ci(&body[tag_end + 1..], "</pre>") {
                let inner = strip_html_tags(&body[tag_end + 1..tag_end + 1 + end_rel]);
                let lines: Vec<String> = inner
                    .lines()
                    .map(sanitize_control_artifacts)
                    .collect();
                if !lines.is_empty() {
                    blocks.push(HelpBlock::Preformatted(lines));
                }
                i = tag_end + 1 + end_rel + "</pre>".len();
                continue;
            }
        } else if tag_lower.starts_with("<tr") {
            if let Some(end_rel) = find_ci(&body[tag_end + 1..], "</tr>") {
                let tr_inner = &body[tag_end + 1..tag_end + 1 + end_rel];
                let mut cells: Vec<Vec<HelpSpan>> = Vec::new();
                let mut td_pos = 0usize;
                while let Some(td_rel) = find_ci(&tr_inner[td_pos..], "<td") {
                    let td_start = td_pos + td_rel;
                    let Some(td_gt_rel) = tr_inner[td_start..].find('>') else {
                        break;
                    };
                    let td_content_start = td_start + td_gt_rel + 1;
                    let Some(td_end_rel) = find_ci(&tr_inner[td_content_start..], "</td>") else {
                        break;
                    };
                    let cell_html = &tr_inner[td_content_start..td_content_start + td_end_rel];
                    let cell_spans = parse_html_inline_spans(cell_html);
                    cells.push(cell_spans);
                    td_pos = td_content_start + td_end_rel + "</td>".len();
                }
                if cells
                    .iter()
                    .any(|c| c.iter().any(|sp| !sp.text.trim().is_empty()))
                {
                    blocks.push(HelpBlock::TableRow(cells));
                }
                i = tag_end + 1 + end_rel + "</tr>".len();
                continue;
            }
        } else if tag_lower.starts_with("<img") {
            let bm_key = extract_attr(tag_str, "data-bm")
                .or_else(|| extract_attr(tag_str, "alt").filter(|a| a.to_ascii_lowercase().starts_with("bm")))
                .or_else(|| {
                    extract_attr(tag_str, "src").and_then(|src| {
                        let stem = Path::new(&src)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&src);
                        let low = stem.to_ascii_lowercase();
                        if low.starts_with("bm") {
                            Some(low)
                        } else {
                            map_shg_to_bm_key(&low).map(|s| s.to_string())
                        }
                    })
                });
            if let Some(bm) = bm_key {
                blocks.push(HelpBlock::Bitmap { bm_key: bm });
            }
            i = tag_end + 1;
            continue;
        } else if tag_lower.starts_with("<p") || tag_lower.starts_with("<li") || tag_lower.starts_with("<dd") {
            let is_li = tag_lower.starts_with("<li");
            let is_dd = tag_lower.starts_with("<dd");
            let close_tag = if is_li {
                "</li>"
            } else if is_dd {
                "</dd>"
            } else {
                "</p>"
            };
            let indent = if is_dd {
                1
            } else {
                extract_attr(tag_str, "data-indent")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(dd_depth)
            };
            if let Some(end_rel) = find_ci(&body[tag_end + 1..], close_tag) {
                let inner = &body[tag_end + 1..tag_end + 1 + end_rel];
                if is_dd
                    && (find_ci(inner, "<ol").is_some()
                        || find_ci(inner, "<ul").is_some()
                        || find_ci(inner, "<table").is_some())
                {
                    dd_depth += 1;
                    i = tag_end + 1;
                    continue;
                }
                if let Some(img_rel) = find_ci(inner, "<img") {
                    let img_tag_start = img_rel;
                    if let Some(img_gt_rel) = inner[img_tag_start..].find('>') {
                        let img_tag = &inner[img_tag_start..=img_tag_start + img_gt_rel];
                        let bm_key = extract_attr(img_tag, "data-bm")
                            .or_else(|| {
                                extract_attr(img_tag, "alt")
                                    .filter(|a| a.to_ascii_lowercase().starts_with("bm"))
                            })
                            .or_else(|| {
                                extract_attr(img_tag, "src").and_then(|src| {
                                    let stem = Path::new(&src)
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or(&src);
                                    let low = stem.to_ascii_lowercase();
                                    if low.starts_with("bm") {
                                        Some(low)
                                    } else {
                                        map_shg_to_bm_key(&low).map(|s| s.to_string())
                                    }
                                })
                            });
                        if let Some(bm) = bm_key {
                            blocks.push(HelpBlock::Bitmap { bm_key: bm });
                        }
                    }
                }
                let current_ol_num = if is_li {
                    if let Some(n) = ol_counter {
                        ol_counter = Some(n + 1);
                        Some(n)
                    } else {
                        None
                    }
                } else {
                    None
                };
                for (line_idx, line_html) in split_html_br_lines(inner).into_iter().enumerate() {
                    let mut spans = parse_html_inline_spans(&line_html);
                    if spans.iter().any(|sp| !sp.text.trim().is_empty()) {
                        let (eff_indent, eff_bullet) = if let Some(n) = current_ol_num {
                            if line_idx == 0 {
                                let num_prefix = format!("{}.", n);
                                let already_prefixed = spans
                                    .first()
                                    .map(|s| s.text.trim_start().starts_with(&num_prefix))
                                    .unwrap_or(false);
                                if !already_prefixed {
                                    spans.insert(
                                        0,
                                        HelpSpan {
                                            text: format!("{}. ", n),
                                            bold: false,
                                            link: None,
                                        },
                                    );
                                }
                            }
                            (indent.max(1), false)
                        } else {
                            (indent, is_li)
                        };
                        blocks.push(HelpBlock::Paragraph {
                            indent: eff_indent,
                            bullet: eff_bullet,
                            spans,
                        });
                    }
                }
                i = tag_end + 1 + end_rel + close_tag.len();
                continue;
            }
        }

        i = tag_end + 1;
    }

    blocks
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    html_unescape(&out)
}

fn parse_html_inline_spans(inner: &str) -> Vec<HelpSpan> {
    let mut spans: Vec<HelpSpan> = Vec::new();
    let mut bold = false;
    let mut cur_link: Option<HelpLink> = None;
    let mut cur_text = String::new();
    let mut i = 0usize;

    let flush = |cur_text: &mut String,
                 spans: &mut Vec<HelpSpan>,
                 bold: bool,
                 cur_link: &Option<HelpLink>| {
        if !cur_text.is_empty() {
            let clean = sanitize_control_artifacts(&html_unescape(cur_text));
            cur_text.clear();
            if !clean.is_empty() {
                spans.push(HelpSpan {
                    text: clean,
                    bold,
                    link: cur_link.clone(),
                });
            }
        }
    };

    while i < inner.len() {
        if let Some(lt_rel) = inner[i..].find('<') {
            cur_text.push_str(&inner[i..i + lt_rel]);
            let tag_start = i + lt_rel;
            let Some(gt_rel) = inner[tag_start..].find('>') else {
                break;
            };
            let tag_end = tag_start + gt_rel;
            let tag_str = &inner[tag_start..=tag_end];
            let tag_lower = tag_str.to_ascii_lowercase();

            if tag_lower.starts_with("<img") {
                // Falls innerhalb eines <P> ein <IMG> liegt, wird es bereits auf Block-Ebene verarbeitet
                i = tag_end + 1;
                continue;
            }

            flush(&mut cur_text, &mut spans, bold, &cur_link);

            if tag_lower == "<b>" || tag_lower == "<strong>" {
                bold = true;
            } else if tag_lower == "</b>" || tag_lower == "</strong>" {
                bold = false;
            } else if tag_lower.starts_with("<a ") || tag_lower == "<a>" {
                if let Some(href) = extract_attr(tag_str, "href") {
                    let target_id = href.strip_prefix('#').unwrap_or(&href).to_string();
                    let is_popup = extract_attr(tag_str, "rel")
                        .map(|v| v.eq_ignore_ascii_case("popup"))
                        .or_else(|| extract_attr(tag_str, "data-popup").map(|v| v == "true"))
                        .unwrap_or(false);
                    if !target_id.is_empty() {
                        cur_link = Some(HelpLink {
                            target_id,
                            is_popup,
                        });
                    }
                }
            } else if tag_lower == "</a>" {
                cur_link = None;
            }
            i = tag_end + 1;
        } else {
            cur_text.push_str(&inner[i..]);
            break;
        }
    }
    flush(&mut cur_text, &mut spans, bold, &cur_link);
    spans
}

/// Entfernt zuverlässig jegliche WinHelp-Steuerzeichen-Reste wie `#$K+`, `#$+`, `#K`, `#$` am Zeilenanfang.
pub fn sanitize_control_artifacts(s: &str) -> String {
    let trimmed_start = s.trim_start();
    // Prüfe, ob am Anfang eine Kombination aus `#`, `$`, `K`, `+` steht (z. B. `#$K+ `, `#$+`, `# `)
    let mut prefix_bytes = 0usize;
    let mut saw_hash_or_dollar = false;
    for ch in trimmed_start.chars() {
        if ch == '#' || ch == '$' || ch == '+' || (saw_hash_or_dollar && ch == 'K') {
            if ch == '#' || ch == '$' {
                saw_hash_or_dollar = true;
            }
            prefix_bytes += ch.len_utf8();
        } else {
            break;
        }
    }
    if saw_hash_or_dollar && prefix_bytes > 0 {
        let rest = &trimmed_start[prefix_bytes..];
        return rest.trim_start().to_string();
    }
    s.to_string()
}

// =========================================================================
// Bereinigter WinHelp-RTF-Konverter (erzeugt garantiert 0 `#$K+`-Artefakte)
// =========================================================================

fn map_shg_to_bm_key(shg_name: &str) -> Option<&'static str> {
    let lower = shg_name.trim().to_ascii_lowercase();
    if lower.starts_with("main.") {
        Some("bm0")
    } else if lower.starts_with("ministat.") {
        Some("bm1")
    } else if lower.starts_with("fmdlg.") || lower.starts_with("fcopy") {
        Some("bm2")
    } else if lower.starts_with("selbox.") {
        Some("bm3")
    } else if lower.starts_with("setup_de.") || lower.starts_with("setup_") || lower.starts_with("setup.") {
        Some("bm4")
    } else if lower.starts_with("setup2.") || lower.starts_with("arcsetup.") || lower.starts_with("arcsu_") {
        Some("bm5")
    } else if lower.starts_with("arcparm.") || lower.starts_with("arcparam.") || lower.starts_with("arcpm_") {
        Some("bm6")
    } else if lower.starts_with("keysel.") || lower.starts_with("hotky_") {
        Some("bm7")
    } else if lower.starts_with("fsel.") || lower.starts_with("browser.") || lower.starts_with("brow") {
        Some("bm8")
    } else if lower.starts_with("open.") || lower.starts_with("open_") {
        Some("bm9")
    } else if lower.starts_with("info.") || lower.starts_with("info_") {
        Some("bm10")
    } else if lower.starts_with("ins.") || lower.starts_with("instrum.") || lower.starts_with("inst_") {
        Some("bm11")
    } else if lower.starts_with("effwnd.") {
        Some("bm12")
    } else if lower.starts_with("effects.") {
        Some("bm13")
    } else if lower.starts_with("regis_de.") || lower.starts_with("regis_") || lower.starts_with("register.") {
        Some("bm14")
    } else if lower.starts_with("recor_de.") || lower.starts_with("recor_") || lower.starts_with("record.") {
        Some("bm15")
    } else {
        None
    }
}

fn cp1252_to_char(b: u8) -> char {
    match b {
        0x84 | 0x93 | 0x94 => '"',
        0x91 | 0x92 => '\'',
        0x96 | 0x97 => '-',
        0x99 => ' ',
        0xA9 => 'c',
        0xAE => 'R',
        0xB7 => '*',
        other => other as char,
    }
}

fn cp1253_to_char(b: u8) -> char {
    match b {
        0x80..=0x9F => cp1252_to_char(b),
        0xA1 => '\u{0385}',
        0xA2 => '\u{0386}',
        0xAF => '\u{2015}',
        0xB4 => '\u{0384}',
        0xB8 => '\u{0388}',
        0xB9 => '\u{0389}',
        0xBA => '\u{038A}',
        0xBC => '\u{038C}',
        0xBE => '\u{038E}',
        0xBF => '\u{038F}',
        0xC0 => '\u{0390}',
        0xC1..=0xD1 => char::from_u32(0x0391 + (b - 0xC1) as u32).unwrap_or('?'),
        0xD3..=0xDB => char::from_u32(0x03A3 + (b - 0xD3) as u32).unwrap_or('?'),
        0xDC => '\u{03AC}',
        0xDD => '\u{03AD}',
        0xDE => '\u{03AE}',
        0xDF => '\u{03AF}',
        0xE0 => '\u{03B0}',
        0xE1..=0xF1 => char::from_u32(0x03B1 + (b - 0xE1) as u32).unwrap_or('?'),
        0xF2 => '\u{03C2}',
        0xF3..=0xFB => char::from_u32(0x03C3 + (b - 0xF3) as u32).unwrap_or('?'),
        0xFC => '\u{03CC}',
        0xFD => '\u{03CD}',
        0xFE => '\u{03CE}',
        other => cp1252_to_char(other),
    }
}

#[derive(Clone, Copy, Debug)]
struct RtfStyle {
    bold: bool,
    ul: bool,
    uldb: bool,
    hidden: bool,
    font_size: u16,
    is_symbol_font: bool,
    ignore_group: bool,
}

impl Default for RtfStyle {
    fn default() -> Self {
        Self {
            bold: false,
            ul: false,
            uldb: false,
            hidden: false,
            font_size: 24,
            is_symbol_font: false,
            ignore_group: false,
        }
    }
}

/// Parst den Inhalt von `MODHELP.RTF` bzw. `MODUSHLP.RTF` in `HelpTopic`-Strukturen ohne `#$K+`-Artefakte.
pub fn parse_modhelp_rtf(rtf: &str) -> Vec<HelpTopic> {
    let is_greek = rtf.contains("\\lang1032");
    let start_pos = rtf
        .find("#{\\footnote")
        .or_else(|| rtf.find("{\\cs41"))
        .unwrap_or(0);
    let body = &rtf[start_pos..];

    let raw_pages: Vec<&str> = body.split("\\page").collect();
    let mut topics = Vec::with_capacity(raw_pages.len());

    for raw_page in raw_pages {
        if let Some(topic) = parse_single_rtf_topic(raw_page, is_greek) {
            topics.push(topic);
        }
    }

    topics
}

fn extract_footnotes(raw: &str, marker: char, is_greek: bool) -> Vec<String> {
    let mut results = Vec::new();
    let pat = format!("{}{{\\footnote", marker);
    let mut search_from = 0usize;
    while let Some(rel) = raw[search_from..].find(&pat) {
        let fn_start = search_from + rel + pat.len();
        let bytes = raw.as_bytes();
        let mut depth = 1i32;
        let mut i = fn_start;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let fn_end = i.saturating_sub(1);
        if fn_end > fn_start {
            let fn_content = &raw[fn_start..fn_end];
            let plain = strip_rtf_to_plain(fn_content, is_greek);
            let cleaned = plain
                .trim()
                .strip_prefix(marker)
                .unwrap_or(plain.trim())
                .trim()
                .to_string();
            if !cleaned.is_empty() {
                results.push(cleaned);
            }
        }
        search_from = i;
    }
    results
}

fn strip_rtf_to_plain(s: &str, is_greek: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut ignore_depth: Option<i32> = None;
    let mut depth = 0i32;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'{' {
            depth += 1;
            if i + 2 < bytes.len() && bytes[i + 1] == b'\\' && bytes[i + 2] == b'*'
                && ignore_depth.is_none() {
                    ignore_depth = Some(depth);
                }
            i += 1;
            continue;
        }
        if b == b'}' {
            if ignore_depth == Some(depth) {
                ignore_depth = None;
            }
            depth -= 1;
            i += 1;
            continue;
        }
        if ignore_depth.is_some() {
            i += 1;
            continue;
        }
        if b == b'\r' || b == b'\n' {
            i += 1;
            continue;
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'\'' if i + 2 < bytes.len() => {
                    if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
                        && let Ok(val) = u8::from_str_radix(hex, 16) {
                            out.push(if is_greek {
                                cp1253_to_char(val)
                            } else {
                                cp1252_to_char(val)
                            });
                        }
                    i += 3;
                }
                b'{' | b'}' | b'\\' => {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                _ => {
                    let cw_start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
                        i += 1;
                    }
                    let cw = &s[cw_start..i];
                    while i < bytes.len()
                        && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'-')
                    {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    if cw == "tab" {
                        out.push(' ');
                    }
                }
            }
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn parse_single_rtf_topic(raw: &str, is_greek: bool) -> Option<HelpTopic> {
    let ids = extract_footnotes(raw, '#', false);
    if ids.is_empty() {
        return None;
    }
    let primary_id = ids[0].clone();
    let title = extract_footnotes(raw, '$', is_greek).into_iter().next();
    let browse_seq = extract_footnotes(raw, '+', false).into_iter().next();

    let bytes = raw.as_bytes();
    let mut i = 0usize;
    let mut style_stack: Vec<RtfStyle> = vec![RtfStyle::default()];
    let mut blocks: Vec<HelpBlock> = Vec::new();

    let mut cur_spans: Vec<HelpSpan> = Vec::new();
    let mut cur_text = String::new();
    let mut cur_link_text = String::new();
    let mut cur_link_is_popup = false;
    let mut in_link_text = false;
    let mut cur_hidden_target = String::new();
    let mut cur_indent = 0i32;
    let mut cur_bullet = false;
    let mut cur_para_max_fs = 24u16;
    let mut cur_table_cells: Vec<Vec<HelpSpan>> = Vec::new();
    let mut in_table_row = false;

    let flush_span = |cur_text: &mut String, cur_spans: &mut Vec<HelpSpan>, bold: bool| {
        if !cur_text.is_empty() {
            let txt = sanitize_control_artifacts(&std::mem::take(cur_text));
            if txt.is_empty() {
                return;
            }
            if let Some(last) = cur_spans.last_mut()
                && last.bold == bold && last.link.is_none() {
                    last.text.push_str(&txt);
                    return;
                }
            cur_spans.push(HelpSpan {
                text: txt,
                bold,
                link: None,
            });
        }
    };

    let flush_paragraph = |blocks: &mut Vec<HelpBlock>,
                           cur_spans: &mut Vec<HelpSpan>,
                           cur_text: &mut String,
                           cur_link_text: &mut String,
                           in_link_text: &mut bool,
                           bold: bool,
                           cur_indent: &mut i32,
                           cur_bullet: &mut bool,
                           cur_para_max_fs: &mut u16| {
        if *in_link_text && !cur_link_text.is_empty() {
            cur_text.push_str(cur_link_text);
            cur_link_text.clear();
            *in_link_text = false;
        }
        flush_span(cur_text, cur_spans, bold);

        // Prüfe auf eingebettete `{bmc <name>.shg}` oder `{bmct <name>.shg}` Grafik im Absatz
        let full_line: String = cur_spans.iter().map(|sp| sp.text.as_str()).collect();
        for bmc_prefix in ["{bmc ", "{bmct ", "{bml ", "{bmr "] {
            if let Some(bmc_pos) = full_line.find(bmc_prefix)
                && let Some(end_pos) = full_line[bmc_pos..].find('}') {
                    let shg_file = &full_line[bmc_pos + bmc_prefix.len()..bmc_pos + end_pos];
                    if let Some(bm_key) = map_shg_to_bm_key(shg_file) {
                        blocks.push(HelpBlock::Bitmap {
                            bm_key: bm_key.to_string(),
                        });
                        cur_spans.clear();
                        *cur_bullet = false;
                        *cur_para_max_fs = 24;
                        return;
                    }
                }
        }

        // Bereinige etwaige führende `#$K+` Footnote-Präfix-Reste im ersten Span
        if let Some(first_sp) = cur_spans.first_mut() {
            first_sp.text = sanitize_control_artifacts(&first_sp.text);
        }
        cur_spans.retain(|sp| !sp.text.is_empty());

        let has_content = cur_spans.iter().any(|sp| !sp.text.trim().is_empty());
        if has_content {
            let only_one_plain =
                cur_spans.len() == 1 && cur_spans[0].link.is_none() && !*cur_bullet;
            if *cur_para_max_fs >= 28 && only_one_plain && blocks.len() <= 2 {
                let h_text = sanitize_control_artifacts(cur_spans[0].text.trim());
                if !h_text.is_empty() {
                    blocks.push(HelpBlock::Heading(h_text));
                }
                cur_spans.clear();
            } else {
                let spans = std::mem::take(cur_spans);
                blocks.push(HelpBlock::Paragraph {
                    indent: *cur_indent,
                    bullet: *cur_bullet,
                    spans,
                });
            }
        } else {
            cur_spans.clear();
        }
        *cur_bullet = false;
        *cur_para_max_fs = 24;
    };

    while i < bytes.len() {
        let b = bytes[i];
        let cur_style = *style_stack.last().unwrap_or(&RtfStyle::default());

        if b == b'{' {
            let mut next_style = cur_style;
            if i + 2 < bytes.len() && bytes[i + 1] == b'\\' && bytes[i + 2] == b'*' {
                next_style.ignore_group = true;
            }
            // Falls `{` eine `{\footnote`-Gruppe öffnet, entferne das unmittelbar davorstehende
            // WinHelp-Footnote-Symbol (`#`, `$`, `K`, `+`, `*`, `!`, `>`) aus `cur_text`!
            if raw[i..].starts_with("{\\footnote") {
                next_style.ignore_group = true;
                while let Some(last_ch) = cur_text.chars().last() {
                    if matches!(last_ch, '#' | '$' | 'K' | '+' | '*' | '!' | '>' | ' ') {
                        cur_text.pop();
                        if last_ch != ' ' {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            style_stack.push(next_style);
            i += 1;
            continue;
        }

        if b == b'}' {
            let popped = style_stack.pop().unwrap_or_default();
            if style_stack.is_empty() {
                style_stack.push(RtfStyle::default());
            }
            let now_style = *style_stack.last().unwrap_or(&RtfStyle::default());

            if popped.hidden && !now_style.hidden {
                let target = cur_hidden_target.trim().to_string();
                cur_hidden_target.clear();
                if !target.is_empty() && !cur_link_text.is_empty() {
                    let ltext = sanitize_control_artifacts(&std::mem::take(&mut cur_link_text));
                    in_link_text = false;
                    if !ltext.is_empty() {
                        cur_spans.push(HelpSpan {
                            text: ltext,
                            bold: popped.bold,
                            link: Some(HelpLink {
                                target_id: target,
                                is_popup: cur_link_is_popup,
                            }),
                        });
                    }
                }
            }
            i += 1;
            continue;
        }

        if b == b'\r' || b == b'\n' {
            i += 1;
            continue;
        }

        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'\'' if i + 2 < bytes.len() => {
                    if !cur_style.ignore_group
                        && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
                            && let Ok(val) = u8::from_str_radix(hex, 16) {
                                let ch = if cur_style.is_symbol_font && val == 0xB7 {
                                    cur_bullet = true;
                                    '\0'
                                } else if is_greek && !cur_style.hidden {
                                    cp1253_to_char(val)
                                } else {
                                    cp1252_to_char(val)
                                };
                                if ch != '\0' {
                                    if cur_style.hidden {
                                        cur_hidden_target.push(ch);
                                    } else if in_link_text {
                                        cur_link_text.push(ch);
                                    } else {
                                        cur_text.push(ch);
                                    }
                                }
                            }
                    i += 3;
                }
                b'{' | b'}' | b'\\' => {
                    if !cur_style.ignore_group && !cur_style.hidden {
                        let ch = bytes[i] as char;
                        if in_link_text {
                            cur_link_text.push(ch);
                        } else {
                            cur_text.push(ch);
                        }
                    }
                    i += 1;
                }
                b'~' => {
                    if !cur_style.ignore_group && !cur_style.hidden {
                        if in_link_text {
                            cur_link_text.push(' ');
                        } else {
                            cur_text.push(' ');
                        }
                    }
                    i += 1;
                }
                b'-' | b'_' => {
                    i += 1;
                }
                _ => {
                    let cw_start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
                        i += 1;
                    }
                    let cw = &raw[cw_start..i];
                    let num_start = i;
                    while i < bytes.len()
                        && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'-')
                    {
                        i += 1;
                    }
                    let num_val: Option<i32> = if i > num_start {
                        raw[num_start..i].parse().ok()
                    } else {
                        None
                    };
                    if i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }

                    if let Some(st) = style_stack.last_mut() {
                        match cw {
                            "footnote" | "pntext" | "fonttbl" | "colortbl" | "stylesheet"
                            | "info" => {
                                st.ignore_group = true;
                            }
                            "pard" | "plain" => {
                                st.bold = false;
                                st.ul = false;
                                st.uldb = false;
                                st.hidden = false;
                                st.is_symbol_font = false;
                                if cw == "pard" {
                                    cur_indent = 0;
                                    in_table_row = false;
                                }
                            }
                            "li" => {
                                let twips = num_val.unwrap_or(0);
                                cur_indent = (twips / 200).clamp(0, 6);
                            }
                            "b" => {
                                let new_b = num_val.unwrap_or(1) != 0;
                                if !st.ignore_group
                                    && !st.hidden
                                    && !in_link_text
                                    && st.bold != new_b
                                {
                                    flush_span(&mut cur_text, &mut cur_spans, st.bold);
                                }
                                st.bold = new_b;
                            }
                            "fs" => {
                                let fs = num_val.unwrap_or(24) as u16;
                                st.font_size = fs;
                                if !st.ignore_group && !st.hidden && fs > cur_para_max_fs {
                                    cur_para_max_fs = fs;
                                }
                            }
                            "f" => {
                                st.is_symbol_font = num_val == Some(1);
                            }
                            "uldb" => {
                                let on = num_val.unwrap_or(1) != 0;
                                st.uldb = on;
                                if on && !st.ignore_group {
                                    flush_span(&mut cur_text, &mut cur_spans, st.bold);
                                    in_link_text = true;
                                    cur_link_is_popup = false;
                                }
                            }
                            "ul" => {
                                let on = num_val.unwrap_or(1) != 0;
                                st.ul = on;
                                if on && !st.ignore_group && st.font_size < 28 {
                                    flush_span(&mut cur_text, &mut cur_spans, st.bold);
                                    in_link_text = true;
                                    cur_link_is_popup = true;
                                }
                            }
                            "ulnone" => {
                                st.ul = false;
                                st.uldb = false;
                            }
                            "v" => {
                                st.hidden = num_val.unwrap_or(1) != 0;
                                if st.hidden {
                                    cur_hidden_target.clear();
                                }
                            }
                            "tab" => {
                                if !st.ignore_group && !st.hidden && !cur_bullet {
                                    if in_link_text {
                                        cur_link_text.push(' ');
                                    } else {
                                        // Saubere Spalten-Ausrichtung für Credits, Adressen und Tabellen
                                        let cur_line_len: usize = cur_spans
                                            .iter()
                                            .map(|sp| sp.text.chars().count())
                                            .sum::<usize>()
                                            + cur_text.chars().count();
                                        let tab_stop = if cur_line_len < 10 {
                                            10
                                        } else if cur_line_len < 24 {
                                            24
                                        } else {
                                            cur_line_len + 2
                                        };
                                        let pad = tab_stop.saturating_sub(cur_line_len).max(2);
                                        for _ in 0..pad {
                                            cur_text.push(' ');
                                        }
                                    }
                                }
                            }
                            "ldblquote" | "rdblquote" => {
                                if !st.ignore_group && !st.hidden {
                                    if in_link_text {
                                        cur_link_text.push('"');
                                    } else {
                                        cur_text.push('"');
                                    }
                                }
                            }
                            "lquote" | "rquote" => {
                                if !st.ignore_group && !st.hidden {
                                    if in_link_text {
                                        cur_link_text.push('\'');
                                    } else {
                                        cur_text.push('\'');
                                    }
                                }
                            }
                            "intbl" => {
                                in_table_row = true;
                            }
                            "cell" => {
                                in_table_row = true;
                                if in_link_text && !cur_link_text.is_empty() {
                                    cur_text.push_str(&cur_link_text);
                                    cur_link_text.clear();
                                    in_link_text = false;
                                }
                                flush_span(&mut cur_text, &mut cur_spans, st.bold);
                                let mut cell_spans = std::mem::take(&mut cur_spans);
                                if let Some(first) = cell_spans.first_mut() {
                                    first.text = first.text.trim_start().to_string();
                                }
                                if let Some(last) = cell_spans.last_mut() {
                                    last.text = last.text.trim_end().to_string();
                                }
                                cell_spans.retain(|sp| !sp.text.is_empty());
                                cur_table_cells.push(cell_spans);
                            }
                            "row" => {
                                if !cur_table_cells.is_empty() {
                                    let cells = std::mem::take(&mut cur_table_cells);
                                    if cells
                                        .iter()
                                        .any(|c| c.iter().any(|sp| !sp.text.trim().is_empty()))
                                    {
                                        blocks.push(HelpBlock::TableRow(cells));
                                    }
                                }
                                in_table_row = false;
                            }
                            "par" | "line"
                                if !st.ignore_group && !in_table_row => {
                                    let b_flag = st.bold;
                                    flush_paragraph(
                                        &mut blocks,
                                        &mut cur_spans,
                                        &mut cur_text,
                                        &mut cur_link_text,
                                        &mut in_link_text,
                                        b_flag,
                                        &mut cur_indent,
                                        &mut cur_bullet,
                                        &mut cur_para_max_fs,
                                    );
                                }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if !cur_style.ignore_group {
            if in_link_text && !cur_style.ul && !cur_style.uldb && !cur_style.hidden {
                cur_text.push_str(&cur_link_text);
                cur_link_text.clear();
                in_link_text = false;
            }
            // In MODUSHLP.RTF wird im Symbol-Font (`\f1`) das Zeichen `*` als Aufzählungspunkt genutzt
            if cur_style.is_symbol_font && b == b'*' {
                cur_bullet = true;
                i += 1;
                continue;
            }
            let ch = cp1252_to_char(b);
            if cur_style.hidden {
                cur_hidden_target.push(ch);
            } else if in_link_text {
                cur_link_text.push(ch);
            } else {
                cur_text.push(ch);
            }
        }
        i += 1;
    }

    let final_bold = style_stack.last().map(|s| s.bold).unwrap_or(false);
    flush_paragraph(
        &mut blocks,
        &mut cur_spans,
        &mut cur_text,
        &mut cur_link_text,
        &mut in_link_text,
        final_bold,
        &mut cur_indent,
        &mut cur_bullet,
        &mut cur_para_max_fs,
    );

    Some(HelpTopic {
        id: primary_id,
        all_ids: ids,
        title,
        browse_seq,
        blocks,
    })
}

// =========================================================================
// WinHelp-Viewer Fenster-Zustand, Layout-Engine & Interaktive Hotspots
// =========================================================================

pub const HELP_WINDOW_WIDTH: u32 = 668;
pub const HELP_WINDOW_HEIGHT: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpClickResult {
    Handled,
    CloseWindow,
    DragWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpFontKind {
    /// Proportionale Windows 3.1 `Times New Roman` / `MS Serif` Dokument-Schriftart (`\f4\fs24` in `MODHELP.RTF`).
    SerifBody,
    /// Größere fette Windows 3.1 `Times New Roman` Überschrift (`\b\fs28\cf2` in `MODHELP.RTF`).
    SerifHeading,
    /// Monospace-Schriftart (`Courier`) für `<PRE>`-Blöcke.
    Mono,
    /// Runder Aufzählungspunkt (`•`, `\f1 \'b7` in `MODHELP.RTF`).
    DiscBullet,
}

#[derive(Clone, Debug)]
struct LaidOutSpan {
    x: i32,
    y: i32,
    w: i32,
    text: String,
    color: [u8; 4],
    bold: bool,
    font_kind: HelpFontKind,
    underline: Option<(bool, String)>, // (is_popup, target_id)
}

#[derive(Clone, Debug)]
struct LaidOutBitmap {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    bm_key: String,
}

#[derive(Clone, Debug, Default)]
struct TopicLayout {
    spans: Vec<LaidOutSpan>,
    bitmaps: Vec<LaidOutBitmap>,
    table_rects: Vec<(i32, i32, u32, u32)>,
    heading_banners: Vec<(i32, i32, u32, u32)>,
    total_height: i32,
}

/// Verfügbare Hilfesprachen im `Datei` / `File`-Menü (`(language_id, code, label)`).
pub const HELP_MENU_LANGUAGES: [(u8, &str, &str); 7] = [
    (1, "de", "Deutsch"),
    (0, "en", "English"),
    (2, "fr", "Français"),
    (3, "es", "Español"),
    (4, "it", "Italiano"),
    (5, "nl", "Nederlands"),
    (6, "grk", "Ελληνικά (Greek)"),
];

/// Zustand des integrierten HTML-/WinHelp-Viewers.
#[derive(Clone, Debug)]
pub struct HelpViewerState {
    pub book: WinHelpBook,
    pub current_topic_id: String,
    pub scroll_y: i32,
    pub history: Vec<String>,
    pub popup_topic_id: Option<String>,
    pub popup_anchor: (i32, i32),
    pub file_menu_open: bool,
}

impl HelpViewerState {
    pub fn new() -> Self {
        Self::new_with_language(1)
    }

    pub fn new_with_language(lang_id: u8) -> Self {
        Self {
            book: WinHelpBook::load_for_language(lang_id),
            current_topic_id: "main_index".to_string(),
            scroll_y: 0,
            history: Vec::new(),
            popup_topic_id: None,
            popup_anchor: (160, 120),
            file_menu_open: false,
        }
    }

    /// Aktualisiert das geladene Hilfebuch (`help_de.html`, `help_en.html`, etc.), falls die Sprache umgeschaltet wurde.
    pub fn set_language(&mut self, lang_id: u8) {
        if self.book.language_id != lang_id {
            self.book = WinHelpBook::load_for_language(lang_id);
        }
    }

    /// Öffnet die aktuelle HTML-Hilfedatei (`help/help_<code>.html`) im System-Browser.
    /// Falls die `.exe` standalone ohne `help/`-Verzeichnis ausgeführt wird, wird die komprimiert
    /// eingebettete HTML-Datei (samt den 16 Dialog-Grafiken `bm0.bmp`..`bm15.bmp`) in `%TEMP%\mod4win_help\` entpackt.
    pub fn open_current_topic_in_browser(&self) {
        let is_ger = self.book.language_id == 1;
        let lang_code = WinHelpBook::lang_code_for_id(self.book.language_id);
        let fname = format!("help_{}.html", lang_code);
        let html_path = WinHelpBook::find_help_html_on_disk(&fname).unwrap_or_else(|| {
            let fallback_dir = std::env::temp_dir().join("mod4win_help");
            let img_dir = if is_ger {
                fallback_dir.join("img")
            } else {
                fallback_dir.join("img").join(lang_code)
            };
            let _ = fs::create_dir_all(&img_dir);
            for (bname, bbytes) in HLP_BM_RAW_BYTES {
                let bp = img_dir.join(bname);
                if !bp.exists() {
                    let _ = fs::write(&bp, bbytes);
                }
            }
            let p = fallback_dir.join(fname);
            let html = WinHelpBook::decompress_embedded_help_html(lang_code)
                .unwrap_or_else(|| export_topics_to_html(&self.book.topics, is_ger));
            let _ = fs::write(&p, html);
            p
        });
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &html_path.to_string_lossy()])
                .spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&html_path)
                .spawn();
        }
    }

    pub fn open_topic(&mut self, topic_id: &str) {
        let Some(topic) = self.book.get_topic(topic_id).cloned() else {
            return;
        };
        if topic.is_full_page() {
            if !self.current_topic_id.eq_ignore_ascii_case(&topic.id) {
                self.history.push(self.current_topic_id.clone());
            }
            self.current_topic_id = topic.id;
            self.scroll_y = 0;
            self.popup_topic_id = None;
        } else {
            self.popup_topic_id = Some(topic.id);
        }
    }

    pub fn go_back(&mut self) {
        if self.popup_topic_id.is_some() {
            self.popup_topic_id = None;
            return;
        }
        if let Some(prev) = self.history.pop() {
            self.current_topic_id = prev;
            self.scroll_y = 0;
        }
    }

    pub fn step_browse(&mut self, delta: isize) {
        if let Some(next_topic) = self.book.step_browse(&self.current_topic_id, delta).cloned() {
            if !self.current_topic_id.eq_ignore_ascii_case(&next_topic.id) {
                self.history.push(self.current_topic_id.clone());
            }
            self.current_topic_id = next_topic.id;
            self.scroll_y = 0;
            self.popup_topic_id = None;
        }
    }

    pub fn scroll_by_lines(&mut self, lines: i32, assets: &EmbeddedAssets) {
        if self.popup_topic_id.is_some() {
            self.popup_topic_id = None;
        }
        let max_scroll = self.max_scroll_y(assets);
        self.scroll_y = (self.scroll_y + lines * 16).clamp(0, max_scroll);
    }

    fn bmp_for_key<'a>(assets: &'a EmbeddedAssets, bm_key: &str) -> Option<&'a BmpImage> {
        match bm_key {
            "bm0" => Some(&assets.bm0_main),
            "bm1" => Some(&assets.bm1_mini),
            "bm2" => Some(&assets.bm2_fileman),
            "bm3" => Some(&assets.bm3_selbox),
            "bm4" => Some(&assets.bm4_setup),
            "bm5" => Some(&assets.bm5_arcopt),
            "bm6" => Some(&assets.bm6_arcparm),
            "bm7" => Some(&assets.bm7_keysel),
            "bm8" => Some(&assets.bm8_fsel),
            "bm9" => Some(&assets.bm9_open),
            "bm10" => Some(&assets.bm10_info),
            "bm11" => Some(&assets.bm11_ins),
            "bm12" => Some(&assets.bm12_effwnd),
            "bm13" => Some(&assets.bm13_effects_help),
            "bm14" => Some(&assets.bm14_regdlg),
            "bm15" => Some(&assets.bm15_recname),
            _ => None,
        }
    }

    fn layout_topic(&self, topic: &HelpTopic, assets: &EmbeddedAssets, max_w: i32) -> TopicLayout {
        let mut layout = TopicLayout::default();
        let mut y = 6i32;

        // Originalfarben aus `\colortbl` in `MODHELP.RTF`:
        // `\cf2` = #0000FF (Blau für Überschriften & fette Hervorhebungen), `\cf11` = #008000 (WinHelp-Grün für Links)
        let col_heading = [0, 0, 216, 255];
        let col_text = [0, 0, 0, 255];
        let col_bold = [0, 0, 168, 255];
        let col_link = [0, 128, 0, 255];
        let is_full_page = topic.is_full_page();
        let mut first_heading = true;

        for block in &topic.blocks {
            match block {
                HelpBlock::Heading(h) => {
                    let banner_pad_x = 12;
                    let lines = wrap_win31_heading_text(h, (max_w - banner_pad_x * 2).max(80));
                    if is_full_page && first_heading {
                        first_heading = false;
                        let banner_y = y.max(2);
                        let banner_h = (lines.len() as i32 * 18 + 8).max(24);
                        layout.heading_banners.push((
                            2,
                            banner_y,
                            (max_w - 4).max(40) as u32,
                            banner_h as u32,
                        ));
                        y = banner_y + 4;
                        for line in lines {
                            let lw = win31_heading_text_width(&line);
                            let lx = ((max_w - lw) / 2).max(8);
                            layout.spans.push(LaidOutSpan {
                                x: lx,
                                y,
                                w: lw,
                                text: line,
                                color: col_heading,
                                bold: true,
                                font_kind: HelpFontKind::SerifHeading,
                                underline: None,
                            });
                            y += 18;
                        }
                        y = banner_y + banner_h + 6;
                    } else {
                        y += 2;
                        for line in lines {
                            let lw = win31_heading_text_width(&line);
                            layout.spans.push(LaidOutSpan {
                                x: 8,
                                y,
                                w: lw,
                                text: line,
                                color: col_heading,
                                bold: true,
                                font_kind: HelpFontKind::SerifHeading,
                                underline: None,
                            });
                            y += 18;
                        }
                        y += 4;
                    }
                }
                HelpBlock::Preformatted(lines) => {
                    let max_chars = ((max_w - 16) / 7).max(20) as usize;
                    for line in lines {
                        let disp: String = line.chars().take(max_chars).collect();
                        let w_px = disp.chars().count() as i32 * 7;
                        layout.spans.push(LaidOutSpan {
                            x: 10,
                            y,
                            w: w_px,
                            text: disp,
                            color: col_bold,
                            bold: false,
                            font_kind: HelpFontKind::Mono,
                            underline: None,
                        });
                        y += 14;
                    }
                    y += 4;
                }
                HelpBlock::Paragraph {
                    indent,
                    bullet,
                    spans,
                } => {
                    let x_base = 8 + (*indent * 16).min(96) + if *bullet { 14 } else { 0 };
                    if *bullet {
                        layout.spans.push(LaidOutSpan {
                            x: x_base - 11,
                            y,
                            w: 6,
                            text: "\u{2022}".to_string(),
                            color: col_heading,
                            bold: true,
                            font_kind: HelpFontKind::DiscBullet,
                            underline: None,
                        });
                    }
                    let mut cur_x = x_base;
                    for sp in spans {
                        let is_link = sp.link.as_ref().map(|l| (l.is_popup, l.target_id.clone()));
                        let color = if is_link.is_some() {
                            col_link
                        } else if sp.bold {
                            col_bold
                        } else {
                            col_text
                        };

                        let full_w = win31_serif_text_width(&sp.text, sp.bold);
                        if is_link.is_some() && full_w <= (max_w - x_base) {
                            if cur_x > x_base && cur_x + full_w > max_w {
                                cur_x = x_base;
                                y += 15;
                            }
                            layout.spans.push(LaidOutSpan {
                                x: cur_x,
                                y,
                                w: full_w,
                                text: sp.text.clone(),
                                color,
                                bold: sp.bold,
                                font_kind: HelpFontKind::SerifBody,
                                underline: is_link,
                            });
                            cur_x += full_w;
                            continue;
                        }

                        for token in split_preserving_spaces(&sp.text) {
                            let w_px = win31_serif_text_width(&token, sp.bold);
                            if token.trim().is_empty() {
                                if cur_x > x_base && cur_x + w_px <= max_w {
                                    cur_x += w_px;
                                }
                                continue;
                            }
                            if cur_x > x_base && cur_x + w_px > max_w {
                                cur_x = x_base;
                                y += 15;
                            }
                            layout.spans.push(LaidOutSpan {
                                x: cur_x,
                                y,
                                w: w_px,
                                text: token,
                                color,
                                bold: sp.bold,
                                font_kind: HelpFontKind::SerifBody,
                                underline: is_link.clone(),
                            });
                            cur_x += w_px;
                        }
                    }
                    y += if *indent > 0 && !*bullet { 15 } else { 17 };
                }
                HelpBlock::TableRow(cells) => {
                    let num_cols = cells.len().max(1) as i32;
                    let col_w = ((max_w - 12) / num_cols).max(48);
                    let row_y = y;
                    let mut max_cell_y = y;
                    for (ci, cell_spans) in cells.iter().enumerate() {
                        let col_x_start = 9 + (ci as i32) * col_w;
                        let col_x_max = col_x_start + col_w - 6;
                        let mut cx = col_x_start;
                        let mut cy = row_y + 2;
                        for sp in cell_spans {
                            let is_link =
                                sp.link.as_ref().map(|l| (l.is_popup, l.target_id.clone()));
                            let color = if is_link.is_some() {
                                col_link
                            } else if sp.bold {
                                col_bold
                            } else {
                                col_text
                            };
                            for token in split_preserving_spaces(&sp.text) {
                                let w_px = win31_serif_text_width(&token, sp.bold);
                                if token.trim().is_empty() {
                                    if cx > col_x_start && cx + w_px <= col_x_max {
                                        cx += w_px;
                                    }
                                    continue;
                                }
                                if cx > col_x_start && cx + w_px > col_x_max {
                                    cx = col_x_start;
                                    cy += 15;
                                }
                                let disp = clip_win31_serif_text(&token, sp.bold, (col_x_max - cx).max(8));
                                let dw_px = win31_serif_text_width(&disp, sp.bold);
                                layout.spans.push(LaidOutSpan {
                                    x: cx,
                                    y: cy,
                                    w: dw_px,
                                    text: disp,
                                    color,
                                    bold: sp.bold,
                                    font_kind: HelpFontKind::SerifBody,
                                    underline: is_link.clone(),
                                });
                                cx += dw_px;
                            }
                        }
                        max_cell_y = max_cell_y.max(cy);
                    }
                    let row_h = (max_cell_y - row_y + 16).max(17);
                    for ci in 0..num_cols {
                        let rx = 6 + ci * col_w;
                        layout
                            .table_rects
                            .push((rx, row_y, col_w as u32 + 1, row_h as u32 + 1));
                    }
                    y += row_h;
                }
                HelpBlock::Bitmap { bm_key } => {
                    if let Some(bmp) = Self::bmp_for_key(assets, bm_key) {
                        y += 4;
                        let bx = ((max_w - bmp.width as i32) / 2).max(2);
                        layout.bitmaps.push(LaidOutBitmap {
                            x: bx,
                            y,
                            w: bmp.width,
                            h: bmp.height,
                            bm_key: bm_key.clone(),
                        });
                        y += bmp.height as i32 + 8;
                    }
                }
            }
        }

        layout.total_height = y + 12;
        layout
    }

    pub fn max_scroll_y(&self, assets: &EmbeddedAssets) -> i32 {
        let Some(topic) = self.book.get_topic(&self.current_topic_id) else {
            return 0;
        };
        let layout = self.layout_topic(topic, assets, 634);
        (layout.total_height - 418).max(0)
    }

    /// Liefert die Bildschirm-Ursprungskoordinaten `(x, y)` einer eingebetteten Dialog-Bitmap (`bm0`..`bm15`) im aktuellen Hilfethema.
    pub fn bitmap_screen_origin(&self, bm_key: &str, assets: &EmbeddedAssets) -> Option<(i32, i32)> {
        let topic = self.book.get_topic(&self.current_topic_id)?;
        let layout = self.layout_topic(topic, assets, 634);
        let max_scroll = (layout.total_height - 418).max(0);
        let scroll_y = self.scroll_y.clamp(0, max_scroll);
        let lb = layout
            .bitmaps
            .iter()
            .find(|b| b.bm_key.eq_ignore_ascii_case(bm_key))?;
        Some((8 + lb.x, 68 + lb.y - scroll_y))
    }

    pub fn render(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        assets: &EmbeddedAssets,
        locale: &LocaleCatalog,
    ) {
        draw_3d_panel(frame, dw, dh, 0, 0, dw, dh, false, [192, 192, 192, 255]);

        let is_ger = self.book.language_id == 1;
        let topic = self
            .book
            .get_topic(&self.current_topic_id)
            .unwrap_or(&self.book.topics[0]);
        let help_prefix = locale.tr_or("Titles", "help_prefix", "MOD4WIN Help");
        let title_str = format!("{} - {}", help_prefix, topic.display_title());

        // 1. Authentische Windows 3.1 WINHELP.EXE Titelleiste (#000080 Dunkelblau, y = 2..20)
        fill_rect(frame, dw, dh, 2, 2, dw.saturating_sub(4), 18, [0, 0, 128, 255]);
        fill_rect(frame, dw, dh, 2, 20, dw.saturating_sub(4), 1, [0, 0, 0, 255]);
        // Links: Windows 3.1 Systemmenü-Button (`18x18` mit weißem Querstrich)
        draw_3d_panel(frame, dw, dh, 2, 2, 18, 18, false, [192, 192, 192, 255]);
        fill_rect(frame, dw, dh, 6, 10, 11, 4, [128, 128, 128, 255]);
        fill_rect(frame, dw, dh, 5, 9, 10, 3, [255, 255, 255, 255]);
        draw_rect_outline(frame, dw, dh, 5, 9, 10, 3, [0, 0, 0, 255]);
        // Rechts: Windows 3.1 Minimieren- & Schließen-Buttons (`▼` / `×`)
        let btn_min_x = dw as i32 - 38;
        let btn_cls_x = dw as i32 - 20;
        draw_3d_panel(frame, dw, dh, btn_min_x, 2, 18, 18, false, [192, 192, 192, 255]);
        draw_win31_arrow(frame, dw, dh, btn_min_x + 5, 9, true);
        draw_3d_panel(frame, dw, dh, btn_cls_x, 2, 18, 18, false, [192, 192, 192, 255]);
        draw_win31_close_x(frame, dw, dh, btn_cls_x + 5, 7);
        // Zentrierter weißer Fenstertitel in proportionaler Windows 3.1 `System`-Schrift
        let disp_t: String = title_str.chars().take(84).collect();
        let title_area_w = (dw as i32 - 62).max(100);
        let title_tw = win31_ui_text_width(&disp_t, true);
        let title_tx = 22 + ((title_area_w - title_tw) / 2).max(2);
        draw_win31_ui_text(frame, dw, dh, title_tx, 6, &disp_t, true, [255, 255, 255, 255]);

        // 2. Klassische Windows 3.1 Menüleiste (Weiß #FFFFFF, y = 21..38) in proportionaler `MS Sans Serif`
        fill_rect(frame, dw, dh, 2, 21, dw.saturating_sub(4), 17, [255, 255, 255, 255]);
        fill_rect(frame, dw, dh, 2, 38, dw.saturating_sub(4), 1, [0, 0, 0, 255]);
        if self.file_menu_open {
            let file_hl_w = if is_ger { 48u32 } else { 38u32 };
            fill_rect(frame, dw, dh, 4, 22, file_hl_w, 15, [0, 0, 128, 255]);
        }
        let menu_items: [(&str, i32); 4] = if is_ger {
            [
                ("&Datei", 10),
                ("&Bearbeiten", 58),
                ("&Lesezeichen", 136),
                ("&Hilfe", 224),
            ]
        } else {
            [
                ("&File", 10),
                ("&Edit", 48),
                ("&Bookmark", 88),
                ("&Help", 162),
            ]
        };
        for (idx, (m_lbl, mx)) in menu_items.iter().enumerate() {
            let col = if idx == 0 && self.file_menu_open {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            draw_win31_ui_text_mnemonic(frame, dw, dh, *mx, 25, m_lbl, false, col);
        }

        // 3. Windows 3.1 WINHELP.EXE Button-Leiste (Grau #C0C0C0, y = 39..63)
        fill_rect(frame, dw, dh, 2, 63, dw.saturating_sub(4), 1, [0, 0, 0, 255]);
        let lbl_contents = if is_ger {
            "Inhalt"
        } else {
            locale.tr_or("Help", "btn_contents", "Contents")
        };
        let lbl_back = if is_ger {
            "Zurück"
        } else {
            locale.tr_or("Help", "btn_back", "Back")
        };
        let lbl_dialogs = if is_ger {
            "Dialoge"
        } else {
            locale.tr_or("Help", "btn_dialogs", "Dialogs")
        };
        let lbl_keys = if is_ger {
            "Tasten"
        } else {
            locale.tr_or("Help", "btn_keys", "Keys")
        };
        let lbl_effects = if is_ger { "Effekte" } else { "Effects" };
        let lbl_history = if is_ger { "Historie" } else { "History" };

        let buttons: [(&str, i32, u32, bool); 9] = [
            (lbl_contents, 6, 66, true),
            (
                lbl_back,
                74,
                64,
                !self.history.is_empty() || self.popup_topic_id.is_some(),
            ),
            (lbl_dialogs, 140, 66, true),
            ("<<", 208, 30, true),
            (">>", 240, 30, true),
            (lbl_keys, 272, 64, true),
            (lbl_effects, 338, 66, true),
            ("FAQ", 406, 40, true),
            (lbl_history, 448, 72, true),
        ];
        for (lbl, bx, bw, enabled) in buttons {
            draw_3d_panel(frame, dw, dh, bx, 40, bw, 22, false, [192, 192, 192, 255]);
            let tw = win31_ui_text_width(lbl, true);
            let tx = bx + ((bw as i32 - tw) / 2).max(3);
            draw_win31_ui_text(
                frame,
                dw,
                dh,
                tx,
                47,
                lbl,
                true,
                if enabled {
                    [0, 0, 0, 255]
                } else {
                    [128, 128, 128, 255]
                },
            );
        }
        draw_win31_ui_text(frame, dw, dh, 528, 47, "Hotspots!", true, [0, 116, 0, 255]);

        // 4. Dokument-Viewport (x = 6..648, y = 64..493, Innenhöhe 418)
        let vp_x = 6i32;
        let vp_y = 64i32;
        let vp_w = 642u32;
        let vp_h = 429u32;
        fill_rect(frame, dw, dh, vp_x, vp_y, vp_w, vp_h, [255, 255, 255, 255]);
        draw_frame_3d_1px(frame, dw, dh, vp_x, vp_y, vp_w, vp_h, true);

        let layout = self.layout_topic(topic, assets, 634);
        let max_scroll = (layout.total_height - 418).max(0);
        let scroll_y = self.scroll_y.clamp(0, max_scroll);

        // Zeichne silbergraue Windows 3.1 WinHelp-Überschriften-Banner (`\cbpat16` = #C0C0C0)
        for &(bx, by, bw, bh) in &layout.heading_banners {
            let sx = vp_x + 2 + bx;
            let sy = vp_y + 4 + by - scroll_y;
            let cy0 = sy.max(vp_y + 2);
            let cy1 = (sy + bh as i32).min(vp_y + vp_h as i32 - 2);
            if cy1 > cy0 {
                fill_rect(
                    frame,
                    dw,
                    dh,
                    sx,
                    cy0,
                    bw,
                    (cy1 - cy0) as u32,
                    [192, 192, 192, 255],
                );
                if sy + bh as i32 > vp_y + 2 && sy + bh as i32 - 1 < vp_y + vp_h as i32 - 2 {
                    fill_rect(
                        frame,
                        dw,
                        dh,
                        sx,
                        sy + bh as i32 - 1,
                        bw,
                        1,
                        [128, 128, 128, 255],
                    );
                }
            }
        }

        // Zeichne eingebettete Dialog-Bitmaps (`bm0`..`bm15`) geclippt auf den Viewport
        for lb in &layout.bitmaps {
            if let Some(bmp) = Self::bmp_for_key(assets, &lb.bm_key) {
                let dst_x = vp_x + 2 + lb.x;
                let dst_y = vp_y + 4 + lb.y - scroll_y;
                blit_bmp_clipped(
                    bmp,
                    frame,
                    dw,
                    dh,
                    dst_x,
                    dst_y,
                    vp_x + 2,
                    vp_y + 2,
                    vp_x + vp_w as i32 - 2,
                    vp_y + vp_h as i32 - 2,
                );
            }
        }

        // Zeichne HTML 3.2 Tabellen-Zellenrahmen (`<TABLE BORDER="1">`) geclippt auf den Viewport
        for &(rx, ry, rw, rh) in &layout.table_rects {
            let sx = vp_x + 2 + rx;
            let sy = vp_y + 4 + ry - scroll_y;
            let cy0 = sy.max(vp_y + 2);
            let cy1 = (sy + rh as i32).min(vp_y + vp_h as i32 - 2);
            if cy1 > cy0 {
                let ch = (cy1 - cy0) as u32;
                if sy >= vp_y + 2 && sy < vp_y + vp_h as i32 - 2 {
                    fill_rect(frame, dw, dh, sx, sy, rw, 1, [160, 160, 160, 255]);
                }
                if sy + rh as i32 > vp_y + 2 && sy + rh as i32 - 1 < vp_y + vp_h as i32 - 2 {
                    fill_rect(
                        frame,
                        dw,
                        dh,
                        sx,
                        sy + rh as i32 - 1,
                        rw,
                        1,
                        [160, 160, 160, 255],
                    );
                }
                fill_rect(frame, dw, dh, sx, cy0, 1, ch, [160, 160, 160, 255]);
                fill_rect(
                    frame,
                    dw,
                    dh,
                    sx + rw as i32 - 1,
                    cy0,
                    1,
                    ch,
                    [160, 160, 160, 255],
                );
            }
        }

        // Zeichne Text-Spans & grüne Link-Unterstreichungen geclippt auf den Viewport
        for sp in &layout.spans {
            let sy = vp_y + 4 + sp.y - scroll_y;
            if sy >= vp_y + 2 && sy + 13 <= vp_y + vp_h as i32 - 2 {
                let sx = vp_x + 2 + sp.x;
                draw_help_span(frame, dw, dh, sx, sy, sp);
            }
        }

        // Vertikale Scrollbar rechts (x = 649..664, y = 64..493)
        draw_help_scrollbar(
            frame,
            dw,
            dh,
            649,
            vp_y,
            15,
            vp_h,
            scroll_y as usize,
            418,
            layout.total_height.max(418) as usize,
        );

        // 5. Grün umrandetes 1995er Popup-Fenster (falls ein Hotspot oder Popup-Link angeklickt wurde)
        if let Some(pop_id) = &self.popup_topic_id
            && let Some(pop_topic) = self.book.get_topic(pop_id) {
                let pop_layout = self.layout_topic(pop_topic, assets, 376);
                let pw = 400u32;
                let ph = (pop_layout.total_height as u32 + 20).clamp(56, 360);
                let px = self
                    .popup_anchor
                    .0
                    .clamp(14, (dw as i32 - pw as i32 - 14).max(14));
                let py = self
                    .popup_anchor
                    .1
                    .clamp(68, (dh as i32 - ph as i32 - 14).max(68));

                fill_rect(frame, dw, dh, px + 4, py + 4, pw, ph, [0, 0, 0, 255]);
                fill_rect(frame, dw, dh, px, py, pw, ph, [255, 255, 242, 255]);
                draw_rect_outline(frame, dw, dh, px, py, pw, ph, [0, 148, 0, 255]);
                draw_rect_outline(
                    frame,
                    dw,
                    dh,
                    px + 1,
                    py + 1,
                    pw.saturating_sub(2),
                    ph.saturating_sub(2),
                    [0, 148, 0, 255],
                );

                for sp in &pop_layout.spans {
                    let sy = py + 8 + sp.y;
                    if sy + 13 <= py + ph as i32 - 4 {
                        let sx = px + 10 + sp.x;
                        draw_help_span(frame, dw, dh, sx, sy, sp);
                    }
                }
            }

        // 6. Klassisches Windows 3.1 `Datei` / `File`-Dropdown-Menü (Sprachumschaltung + Beenden)
        if self.file_menu_open {
            let menu_x = 4i32;
            let menu_y = 38i32;
            let menu_w = 152u32;
            let menu_h = 140u32;
            // Windows 3.1 Schlagschatten rechts & unten
            fill_rect(frame, dw, dh, menu_x + 2, menu_y + 2, menu_w, menu_h, [0, 0, 0, 255]);
            fill_rect(frame, dw, dh, menu_x, menu_y, menu_w, menu_h, [255, 255, 255, 255]);
            draw_rect_outline(frame, dw, dh, menu_x, menu_y, menu_w, menu_h, [0, 0, 0, 255]);

            for (idx, &(lang_id, _code, label)) in HELP_MENU_LANGUAGES.iter().enumerate() {
                let ry = menu_y + 3 + (idx as i32) * 16;
                if self.book.language_id == lang_id {
                    draw_win31_checkmark(frame, dw, dh, menu_x + 7, ry + 3);
                }
                draw_win31_ui_text(frame, dw, dh, menu_x + 20, ry + 3, label, false, [0, 0, 0, 255]);
            }

            // Horizontale Trennlinie vor `Beenden` / `Exit`
            let sep_y = menu_y + 3 + (HELP_MENU_LANGUAGES.len() as i32) * 16 + 3;
            fill_rect(
                frame,
                dw,
                dh,
                menu_x + 1,
                sep_y,
                menu_w.saturating_sub(2),
                1,
                [0, 0, 0, 255],
            );
            let exit_y = sep_y + 4;
            let exit_lbl = if is_ger { "&Beenden" } else { "E&xit" };
            draw_win31_ui_text_mnemonic(
                frame,
                dw,
                dh,
                menu_x + 20,
                exit_y + 3,
                exit_lbl,
                false,
                [0, 0, 0, 255],
            );
        }
    }

    pub fn handle_click(&mut self, vx: i32, vy: i32, assets: &EmbeddedAssets) -> HelpClickResult {
        if vy <= 20 {
            self.file_menu_open = false;
            if vx <= 20 || vx >= (HELP_WINDOW_WIDTH as i32 - 38) {
                return HelpClickResult::CloseWindow;
            }
            return HelpClickResult::DragWindow;
        }

        // 0. Falls das `Datei` / `File`-Dropdown-Menü geöffnet ist:
        if self.file_menu_open {
            let menu_x = 4i32;
            let menu_y = 38i32;
            let menu_w = 152i32;
            let menu_h = 140i32;
            if (menu_x..=menu_x + menu_w).contains(&vx) && (menu_y..=menu_y + menu_h).contains(&vy) {
                let rel_y = vy - (menu_y + 3);
                let langs_h = (HELP_MENU_LANGUAGES.len() as i32) * 16;
                if (0..langs_h).contains(&rel_y) {
                    let idx = (rel_y / 16) as usize;
                    if let Some(&(lang_id, _, _)) = HELP_MENU_LANGUAGES.get(idx) {
                        self.set_language(lang_id);
                    }
                    self.file_menu_open = false;
                    return HelpClickResult::Handled;
                } else if rel_y >= langs_h + 7 {
                    self.file_menu_open = false;
                    return HelpClickResult::CloseWindow;
                }
                return HelpClickResult::Handled;
            }
            if (21..=38).contains(&vy) && vx < 56 {
                self.file_menu_open = false;
                return HelpClickResult::Handled;
            }
            self.file_menu_open = false;
        }

        // 1. Falls gerade ein grünes Popup offen ist:
        if let Some(pop_id) = self.popup_topic_id.clone() {
            if let Some(pop_topic) = self.book.get_topic(&pop_id).cloned() {
                let pop_layout = self.layout_topic(&pop_topic, assets, 376);
                let pw = 400u32;
                let ph = (pop_layout.total_height as u32 + 20).clamp(56, 360);
                let px = self
                    .popup_anchor
                    .0
                    .clamp(14, (HELP_WINDOW_WIDTH as i32 - pw as i32 - 14).max(14));
                let py = self
                    .popup_anchor
                    .1
                    .clamp(68, (HELP_WINDOW_HEIGHT as i32 - ph as i32 - 14).max(68));

                if vx >= px && vx < px + pw as i32 && vy >= py && vy < py + ph as i32 {
                    for sp in &pop_layout.spans {
                        if let Some((is_popup, target_id)) = &sp.underline {
                            let sx = px + 10 + sp.x;
                            let sy = py + 8 + sp.y;
                            let sw = sp.w.max(4);
                            if vx >= sx && vx < sx + sw && vy >= sy && vy <= sy + 13
                                && let Some(t) = self.book.get_topic(target_id).cloned() {
                                    if !*is_popup && t.is_full_page() {
                                        self.open_topic(&t.id);
                                    } else {
                                        self.popup_topic_id = Some(t.id);
                                    }
                                    return HelpClickResult::Handled;
                                }
                        }
                    }
                }
            }
            self.popup_topic_id = None;
            return HelpClickResult::Handled;
        }

        // 2. Windows 3.1 Menüleiste (y = 21..=38)
        if (21..=38).contains(&vy) {
            if vx < 56 {
                self.file_menu_open = !self.file_menu_open;
            } else if (128..216).contains(&vx) {
                self.open_topic("KnopfIndex");
            } else if vx >= 216 {
                self.open_topic("main_index");
            }
            return HelpClickResult::Handled;
        }

        // 3. Windows 3.1 WINHELP.EXE Button-Leiste (y = 39..=63)
        if (39..=63).contains(&vy) {
            if (6..=72).contains(&vx) {
                self.open_topic("main_index");
            } else if (74..=138).contains(&vx) {
                self.go_back();
            } else if (140..=206).contains(&vx) {
                self.open_topic("KnopfIndex");
            } else if (208..=238).contains(&vx) {
                self.step_browse(-1);
            } else if (240..=270).contains(&vx) {
                self.step_browse(1);
            } else if (272..=336).contains(&vx) {
                self.open_topic("KeyIndex");
            } else if (338..=404).contains(&vx) {
                self.open_topic("Effects");
            } else if (406..=446).contains(&vx) {
                self.open_topic("FAQ");
            } else if (448..=520).contains(&vx) {
                self.open_topic("Revisions");
            }
            return HelpClickResult::Handled;
        }

        let vp_x = 6i32;
        let vp_y = 64i32;
        let vp_w = 642i32;
        let vp_h = 429i32;

        // 4. Scrollbar rechts (x = 649..=664, y = 64..=493)
        if (649..=664).contains(&vx) && (vp_y..=vp_y + vp_h).contains(&vy) {
            let max_scroll = self.max_scroll_y(assets);
            if vy <= vp_y + 15 {
                self.scroll_y = (self.scroll_y - 28).max(0);
            } else if vy >= vp_y + vp_h - 15 {
                self.scroll_y = (self.scroll_y + 28).min(max_scroll);
            } else if vy < vp_y + vp_h / 2 {
                self.scroll_y = (self.scroll_y - 260).max(0);
            } else {
                self.scroll_y = (self.scroll_y + 260).min(max_scroll);
            }
            return HelpClickResult::Handled;
        }

        // 5. Klick im Dokument-Viewport (Hotspots auf `bm0`..`bm15` & grüne Text-Links)
        if (vp_x..=vp_x + vp_w).contains(&vx) && (vp_y..=vp_y + vp_h).contains(&vy) {
            let Some(topic) = self.book.get_topic(&self.current_topic_id).cloned() else {
                return HelpClickResult::Handled;
            };
            let layout = self.layout_topic(&topic, assets, 634);
            let max_scroll = (layout.total_height - 418).max(0);
            let scroll_y = self.scroll_y.clamp(0, max_scroll);

            let doc_x = vx - (vp_x + 2);
            let doc_y = vy - (vp_y + 4) + scroll_y;

            for lb in &layout.bitmaps {
                if doc_x >= lb.x
                    && doc_x < lb.x + lb.w as i32
                    && doc_y >= lb.y
                    && doc_y < lb.y + lb.h as i32
                {
                    let rel_x = doc_x - lb.x;
                    let rel_y = doc_y - lb.y;
                    if let Some(hs_block) = assets.block(&lb.bm_key)
                        && let Some(hs) = hs_block.hit_test(rel_x, rel_y)
                            && let Some(target_topic) = self.book.get_topic(&hs.id).cloned() {
                                if target_topic.is_full_page() {
                                    self.open_topic(&target_topic.id);
                                } else {
                                    self.popup_topic_id = Some(target_topic.id);
                                    self.popup_anchor = (vx, vy + 12);
                                }
                                return HelpClickResult::Handled;
                            }
                }
            }

            for sp in &layout.spans {
                if let Some((is_popup, target_id)) = &sp.underline {
                    let sw = sp.w.max(4);
                    if doc_x >= sp.x
                        && doc_x < sp.x + sw
                        && doc_y >= sp.y
                        && doc_y <= sp.y + 13
                        && let Some(target_topic) = self.book.get_topic(target_id).cloned() {
                            if !*is_popup && target_topic.is_full_page() {
                                self.open_topic(&target_topic.id);
                            } else {
                                self.popup_topic_id = Some(target_topic.id);
                                self.popup_anchor = (vx, vy + 12);
                            }
                            return HelpClickResult::Handled;
                        }
                }
            }
        }

        HelpClickResult::Handled
    }
}

impl Default for HelpViewerState {
    fn default() -> Self {
        Self::new()
    }
}

fn draw_help_span(frame: &mut [u8], dw: u32, dh: u32, sx: i32, sy: i32, sp: &LaidOutSpan) {
    match sp.font_kind {
        HelpFontKind::DiscBullet => {
            fill_rect(frame, dw, dh, sx + 1, sy + 4, 2, 4, sp.color);
            fill_rect(frame, dw, dh, sx, sy + 5, 4, 2, sp.color);
        }
        HelpFontKind::Mono => {
            for (ci, ch) in sp.text.chars().enumerate() {
                let mut tmp = [0u8; 4];
                let s = ch.encode_utf8(&mut tmp);
                draw_oem_text(frame, dw, dh, sx + (ci as i32) * 7, sy, s, sp.color);
            }
        }
        HelpFontKind::SerifHeading => {
            draw_win31_heading_text(frame, dw, dh, sx, sy, &sp.text, sp.color);
        }
        HelpFontKind::SerifBody => {
            draw_win31_serif_text(frame, dw, dh, sx, sy, &sp.text, sp.bold, sp.color);
            if let Some((is_popup, _)) = &sp.underline {
                let uw = (sp.w.max(2) as u32).saturating_sub(1);
                let uy = sy + 11;
                if *is_popup {
                    for dx in (0..uw as i32).step_by(2) {
                        fill_rect(frame, dw, dh, sx + dx, uy, 1, 1, sp.color);
                    }
                } else {
                    fill_rect(frame, dw, dh, sx, uy, uw, 1, sp.color);
                }
            }
        }
    }
}

fn draw_win31_arrow(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, down: bool) {
    let col = [0, 0, 0, 255];
    if down {
        fill_rect(frame, dw, dh, x, y, 7, 1, col);
        fill_rect(frame, dw, dh, x + 1, y + 1, 5, 1, col);
        fill_rect(frame, dw, dh, x + 2, y + 2, 3, 1, col);
        fill_rect(frame, dw, dh, x + 3, y + 3, 1, 1, col);
    } else {
        fill_rect(frame, dw, dh, x + 3, y, 1, 1, col);
        fill_rect(frame, dw, dh, x + 2, y + 1, 3, 1, col);
        fill_rect(frame, dw, dh, x + 1, y + 2, 5, 1, col);
        fill_rect(frame, dw, dh, x, y + 3, 7, 1, col);
    }
}

/// Zeichnet das symmetrische `8×7`-Schließen-Kreuz (`×` aus `SYSBUTTONS_W95.bmp`) zentriert im `18×18`-Button.
fn draw_win31_close_x(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32) {
    let col = [0, 0, 0, 255];
    const ROWS: [u8; 7] = [
        0b1100_0011,
        0b0110_0110,
        0b0011_1100,
        0b0001_1000,
        0b0011_1100,
        0b0110_0110,
        0b1100_0011,
    ];
    for (ry, &mask) in ROWS.iter().enumerate() {
        for rx in 0..8i32 {
            if (mask & (1 << (7 - rx))) != 0 {
                fill_rect(frame, dw, dh, x + rx, y + ry as i32, 1, 1, col);
            }
        }
    }
}

/// Zeichnet ein klassisches Windows 3.1 Menü-Häkchen (`7×7` Pixel) vor der aktiven Sprache im `File`-Menü.
fn draw_win31_checkmark(frame: &mut [u8], dw: u32, dh: u32, x: i32, y: i32) {
    let col = [0, 0, 0, 255];
    const ROWS: [u8; 7] = [
        0b000_0001,
        0b000_0011,
        0b000_0110,
        0b100_1100,
        0b111_1000,
        0b011_0000,
        0b001_0000,
    ];
    for (ry, &mask) in ROWS.iter().enumerate() {
        for rx in 0..7i32 {
            if (mask & (1 << (6 - rx))) != 0 {
                fill_rect(frame, dw, dh, x + rx, y + ry as i32, 1, 1, col);
            }
        }
    }
}

// =========================================================================
// Proportionale Windows 3.1 Schriften (`MS Sans Serif` UI & `Times New Roman` / `MS Serif` Dokument)
// =========================================================================

fn win31_ui_text_width(text: &str, bold: bool) -> i32 {
    let mut w = 0i32;
    let mut first = true;
    for ch in text.chars() {
        if !first {
            w += 1;
        }
        first = false;
        let (gw, _) = modern_ui_glyph_6x10(ch);
        w += gw as i32 + if bold && ch != ' ' { 1 } else { 0 };
    }
    w
}

#[allow(clippy::too_many_arguments)]
fn draw_win31_ui_text(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    text: &str,
    bold: bool,
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
                let bit = (gw as i32) - 1 - rx;
                if (mask & (1 << bit)) != 0 {
                    for bx in 0..=i32::from(bold && ch != ' ') {
                        let px = cur_x + rx + bx;
                        if px >= 0 && px < dw {
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
            }
        }
        cur_x += (gw as i32) + if bold && ch != ' ' { 2 } else { 1 };
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_win31_ui_text_mnemonic(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    text: &str,
    bold: bool,
    color: [u8; 4],
) {
    let mut cur_x = x;
    let mut underline_next = false;
    for ch in text.chars() {
        if ch == '&' && !underline_next {
            underline_next = true;
            continue;
        }
        let mut tmp = [0u8; 4];
        let s = ch.encode_utf8(&mut tmp);
        let cw = win31_ui_text_width(s, bold);
        draw_win31_ui_text(buf, w, h, cur_x, y, s, bold, color);
        if underline_next {
            fill_rect(buf, w, h, cur_x, y + 8, cw.max(2) as u32, 1, color);
            underline_next = false;
        }
        cur_x += cw + 1;
    }
}

/// Liefert das proportionale `13px`-Bitmap-Glyph im Stil der originalen Windows 3.1 `Times New Roman` (`MS Serif`, `\f4` in `MODHELP.RTF`):
/// - Zeilen `0..=9`: Oberlängen & Großbuchstaben (`Rows 1..=9`), Kleinbuchstaben (`Rows 4..=9`, Baseline auf `Row 9`), echte Serifen oben/unten.
/// - Zeilen `10..=12`: Echte Unterlängen (`g, j, p, q, y`, `,`, `;`) mit Serifen.
fn win31_serif_glyph(ch: char) -> (u8, [u16; 13]) {
    let normalized = match ch {
        'á' | 'à' | 'â' | 'ã' => 'a',
        'Á' | 'À' | 'Â' | 'Ã' | 'Α' | 'Ά' => 'A',
        'Β' => 'B',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' | 'Ε' | 'Έ' => 'E',
        'Ζ' => 'Z',
        'Η' | 'Ή' => 'H',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ι' | 'Ί' | 'Ϊ' => 'I',
        'Κ' => 'K',
        'Μ' => 'M',
        'ñ' => 'n',
        'Ñ' | 'Ν' => 'N',
        'ó' | 'ò' | 'ô' | 'õ' | 'ο' | 'ό' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ο' | 'Ό' => 'O',
        'Ρ' => 'P',
        'Τ' => 'T',
        'ú' | 'ù' | 'û' => 'u',
        'Ú' | 'Ù' | 'Û' => 'U',
        'Υ' | 'Ύ' | 'Ϋ' => 'Y',
        'Χ' => 'X',
        'ç' => 'c',
        'Ç' => 'C',
        other => other,
    };
    match normalized {
        ' ' => (3, [0; 13]),
        '!' => (3, [0, 0b010, 0b010, 0b010, 0b010, 0b010, 0b010, 0, 0b010, 0b010, 0, 0, 0]),
        '"' => (5, [0, 0b10010, 0b10010, 0b01001, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        '\'' | '`' | '´' => (2, [0, 0b10, 0b10, 0b01, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        '#' => (7, [0, 0, 0b0100100, 0b1111111, 0b0100100, 0b0100100, 0b1111111, 0b0100100, 0, 0, 0, 0, 0]),
        '$' => (6, [0, 0b001000, 0b011110, 0b101001, 0b101000, 0b011110, 0b001001, 0b101001, 0b011110, 0b001000, 0, 0, 0]),
        '%' => (7, [0, 0b0110001, 0b1001010, 0b0110010, 0b0000100, 0b0001000, 0b0010110, 0b0101001, 0b1000110, 0, 0, 0, 0]),
        '&' => (7, [0, 0b0011000, 0b0100100, 0b0100100, 0b0011000, 0b0101010, 0b1000100, 0b1000100, 0b0111011, 0, 0, 0, 0]),
        '(' => (3, [0, 0b001, 0b010, 0b010, 0b100, 0b100, 0b100, 0b100, 0b010, 0b010, 0b001, 0, 0]),
        ')' => (3, [0, 0b100, 0b010, 0b010, 0b001, 0b001, 0b001, 0b001, 0b010, 0b010, 0b100, 0, 0]),
        '*' => (5, [0, 0, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0, 0, 0, 0, 0, 0]),
        '+' => (5, [0, 0, 0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0, 0, 0, 0, 0]),
        ',' => (2, [0, 0, 0, 0, 0, 0, 0, 0, 0b11, 0b11, 0b01, 0b10, 0]),
        '-' | '–' | '—' => (5, [0, 0, 0, 0, 0, 0, 0b11111, 0, 0, 0, 0, 0, 0]),
        '.' => (2, [0, 0, 0, 0, 0, 0, 0, 0, 0b11, 0b11, 0, 0, 0]),
        '/' => (5, [0, 0b00001, 0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b10000, 0b10000, 0, 0, 0, 0]),
        '\\' => (5, [0, 0b10000, 0b10000, 0b01000, 0b00100, 0b00100, 0b00010, 0b00001, 0b00001, 0, 0, 0, 0]),
        ':' => (2, [0, 0, 0, 0, 0b11, 0b11, 0, 0, 0b11, 0b11, 0, 0, 0]),
        ';' => (2, [0, 0, 0, 0, 0b11, 0b11, 0, 0, 0b11, 0b11, 0b01, 0b10, 0]),
        '<' | '«' => (5, [0, 0, 0, 0b00001, 0b00010, 0b00100, 0b01000, 0b00100, 0b00010, 0b00001, 0, 0, 0]),
        '=' => (5, [0, 0, 0, 0, 0b11111, 0, 0, 0b11111, 0, 0, 0, 0, 0]),
        '>' | '»' => (5, [0, 0, 0, 0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0, 0]),
        '?' => (5, [0, 0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0, 0, 0]),
        '@' => (7, [0, 0b0011110, 0b0100001, 0b1001111, 0b1010011, 0b1010011, 0b1001110, 0b0100000, 0b0011110, 0, 0, 0, 0]),
        '[' => (3, [0, 0b111, 0b100, 0b100, 0b100, 0b100, 0b100, 0b100, 0b100, 0b111, 0, 0, 0]),
        ']' => (3, [0, 0b111, 0b001, 0b001, 0b001, 0b001, 0b001, 0b001, 0b001, 0b111, 0, 0, 0]),
        '^' => (5, [0, 0b00100, 0b01010, 0b10001, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        '_' => (6, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0b111111, 0, 0, 0]),
        '|' => (2, [0, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0b10, 0, 0]),
        '~' => (6, [0, 0, 0, 0, 0b011001, 0b100110, 0, 0, 0, 0, 0, 0, 0]),
        // Ziffern (0..9) im klassischen Serif-Stil (8px hoch auf Zeilen 2..=9)
        '0' => (6, [0, 0, 0b011110, 0b100001, 0b100001, 0b100001, 0b100001, 0b100001, 0b100001, 0b011110, 0, 0, 0]),
        '1' => (5, [0, 0, 0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110, 0, 0, 0]),
        '2' => (6, [0, 0, 0b011110, 0b100001, 0b000001, 0b000010, 0b000100, 0b001000, 0b010001, 0b111111, 0, 0, 0]),
        '3' => (6, [0, 0, 0b011110, 0b100001, 0b000001, 0b001110, 0b000001, 0b000001, 0b100001, 0b011110, 0, 0, 0]),
        '4' => (6, [0, 0, 0b000010, 0b000110, 0b001010, 0b010010, 0b100010, 0b111111, 0b000010, 0b000111, 0, 0, 0]),
        '5' => (6, [0, 0, 0b111111, 0b100000, 0b100000, 0b111110, 0b000001, 0b000001, 0b100001, 0b011110, 0, 0, 0]),
        '6' => (6, [0, 0, 0b001110, 0b010000, 0b100000, 0b111110, 0b100001, 0b100001, 0b100001, 0b011110, 0, 0, 0]),
        '7' => (6, [0, 0, 0b111111, 0b100001, 0b000010, 0b000100, 0b000100, 0b001000, 0b001000, 0b001000, 0, 0, 0]),
        '8' => (6, [0, 0, 0b011110, 0b100001, 0b100001, 0b011110, 0b100001, 0b100001, 0b100001, 0b011110, 0, 0, 0]),
        '9' => (6, [0, 0, 0b011110, 0b100001, 0b100001, 0b100001, 0b011111, 0b000001, 0b000010, 0b011100, 0, 0, 0]),
        // Großbuchstaben (A..Z, Ä, Ö, Ü) mit Serifen oben (Row 2) und unten (Row 9)
        'A' => (7, [0, 0, 0b0001000, 0b0010100, 0b0010100, 0b0100010, 0b0111110, 0b0100010, 0b0100010, 0b1110111, 0, 0, 0]),
        'Ä' => (7, [0b0100010, 0, 0b0001000, 0b0010100, 0b0010100, 0b0100010, 0b0111110, 0b0100010, 0b0100010, 0b1110111, 0, 0, 0]),
        'B' => (7, [0, 0, 0b1111110, 0b0100001, 0b0100001, 0b0111110, 0b0100001, 0b0100001, 0b0100001, 0b1111110, 0, 0, 0]),
        'C' => (6, [0, 0, 0b001111, 0b010001, 0b100000, 0b100000, 0b100000, 0b100000, 0b010001, 0b001110, 0, 0, 0]),
        'D' => (7, [0, 0, 0b1111100, 0b0100010, 0b0100001, 0b0100001, 0b0100001, 0b0100001, 0b0100010, 0b1111100, 0, 0, 0]),
        'E' => (6, [0, 0, 0b111111, 0b010001, 0b010000, 0b011110, 0b010000, 0b010000, 0b010001, 0b111111, 0, 0, 0]),
        'F' => (6, [0, 0, 0b111111, 0b010001, 0b010000, 0b011110, 0b010000, 0b010000, 0b010000, 0b111000, 0, 0, 0]),
        'G' => (7, [0, 0, 0b0011110, 0b0100001, 0b1000000, 0b1000000, 0b1000111, 0b1000010, 0b0100010, 0b0011100, 0, 0, 0]),
        'H' => (7, [0, 0, 0b1110111, 0b0100010, 0b0100010, 0b0111110, 0b0100010, 0b0100010, 0b0100010, 0b1110111, 0, 0, 0]),
        'I' => (5, [0, 0, 0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111, 0, 0, 0]),
        'J' => (5, [0, 0, 0b01111, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100, 0, 0, 0]),
        'K' => (7, [0, 0, 0b1110111, 0b0100100, 0b0101000, 0b0110000, 0b0101000, 0b0100100, 0b0100010, 0b1110111, 0, 0, 0]),
        'L' => (6, [0, 0, 0b111000, 0b010000, 0b010000, 0b010000, 0b010000, 0b010000, 0b010001, 0b111111, 0, 0, 0]),
        'M' => (9, [0, 0, 0b110000011, 0b011000110, 0b010101010, 0b010101010, 0b010010010, 0b010010010, 0b010000010, 0b111000111, 0, 0, 0]),
        'N' => (7, [0, 0, 0b1100111, 0b0110010, 0b0110010, 0b0101010, 0b0101010, 0b0100110, 0b0100010, 0b1110010, 0, 0, 0]),
        'O' => (7, [0, 0, 0b0011100, 0b0100010, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100010, 0b0011100, 0, 0, 0]),
        'Ö' => (7, [0b0100010, 0, 0b0011100, 0b0100010, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100010, 0b0011100, 0, 0, 0]),
        'P' => (6, [0, 0, 0b111110, 0b010001, 0b010001, 0b011110, 0b010000, 0b010000, 0b010000, 0b111000, 0, 0, 0]),
        'Q' => (7, [0, 0, 0b0011100, 0b0100010, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100010, 0b0011100, 0b0001100, 0b0000011, 0]),
        'R' => (7, [0, 0, 0b1111100, 0b0100010, 0b0100010, 0b0111100, 0b0101000, 0b0100100, 0b0100010, 0b1110011, 0, 0, 0]),
        'S' => (6, [0, 0, 0b011111, 0b100001, 0b100000, 0b011110, 0b000001, 0b000001, 0b100001, 0b111110, 0, 0, 0]),
        'T' => (7, [0, 0, 0b1111111, 0b1001001, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0011100, 0, 0, 0]),
        'U' => (7, [0, 0, 0b1110111, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0011100, 0, 0, 0]),
        'Ü' => (7, [0b0100010, 0, 0b1110111, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0011100, 0, 0, 0]),
        'V' => (7, [0, 0, 0b1110111, 0b0100010, 0b0100010, 0b0010100, 0b0010100, 0b0010100, 0b0001000, 0b0001000, 0, 0, 0]),
        'W' => (9, [0, 0, 0b111000111, 0b010010010, 0b010010010, 0b010101010, 0b010101010, 0b001000100, 0b001000100, 0b001000100, 0, 0, 0]),
        'X' => (7, [0, 0, 0b1110111, 0b0100010, 0b0010100, 0b0001000, 0b0001000, 0b0010100, 0b0100010, 0b1110111, 0, 0, 0]),
        'Y' => (7, [0, 0, 0b1110111, 0b0100010, 0b0010100, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0011100, 0, 0, 0]),
        'Z' => (6, [0, 0, 0b111111, 0b100010, 0b000100, 0b001000, 0b001000, 0b010000, 0b100001, 0b111111, 0, 0, 0]),
        'ß' => (6, [0, 0, 0b011100, 0b100010, 0b100010, 0b101100, 0b100010, 0b100001, 0b100001, 0b101110, 0b100000, 0, 0]),
        // Kleinbuchstaben (a..z, ä, ö, ü) mit Times-Serifen (x-height Rows 4..=9)
        'a' => (6, [0, 0, 0, 0, 0b011100, 0b000010, 0b011110, 0b100010, 0b100010, 0b011101, 0, 0, 0]),
        'ä' => (6, [0, 0, 0b010010, 0, 0b011100, 0b000010, 0b011110, 0b100010, 0b100010, 0b011101, 0, 0, 0]),
        'b' => (6, [0, 0, 0b110000, 0b010000, 0b011110, 0b010001, 0b010001, 0b010001, 0b010001, 0b111110, 0, 0, 0]),
        'c' => (5, [0, 0, 0, 0, 0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01110, 0, 0, 0]),
        'd' => (6, [0, 0, 0b000110, 0b000010, 0b011110, 0b100010, 0b100010, 0b100010, 0b100010, 0b011101, 0, 0, 0]),
        'e' => (5, [0, 0, 0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b10000, 0b01111, 0, 0, 0]),
        'f' => (5, [0, 0, 0b00110, 0b01001, 0b01000, 0b11110, 0b01000, 0b01000, 0b01000, 0b11100, 0, 0, 0]),
        'g' => (6, [0, 0, 0, 0, 0b011101, 0b100010, 0b100010, 0b011100, 0b100000, 0b011110, 0b100001, 0b100001, 0b011110]),
        'h' => (6, [0, 0, 0b110000, 0b010000, 0b011100, 0b010010, 0b010010, 0b010010, 0b010010, 0b111011, 0, 0, 0]),
        'i' => (3, [0, 0, 0b010, 0, 0b110, 0b010, 0b010, 0b010, 0b010, 0b111, 0, 0, 0]),
        'j' => (4, [0, 0, 0b0010, 0, 0b0110, 0b0010, 0b0010, 0b0010, 0b0010, 0b0010, 0b0010, 0b1010, 0b0100]),
        'k' => (6, [0, 0, 0b110000, 0b010000, 0b010111, 0b010100, 0b011000, 0b010100, 0b010010, 0b111011, 0, 0, 0]),
        'l' => (3, [0, 0, 0b110, 0b010, 0b010, 0b010, 0b010, 0b010, 0b010, 0b111, 0, 0, 0]),
        'm' => (9, [0, 0, 0, 0, 0b110110100, 0b010010010, 0b010010010, 0b010010010, 0b010010010, 0b111011011, 0, 0, 0]),
        'n' => (6, [0, 0, 0, 0, 0b110100, 0b010010, 0b010010, 0b010010, 0b010010, 0b111011, 0, 0, 0]),
        'o' => (5, [0, 0, 0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'ö' => (5, [0, 0, 0b10001, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'p' => (6, [0, 0, 0, 0, 0b110110, 0b010001, 0b010001, 0b010001, 0b010001, 0b011110, 0b010000, 0b010000, 0b111000]),
        'q' => (6, [0, 0, 0, 0, 0b011110, 0b100010, 0b100010, 0b100010, 0b100010, 0b011110, 0b000010, 0b000010, 0b000111]),
        'r' => (5, [0, 0, 0, 0, 0b11011, 0b01101, 0b01000, 0b01000, 0b01000, 0b11100, 0, 0, 0]),
        's' => (5, [0, 0, 0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b10001, 0b11110, 0, 0, 0]),
        't' => (4, [0, 0, 0b0100, 0b0100, 0b1111, 0b0100, 0b0100, 0b0100, 0b0100, 0b0011, 0, 0, 0]),
        'u' => (6, [0, 0, 0, 0, 0b110110, 0b010010, 0b010010, 0b010010, 0b010010, 0b001101, 0, 0, 0]),
        'ü' => (6, [0, 0, 0b010010, 0, 0b110110, 0b010010, 0b010010, 0b010010, 0b010010, 0b001101, 0, 0, 0]),
        'v' => (5, [0, 0, 0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100, 0, 0, 0]),
        'w' => (7, [0, 0, 0, 0, 0b1000001, 0b1001001, 0b1001001, 0b0101010, 0b0101010, 0b0010100, 0, 0, 0]),
        'x' => (5, [0, 0, 0, 0, 0b10001, 0b01010, 0b00100, 0b00100, 0b01010, 0b10001, 0, 0, 0]),
        'y' => (5, [0, 0, 0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100, 0b00100, 0b01000, 0b10000]),
        'z' => (5, [0, 0, 0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0, 0, 0]),
        // Griechische Groß- und Kleinbuchstaben (für `help_grk.html`)
        'Γ' => (6, [0, 0, 0b111111, 0b010001, 0b010000, 0b010000, 0b010000, 0b010000, 0b010000, 0b111000, 0, 0, 0]),
        'Δ' => (7, [0, 0, 0b0001000, 0b0010100, 0b0010100, 0b0100010, 0b0100010, 0b1000001, 0b1000001, 0b1111111, 0, 0, 0]),
        'Θ' => (7, [0, 0, 0b0011100, 0b0100010, 0b1000001, 0b1011101, 0b1000001, 0b1000001, 0b0100010, 0b0011100, 0, 0, 0]),
        'Λ' => (7, [0, 0, 0b0001000, 0b0010100, 0b0010100, 0b0100010, 0b0100010, 0b0100010, 0b1000001, 0b1110111, 0, 0, 0]),
        'Ξ' => (6, [0, 0, 0b111111, 0b100001, 0, 0b011110, 0, 0, 0b100001, 0b111111, 0, 0, 0]),
        'Π' => (7, [0, 0, 0b1111111, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b0100010, 0b1110111, 0, 0, 0]),
        'Σ' => (6, [0, 0, 0b111111, 0b010001, 0b001000, 0b000100, 0b001000, 0b010000, 0b100001, 0b111111, 0, 0, 0]),
        'Φ' => (7, [0, 0, 0b0011100, 0b0001000, 0b0111110, 0b1001001, 0b1001001, 0b0111110, 0b0001000, 0b0011100, 0, 0, 0]),
        'Ψ' => (7, [0, 0, 0b1001001, 0b1001001, 0b1001001, 0b0101010, 0b0011100, 0b0001000, 0b0001000, 0b0011100, 0, 0, 0]),
        'Ω' | 'Ώ' => (7, [0, 0, 0b0011100, 0b0100010, 0b1000001, 0b1000001, 0b0100010, 0b0010100, 0b0010100, 0b1110111, 0, 0, 0]),
        'α' | 'ά' => (6, [0, 0, 0, 0, 0b011101, 0b100010, 0b100010, 0b100010, 0b100010, 0b011101, 0, 0, 0]),
        'β' => (6, [0, 0, 0b011100, 0b100010, 0b100010, 0b101100, 0b100010, 0b100001, 0b100001, 0b101110, 0b100000, 0b100000, 0]),
        'γ' => (5, [0, 0, 0, 0, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0]),
        'δ' => (5, [0, 0, 0b01111, 0b01000, 0b00110, 0b01001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'ε' | 'έ' => (5, [0, 0, 0, 0, 0b01110, 0b10000, 0b01110, 0b10000, 0b10001, 0b01110, 0, 0, 0]),
        'ζ' => (5, [0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b01110, 0]),
        'η' | 'ή' => (6, [0, 0, 0, 0, 0b110110, 0b010001, 0b010001, 0b010001, 0b010001, 0b010001, 0b000001, 0b000001, 0]),
        'θ' => (5, [0, 0, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'ι' | 'ί' | 'ϊ' | 'ΐ' => (3, [0, 0, 0, 0, 0b010, 0b010, 0b010, 0b010, 0b010, 0b011, 0, 0, 0]),
        'κ' => (5, [0, 0, 0, 0, 0b10011, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001, 0, 0, 0]),
        'λ' => (6, [0, 0, 0b110000, 0b001000, 0b001000, 0b010100, 0b010100, 0b100010, 0b100010, 0b100001, 0, 0, 0]),
        'μ' => (6, [0, 0, 0, 0, 0b010010, 0b010010, 0b010010, 0b010010, 0b010010, 0b011101, 0b010000, 0b010000, 0]),
        'ν' => (5, [0, 0, 0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100, 0, 0, 0]),
        'ξ' => (5, [0, 0, 0b11111, 0b01000, 0b00110, 0b01000, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b01110, 0]),
        'π' => (6, [0, 0, 0, 0, 0b111111, 0b010010, 0b010010, 0b010010, 0b010010, 0b010011, 0, 0, 0]),
        'ρ' => (6, [0, 0, 0, 0, 0b011110, 0b100001, 0b100001, 0b100001, 0b100001, 0b101110, 0b100000, 0b100000, 0]),
        'σ' | 'ς' => (6, [0, 0, 0, 0, 0b011111, 0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0, 0, 0]),
        'τ' => (5, [0, 0, 0, 0, 0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00011, 0, 0, 0]),
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => (5, [0, 0, 0, 0, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0, 0]),
        'φ' => (7, [0, 0, 0, 0, 0b0110110, 0b1001001, 0b1001001, 0b1001001, 0b1001001, 0b0111110, 0b0001000, 0b0001000, 0]),
        'χ' => (5, [0, 0, 0, 0, 0b10001, 0b01010, 0b00100, 0b00100, 0b01010, 0b10001, 0b10001, 0, 0]),
        'ψ' => (7, [0, 0, 0, 0, 0b1001001, 0b1001001, 0b1001001, 0b0101010, 0b0011100, 0b0001000, 0b0001000, 0b0001000, 0]),
        'ω' | 'ώ' => (7, [0, 0, 0, 0, 0b0100010, 0b1001001, 0b1001001, 0b1001001, 0b1001001, 0b0110110, 0, 0, 0]),
        _ => (5, [0, 0, 0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0, 0, 0]),
    }
}

fn win31_serif_text_width(text: &str, bold: bool) -> i32 {
    let mut w = 0i32;
    let mut first = true;
    for ch in text.chars() {
        if !first {
            w += 1;
        }
        first = false;
        let (gw, _) = win31_serif_glyph(ch);
        w += gw as i32 + if bold && ch != ' ' { 1 } else { 0 };
    }
    w
}

fn clip_win31_serif_text(text: &str, bold: bool, max_w: i32) -> String {
    let mut out = String::new();
    let mut cur_w = 0i32;
    for ch in text.chars() {
        let (gw, _) = win31_serif_glyph(ch);
        let cw = gw as i32 + if bold && ch != ' ' { 1 } else { 0 };
        let sep = if out.is_empty() { 0 } else { 1 };
        if cur_w + sep + cw > max_w && !out.is_empty() {
            break;
        }
        cur_w += sep + cw;
        out.push(ch);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn draw_win31_serif_text(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    text: &str,
    bold: bool,
    color: [u8; 4],
) {
    let dw = w as i32;
    let dh = h as i32;
    let mut cur_x = x;
    for ch in text.chars() {
        let (gw, rows) = win31_serif_glyph(ch);
        for (ry, &mask) in rows.iter().enumerate() {
            if mask == 0 {
                continue;
            }
            let py = y + ry as i32;
            if py < 0 || py >= dh {
                continue;
            }
            for rx in 0..(gw as i32) {
                let bit = (gw as i32) - 1 - rx;
                if (mask & (1 << bit)) != 0 {
                    for bx in 0..=i32::from(bold && ch != ' ') {
                        let px = cur_x + rx + bx;
                        if px >= 0 && px < dw {
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
            }
        }
        cur_x += (gw as i32) + if bold && ch != ' ' { 2 } else { 1 };
    }
}

fn win31_heading_text_width(text: &str) -> i32 {
    let mut w = 0i32;
    let mut first = true;
    for ch in text.chars() {
        if !first {
            w += 1;
        }
        first = false;
        let (gw, _) = win31_serif_glyph(ch);
        w += gw as i32 + if ch != ' ' { 2 } else { 1 };
    }
    w
}

fn wrap_win31_heading_text(s: &str, max_w: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", cur, word)
        };
        if !cur.is_empty() && win31_heading_text_width(&cand) > max_w {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn draw_win31_heading_text(
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
        let (gw, rows) = win31_serif_glyph(ch);
        for (ry, &mask) in rows.iter().enumerate() {
            if mask == 0 {
                continue;
            }
            // Leicht vergrößerte 14pt-Höhe: obere Hälfte auf `y + ry`, untere Hälfte ab `ry >= 5` um +1px gestreckt
            let y_offsets: &[i32] = if ry == 5 { &[5, 6] } else if ry > 5 { &[ry as i32 + 1] } else { &[ry as i32] };
            for &yo in y_offsets {
                let py = y + yo;
                if py < 0 || py >= dh {
                    continue;
                }
                for rx in 0..(gw as i32) {
                    let bit = (gw as i32) - 1 - rx;
                    if (mask & (1 << bit)) != 0 {
                        for bx in 0..=i32::from(ch != ' ') {
                            let px = cur_x + rx + bx;
                            if px >= 0 && px < dw {
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
                }
            }
        }
        cur_x += (gw as i32) + if ch != ' ' { 3 } else { 2 };
    }
}

fn split_preserving_spaces(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_space = false;
    for ch in s.chars() {
        let is_sp = ch == ' ';
        if !cur.is_empty() && is_sp != in_space {
            out.push(std::mem::take(&mut cur));
        }
        in_space = is_sp;
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn blit_bmp_clipped(
    bmp: &BmpImage,
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    dst_x: i32,
    dst_y: i32,
    clip_x0: i32,
    clip_y0: i32,
    clip_x1: i32,
    clip_y1: i32,
) {
    for sy in 0..bmp.height as i32 {
        let fy = dst_y + sy;
        if fy < clip_y0 || fy >= clip_y1 || fy < 0 || fy >= dh as i32 {
            continue;
        }
        for sx in 0..bmp.width as i32 {
            let fx = dst_x + sx;
            if fx < clip_x0 || fx >= clip_x1 || fx < 0 || fx >= dw as i32 {
                continue;
            }
            let src_idx = ((sy as u32 * bmp.width + sx as u32) * 4) as usize;
            let dst_idx = ((fy as u32 * dw + fx as u32) * 4) as usize;
            if src_idx + 3 < bmp.data.len() && dst_idx + 3 < frame.len() {
                frame[dst_idx..dst_idx + 4].copy_from_slice(&bmp.data[src_idx..src_idx + 4]);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_help_scrollbar(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scroll: usize,
    visible: usize,
    total: usize,
) {
    fill_rect(frame, dw, dh, x, y, w, h, [212, 212, 212, 255]);
    draw_3d_panel(frame, dw, dh, x, y, w, 15, false, [192, 192, 192, 255]);
    draw_oem_text(frame, dw, dh, x + 4, y + 1, "^", [0, 0, 0, 255]);
    let bot_y = y + h as i32 - 15;
    draw_3d_panel(frame, dw, dh, x, bot_y, w, 15, false, [192, 192, 192, 255]);
    draw_oem_text(frame, dw, dh, x + 4, bot_y + 1, "v", [0, 0, 0, 255]);

    let track_h = (h as i32 - 30).max(16);
    let max_scroll = total.saturating_sub(visible);
    let thumb_h = if total <= visible {
        track_h
    } else {
        ((visible as i32 * track_h) / (total as i32)).clamp(16, track_h)
    };
    let thumb_y = if max_scroll == 0 {
        y + 15
    } else {
        y + 15 + ((scroll.min(max_scroll) as i32 * (track_h - thumb_h)) / max_scroll as i32)
    };
    draw_3d_panel(
        frame,
        dw,
        dh,
        x,
        thumb_y,
        w,
        thumb_h as u32,
        false,
        [192, 192, 192, 255],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::assets::EmbeddedAssets;

    #[test]
    fn test_winhelp_book_parses_all_topics_and_hotspots() {
        // Erzeuge (und aktualisiere) `help/help_de.html` und `help/help_en.html`, falls noch nicht vorhanden
        let de_topics = WinHelpBook::build_clean_topics_from_rtf(true);
        let en_topics = WinHelpBook::build_clean_topics_from_rtf(false);

        let html_de = export_topics_to_html(&de_topics, true);
        let html_en = export_topics_to_html(&en_topics, false);

        assert!(
            !html_de.contains("#$K+") && !html_de.contains("#$+") && !html_de.contains("#K+"),
            "help_de.html darf keinerlei #$K+ Steuerzeichen-Artefakte enthalten!"
        );
        assert!(
            !html_en.contains("#$K+") && !html_en.contains("#$+") && !html_en.contains("#K+"),
            "help_en.html darf keinerlei #$K+ Steuerzeichen-Artefakte enthalten!"
        );

        let manifest_help_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("help");
        let _ = fs::create_dir_all(&manifest_help_dir);
        let de_manifest_path = manifest_help_dir.join("help_de.html");
        let en_manifest_path = manifest_help_dir.join("help_en.html");
        // Wichtig: Nur anlegen, falls die Datei noch NICHT existiert, damit manuelle Änderungen
        // an `help/help_*.html` niemals durch `cargo test` überschrieben werden!
        if !de_manifest_path.exists() {
            let _ = fs::write(&de_manifest_path, &html_de);
        }
        if !en_manifest_path.exists() {
            let _ = fs::write(&en_manifest_path, &html_en);
        }
        if let Some(root_dir) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            let root_help_dir = root_dir.join("help");
            let _ = fs::create_dir_all(&root_help_dir);
            let de_root_path = root_help_dir.join("help_de.html");
            let en_root_path = root_help_dir.join("help_en.html");
            if !de_root_path.exists() {
                let _ = fs::write(&de_root_path, &html_de);
            }
            if !en_root_path.exists() {
                let _ = fs::write(&en_root_path, &html_en);
            }

            // Konvertiere auch alle weiteren Sprachen aus `legacy_src/help/` (FR, ES, IT, NL, GRK),
            // sofern die jeweilige HTML-Datei noch nicht existiert.
            let assets = EmbeddedAssets::load();
            let extra_langs = [
                ("fr", "FRE", "MODFRHLP.RTF"),
                ("es", "SPA", "MODSPHLP.RTF"),
                ("it", "ITA", "MODITHLP.RTF"),
                ("nl", "NL", "MODNLHLP.RTF"),
                ("grk", "GRK", "MODGKHLP.RTF"),
            ];
            for (code, subdir, rtf_file) in extra_langs {
                let rtf_path = root_dir
                    .join("legacy_src")
                    .join("help")
                    .join(subdir)
                    .join(rtf_file);
                if let Ok(raw_bytes) = fs::read(&rtf_path) {
                    let rtf_str: String = raw_bytes.iter().map(|&b| b as char).collect();
                    let mut topics = parse_modhelp_rtf(&rtf_str);
                    WinHelpBook::ensure_supplemental_topics(&mut topics, false);
                    let html = export_topics_to_html_for_lang(&topics, code, &assets.hotspots);
                    assert!(
                        !html.to_ascii_lowercase().contains("<style")
                            && !html.to_ascii_lowercase().contains("<script")
                            && !html.to_ascii_lowercase().contains("<section"),
                        "HTML 3.2 für '{}' darf kein <style>, <script> oder <section> enthalten!",
                        code
                    );
                    let fname = format!("help_{}.html", code);
                    let m_path = manifest_help_dir.join(&fname);
                    let r_path = root_help_dir.join(&fname);
                    if fs::read_to_string(&m_path)
                        .map(|s| !s.contains("<BASEFONT") || s.contains("<P><FONT FACE="))
                        .unwrap_or(true)
                    {
                        let _ = fs::write(&m_path, &html);
                    }
                    if fs::read_to_string(&r_path)
                        .map(|s| !s.contains("<BASEFONT") || s.contains("<P><FONT FACE="))
                        .unwrap_or(true)
                    {
                        let _ = fs::write(&r_path, &html);
                    }
                }
            }
        }

        // Prüfe, dass alle 7 per `build.rs` komprimiert eingebetteten HTML-Hilfedateien
        // im RAM entpackt und geparst werden können
        for code in ["de", "en", "fr", "es", "it", "nl", "grk"] {
            let decompressed = WinHelpBook::decompress_embedded_help_html(code)
                .unwrap_or_else(|| panic!("Komprimiertes Hilfe-HTML '{}' konnte nicht entpackt werden", code));
            let parsed = parse_help_html(&decompressed);
            assert!(
                parsed.len() >= 300,
                "Entpacktes Hilfe-HTML '{}' hat zu wenige Themen: {}",
                code,
                parsed.len()
            );
        }

        // Lade alle 7 Bücher über den HTML-Parser (`load_for_language`)
        let book_de = WinHelpBook::load_for_language(1);
        let book_en = WinHelpBook::load_for_language(0);
        let book_fr = WinHelpBook::load_for_language(2);
        let book_es = WinHelpBook::load_for_language(3);
        let book_it = WinHelpBook::load_for_language(4);
        let book_nl = WinHelpBook::load_for_language(5);
        let book_grk = WinHelpBook::load_for_language(6);

        // Falls `help_de.html` / `help_en.html` auf der Festplatte noch die alte zeilenweise `<P><FONT FACE=`-Struktur
        // verwenden, konvertiere sie einmalig verlustfrei in das kompakte `<BASEFONT>` + `<P>...<BR>...</P>`-Format.
        if fs::read_to_string(&de_manifest_path)
            .map(|s| !s.contains("<BASEFONT") || s.contains("<P><FONT FACE="))
            .unwrap_or(true)
        {
            let enriched_de = export_topics_to_html(&book_de.topics, true);
            let _ = fs::write(&de_manifest_path, &enriched_de);
            if let Some(root_dir) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
                let _ = fs::write(root_dir.join("help").join("help_de.html"), &enriched_de);
            }
        }
        if fs::read_to_string(&en_manifest_path)
            .map(|s| !s.contains("<BASEFONT") || s.contains("<P><FONT FACE="))
            .unwrap_or(true)
        {
            let enriched_en = export_topics_to_html(&book_en.topics, false);
            let _ = fs::write(&en_manifest_path, &enriched_en);
            if let Some(root_dir) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
                let _ = fs::write(root_dir.join("help").join("help_en.html"), &enriched_en);
            }
        }

        for (lang_label, book) in [
            ("DE", &book_de),
            ("EN", &book_en),
            ("FR", &book_fr),
            ("ES", &book_es),
            ("IT", &book_it),
            ("NL", &book_nl),
            ("GRK", &book_grk),
        ] {
            assert!(
                book.topics.len() >= 310,
                "[{}] Erwartet mindestens 310 Hilfethemen, gefunden: {}",
                lang_label,
                book.topics.len()
            );

            for v30_id in [
                "new_in_v30",
                "test_suite",
                "otvdm_setup",
                "accuracy_9997",
                "clean_mode",
                "Lizenz",
                "Lizenz_Hist",
            ] {
                let topic = book.get_topic(v30_id).unwrap_or_else(|| {
                    panic!("[{}] Thema '{}' fehlt im WinHelpBook", lang_label, v30_id)
                });
                assert!(
                    topic.is_full_page(),
                    "[{}] Thema '{}' sollte als Hauptseite erkannt werden",
                    lang_label,
                    v30_id
                );
                assert!(
                    !topic.blocks.is_empty(),
                    "[{}] Thema '{}' darf nicht leer sein",
                    lang_label,
                    v30_id
                );
            }
            for main_id in [
                "main_index",
                "HauptDlg",
                "MiniState",
                "SetupDlg",
                "OpenDlg",
                "InfoDlg",
                "InstrWindow",
                "EffWnd",
                "Effects",
                "keyseldlg",
                "ArcOptDlg",
                "arcparm",
                "fmdlg",
                "FselDlg",
                "QuickSelBox",
                "RecordingDlg",
                "RegisterDlg",
            ] {
                let topic = book.get_topic(main_id).unwrap_or_else(|| {
                    panic!("[{}] Hauptthema '{}' fehlt im WinHelpBook", lang_label, main_id)
                });
                assert!(
                    topic.is_full_page(),
                    "[{}] Thema '{}' sollte als Hauptseite erkannt werden",
                    lang_label,
                    main_id
                );
                assert!(
                    !topic.blocks.is_empty(),
                    "[{}] Thema '{}' darf nicht leer sein",
                    lang_label,
                    main_id
                );
            }

            // Prüfe, dass ALLE 266 Hotspots aus `hotspots.json` (`bm0`..`bm15`) ein gültiges Zielthema haben
            let assets = EmbeddedAssets::load();
            let mut total_hotspots = 0usize;
            for (bm_key, block) in &assets.hotspots {
                for hs in &block.hotspots {
                    total_hotspots += 1;
                    let topic = book.get_topic(&hs.id).unwrap_or_else(|| {
                        panic!(
                            "[{}] Hotspot '{}' in '{}' verweist auf unbekanntes Thema",
                            lang_label, hs.id, bm_key
                        )
                    });
                    assert!(
                        !topic.blocks.is_empty(),
                        "[{}] Hotspot-Thema '{}' in '{}' hat keinen Erklärungstext",
                        lang_label,
                        hs.id,
                        bm_key
                    );
                }
            }
            assert_eq!(total_hotspots, 266);
        }

        // Prüfe das `Datei` / `File`-Dropdown-Menü zum Umschalten der Sprache im Hilfe-Fenster:
        let assets = EmbeddedAssets::load();
        let mut viewer = HelpViewerState::new_with_language(0); // Startet mit Englisch (0)
        viewer.open_topic("HauptDlg");
        assert!(!viewer.file_menu_open);

        // Klick auf `&File` in der Menüleiste (vx = 20, vy = 28) öffnet das Menü
        assert_eq!(viewer.handle_click(20, 28, &assets), HelpClickResult::Handled);
        assert!(viewer.file_menu_open);

        // Klick auf `Deutsch` (Eintrag 0 im Dropdown bei vy = 48) schaltet auf Deutsch (1) um und hält `HauptDlg` offen
        assert_eq!(viewer.handle_click(40, 48, &assets), HelpClickResult::Handled);
        assert!(!viewer.file_menu_open);
        assert_eq!(viewer.book.language_id, 1);
        assert_eq!(viewer.current_topic_id, "HauptDlg");

        // Öffne erneut `&Datei` und klicke auf `Français` (Eintrag 2 bei vy = 80) -> schaltet auf Französisch (2)
        assert_eq!(viewer.handle_click(20, 28, &assets), HelpClickResult::Handled);
        assert_eq!(viewer.handle_click(40, 80, &assets), HelpClickResult::Handled);
        assert_eq!(viewer.book.language_id, 2);

        // Öffne erneut `&File` und klicke auf `Exit` (unterhalb der Trennlinie bei vy = 166) -> schließt das Hilfe-Fenster
        assert_eq!(viewer.handle_click(20, 28, &assets), HelpClickResult::Handled);
        assert_eq!(viewer.handle_click(40, 166, &assets), HelpClickResult::CloseWindow);
    }
}
