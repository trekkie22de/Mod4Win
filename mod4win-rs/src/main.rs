#![cfg_attr(
    all(target_os = "windows", not(feature = "diff-suite")),
    windows_subsystem = "windows"
)]
#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::field_reassign_with_default
)]

use std::env;
#[cfg(not(feature = "retro-win32"))]
use std::fs;
use std::path::Path;
#[cfg(not(feature = "retro-win32"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(not(feature = "retro-win32"))]
use std::time::Duration;

#[cfg(not(feature = "retro-win32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(feature = "retro-win32"))]
use cpal::{SampleFormat, StreamConfig};

#[cfg(feature = "diff-suite")]
use mod4win_rs::diff_suite::{
    run_batch_ffmpeg_diff, run_batch_m4w_diff, run_batch_verify, run_diff_suite,
    BatchFfmpegDiffOptions, BatchM4wDiffOptions,
};
use mod4win_rs::engine::loader::{load_module_with_ext, LoadedModule};
use mod4win_rs::engine::sequencer::Sequencer;
use mod4win_rs::engine::types::{ModType, Note, SoundSettings, MAX_M4W_CHANNELS};
#[cfg(not(feature = "retro-win32"))]
use mod4win_rs::ui::{handle_audio_cycle_and_sync, run_ui, LiveAudioProcessor};
#[cfg(feature = "retro-win32")]
#[allow(unused_imports)]
use mod4win_rs::win32s_shim as _;
use mod4win_rs::ui::{
    compute_module_total_time_ms_on_seq, run_headless_render_wav, run_peak_search_isolated,
    CpuProfile, Mod4WinIniConfig, RenderWavCliOptions, SkinMode, UiSharedState,
};

/// Verbindet im reinen Player-Build (`windows_subsystem = "windows"`) bei Übergabe von
/// Kommandozeilen-Flags (`--help`, `--render-wav`, `--console` etc.) automatisch `stdout`/`stderr`
/// mit der aufrufenden Eltern-Konsole (PowerShell/CMD), ohne beim Doppelklick im Explorer
/// ein störendes Konsolenfenster im Hintergrund zu öffnen.
#[cfg(all(
    target_os = "windows",
    not(feature = "diff-suite"),
    not(feature = "retro-win32")
))]
fn attach_parent_console_if_needed(args: &[String]) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn AllocConsole() -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;

    if args.len() > 1 {
        unsafe {
            let attached = AttachConsole(ATTACH_PARENT_PROCESS);
            let wants_console = args
                .iter()
                .any(|a| a == "--no-gui" || a == "--console" || a == "-c");
            if attached == 0 && wants_console {
                let _ = AllocConsole();
            }
        }
    }
}

fn create_idle_module() -> LoadedModule {
    let mut m = LoadedModule::default();
    m.patterns = vec![vec![[Note::default(); MAX_M4W_CHANNELS]; 64]];
    m.state.mi.num_tracks = 4;
    m.state.mi.num_patterns = 1;
    m.state.mi.diff_patterns = 1;
    m.state.init_speed = 6;
    m.state.init_bpm_speed = 125;
    m.state.speed = 6;
    m.state.bpm_speed = 125;
    m
}

