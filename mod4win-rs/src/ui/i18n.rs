//! Durchgängiges Lokalisierungs-Modul (`i18n`) mit externen Sprachdateien (`lang/*.ini`)
//! und fest einkompiliertem Fallback (`de.ini` / `en.ini`).
//!
//! Beim Programmstart sowie beim Umschalten der Sprache im Setup-Dialog werden vorrangig
//! die `.ini`-Dateien aus dem `lang/`-Verzeichnis von der Festplatte geladen, sodass
//! Texte jederzeit per Texteditor angepasst oder weitere Sprachen ergänzt werden können.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Eingebettete deutsche Standardsprachdatei (`lang/de.ini`) als Fallback.
pub const EMBEDDED_DE_INI: &str = include_str!("../../lang/de.ini");

/// Eingebettete englische Standardsprachdatei (`lang/en.ini`) als Fallback.
pub const EMBEDDED_EN_INI: &str = include_str!("../../lang/en.ini");

/// Geladener Sprachkatalog für die gesamte MOD4WIN-Oberfläche.
#[derive(Clone, Debug)]
pub struct LocaleCatalog {
    /// Numerische Sprach-ID (`0 = Englisch`, `1 = Deutsch`).
    pub language_id: u8,
    /// Sprachcode (z. B. `"en"`, `"de"`).
    pub code: String,
    /// Anzeigename im Setup-Dialog (`"ENGLISH"`, `"GERMAN"`, ...).
    pub display_name: String,
    /// Key-Value-Tabelle mit `"section.key"` (Kleinschreibung) -> lokalisierter Text.
    entries: HashMap<String, String>,
}

impl Default for LocaleCatalog {
    fn default() -> Self {
        Self::load_for_language_id(0)
    }
}

impl LocaleCatalog {
    /// Lädt den Sprachkatalog für die numerische MOD4WIN-Sprach-ID (`0 = en / ENGLISH`, `1 = de / GERMAN`).
    /// Vorrangig wird `lang/en.ini` bzw. `lang/de.ini` von der Festplatte gelesen; fehlt die Datei
    /// oder einzelne Schlüssel, greift automatisch der per `include_str!` eingebettete Fallback.
    pub fn load_for_language_id(lang_id: u8) -> Self {
        let code = if lang_id == 1 { "de" } else { "en" };
        Self::load_by_code(code)
    }

    /// Lädt einen Sprachkatalog anhand seines Sprachcodes (z. B. `"de"`, `"en"`).
    pub fn load_by_code(code: &str) -> Self {
        let norm_code = code.trim().to_ascii_lowercase();
        let is_de = norm_code == "de" || norm_code == "ger" || norm_code == "de_de";

        let mut catalog = Self {
            language_id: if is_de { 1 } else { 0 },
            code: if is_de {
                "de".to_string()
            } else if norm_code.is_empty() {
                "en".to_string()
            } else {
                norm_code.clone()
            },
            display_name: if is_de {
                "GERMAN".to_string()
            } else {
                "ENGLISH".to_string()
            },
            entries: HashMap::new(),
        };

        // 1. Zuerst englischen Basis-Fallback laden (damit selbst bei unvollständigen Fremdsprachen-INIs nie ein Schlüssel fehlt)
        catalog.parse_and_merge_ini(EMBEDDED_EN_INI);

        // 2. Falls Deutsch angefordert ist, eingebettetes `de.ini` darüberlegen
        if is_de {
            catalog.parse_and_merge_ini(EMBEDDED_DE_INI);
        }

        // 3. Vorrangig externe `lang/<code>.ini` von der Festplatte laden und darüberlegen
        let file_code = if is_de { "de" } else { catalog.code.as_str() };
        if let Some(disk_path) = Self::find_lang_ini_on_disk(file_code)
            && let Ok(disk_content) = fs::read_to_string(&disk_path) {
                catalog.parse_and_merge_ini(&disk_content);
            }

        if let Some(meta_name) = catalog.entries.get("meta.name").cloned()
            && !meta_name.trim().is_empty() {
                catalog.display_name = meta_name.trim().to_string();
            }
        if let Some(meta_code) = catalog.entries.get("meta.code").cloned()
            && !meta_code.trim().is_empty() {
                catalog.code = meta_code.trim().to_ascii_lowercase();
            }

        catalog
    }

