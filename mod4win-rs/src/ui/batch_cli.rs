//! Headless-Render-CLI (`--render-wav`) und RIFF-WAVE-Hilfsfunktionen.
//! Die erweiterte Multi-Core Batch-Diff-Suite (`--batch-diff`, `--batch-ffmpeg-diff`, `--batch-verify`)
//! ist im optionalen Feature-Modul `crate::diff_suite` (`--features diff-suite`) gekapselt.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::engine::loader::load_module_with_ext;
use crate::engine::types::DEV_HDD;

use super::app::UiSharedState;
use super::archive_mol::{extract_zip_entry_bytes, parse_zip_entries};
use super::ini_config::{export_module_to_wav_file, Mod4WinIniConfig};

#[cfg(feature = "diff-suite")]
pub use crate::diff_suite::{
    render_ffmpeg_reference_pcm, resolve_ffmpeg_binary, run_batch_ffmpeg_diff, run_batch_m4w_diff,
    run_batch_verify, BatchFfmpegDiffOptions, BatchFfmpegDiffReport, BatchM4wDiffOptions,
    BatchVerifyReport,
};

/// Optionale CLI-Überschreibungen für `--render-wav <input_mod> <output_wav>`.
#[derive(Clone, Debug, Default)]
pub struct RenderWavCliOptions {
    pub input_mod: String,
    pub output_wav: String,
    pub ido: Option<u8>,
    pub rate: Option<u32>,
    pub bits: Option<u8>,
    pub channels_mode: Option<String>,
    pub pan: Option<bool>,
    pub peak_search: Option<bool>,
}