fn print_cli_help(bin_name: &str) {
    println!(
        "\
======================================================================
MOD4WIN 3.10 - 100% Bit-Exact Rust Port (Audio Engine & UI)
======================================================================

VERWENDUNG:
  {bin} [<modul_datei>] [OPTIONEN]

ALLGEMEINE & UI-OPTIONEN:
  -h, --help                     Zeigt diese Hilfe-Übersicht an und beendet das Programm
  --skin <2.40|modern>           Startet mit dem gewählten UI-Skin
  --cpu-sim <profil>             Historisches CPU-Profil aktivieren:
                                 native, 386sx16, 386dx33, 486dx33, 486dx2-66,
                                 486dx4-100, p90, k5-pr133, p133, k6-200, k6-2-450
  --authentic-stutter            Echtes Audio-Stottern bei >100% CPU-Last erzwingen
  -p, --seek-pattern <N>         Startet die Wiedergabe direkt bei Order/Pattern N (1-basiert)
  -c, --no-gui, --console        Startet im reinen Konsolenmodus ohne grafische Oberfläche
  --classic-input                Klassisches 1995er Mausverhalten im Open-Dialog

HEADLESS WAV-EXPORT (DIRECT-TO-DISK OHNE GUI):
  {bin} --render-wav <input_mod> <output.wav> [EXPORT-OPTIONEN]

EXPORT-OPTIONEN:
  -ido, --ido <0..6>             Oversampling / Interpolation:
                                   0 = Off (Kein Oversampling)
                                   1 = Linear (fOverSamp)
                                   2 = Kubisch CPU (fCPUCubic, 16-Bit Integer-LUT)
                                   3 = Kubisch FPU (fFPUCubic, 80-Bit x87/f64)
                                   4 = IT2 No-Interp + Resonanzfilter (nur .IT/.UMX)
                                   5 = IT2 Linear + Resonanzfilter (nur .IT/.UMX)
                                   6 = IT2 Kubisch + Resonanzfilter (nur .IT/.UMX)
  --rate <11025..48000>          Abtastrate in Hz (Standard: 44100)
  --bits <8|16>                  Auflösung in Bit (Standard: 16)
  --channels <mono|stereo|surround> Kanalmodus (Standard: stereo)
  --pan <0|1>                    Panning aktivieren/deaktivieren (Standard: 1)
  --peak-search <0|1>            Exakten Vorab-PeakSearch ausführen (Standard: 1)",
        bin = bin_name
    );

    println!(
        "\n\
======================================================================
DIFF-SUITE (KOMPILIERT via `--features diff-suite`):
======================================================================

VOLLAUTOMATISCHER MULTI-CORE BATCH-DIFF (INKL. AUTO-PATCHER & RAM-DIFF):
  {bin} --batch-diff <mod_quellordner> [<ergebnis_ordner>] [BATCH-OPTIONEN]
    --m4w-dir <ordner>           Ordner mit originaler MOD4WIN.EXE & PLAYER32.DLL
                                 (patcht sich automatisch MOD4WIN_IDO0..3.EXE zusammen)
    --otvdm <pfad>               Pfad zu otvdmw.exe (unter Windows)
    --wine <pfad>                Pfad zu wine (unter Linux)
    --ffmpeg <pfad>              Pfad zu ffmpeg (automatischer Wechsel bei .IT/.UMX)
    --work-dir <ordner>          Kurzes Temp-/RAM-Disk-Verzeichnis (z.B. R:\\ oder /dev/shm)
    -j, --workers, --threads <N> Anzahl paralleler Worker-Prozesse
    --ido <0,1,2,3>              Zu testende IDO-Modi (Standard: 0,1,2,3)
    --retry-fails                Bisherige fail/*.TXT im Zielordner erneut prüfen
    --retry-timeouts             Bisherige timeout/*.TXT im Zielordner erneut prüfen
    --allow-1lsb                 Erlaubt 1-LSB-Rundungsabweichungen als PASS
    --max-idle-sec <N>           Watchdog: 0%-CPU-Timeout bei defekter Datei (Standard: 6)
    --max-peak-sec <N>           Watchdog: Max. Sekunden in PeakSearch (Standard: 600)
    --max-stall-sec <N>          Watchdog: Max. Sekunden ohne WAV-Wachstum (Standard: 25)
    --max-wav-mb <N>             Watchdog: Max. WAV-Größe in MB (Standard: 350)

EINZELMODUL-DIAGNOSE (--diff-suite):
  1) Direktvergleich gegen FFmpeg (libopenmpt) im RAM:
     {bin} <modul_datei> --diff-suite ffmpeg [--ido <0..6>]
  2) Einzelvergleich gegen vorhandene Referenz-WAV (MOD4WIN.EXE oder FFmpeg):
     {bin} <modul_datei> --diff-suite <referenz.wav> [--ido <0..6>]
  3) Komplette 3-WAV-Suite (prüft IDO 0, IDO 1, IDO 2 und IDO 3 in einem Lauf):
     {bin} <modul_datei> --diff-suite <ido_off.wav> <ido_lin.wav> <ido_cub.wav>

MULTI-CORE FFMPEG BATCH-DIFF & ORDNER-VERIFIKATION:
  {bin} --batch-ffmpeg-diff <ordner_pfad> [--ref-dir <wav_cache>] [--threads <N>]
        [--max-secs <N>] [--ffmpeg <ffmpeg.exe>] [--report <diff_report.json>]
  {bin} --batch-verify <ordner_pfad> [--report <ergebnis.json>]",
        bin = bin_name
    );
    println!();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(feature = "retro-win32", target_arch = "x86"))]
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{}", info);
        mod4win_rs::win32s_shim::show_retro_panic_and_exit(&msg);
    }));

    let args: Vec<String> = env::args().collect();

    #[cfg(all(
        target_os = "windows",
        not(feature = "diff-suite"),
        not(feature = "retro-win32")
    ))]
    attach_parent_console_if_needed(&args);

    let mut file_arg: Option<String> = None;
    let mut no_gui = false;
    let mut cli_skin_mode: Option<SkinMode> = None;
    let mut classic_input = false;
    let mut cli_cpu_sim: Option<CpuProfile> = None;
    let mut cli_authentic_stutter = false;
    let mut seek_pattern: Option<u16> = None;
    let mut render_wav_pair: Option<(String, String)> = None;
    let mut render_wav_opts = RenderWavCliOptions::default();

    #[cfg(feature = "diff-suite")]
    let mut diff_suite_wavs: Option<Vec<String>> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_m4w_diff_pair: Option<(String, String)> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_m4w_opts = BatchM4wDiffOptions::default();
    #[cfg(feature = "diff-suite")]
    let mut batch_idos_override: Option<Vec<u8>> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_verify_folder: Option<String> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_report_path: Option<String> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_ffmpeg_diff_folder: Option<String> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_ref_dir: Option<String> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_ffmpeg_bin: Option<String> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_threads: Option<usize> = None;
    #[cfg(feature = "diff-suite")]
    let mut batch_max_secs: Option<u32> = None;

    let mut idx = 1usize;
    while idx < args.len() {
        let arg = &args[idx];
        if arg == "--help" || arg == "-h" || arg == "-help" || arg == "/?" || arg == "-?" {
            print_cli_help(&args[0]);
            return Ok(());
        } else if arg == "--no-gui" || arg == "--console" || arg == "-c" {
            no_gui = true;
        } else if arg == "--classic-input" || arg == "--classic" {
            classic_input = true;
        } else if arg == "--authentic-stutter" {
            cli_authentic_stutter = true;
        } else if arg == "--cpu-sim" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --cpu-sim <profilname>".into());
            }
            let val = &args[idx + 1];
            let prof = CpuProfile::parse_str(val).ok_or_else(|| {
                format!(
                    "Unbekanntes CPU-Profil '{}'. Verfügbar: native, 386sx16, 386dx33, 486dx33, 486dx2-66, 486dx4-100, p90, k5-pr133, p133, k6-200",
                    val
                )
            })?;
            cli_cpu_sim = Some(prof);
            idx += 1;
        } else if let Some(val) = arg.strip_prefix("--cpu-sim=") {
            let prof = CpuProfile::parse_str(val).ok_or_else(|| {
                format!(
                    "Unbekanntes CPU-Profil '{}'. Verfügbar: native, 386sx16, 386dx33, 486dx33, 486dx2-66, 486dx4-100, p90, k5-pr133, p133, k6-200",
                    val
                )
            })?;
            cli_cpu_sim = Some(prof);
        } else if arg == "--skin" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --skin <2.40|modern>".into());
            }
            let val = &args[idx + 1];
            cli_skin_mode = Some(SkinMode::parse_str(val).ok_or_else(|| {
                format!("Unbekannter Skin '{}'. Verfügbar: 2.40, modern", val)
            })?);
            idx += 1;
        } else if let Some(val) = arg.strip_prefix("--skin=") {
            cli_skin_mode = Some(SkinMode::parse_str(val).ok_or_else(|| {
                format!("Unbekannter Skin '{}'. Verfügbar: 2.40, modern", val)
            })?);
        } else if arg == "--render-wav" {
            if idx + 2 >= args.len() {
                return Err("Verwendung: --render-wav <input_mod> <output_wav>".into());
            }
            render_wav_pair = Some((args[idx + 1].clone(), args[idx + 2].clone()));
            idx += 2;
        } else if arg == "--ido" || arg == "-ido" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --ido/-ido <0..6>".into());
            }
            let raw_val = &args[idx + 1];
            if raw_val.contains(',') {
                #[cfg(feature = "diff-suite")]
                {
                    let mut list = Vec::new();
                    for part in raw_val.split(',') {
                        let v: u8 = part
                            .trim()
                            .parse()
                            .map_err(|_| format!("Ungültiger Wert in --ido: '{}'", raw_val))?;
                        list.push(v.min(6));
                    }
                    if let Some(&first) = list.first() {
                        render_wav_opts.ido = Some(first);
                    }
                    batch_idos_override = Some(list);
                }
                #[cfg(not(feature = "diff-suite"))]
                {
                    return Err(format!("Ungültiger Wert für --ido: '{}'", raw_val).into());
                }
            } else {
                let v: u8 = raw_val
                    .parse()
                    .map_err(|_| format!("Ungültiger Wert für --ido: '{}'", raw_val))?;
                render_wav_opts.ido = Some(v.min(6));
                #[cfg(feature = "diff-suite")]
                {
                    batch_idos_override = Some(vec![v.min(6)]);
                }
            }
            idx += 1;
        } else if let Some(val) = arg.strip_prefix("--ido=").or_else(|| arg.strip_prefix("-ido=")) {
            if val.contains(',') {
                #[cfg(feature = "diff-suite")]
                {
                    let mut list = Vec::new();
                    for part in val.split(',') {
                        let v: u8 = part
                            .trim()
                            .parse()
                            .map_err(|_| format!("Ungültiger Wert in --ido: '{}'", val))?;
                        list.push(v.min(6));
                    }
                    if let Some(&first) = list.first() {
                        render_wav_opts.ido = Some(first);
                    }
                    batch_idos_override = Some(list);
                }
                #[cfg(not(feature = "diff-suite"))]
                {
                    return Err(format!("Ungültiger Wert für --ido: '{}'", val).into());
                }
            } else {
                let v: u8 = val
                    .parse()
                    .map_err(|_| format!("Ungültiger Wert für --ido: '{}'", val))?;
                render_wav_opts.ido = Some(v.min(6));
                #[cfg(feature = "diff-suite")]
                {
                    batch_idos_override = Some(vec![v.min(6)]);
                }
            }
        } else if arg == "--rate" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --rate <11025..48000>".into());
            }
            let v: u32 = args[idx + 1]
                .parse()
                .map_err(|_| format!("Ungültiger Wert für --rate: '{}'", args[idx + 1]))?;
            render_wav_opts.rate = Some(v.clamp(11025, 48000));
            idx += 1;
        } else if arg == "--bits" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --bits <8|16>".into());
            }
            let v: u8 = args[idx + 1]
                .parse()
                .map_err(|_| format!("Ungültiger Wert für --bits: '{}'", args[idx + 1]))?;
            render_wav_opts.bits = Some(if v == 8 { 8 } else { 16 });
            idx += 1;
        } else if arg == "--channels" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --channels <mono|stereo|surround>".into());
            }
            render_wav_opts.channels_mode = Some(args[idx + 1].clone());
            idx += 1;
        } else if arg == "--pan" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --pan <0|1>".into());
            }
            let v = &args[idx + 1];
            render_wav_opts.pan = Some(
                v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off"),
            );
            idx += 1;
        } else if arg == "--peak-search" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --peak-search <0|1>".into());
            }
            let v = &args[idx + 1];
            render_wav_opts.peak_search = Some(
                v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off"),
            );
            idx += 1;
        } else if arg == "--seek-pattern" || arg == "--pattern" || arg == "-p" {
            if idx + 1 >= args.len() {
                return Err("Fehlender Wert für --seek-pattern <N>".into());
            }
            let n: u16 = args[idx + 1]
                .parse()
                .map_err(|_| format!("Ungültige Pattern-Nummer: '{}'", args[idx + 1]))?;
            seek_pattern = Some(n);
            idx += 1;
        } else if let Some(val) = arg.strip_prefix("--seek-pattern=") {
            let n: u16 = val
                .parse()
                .map_err(|_| format!("Ungültige Pattern-Nummer: '{}'", val))?;
            seek_pattern = Some(n);
        } else {
            #[cfg(feature = "diff-suite")]
            {
                if arg == "--batch-diff" || arg == "--batch-m4w-diff" {
                    if idx + 2 < args.len() && !args[idx + 2].starts_with('-') {
                        batch_m4w_diff_pair =
                            Some((args[idx + 1].clone(), args[idx + 2].clone()));
                        idx += 2;
                    } else if idx + 1 < args.len() {
                        batch_m4w_diff_pair =
                            Some((args[idx + 1].clone(), "ModBatchResult".to_string()));
                        idx += 1;
                    } else {
                        return Err(
                            "Verwendung: --batch-diff <mod_quellordner> <ergebnis_ordner>".into(),
                        );
                    }
                    idx += 1;
                    continue;
                } else if arg == "--m4w-dir" || arg == "--mod4win-dir" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --m4w-dir <ordner_pfad>".into());
                    }
                    batch_m4w_opts.m4w_dir = Some(Path::new(&args[idx + 1]).to_path_buf());
                    idx += 2;
                    continue;
                } else if arg == "--otvdm" || arg == "--wine" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --otvdm/--wine <pfad>".into());
                    }
                    batch_m4w_opts.otvdm_or_wine = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--work-dir" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --work-dir <ordner_pfad>".into());
                    }
                    batch_m4w_opts.work_dir = Some(Path::new(&args[idx + 1]).to_path_buf());
                    idx += 2;
                    continue;
                } else if arg == "--retry-fails" {
                    batch_m4w_opts.retry_fails = true;
                    idx += 1;
                    continue;
                } else if arg == "--retry-timeouts" {
                    batch_m4w_opts.retry_timeouts = true;
                    idx += 1;
                    continue;
                } else if arg == "--allow-1lsb" {
                    batch_m4w_opts.allow_1lsb = true;
                    idx += 1;
                    continue;
                } else if arg == "--max-idle-sec" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --max-idle-sec <N>".into());
                    }
                    batch_m4w_opts.max_idle_sec = args[idx + 1].parse()?;
                    idx += 2;
                    continue;
                } else if arg == "--max-peak-sec" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --max-peak-sec <N>".into());
                    }
                    batch_m4w_opts.max_active_peak_sec = args[idx + 1].parse()?;
                    idx += 2;
                    continue;
                } else if arg == "--max-stall-sec" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --max-stall-sec <N>".into());
                    }
                    batch_m4w_opts.max_stall_sec = args[idx + 1].parse()?;
                    idx += 2;
                    continue;
                } else if arg == "--max-wav-mb" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --max-wav-mb <N>".into());
                    }
                    batch_m4w_opts.max_wav_mb = args[idx + 1].parse()?;
                    idx += 2;
                    continue;
                } else if arg == "--batch-verify" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --batch-verify <folder_path>".into());
                    }
                    batch_verify_folder = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--batch-ffmpeg-diff" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --batch-ffmpeg-diff <folder_path>".into());
                    }
                    batch_ffmpeg_diff_folder = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--ref-dir" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --ref-dir <wav_cache_folder>".into());
                    }
                    batch_ref_dir = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--ffmpeg" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --ffmpeg <path_to_ffmpeg>".into());
                    }
                    batch_ffmpeg_bin = Some(args[idx + 1].clone());
                    batch_m4w_opts.ffmpeg_bin = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--threads" || arg == "--workers" || arg == "-j" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --workers/--threads/-j <N>".into());
                    }
                    let n: usize = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Ungültige Worker-Anzahl: '{}'", args[idx + 1]))?;
                    batch_threads = Some(n);
                    batch_m4w_opts.workers = Some(n);
                    idx += 2;
                    continue;
                } else if arg == "--max-secs" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --max-secs <N>".into());
                    }
                    let s: u32 = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Ungültiger Wert für --max-secs: '{}'", args[idx + 1]))?;
                    batch_max_secs = Some(s);
                    idx += 2;
                    continue;
                } else if arg == "--report" {
                    if idx + 1 >= args.len() {
                        return Err("Verwendung: --report <json_path>".into());
                    }
                    batch_report_path = Some(args[idx + 1].clone());
                    idx += 2;
                    continue;
                } else if arg == "--diff-suite" {
                    if diff_suite_wavs.is_none() {
                        diff_suite_wavs = Some(Vec::new());
                    }
                    while idx + 1 < args.len() && !args[idx + 1].starts_with('-') {
                        idx += 1;
                        let item = args[idx].clone();
                        let is_wav_or_ffmpeg = item.to_ascii_lowercase().ends_with(".wav")
                            || item.eq_ignore_ascii_case("ffmpeg");
                        if file_arg.is_none() && !is_wav_or_ffmpeg {
                            file_arg = Some(item);
                        } else if let Some(ref mut list) = diff_suite_wavs {
                            list.push(item);
                        }
                    }
                    idx += 1;
                    continue;
                }
            }

            if !arg.starts_with('-') {
                #[cfg(feature = "diff-suite")]
                if let Some(ref mut list) = diff_suite_wavs {
                    let is_wav_or_ffmpeg = arg.to_ascii_lowercase().ends_with(".wav")
                        || arg.eq_ignore_ascii_case("ffmpeg");
                    if file_arg.is_none() && !is_wav_or_ffmpeg {
                        file_arg = Some(arg.clone());
                    } else {
                        list.push(arg.clone());
                    }
                    idx += 1;
                    continue;
                }
                if file_arg.is_none() {
                    file_arg = Some(arg.clone());
                }
            }
        }
        idx += 1;
    }

    #[cfg(feature = "diff-suite")]
    {
        if let Some(ref list) = diff_suite_wavs
            && list.is_empty()
        {
            eprintln!(
                "Verwendung:\n  Einzeldatei: [<modul_datei>] --diff-suite <referenz.wav|ffmpeg> [-ido <0..6>] (oder 3 WAV-Pfade)\n  Ganzer Ordner: --batch-diff <mod_quellordner> [<ergebnis_ordner>] [--workers <N>]"
            );
            std::process::exit(1);
        }
        if file_arg.is_none() && diff_suite_wavs.is_some() {
            file_arg = Some("M:\\Audio\\MOD_MID_SID\\MOD\\XM\\PAHGIANA.XM".to_string());
        }
    }

    // Lade MOD4WIN.INI beim Start (falls vorhanden)
    let ini_path = Mod4WinIniConfig::default_ini_path();
    let mut ini_cfg = Mod4WinIniConfig::load_from_file(&ini_path);
    if let Some(prof) = cli_cpu_sim {
        ini_cfg.cpu_sim_profile = prof.to_u8();
        ini_cfg.authentic_stutter = prof != CpuProfile::Native || cli_authentic_stutter;
    } else if cli_authentic_stutter {
        ini_cfg.authentic_stutter = true;
    }
    if let Some(sm) = cli_skin_mode {
        ini_cfg.skin_mode = sm.to_u8();
    }

    #[cfg(feature = "diff-suite")]
    {
        if let Some((src_str, res_str)) = batch_m4w_diff_pair {
            batch_m4w_opts.mod_source = Path::new(&src_str).to_path_buf();
            batch_m4w_opts.result_base = Path::new(&res_str).to_path_buf();
            if let Some(idos) = batch_idos_override {
                batch_m4w_opts.idos = idos;
            }
            run_batch_m4w_diff(&batch_m4w_opts)?;
            return Ok(());
        }

        if let Some(folder_str) = batch_ffmpeg_diff_folder {
            let diff_opts = BatchFfmpegDiffOptions {
                folder_path: Path::new(&folder_str).to_path_buf(),
                ref_dir: batch_ref_dir.as_deref().map(|s| Path::new(s).to_path_buf()),
                ffmpeg_bin: batch_ffmpeg_bin,
                threads: batch_threads,
                max_secs: batch_max_secs,
                report_json: batch_report_path
                    .as_deref()
                    .map(|s| Path::new(s).to_path_buf()),
                ido: render_wav_opts.ido,
            };
            run_batch_ffmpeg_diff(&diff_opts)?;
            return Ok(());
        }

        if let Some(folder_str) = batch_verify_folder {
            let rep_path = batch_report_path.as_deref().map(Path::new);
            run_batch_verify(Path::new(&folder_str), rep_path)?;
            return Ok(());
        }
    }

    if let Some((in_mod, out_wav)) = render_wav_pair {
        render_wav_opts.input_mod = in_mod;
        render_wav_opts.output_wav = out_wav;
        run_headless_render_wav(&render_wav_opts, &ini_cfg)?;
        return Ok(());
    }

    #[cfg(feature = "diff-suite")]
    let is_diff_suite_active = diff_suite_wavs.is_some();
    #[cfg(not(feature = "diff-suite"))]
    let is_diff_suite_active = false;

    let mut restored_from_state_saver = false;
    if file_arg.is_none() && !no_gui && !is_diff_suite_active && ini_cfg.state_saver
        && !ini_cfg.saved_playlist.is_empty() {
            let idx = ini_cfg
                .saved_playlist_idx
                .min(ini_cfg.saved_playlist.len().saturating_sub(1));
            let candidate = &ini_cfg.saved_playlist[idx];
            if candidate.exists() || mod4win_rs::ui::parse_zip_virtual_path(candidate).is_some() {
                file_arg = Some(candidate.to_string_lossy().into_owned());
                restored_from_state_saver = true;
                if seek_pattern.is_none() && ini_cfg.saved_order_pos > 1 {
                    seek_pattern = Some(ini_cfg.saved_order_pos as u16);
                }
            }
        }

    if file_arg.is_none() && no_gui {
        eprintln!(
            "Verwendung im Konsolenmodus: {} <Pfad zur Tracker-Datei> [--seek-pattern <N>] [--no-gui]",
            args[0]
        );
        std::process::exit(1);
    }

    let mut state_saver_load_failed = false;
    let (loaded_module, has_initial_module, initial_file_bytes) = if let Some(ref f_arg) = file_arg
    {
        let resolved_arg = mod4win_rs::ui::resolve_path_case_insensitive(Path::new(f_arg));
        let file_path = resolved_arg.as_path();
        println!("Lade Tracker-Modul: {}", file_path.display());
        match mod4win_rs::ui::read_module_bytes_universal(file_path) {
            Ok((file_data, ext_opt)) => {
                let f_len = file_data.len() as u32;
                match load_module_with_ext(&file_data, ext_opt.as_deref()) {
                    Ok(m) => (m, true, f_len),
                    Err(err_code) => {
                        eprintln!("Fehler beim Laden des Moduls (Fehlercode: {}).", err_code);
                        if restored_from_state_saver && !no_gui {
                            state_saver_load_failed = true;
                            (create_idle_module(), false, 0u32)
                        } else {
                            std::process::exit(1);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Fehler: Datei '{}' konnte nicht gelesen werden: {}",
                    file_path.display(),
                    e
                );
                if restored_from_state_saver && !no_gui {
                    state_saver_load_failed = true;
                    (create_idle_module(), false, 0u32)
                } else {
                    std::process::exit(1);
                }
            }
        }
    } else {
        println!("Starte MOD4WIN 3.10 ohne Startdatei (bereit für Drag & Drop / Open)...");
        (create_idle_module(), false, 0u32)
    };
    if state_saver_load_failed {
        restored_from_state_saver = false;
        file_arg = None;
        seek_pattern = None;
    }

    let mod_name_len = loaded_module.state.mi.mod_name_len as usize;
    let mod_name_bytes = loaded_module.state.mi.mod_name;
    let mod_name =
        mod4win_rs::ui::live_audio::decode_tracker_text(&mod_name_bytes[..mod_name_len.min(40)]);

    let num_tracks = loaded_module.state.mi.num_tracks;
    let num_patterns = loaded_module.state.mi.num_patterns;
    let num_instruments = loaded_module.state.mi.num_instruments;
    let file_type = loaded_module.state.mi.file_type;
    let init_speed = loaded_module.state.init_speed;
    let init_bpm_speed = loaded_module.state.init_bpm_speed;

    if has_initial_module {
        println!("==================================================");
        println!(
            "Titel:       {}",
            if mod_name.is_empty() {
                "<Kein Titel>"
            } else {
                &mod_name
            }
        );
        println!("Format:      {:?}", file_type);
        println!("Kanäle:      {}", num_tracks);
        println!("Patterns:    {}", num_patterns);
        println!("Instrumente: {}", num_instruments);
        println!("Tempo / BPM: {} / {}", init_speed, init_bpm_speed);
        println!("==================================================");
    }

    #[cfg(feature = "diff-suite")]
    if let Some(wav_paths) = diff_suite_wavs {
        match run_diff_suite(
            loaded_module,
            file_arg.as_deref(),
            &wav_paths,
            &render_wav_opts,
            batch_ffmpeg_bin.as_deref(),
        ) {
            Ok(true) => std::process::exit(0),
            Ok(false) => std::process::exit(1),
            Err(e) => {
                eprintln!("Fehler im --diff-suite Lauf: {}", e);
                std::process::exit(1);
            }
        }
    }

    // SoundSettings für die MOD4WIN Audio-Engine anpassen (alle Parameter zuverlässig aus MOD4WIN.INI übernehmen!)
    // Engine-Samplerate bleibt auf 4000..=48000 Hz begrenzt (wie in `apply_live_sound_settings`),
    // damit `num_samples` niemals `MAX_MIX_BUF_SAMPLES` (2000) überschreitet; höhere OS-Sampleraten
    // (z. B. 88.2 / 96 kHz) übernimmt der äußere Linear-Resampler in `live_audio.rs`.
    let mut settings = SoundSettings::default();
    settings.samplerate = ini_cfg.sample_rate.clamp(4000, 48000) as i32;
    settings.stereo = ini_cfg.channels.clamp(1, 2);
    settings.bits = if ini_cfg.bits_per_sample == 8 { 8 } else { 16 };
    settings.surround = ini_cfg.surround && settings.stereo == 2;
    settings.panning = ini_cfg.enable_panning;
    settings.oversamp = ini_cfg.over_sampling.min(6);
    if loaded_module.state.mi.file_type != ModType::IT && settings.oversamp >= 4 {
        settings.oversamp = match settings.oversamp {
            4 => 0,
            5 => 1,
            _ => 2,
        };
        ini_cfg.over_sampling = settings.oversamp;
    }
    let is_16bit_xm_it = has_initial_module && loaded_module.state.mi.is_16bit_xm_or_it();
    settings.set_timer_mode(ini_cfg.timer_mode.min(2));
    settings.set_clean_mode(ini_cfg.clean_mode);
    settings.set_declick(ini_cfg.declick);
    settings.set_amiga_filter(ini_cfg.amiga_filter);
    settings.set_dehiss(ini_cfg.dehiss && !is_16bit_xm_it);
    settings.n_buffers = ini_cfg.output_buffers.clamp(2, 99);
    let clip_bit = if ini_cfg.clipping { 0x80 } else { 0x00 };
    settings.pre_amp = clip_bit | ini_cfg.pre_amplification.min(40);

    let diff_patterns = loaded_module.state.mi.diff_patterns;

    // Instrumentennamen und detaillierte Metadaten für das Module-Info- & Instrumenten-Fenster extrahieren
    let (inst_names, inst_infos) = if has_initial_module {
        mod4win_rs::ui::live_audio::extract_ui_instruments(&loaded_module.state.mi)
    } else {
        (Vec::new(), Vec::new())
    };
    let _ = num_instruments;

    let file_size_bytes = initial_file_bytes;
    let file_size_kb = (file_size_bytes / 1024) as usize;

    // UI-Shared State vorbereiten
    let mut init_ui_state = UiSharedState::new(
        if has_initial_module { &mod_name } else { "MOD4WIN 3.10" },
        &format!("{:?}", file_type),
        if has_initial_module { num_tracks as usize } else { 0 },
        if has_initial_module { diff_patterns as usize } else { 0 },
        if has_initial_module { num_patterns as usize } else { 0 },
        if has_initial_module { num_patterns as usize } else { 0 },
        inst_names,
        file_size_kb,
    );
    init_ui_state.ini_config = ini_cfg;
    init_ui_state
        .cpu_telemetry
        .reset_buffer(init_ui_state.ini_config.output_buffers);
    if state_saver_load_failed {
        init_ui_state.set_status_banner("SAVED FILE NOT FOUND".to_string());
    }
    if cli_cpu_sim.is_some() || cli_authentic_stutter {
        let prof = CpuProfile::from_u8(init_ui_state.ini_config.cpu_sim_profile);
        let banner = prof.lcd_banner_with_stutter(init_ui_state.ini_config.authentic_stutter);
        init_ui_state.set_status_banner(banner);
    }
    let active_skin = SkinMode::from_u8(init_ui_state.ini_config.skin_mode);
    init_ui_state.set_skin_mode(active_skin);
    init_ui_state.classic_input = classic_input;
    init_ui_state.is_playing = if restored_from_state_saver {
        init_ui_state.ini_config.saved_playing && !init_ui_state.ini_config.saved_paused
    } else {
        has_initial_module
    };
    init_ui_state.is_paused = if restored_from_state_saver {
        init_ui_state.ini_config.saved_paused
    } else {
        false
    };
    init_ui_state.repeat_mode = init_ui_state.ini_config.saved_repeat_mode.min(2);
    init_ui_state.shuffle = init_ui_state.ini_config.saved_shuffle;
    init_ui_state.intro_scan = init_ui_state.ini_config.saved_intro_scan;
    init_ui_state.minus_mode = init_ui_state.ini_config.saved_minus_mode;
    init_ui_state.show_filename = init_ui_state.ini_config.force_filename_display;
    init_ui_state.curr_pos = if has_initial_module { 1 } else { 0 };
    init_ui_state.instruments = inst_infos;
    init_ui_state.dehiss_auto_disabled = is_16bit_xm_it && init_ui_state.ini_config.dehiss;
    init_ui_state.effects_used_set = loaded_module.state.mi.effects_used_set;
    init_ui_state.song_mem_bytes = loaded_module.state.mi.song_mem as usize;
    init_ui_state.total_time_secs = (loaded_module.state.mi.total_time / 1000).max(0) as u32;

    // Falls Auto-PreAmp PeakSearch aktiv ist:
    // Im GUI-Modus NIEMals vor dem ersten Fenster-Frame blockieren!
    // Wenn `Load State: On` (`state_saver`) aktiv ist und der Peak bereits gecacht ist, sofort übernehmen;
    // andernfalls erst nach dem Öffnen des Hauptfensters asynchron im Hintergrund starten!
    if has_initial_module && init_ui_state.ini_config.auto_preamp_peaksearch {
        let pks_path = Mod4WinIniConfig::default_pks_path();
        if no_gui {
            let found = run_peak_search_isolated(&loaded_module, settings) & 0x7F;
            init_ui_state
                .ini_config
                .store_peak(file_size_bytes, &mod_name, found, &pks_path);
            let pa = found.min(40);
            settings.pre_amp = (settings.pre_amp & 0x80) | pa;
            init_ui_state.ini_config.pre_amplification = pa;
        } else if init_ui_state.ini_config.state_saver {
            if let Some(cached_peak) =
                init_ui_state
                    .ini_config
                    .find_peak(file_size_bytes, &mod_name, &pks_path)
            {
                let pa = (cached_peak & 0x7F).min(40);
                settings.pre_amp = (settings.pre_amp & 0x80) | pa;
                init_ui_state.ini_config.pre_amplification = pa;
            } else {
                init_ui_state.pending_initial_peak_search = Some(mod4win_rs::ui::PendingPeakSearch(
                    Arc::new(loaded_module.clone()),
                    settings,
                    file_size_bytes,
                    mod_name.clone(),
                ));
            }
        } else {
            init_ui_state.pending_initial_peak_search = Some(mod4win_rs::ui::PendingPeakSearch(
                Arc::new(loaded_module.clone()),
                settings,
                file_size_bytes,
                mod_name.clone(),
            ));
        }
    }
    init_ui_state.sound_settings = settings;

    if let Some(ref f_arg) = file_arg {
        let file_path = Path::new(f_arg);
        init_ui_state.filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_uppercase();
        init_ui_state.current_file_path = Some(file_path.to_path_buf());

        if restored_from_state_saver && !init_ui_state.ini_config.saved_playlist.is_empty() {
            init_ui_state.playlist = init_ui_state.ini_config.saved_playlist.clone();
            init_ui_state.playlist_idx = init_ui_state
                .ini_config
                .saved_playlist_idx
                .min(init_ui_state.playlist.len().saturating_sub(1));
        } else if let Some(raw_parent) = file_path.parent() {
            let parent_dir = if raw_parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                raw_parent
            };
            // Playlist aus dem aktuellen Verzeichnis aufbauen
            if let Ok(entries) = std::fs::read_dir(parent_dir) {
                let mut list: Vec<std::path::PathBuf> = entries
                    .filter_map(|e| e.ok().map(|d| d.path()))
                    .filter(|p| UiSharedState::is_tracker_file(p))
                    .collect();
                list.sort();
                if let Some(pos) = list.iter().position(|p| {
                    p == file_path
                        || (raw_parent.as_os_str().is_empty()
                            && p.file_name() == file_path.file_name())
                }) {
                    init_ui_state.playlist_idx = pos;
                }
                init_ui_state.playlist = list;
            }
        }
        if init_ui_state.playlist.is_empty() {
            init_ui_state.playlist.push(file_path.to_path_buf());
        }
        init_ui_state.rebuild_number_list();
    }

    let shared_ui_state = Arc::new(Mutex::new(init_ui_state));
    let sequencer_mutex = create_initial_sequencer(
        loaded_module,
        settings,
        seek_pattern,
        &shared_ui_state,
    );

    #[cfg(feature = "retro-win32")]
    {
        mod4win_rs::ui::retro_host::run_retro_win32_player(
            shared_ui_state,
            sequencer_mutex,
            no_gui,
        )?;
        return Ok(());
    }

    #[cfg(not(feature = "retro-win32"))]
    {
        let _ = fs::metadata(".");
        // Audio-Gerät über CPAL initialisieren
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("Kein Standard-Audio-Ausgabegerät gefunden")?;

        println!("Verwende Audio-Gerät: {}", device);

        let default_config = device.default_output_config()?;
        println!(
            "Geräte-Konfiguration: {} Hz, {} Kanäle, Format: {:?}",
            default_config.sample_rate(),
            default_config.channels(),
            default_config.sample_format()
        );

        let sample_rate = if default_config.sample_rate() > 0 {
            default_config.sample_rate()
        } else {
            48000
        };
        let channels = if default_config.channels() > 0 {
            default_config.channels()
        } else {
            2
        };
        let ui_state_for_audio = Arc::clone(&shared_ui_state);
        let seq_for_audio = Arc::clone(&sequencer_mutex);

        let finished = Arc::new(AtomicBool::new(false));
        let finished_clone = Arc::clone(&finished);
        let is_stereo = channels >= 2;

        let err_fn = |err| eprintln!("Audio-Stream-Fehler: {}", err);
        let mut stream_config: StreamConfig = default_config.into();
        stream_config.sample_rate = sample_rate;
        stream_config.channels = channels;
        let sr_f32 = (sample_rate as f32).max(8000.0);

        let stream = match default_config.sample_format() {
            SampleFormat::F32 => {
                let mut stereo_frames: Vec<(i16, i16)> = Vec::new();
                let mut live_proc = LiveAudioProcessor::new(sample_rate);
                let mut cached_playing = !no_gui;
                let mut cached_paused = false;
                let mut cached_stutter = false;
                let mut cached_muted = [false; 32];
                let mut pending_settings: Option<SoundSettings> = None;

                device.build_output_stream(
                    stream_config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        if finished_clone.load(Ordering::Relaxed) {
                            data.fill(0.0);
                            return;
                        }

                        let mut seq = match seq_for_audio.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            handle_audio_cycle_and_sync(
                                &mut ui,
                                &mut seq,
                                &mut live_proc,
                                &mut pending_settings,
                                no_gui,
                                &finished_clone,
                            );
                            cached_playing = ui.is_playing;
                            cached_paused = ui.is_paused;
                            cached_stutter =
                                ui.ini_config.authentic_stutter && ui.cpu_telemetry.buffer_underrun;
                            cached_muted = ui.channel_muted;
                        }

                        let num_frames = (data.len() / channels.max(1) as usize).max(1);
                        if stereo_frames.len() < num_frames {
                            stereo_frames.resize(num_frames, (0, 0));
                        }

                        let t0 = std::time::Instant::now();
                        live_proc.fill_cpal_stereo_frames(
                            &mut seq,
                            &mut stereo_frames[..num_frames],
                            cached_playing,
                            cached_paused,
                            cached_stutter,
                            &cached_muted,
                            &mut pending_settings,
                        );
                        let cb_secs = t0.elapsed().as_secs_f32();
                        let buf_secs = (num_frames as f32) / sr_f32;
                        let measured_ratio = (cb_secs / buf_secs.max(0.0005)).clamp(0.001, 1.0);

                        const LIVE_HEADROOM_F32: f32 = 0.65 / 32768.0;
                        if is_stereo {
                            for (frame_idx, chunk) in data.chunks_mut(channels as usize).enumerate()
                            {
                                let (left_i16, right_i16) = stereo_frames[frame_idx];
                                chunk[0] = (left_i16 as f32) * LIVE_HEADROOM_F32;
                                chunk[1] = (right_i16 as f32) * LIVE_HEADROOM_F32;
                                for ch in &mut chunk[2..] {
                                    *ch = 0.0;
                                }
                            }
                        } else {
                            for (out_sample, &(l, r)) in
                                data.iter_mut().zip(&stereo_frames[..num_frames])
                            {
                                *out_sample =
                                    (((l as i32 + r as i32) / 2) as f32) * LIVE_HEADROOM_F32;
                            }
                        }

                        let elapsed = (seq.module.state.play_time / 1000).max(0) as u32;
                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            if cached_playing && !cached_paused {
                                ui.native_callback_ratio =
                                    ui.native_callback_ratio * 0.8 + measured_ratio * 0.2;
                            }
                            if ui.current_file_path.is_some() || cached_playing {
                                ui.update_from_sequencer(&seq, elapsed);
                            }
                            ui.update_output_vu_from_pcm(&stereo_frames[..num_frames]);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let mut stereo_frames: Vec<(i16, i16)> = Vec::new();
                let mut live_proc = LiveAudioProcessor::new(sample_rate);
                let mut cached_playing = !no_gui;
                let mut cached_paused = false;
                let mut cached_stutter = false;
                let mut cached_muted = [false; 32];
                let mut pending_settings: Option<SoundSettings> = None;

                device.build_output_stream(
                    stream_config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        if finished_clone.load(Ordering::Relaxed) {
                            data.fill(0);
                            return;
                        }

                        let mut seq = match seq_for_audio.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            handle_audio_cycle_and_sync(
                                &mut ui,
                                &mut seq,
                                &mut live_proc,
                                &mut pending_settings,
                                no_gui,
                                &finished_clone,
                            );
                            cached_playing = ui.is_playing;
                            cached_paused = ui.is_paused;
                            cached_stutter =
                                ui.ini_config.authentic_stutter && ui.cpu_telemetry.buffer_underrun;
                            cached_muted = ui.channel_muted;
                        }

                        let num_frames = (data.len() / channels.max(1) as usize).max(1);
                        if stereo_frames.len() < num_frames {
                            stereo_frames.resize(num_frames, (0, 0));
                        }

                        let t0 = std::time::Instant::now();
                        live_proc.fill_cpal_stereo_frames(
                            &mut seq,
                            &mut stereo_frames[..num_frames],
                            cached_playing,
                            cached_paused,
                            cached_stutter,
                            &cached_muted,
                            &mut pending_settings,
                        );
                        let cb_secs = t0.elapsed().as_secs_f32();
                        let buf_secs = (num_frames as f32) / sr_f32;
                        let measured_ratio = (cb_secs / buf_secs.max(0.0005)).clamp(0.001, 1.0);

                        if is_stereo {
                            for (frame_idx, chunk) in data.chunks_mut(channels as usize).enumerate()
                            {
                                let (left_i16, right_i16) = stereo_frames[frame_idx];
                                chunk[0] = (((left_i16 as i32) * 166) >> 8) as i16;
                                chunk[1] = (((right_i16 as i32) * 166) >> 8) as i16;
                                for ch in &mut chunk[2..] {
                                    *ch = 0;
                                }
                            }
                        } else {
                            for (out_sample, &(l, r)) in
                                data.iter_mut().zip(&stereo_frames[..num_frames])
                            {
                                *out_sample = ((((l as i32 + r as i32) / 2) * 166) >> 8) as i16;
                            }
                        }

                        let elapsed = (seq.module.state.play_time / 1000).max(0) as u32;
                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            if cached_playing && !cached_paused {
                                ui.native_callback_ratio =
                                    ui.native_callback_ratio * 0.8 + measured_ratio * 0.2;
                            }
                            if ui.current_file_path.is_some() || cached_playing {
                                ui.update_from_sequencer(&seq, elapsed);
                            }
                            ui.update_output_vu_from_pcm(&stereo_frames[..num_frames]);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let mut stereo_frames: Vec<(i16, i16)> = Vec::new();
                let mut live_proc = LiveAudioProcessor::new(sample_rate);
                let mut cached_playing = !no_gui;
                let mut cached_paused = false;
                let mut cached_stutter = false;
                let mut cached_muted = [false; 32];
                let mut pending_settings: Option<SoundSettings> = None;

                device.build_output_stream(
                    stream_config,
                    move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                        if finished_clone.load(Ordering::Relaxed) {
                            data.fill(32768);
                            return;
                        }

                        let mut seq = match seq_for_audio.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            handle_audio_cycle_and_sync(
                                &mut ui,
                                &mut seq,
                                &mut live_proc,
                                &mut pending_settings,
                                no_gui,
                                &finished_clone,
                            );
                            cached_playing = ui.is_playing;
                            cached_paused = ui.is_paused;
                            cached_stutter =
                                ui.ini_config.authentic_stutter && ui.cpu_telemetry.buffer_underrun;
                            cached_muted = ui.channel_muted;
                        }

                        let num_frames = (data.len() / channels.max(1) as usize).max(1);
                        if stereo_frames.len() < num_frames {
                            stereo_frames.resize(num_frames, (0, 0));
                        }

                        let t0 = std::time::Instant::now();
                        live_proc.fill_cpal_stereo_frames(
                            &mut seq,
                            &mut stereo_frames[..num_frames],
                            cached_playing,
                            cached_paused,
                            cached_stutter,
                            &cached_muted,
                            &mut pending_settings,
                        );
                        let cb_secs = t0.elapsed().as_secs_f32();
                        let buf_secs = (num_frames as f32) / sr_f32;
                        let measured_ratio = (cb_secs / buf_secs.max(0.0005)).clamp(0.001, 1.0);

                        if is_stereo {
                            for (frame_idx, chunk) in data.chunks_mut(channels as usize).enumerate()
                            {
                                let (left_i16, right_i16) = stereo_frames[frame_idx];
                                chunk[0] = ((((left_i16 as i32) * 166) >> 8) + 32768) as u16;
                                chunk[1] = ((((right_i16 as i32) * 166) >> 8) + 32768) as u16;
                                for ch in &mut chunk[2..] {
                                    *ch = 32768;
                                }
                            }
                        } else {
                            for (out_sample, &(l, r)) in
                                data.iter_mut().zip(&stereo_frames[..num_frames])
                            {
                                *out_sample =
                                    (((((l as i32 + r as i32) / 2) * 166) >> 8) + 32768) as u16;
                            }
                        }

                        let elapsed = (seq.module.state.play_time / 1000).max(0) as u32;
                        if let Ok(mut ui) = ui_state_for_audio.try_lock() {
                            if cached_playing && !cached_paused {
                                ui.native_callback_ratio =
                                    ui.native_callback_ratio * 0.8 + measured_ratio * 0.2;
                            }
                            if ui.current_file_path.is_some() || cached_playing {
                                ui.update_from_sequencer(&seq, elapsed);
                            }
                            ui.update_output_vu_from_pcm(&stereo_frames[..num_frames]);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            _ => return Err("Nicht unterstütztes Sample-Format".into()),
        };

        stream.play()?;
        println!("Wiedergabe gestartet... (Beenden mit ESC oder Fenster schließen)");

        if !no_gui {
            // UI-Schleife auf dem Haupt-Thread ausführen
            println!("Öffne grafische Benutzeroberfläche...");
            if let Err(e) = run_ui(Arc::clone(&shared_ui_state)) {
                eprintln!("UI-Fehler: {}. Schalte auf Konsolenmodus um...", e);
                run_console_loop(&finished, &sequencer_mutex);
            }
        } else {
            println!("Konsolenmodus aktiv.");
            run_console_loop(&finished, &sequencer_mutex);
        }

        finished.store(true, Ordering::SeqCst);
        println!("\nWiedergabe beendet.");
        Ok(())
    }
}

/// Erzeugt den initialen `Sequencer` in einem isolierten Stack-Frame, damit `main()` vor `run_ui()` minimalen Stack belegt.
#[inline(never)]
fn create_initial_sequencer(
    loaded_module: LoadedModule,
    settings: SoundSettings,
    seek_pattern: Option<u16>,
    shared_ui_state: &Arc<Mutex<UiSharedState>>,
) -> Arc<Mutex<Sequencer>> {
    let mut sequencer = Sequencer::new(loaded_module, settings);
    sequencer.module.state.repeat_song = false;
    let total_ms = compute_module_total_time_ms_on_seq(&mut sequencer);
    if let Ok(mut ui) = shared_ui_state.lock() {
        ui.total_time_secs = ((total_ms + 500) / 1000).max(0) as u32;
    }

    if let Some(target_pat) = seek_pattern {
        sequencer.seek_to_pattern(target_pat);
        let actual_pat = sequencer.module.state.curr_pattern + 1;
        let phys_pat = sequencer
            .module
            .state
            .p_arrangement
            .get(sequencer.module.state.curr_pattern as usize)
            .copied()
            .unwrap_or(0);
        println!(
            "Springe zu Pattern {} (Order {}, physisches Pattern {}, Speed {}, BPM {})",
            actual_pat,
            sequencer.module.state.curr_pattern,
            phys_pat,
            sequencer.module.state.speed,
            sequencer.module.state.bpm_speed
        );
        let elapsed = (sequencer.module.state.play_time / 1000).max(0) as u32;
        if let Ok(mut ui) = shared_ui_state.lock() {
            ui.update_from_sequencer(&sequencer, elapsed);
        }
    }

    Arc::new(Mutex::new(sequencer))
}

#[cfg(all(windows, not(feature = "retro-win32")))]
unsafe extern "C" {
    fn _kbhit() -> i32;
    fn _getch() -> i32;
}

#[cfg(not(feature = "retro-win32"))]
fn run_console_loop(finished: &Arc<AtomicBool>, sequencer_mutex: &Arc<Mutex<Sequencer>>) {
    println!("Steuerung: [Pfeil Links/Rechts] Pattern -/+1 | [Bild Auf/Ab] Pattern -/+5 | [ESC] Beenden");
    while !finished.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(80));

        #[cfg(windows)]
        unsafe {
            while _kbhit() != 0 {
                let ch = _getch();
                if ch == 0x1B {
                    // ESC
                    finished.store(true, Ordering::SeqCst);
                    break;
                } else if ch == 0x00 || ch == 0xE0 {
                    let ext = _getch();
                    let delta = match ext {
                        0x4D => 1,  // ArrowRight
                        0x4B => -1, // ArrowLeft
                        0x51 => 5,  // PageDown
                        0x49 => -5, // PageUp
                        _ => 0,
                    };
                    if delta != 0
                        && let Ok(mut seq) = sequencer_mutex.lock() {
                            seq.step_order(delta);
                        }
                }
            }
        }

        let (pat, row, spd, bpm, time_ms) = {
            let seq = match sequencer_mutex.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            (
                seq.module.state.curr_pattern + 1,
                seq.module.state.curr_note + 1,
                seq.module.state.speed,
                seq.module.state.bpm_speed,
                seq.module.state.play_time,
            )
        };

        let mins = time_ms / 60000;
        let secs = (time_ms % 60000) / 1000;
        print!(
            "\rPattern: {:02} | Row: {:02} | Speed: {:02} | BPM: {:03} | Zeit: {:02}:{:02} ",
            pat, row, spd, bpm, mins, secs
        );
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
}