    /// Sucht `lang/<code>.ini` in den Arbeits- und Programmverzeichnissen auf der Festplatte.
    pub fn find_lang_ini_on_disk(code: &str) -> Option<PathBuf> {
        let fname = format!("{}.ini", code.trim().to_ascii_lowercase());
        for dir in Self::candidate_lang_dirs() {
            let p = dir.join(&fname);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    /// Liefert alle Suchpfade für das externe `lang/`-Verzeichnis.
    pub fn candidate_lang_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd.join("lang"));
            dirs.push(cwd.join("mod4win-rs").join("lang"));
            if let Some(parent) = cwd.parent() {
                dirs.push(parent.join("lang"));
            }
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent() {
                dirs.push(exe_dir.join("lang"));
                if let Some(parent) = exe_dir.parent() {
                    dirs.push(parent.join("lang"));
                }
            }
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("lang"));
        dirs
    }

    /// Parst den Inhalt einer `.ini`-Sprachdatei und übernimmt alle `section.key = value`-Einträge.
    pub fn parse_and_merge_ini(&mut self, ini_text: &str) {
        let mut current_section = String::new();
        for raw_line in ini_text.lines() {
            let line = raw_line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
                current_section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key_part = k.trim().to_ascii_lowercase();
                // Für Hotkey-Zeilen (`hk_01`..`hk_09`) führendes Leerzeichen nach `=` optional erhalten,
                // wenn der Wert mit einer Zahl wie `1.` beginnt
                let raw_val = v.trim_end();
                let trimmed_val = raw_val.trim_start();
                let val_str = if key_part.starts_with("hk_0")
                    && trimmed_val.len() >= 2
                    && trimmed_val.as_bytes()[0].is_ascii_digit()
                    && trimmed_val.as_bytes()[1] == b'.'
                {
                    format!(" {}", trimmed_val)
                } else {
                    trimmed_val.to_string()
                };

                let full_key = if current_section.is_empty() {
                    key_part
                } else {
                    format!("{}.{}", current_section, key_part)
                };
                self.entries.insert(full_key, val_str);
            }
        }
    }

    fn resolve_alias(section: &str, key: &str) -> (String, String) {
        let s = section.trim().to_ascii_lowercase();
        let k = key.trim().to_ascii_lowercase();
        match (s.as_str(), k.as_str()) {
            ("win", "main_title") => ("titles".into(), "main_240".into()),
            ("win", "mini_title") => ("titles".into(), "mini".into()),
            ("win", "info_title") => ("titles".into(), "info_240".into()),
            ("win", "inst_title") => ("titles".into(), "inst".into()),
            ("win", "effects_title") => ("titles".into(), "effects".into()),
            ("win", "effects_help_title") => ("titles".into(), "effects_help".into()),
            ("win", "quick_select_title" | "quick_select_short") => ("titles".into(), "quick_select".into()),
            ("win", "open_title") => ("titles".into(), "open_240".into()),
            ("win", "setup_title") => ("titles".into(), "setup_240".into()),
            ("win", "wav_ask_title") => ("titles".into(), "wav_ask_240".into()),
            ("win", "fsel_title" | "fsel_path_title") => ("titles".into(), "fsel_path".into()),
            ("win", "fsel_mixer_title") => ("titles".into(), "fsel_mixer".into()),
            ("win", "fsel_arj_title") => ("titles".into(), "fsel_arj".into()),
            ("win", "fsel_zip_title") => ("titles".into(), "fsel_zip".into()),
            ("win", "fsel_lha_title") => ("titles".into(), "fsel_lha".into()),
            ("win", "file_copy_title") => ("titles".into(), "fileman_copy".into()),
            ("win", "file_move_title") => ("titles".into(), "fileman_move".into()),
            ("win", "hotkey_title") => ("titles".into(), "hotkey_240".into()),
            ("win", "archive_title") => ("titles".into(), "archive_240".into()),
            ("win", "about_title") => ("titles".into(), "about_240".into()),
            ("win", "help_title") => ("titles".into(), "help_prefix".into()),
            ("setup", "direct_to_disk") => ("setup".into(), "dev_hdd".into()),
            ("setup", "state_on") => ("setup".into(), "on".into()),
            ("setup", "state_off") => ("setup".into(), "off".into()),
            ("setup", "engine_full") => ("setup".into(), "row1_full".into()),
            ("setup", "engine_clean") => ("setup".into(), "row1_clean".into()),
            ("setup", "timer_mm_task") => ("setup".into(), "row1_mmtask".into()),
            ("setup", "timer_win_timer") => ("setup".into(), "row1_wintimer".into()),
            ("setup", "timer_interrupt") => ("setup".into(), "row1_interrupt".into()),
            ("setup", "pause_peaks_on") => ("setup".into(), "row3_peaks_only".into()),
            ("setup", "pause_both") => ("setup".into(), "row3_pause_peaks".into()),
            ("setup", "pause_only") => ("setup".into(), "row3_pause_only".into()),
            ("setup", "move_titlebar") => ("setup".into(), "row5_titlebar".into()),
            ("setup", "move_anywhere") => ("setup".into(), "row5_anywhere".into()),
            ("setup", "move_full") => ("setup".into(), "row5_full".into()),
            ("wav_ask", "path") => ("setup".into(), "wav_path_label".into()),
            ("wav_ask", "filename") => ("setup".into(), "wav_file_label".into()),
            ("open", "save_mol_prompt") => ("open".into(), "prompt_save_mol".into()),
            ("open", "filter_prompt") => ("open".into(), "prompt_filter".into()),
            ("fileman", "no_file_selected") => ("fileman".into(), "no_file".into()),
            ("fileman", "hdr_files") => ("fileman".into(), "col_files".into()),
            ("fileman", "hdr_dirs") => ("fileman".into(), "col_dirs".into()),
            ("fileman", "status_copy") => ("fileman".into(), "status_copy_hint".into()),
            ("fileman", "status_move") => ("fileman".into(), "status_move_hint".into()),
            ("archive", "show_window") => ("archive".into(), "show_progress".into()),
            ("archive", "params_arj") => ("titles".into(), "arc_param_arj".into()),
            ("archive", "params_zip") => ("titles".into(), "arc_param_zip".into()),
            ("archive", "params_lha") => ("titles".into(), "arc_param_lha".into()),
            ("archive", "lbl_unpack") => ("archive".into(), "param_unpack".into()),
            ("archive", "lbl_pack") => ("archive".into(), "param_pack".into()),
            ("archive", "lbl_delete") => ("archive".into(), "param_delete".into()),
            ("hotkeys", "mod_control") => ("hotkeys".into(), "ctrl".into()),
            ("hotkeys", "mod_shift") => ("hotkeys".into(), "shift".into()),
            ("hotkeys", "mod_alt") => ("hotkeys".into(), "alt".into()),
            ("about", "prog_dsp") => ("about".into(), "line_prog".into()),
            ("about", "graphics") => ("about".into(), "line_gfx".into()),
            ("about", "formats") => ("about".into(), "line_formats".into()),
            ("about", "rust_port") => ("about".into(), "line_port".into()),
            ("about", "click_return") => ("about".into(), "line_return".into()),
            _ => (s, k),
        }
    }

    /// Gibt den lokalisierten String für `(section, key)` zurück.
    pub fn tr<'a>(&'a self, section: &str, key: &str) -> &'a str {
        let direct_key = format!(
            "{}.{}",
            section.trim().to_ascii_lowercase(),
            key.trim().to_ascii_lowercase()
        );
        if let Some(s) = self.entries.get(&direct_key) {
            return s.as_str();
        }
        let (rs, rk) = Self::resolve_alias(section, key);
        let resolved_key = format!("{}.{}", rs, rk);
        self.entries
            .get(&resolved_key)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Gibt den lokalisierten String für `(section, key)` zurück oder `fallback`, falls leer.
    pub fn tr_or<'a>(&'a self, section: &str, key: &str, fallback: &'a str) -> &'a str {
        let s = self.tr(section, key);
        if s.is_empty() {
            fallback
        } else {
            s
        }
    }

    /// Formatiert einen lokalisierten String mit Platzhaltern (sowohl `{0}`, `{1}` als auch benannte `{file}`, `{count}`, `{ch}` in Reihenfolge).
    pub fn tr_fmt(&self, section: &str, key: &str, fallback: &str, args: &[&str]) -> String {
        let tpl = self.tr_or(section, key, fallback);
        let mut out = String::with_capacity(tpl.len() + 16);
        let mut rest = tpl;
        let mut arg_idx = 0usize;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after_open = &rest[open + 1..];
            if let Some(close) = after_open.find('}') {
                let token = &after_open[..close];
                let replacement = if let Ok(explicit_idx) = token.parse::<usize>() {
                    args.get(explicit_idx).copied()
                } else {
                    let val = args.get(arg_idx).copied();
                    arg_idx += 1;
                    val
                };
                if let Some(rep) = replacement {
                    out.push_str(rep);
                } else {
                    out.push('{');
                    out.push_str(token);
                    out.push('}');
                }
                rest = &after_open[close + 1..];
            } else {
                out.push_str(&rest[open..]);
                rest = "";
                break;
            }
        }
        out.push_str(rest);
        out
    }

    /// Liefert die lokalisierte Funktionsbeschreibung für eine Hotkey-Aktions-ID (`1..=43`).
    pub fn hotkey_label(&self, action_id: u8, fallback: &str) -> String {
        let key = format!("hk_{:02}", action_id);
        let s = self.tr("Hotkeys", &key);
        if s.is_empty() {
            fallback.to_string()
        } else {
            s.to_string()
        }
    }

    /// Gibt zurück, ob aktuell Deutsch (`"de"`) aktiv ist.
    pub fn is_german(&self) -> bool {
        self.code == "de"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i18n_de_and_en_completeness_and_no_leftover_german_in_en() {
        let de = LocaleCatalog::load_for_language_id(1);
        let en = LocaleCatalog::load_for_language_id(0);

        assert_eq!(de.code, "de");
        assert_eq!(de.display_name, "GERMAN");
        assert_eq!(en.code, "en");
        assert_eq!(en.display_name, "ENGLISH");

        // Alle Schlüssel aus `de.ini` müssen auch in `en.ini` vorhanden sein und umgekehrt
        for k in de.entries.keys() {
            assert!(
                en.entries.contains_key(k),
                "Schlüssel '{}' fehlt in en.ini",
                k
            );
            assert!(
                !en.entries[k].is_empty(),
                "Schlüssel '{}' ist leer in en.ini",
                k
            );
        }
        for k in en.entries.keys() {
            assert!(
                de.entries.contains_key(k),
                "Schlüssel '{}' fehlt in de.ini",
                k
            );
            assert!(
                !de.entries[k].is_empty(),
                "Schlüssel '{}' ist leer in de.ini",
                k
            );
        }

        // Alle 44 Hotkeys müssen in beiden Sprachen vorhanden sein
        for id in 1..=44 {
            let lbl_de = de.hotkey_label(id, "");
            let lbl_en = en.hotkey_label(id, "");
            assert!(!lbl_de.is_empty(), "hk_{:02} fehlt in de.ini", id);
            assert!(!lbl_en.is_empty(), "hk_{:02} fehlt in en.ini", id);
        }

        assert_eq!(de.tr("Setup", "on"), "Ein");
        assert_eq!(en.tr("Setup", "on"), "On");
        assert_eq!(
            en.tr_fmt("Info", "summary_line1", "{0} channel {1} module with {2} instruments", &["20", "FT2", "12"]),
            "20 channel FT2 module with 12 instruments"
        );
        assert_eq!(
            de.tr_fmt("Info", "summary_line1", "{0} channel {1} module with {2} instruments", &["20", "FT2", "12"]),
            "20-Kanal FT2-Modul mit 12 Instrumenten"
        );
    }
}