/// Lädt die Rohbytes und Dateiendung eines Tracker-Moduls – entweder direkt von der Festplatte,
/// aus einem `.ZIP`-Archiv (`archiv.zip` -> erstes unterstütztes Modul) oder über die Notation
/// `archiv.zip::datei.xm` bzw. `archiv.zip/datei.xm`.
#[allow(clippy::type_complexity)]
pub fn load_input_module_bytes(
    input_spec: &str,
) -> Result<(Vec<u8>, Option<String>, String), Box<dyn std::error::Error>> {
    // 1. Prüfe explizite `zip::entry`-Notation
    if let Some((zip_part, entry_part)) = input_spec.split_once("::") {
        let zip_path = Path::new(zip_part);
        let zip_bytes = fs::read(zip_path)?;
        let entries = parse_zip_entries(&zip_bytes);
        let target = entries
            .iter()
            .find(|e| {
                e.file_name.eq_ignore_ascii_case(entry_part)
                    || e.full_name.eq_ignore_ascii_case(entry_part)
            })
            .ok_or_else(|| {
                format!(
                    "Eintrag '{}' nicht in ZIP '{}' gefunden",
                    entry_part, zip_part
                )
            })?;
        let raw = extract_zip_entry_bytes(&zip_bytes, target)
            .ok_or_else(|| format!("Konnte '{}' aus '{}' nicht entpacken", entry_part, zip_part))?;
        let ext = Path::new(&target.file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        return Ok((raw, ext, format!("{}::{}", zip_part, target.file_name)));
    }

    let p = Path::new(input_spec);
    if p.exists() && p.is_file() {
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        if ext.as_deref() == Some("zip") {
            let zip_bytes = fs::read(p)?;
            let entries = parse_zip_entries(&zip_bytes);
            let target = entries
                .iter()
                .find(|e| UiSharedState::is_tracker_file(Path::new(&e.file_name)))
                .ok_or_else(|| {
                    format!(
                        "Kein unterstütztes Tracker-Modul in ZIP '{}' gefunden",
                        p.display()
                    )
                })?;
            let raw = extract_zip_entry_bytes(&zip_bytes, target).ok_or_else(|| {
                format!(
                    "Konnte '{}' aus ZIP '{}' nicht entpacken",
                    target.file_name,
                    p.display()
                )
            })?;
            let entry_ext = Path::new(&target.file_name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase());
            return Ok((
                raw,
                entry_ext,
                format!("{}::{}", p.display(), target.file_name),
            ));
        }
        let raw = fs::read(p)?;
        return Ok((raw, ext, p.display().to_string()));
    }

    // 2. Falls `pfad/archiv.zip/datei.xm` übergeben wurde
    let mut anc = p.parent();
    while let Some(parent_path) = anc {
        if parent_path.is_file()
            && parent_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
            && let Ok(rel) = p.strip_prefix(parent_path) {
                let entry_part = rel.to_string_lossy();
                let zip_bytes = fs::read(parent_path)?;
                let entries = parse_zip_entries(&zip_bytes);
                if let Some(target) = entries.iter().find(|e| {
                    e.file_name.eq_ignore_ascii_case(&entry_part)
                        || e.full_name.eq_ignore_ascii_case(&entry_part)
                }) {
                    let raw = extract_zip_entry_bytes(&zip_bytes, target).ok_or_else(|| {
                        format!(
                            "Konnte '{}' aus '{}' nicht entpacken",
                            entry_part,
                            parent_path.display()
                        )
                    })?;
                    let ext = Path::new(&target.file_name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase());
                    return Ok((
                        raw,
                        ext,
                        format!("{}::{}", parent_path.display(), target.file_name),
                    ));
                }
            }
        anc = parent_path.parent();
    }

    Err(format!("Eingabedatei '{}' existiert nicht.", input_spec).into())
}

/// Führt den Headless-WAV-Render-Modus (`--render-wav <input_mod> <output_wav>`) mit maximaler
/// CPU-Geschwindigkeit über die bit-exakte `DEV_HDD`-Pipeline aus.
pub fn run_headless_render_wav(
    opts: &RenderWavCliOptions,
    base_ini: &Mod4WinIniConfig,
) -> Result<usize, Box<dyn std::error::Error>> {
    let t0 = Instant::now();
    let (raw_bytes, ext_opt, display_label) = load_input_module_bytes(&opts.input_mod)?;
    let file_size_bytes = raw_bytes.len() as u32;

    let loaded_module = load_module_with_ext(&raw_bytes, ext_opt.as_deref())
        .map_err(|code| format!("Fehler beim Laden von '{}' (Code {})", display_label, code))?;

    let mut cfg = base_ini.clone();
    cfg.device_id = DEV_HDD as i32;

    if let Some(ido) = opts.ido {
        cfg.over_sampling = ido.min(6);
    }
    if let Some(rate) = opts.rate {
        cfg.sample_rate = rate.clamp(11025, 48000);
    }
    if let Some(bits) = opts.bits {
        cfg.bits_per_sample = if bits == 8 { 8 } else { 16 };
    }
    if let Some(ref ch_mode) = opts.channels_mode {
        match ch_mode.trim().to_ascii_lowercase().as_str() {
            "1" | "mono" => {
                cfg.channels = 1;
                cfg.surround = false;
            }
            "2" | "stereo" => {
                cfg.channels = 2;
                cfg.surround = false;
            }
            "surround" | "sur" => {
                cfg.channels = 2;
                cfg.surround = true;
            }
            other => {
                return Err(format!(
                    "Ungültiger Wert für --channels '{}'. Erlaubt: mono, stereo, surround",
                    other
                )
                .into());
            }
        }
    }
    if let Some(pan) = opts.pan {
        cfg.enable_panning = pan;
    }
    if let Some(ps) = opts.peak_search {
        cfg.auto_preamp_peaksearch = ps;
    }

    let mod_name_len = (loaded_module.state.mi.mod_name_len as usize).min(40);
    let mod_title = String::from_utf8_lossy(&loaded_module.state.mi.mod_name[..mod_name_len])
        .trim()
        .to_string();

    let out_path = Path::new(&opts.output_wav);
    let rendered_frames =
        export_module_to_wav_file(loaded_module.clone(), file_size_bytes, &mut cfg, out_path)?;
    let elapsed_s = t0.elapsed().as_secs_f64().max(0.000_001);
    let audio_secs = rendered_frames as f64 / (cfg.sample_rate.max(1) as f64);
    let speedup = audio_secs / elapsed_s;

    println!("======================================================================");
    println!("MOD4WIN 2.40 Headless Direct-to-Disk Renderer (--render-wav)");
    println!("======================================================================");
    println!("Eingabe:     {} ({} Bytes)", display_label, file_size_bytes);
    let file_type = loaded_module.state.mi.file_type;
    let num_tracks = loaded_module.state.mi.num_tracks;
    println!(
        "Titel/Typ:   {} [{:?}, {} Ch]",
        if mod_title.is_empty() {
            "<Kein Titel>"
        } else {
            &mod_title
        },
        file_type,
        num_tracks
    );
    println!(
        "Pipeline:    {} Hz | {}-Bit | Ch={} (Surround={}) | Pan={} | IDO={} | PeakSearch={}",
        cfg.sample_rate,
        cfg.bits_per_sample,
        cfg.channels,
        cfg.surround,
        cfg.enable_panning,
        cfg.over_sampling,
        cfg.auto_preamp_peaksearch
    );
    println!(
        "Ausgabe:     {} ({} Frames / {:.2}s Audio in {:.3}s -> {:.1}x Echtzeit)",
        out_path.display(),
        rendered_frames,
        audio_secs,
        elapsed_s,
        speedup
    );
    println!("======================================================================");

    Ok(rendered_frames)
}

/// Geparste RIFF-WAVE-Daten inkl. Metadaten-Erkennung (`is_ffmpeg` via `LIST`/`INFO`/`Lavf`).
#[derive(Clone, Debug)]
pub struct ParsedWavData {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_ffmpeg: bool,
    pub samples: Vec<i16>,
}

/// Robuster Chunk-basierter RIFF-WAVE-Parser, der `LIST`/`INFO`/`JUNK`-Chunks (z. B. von `ffmpeg`)
/// sauber überspringt und 16-Bit Signed PCM-Daten extrahiert.
pub fn parse_wav_bytes_i16(raw: &[u8]) -> Result<ParsedWavData, String> {
    if raw.len() < 44 || &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
        return Err("Ungültiger RIFF/WAVE-Header".to_string());
    }
    let mut channels = 2u16;
    let mut sample_rate = 44100u32;
    let mut bits_per_sample = 16u16;
    let mut is_ffmpeg = false;
    let mut samples = Vec::new();
    let mut found_data = false;

    let mut pos = 12usize;
    while pos + 8 <= raw.len() {
        let id = &raw[pos..pos + 4];
        let sz =
            u32::from_le_bytes([raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]]) as usize;
        let chunk_start = pos + 8;
        let chunk_end = (chunk_start + sz).min(raw.len());

        if id == b"fmt " && chunk_end >= chunk_start + 16 {
            channels = u16::from_le_bytes([raw[chunk_start + 2], raw[chunk_start + 3]]);
            sample_rate = u32::from_le_bytes([
                raw[chunk_start + 4],
                raw[chunk_start + 5],
                raw[chunk_start + 6],
                raw[chunk_start + 7],
            ]);
            bits_per_sample = u16::from_le_bytes([raw[chunk_start + 14], raw[chunk_start + 15]]);
        } else if id == b"LIST" {
            let slice = &raw[chunk_start..chunk_end];
            if slice.windows(4).any(|w| w == b"Lavf" || w == b"Lavc") {
                is_ffmpeg = true;
            }
        } else if id == b"data" {
            let payload = &raw[chunk_start..chunk_end];
            if bits_per_sample == 8 {
                samples.reserve(payload.len());
                for &b in payload {
                    samples.push(((b as i16) - 128) << 8);
                }
            } else {
                samples.reserve(payload.len() / 2);
                for chunk in payload.as_chunks::<2>().0 {
                    samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                }
            }
            found_data = true;
            break;
        }
        pos = chunk_start.saturating_add(sz).saturating_add(sz & 1);
    }

    if !found_data {
        return Err("Kein 'data'-Chunk in WAV-Datei gefunden".to_string());
    }

    Ok(ParsedWavData {
        sample_rate,
        channels,
        bits_per_sample,
        is_ffmpeg,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headless_render_cli() {
        let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Workspace root");
        let iface_dir = root_dir.join("legacy_src").join("IFACE32");
        let tmp_dir = std::env::temp_dir().join("mod4win_test_headless_render");
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();

        #[cfg(feature = "diff-suite")]
        {
            let report_path = tmp_dir.join("batch_verify_report.json");
            let rep = run_batch_verify(&iface_dir, Some(&report_path)).expect("batch verify");
            assert!(rep.total_files >= 1);
            assert_eq!(rep.failed_files, 0);
            assert!(report_path.exists());
        }

        // Prüfe Headless-WAV-Render mit NO-SN.XM
        let nosn_path = iface_dir.join("NO-SN.XM");
        let out_wav = tmp_dir.join("nosn_test.wav");
        let opts = RenderWavCliOptions {
            input_mod: nosn_path.to_string_lossy().into_owned(),
            output_wav: out_wav.to_string_lossy().into_owned(),
            ido: Some(1),
            rate: Some(22050),
            bits: Some(16),
            channels_mode: Some("stereo".to_string()),
            pan: Some(true),
            peak_search: Some(true),
        };
        let frames =
            run_headless_render_wav(&opts, &Mod4WinIniConfig::default()).expect("render-wav");
        assert!(frames > 1000);
        assert!(out_wav.exists());

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
