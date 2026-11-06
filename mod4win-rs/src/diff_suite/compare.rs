//! Einzelmodul-Diagnose (`--diff-suite`) und In-Memory-Diff-Engine für den Vergleich
//! gegen 16-Bit `MOD4WIN.EXE` (`IDO 0..3`) sowie `FFmpeg` (`libopenmpt`).

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

use crate::engine::loader::LoadedModule;
use crate::engine::mixer::{process_sample_block, MixOutputBuffer};
use crate::engine::sequencer::Sequencer;
use crate::engine::types::{ModType, SoundSettings, DEV_HDD};
use crate::ui::archive_mol::resolve_path_case_insensitive;
use crate::ui::batch_cli::{parse_wav_bytes_i16, ParsedWavData, RenderWavCliOptions};

use super::batch_runner::{render_ffmpeg_reference_pcm, resolve_ffmpeg_binary};

/// Geladene 16-Bit-Stereo-Referenz-WAV (von Festplatte oder aus FFmpeg-Pipe).
pub struct WavData {
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_ffmpeg: bool,
    pub samples: Vec<i16>,
}

impl WavData {
    pub fn from_parsed(path: String, parsed: ParsedWavData) -> Self {
        Self {
            path,
            sample_rate: parsed.sample_rate,
            channels: parsed.channels,
            bits_per_sample: parsed.bits_per_sample,
            is_ffmpeg: parsed.is_ffmpeg,
            samples: parsed.samples,
        }
    }
}

pub fn load_wav_i16(
    path: &str,
    mod_file_arg: Option<&str>,
    ffmpeg_bin_arg: Option<&str>,
) -> Result<WavData, Box<dyn std::error::Error>> {
    if path.eq_ignore_ascii_case("ffmpeg") {
        let mod_p = mod_file_arg.ok_or("Kein Modul-Dateipfad für FFmpeg-Referenz angegeben")?;
        let resolved = resolve_path_case_insensitive(Path::new(mod_p));
        let ff_bin = resolve_ffmpeg_binary(ffmpeg_bin_arg);
        println!(
            "[FFmpeg-Direct-Pipe] Rendere Referenz für '{}' via '{}' (libopenmpt @ 44100 Hz)...",
            resolved.display(),
            ff_bin
        );
        let samples = render_ffmpeg_reference_pcm(&ff_bin, &resolved, 44100, None, None)?;
        return Ok(WavData {
            path: format!("ffmpeg:libopenmpt({})", resolved.display()),
            sample_rate: 44100,
            channels: 2,
            bits_per_sample: 16,
            is_ffmpeg: true,
            samples,
        });
    }

    let raw = fs::read(path)?;
    let parsed = parse_wav_bytes_i16(&raw)
        .map_err(|e| format!("Ungültige WAV-Datei '{}': {}", path, e))?;
    Ok(WavData::from_parsed(path.to_string(), parsed))
}

pub fn first_nonzero_frame(samples: &[i16]) -> Option<(usize, i16, i16)> {
    for (frame_idx, pair) in samples.chunks_exact(2).enumerate() {
        if pair[0] != 0 || pair[1] != 0 {
            return Some((frame_idx, pair[0], pair[1]));
        }
    }
    None
}

pub fn format_module_header(loaded_module: &LoadedModule, display_path: &str) -> String {
    let mod_name_len = loaded_module.state.mi.mod_name_len as usize;
    let mod_name_bytes = loaded_module.state.mi.mod_name;
    let mod_name = String::from_utf8_lossy(&mod_name_bytes[..mod_name_len.min(40)])
        .trim()
        .to_string();

    let num_tracks = loaded_module.state.mi.num_tracks;
    let num_patterns = loaded_module.state.mi.num_patterns;
    let num_instruments = loaded_module.state.mi.num_instruments;
    let file_type = loaded_module.state.mi.file_type;
    let init_speed = loaded_module.state.init_speed;
    let init_bpm_speed = loaded_module.state.init_bpm_speed;

    let mut out = String::with_capacity(512);
    let _ = writeln!(out, "Lade Tracker-Modul: {}", display_path);
    let _ = writeln!(out, "==================================================");
    let _ = writeln!(
        out,
        "Titel:       {}",
        if mod_name.is_empty() {
            "<Kein Titel>"
        } else {
            &mod_name
        }
    );
    let _ = writeln!(out, "Format:      {:?}", file_type);
    let _ = writeln!(out, "Kanäle:      {}", num_tracks);
    let _ = writeln!(out, "Patterns:    {}", num_patterns);
    let _ = writeln!(out, "Instrumente: {}", num_instruments);
    let _ = writeln!(out, "Tempo / BPM: {} / {}", init_speed, init_bpm_speed);
    let _ = writeln!(out, "==================================================");
    out
}

/// Führt für einen einzelnen `ido_mode` (`0..=3`) den kompletten `PeakSearch`- und Sample-Diff
/// komplett im RAM aus und gibt `(passed, max_abs_diff, report_text)` zurück.
pub fn run_single_ido_diff_in_memory(
    loaded_module: &LoadedModule,
    mod_display_path: &str,
    ref_wav: &WavData,
    ido_mode: u8,
    max_lsb_tolerance: i32,
) -> (bool, i32, String) {
    let mut report = format_module_header(loaded_module, mod_display_path);
    let _ = writeln!(
        report,
        "\n======================================================================"
    );
    let _ = writeln!(
        report,
        "MOD4WIN 3.00 Direct-to-Disk Diagnose-Suite (--diff-suite)"
    );
    let _ = writeln!(
        report,
        "======================================================================"
    );

    let frames = ref_wav.samples.len() / 2;
    let fnz = first_nonzero_frame(&ref_wav.samples);
    let ido_label = match ido_mode {
        0 => "IDO OFF (OverSamp = 0)",
        1 => "IDO LINEAR (OverSamp = 1 / fOverSamp)",
        2 => "IDO KUBISCH CPU (OverSamp = 2 / fCPUCubic)",
        _ => "IDO KUBISCH FPU (OverSamp = 3 / fFPUCubic)",
    };
    let _ = writeln!(
        report,
        "[{}] {} | {} Hz, {}-Bit, {} Ch | {} Stereo-Frames ({:.2}s) | Erster Non-Zero Frame: {:?}",
        ido_label,
        ref_wav.path,
        ref_wav.sample_rate,
        ref_wav.bits_per_sample,
        ref_wav.channels,
        frames,
        frames as f64 / ref_wav.sample_rate.max(1) as f64,
        fnz
    );

    let mut hdd_settings = SoundSettings::default();
    hdd_settings.device_id = DEV_HDD;
    hdd_settings.samplerate = ref_wav.sample_rate as i32;
    hdd_settings.stereo = 2;
    hdd_settings.bits = 16;
    hdd_settings.oversamp = ido_mode.min(3);
    hdd_settings.panning = true;
    hdd_settings.surround = false;
    hdd_settings.pre_amp = 0x80 + 40;

    let mut search_seq = Sequencer::new(loaded_module.clone(), hdd_settings);
    let found_pre_amp = search_seq.run_exact_peak_search();
    let _ = writeln!(
        report,
        "\n[PEAK SEARCH (DEV_HDD, OverSamp={})]: Ermittelter PreAmp = 0x{:02X} (raw={}, {:+} dB), VolMaster = {}",
        ido_mode,
        found_pre_amp,
        found_pre_amp & 0x7F,
        (found_pre_amp & 0x7F) as i32 - 20,
        search_seq.module.state.vol_master
    );

    let stage_title = format!("DIFF-VERGLEICH: {} vs. {}", ido_label, ref_wav.path);
    let (max_diff, frame_delta, total_compared, stage_txt) = run_stage_comparison_to_string(
        &stage_title,
        loaded_module,
        ref_wav,
        ido_mode.min(3),
        found_pre_amp,
        true,
    );
    report.push_str(&stage_txt);

    let passed =
        max_diff <= max_lsb_tolerance && frame_delta == 0 && total_compared > 0;
    (passed, max_diff, report)
}

pub fn run_diff_suite(
    loaded_module: LoadedModule,
    mod_file_arg: Option<&str>,
    wav_paths: &[String],
    cli_opts: &RenderWavCliOptions,
    ffmpeg_bin_arg: Option<&str>,
) -> Result<bool, Box<dyn std::error::Error>> {
    println!("\n======================================================================");
    println!("MOD4WIN 3.00 Direct-to-Disk Diagnose-Suite (--diff-suite)");
    println!("======================================================================");

    let is_it = loaded_module.state.mi.file_type == ModType::IT;

    if wav_paths.len() == 1 {
        let wav_path = &wav_paths[0];
        let ref_wav = load_wav_i16(wav_path, mod_file_arg, ffmpeg_bin_arg)?;
        if is_it || ref_wav.is_ffmpeg {
            let oversamp = cli_opts.ido.unwrap_or(if is_it { 6 } else { 2 }).min(6);
            let do_peak = cli_opts.peak_search.unwrap_or(false);
            let pre_amp = if do_peak {
                let mut hdd_settings = SoundSettings::default();
                hdd_settings.device_id = DEV_HDD;
                hdd_settings.samplerate = ref_wav.sample_rate as i32;
                hdd_settings.stereo = 2;
                hdd_settings.bits = 16;
                hdd_settings.oversamp = oversamp;
                hdd_settings.panning = cli_opts.pan.unwrap_or(true);
                hdd_settings.surround = false;
                hdd_settings.pre_amp = 0x80 + 40;
                let mut search_seq = Sequencer::new(loaded_module.clone(), hdd_settings);
                search_seq.run_exact_peak_search()
            } else {
                0x80 | 20
            };
            let (ok, report_txt) = run_ffmpeg_stage_comparison_to_string(
                &loaded_module,
                &ref_wav,
                oversamp,
                pre_amp,
                cli_opts.pan.unwrap_or(true),
            );
            print!("{}", report_txt);
            return Ok(ok);
        }
    }

    if let Some(ido_mode) = cli_opts.ido {
        let wav_path = wav_paths.last().ok_or("Keine Referenz-WAV angegeben")?;
        let ref_wav = load_wav_i16(wav_path, mod_file_arg, ffmpeg_bin_arg)?;
        let frames = ref_wav.samples.len() / 2;
        let fnz = first_nonzero_frame(&ref_wav.samples);
        let ido_label = match ido_mode {
            0 => "IDO OFF (OverSamp = 0)",
            1 => "IDO LINEAR (OverSamp = 1 / fOverSamp)",
            2 => "IDO KUBISCH CPU (OverSamp = 2 / fCPUCubic)",
            _ => "IDO KUBISCH FPU (OverSamp = 3 / fFPUCubic)",
        };
        println!(
            "[{}] {} | {} Hz, {}-Bit, {} Ch | {} Stereo-Frames ({:.2}s) | Erster Non-Zero Frame: {:?}",
            ido_label,
            ref_wav.path,
            ref_wav.sample_rate,
            ref_wav.bits_per_sample,
            ref_wav.channels,
            frames,
            frames as f64 / ref_wav.sample_rate as f64,
            fnz
        );

        let pan_enabled = cli_opts.pan.unwrap_or(true);
        let do_peak_search = cli_opts.peak_search.unwrap_or(true);

        let mut hdd_settings = SoundSettings::default();
        hdd_settings.device_id = DEV_HDD;
        hdd_settings.samplerate = ref_wav.sample_rate as i32;
        hdd_settings.stereo = 2;
        hdd_settings.bits = 16;
        hdd_settings.oversamp = ido_mode.min(3);
        hdd_settings.panning = pan_enabled;
        hdd_settings.surround = false;
        hdd_settings.pre_amp = 0x80 + 40;

        let used_pre_amp = if do_peak_search {
            let mut search_seq = Sequencer::new(loaded_module.clone(), hdd_settings);
            let found_pre_amp = search_seq.run_exact_peak_search();
            println!(
                "\n[PEAK SEARCH (DEV_HDD, OverSamp={})]: Ermittelter PreAmp = 0x{:02X} (raw={}, {:+} dB), VolMaster = {}",
                ido_mode,
                found_pre_amp,
                found_pre_amp & 0x7F,
                (found_pre_amp & 0x7F) as i32 - 20,
                search_seq.module.state.vol_master
            );
            found_pre_amp
        } else {
            0x80 + 20
        };

        let stage_title = format!("DIFF-VERGLEICH: {} vs. {}", ido_label, ref_wav.path);
        let is_exact = run_stage_comparison(
            &stage_title,
            &loaded_module,
            &ref_wav,
            ido_mode.min(3),
            used_pre_amp,
            true,
        );
        return Ok(is_exact);
    }

    if wav_paths.len() >= 3 {
        let wav_off = load_wav_i16(&wav_paths[0], mod_file_arg, ffmpeg_bin_arg)?;
        let wav_lin = load_wav_i16(&wav_paths[1], mod_file_arg, ffmpeg_bin_arg)?;
        let wav_cub = load_wav_i16(&wav_paths[2], mod_file_arg, ffmpeg_bin_arg)?;

        for (label, w) in [
            ("IDO OFF   ", &wav_off),
            ("IDO Linear", &wav_lin),
            ("IDO Cubic ", &wav_cub),
        ] {
            let frames = w.samples.len() / 2;
            let fnz = first_nonzero_frame(&w.samples);
            println!(
                "[{}] {} | {} Hz, {}-Bit, {} Ch | {} Stereo-Frames ({:.2}s) | Erster Non-Zero Frame: {:?}",
                label,
                w.path,
                w.sample_rate,
                w.bits_per_sample,
                w.channels,
                frames,
                frames as f64 / w.sample_rate as f64,
                fnz
            );
        }

        let mut all_exact = true;
        for (stage_title, ref_wav, oversamp, detail) in [
            (
                format!("STUFE 1: IDO OFF (OverSamp = 0 vs. {})", wav_off.path),
                &wav_off,
                0u8,
                true,
            ),
            (
                format!(
                    "STUFE 2: IDO LINEAR (OverSamp = 1 / fOverSamp vs. {})",
                    wav_lin.path
                ),
                &wav_lin,
                1u8,
                true,
            ),
            (
                format!(
                    "STUFE 3a: IDO KUBISCH CPU (UI-Einstellung OverSamp = 2 / fCPUCubic vs. {})",
                    wav_cub.path
                ),
                &wav_cub,
                2u8,
                true,
            ),
            (
                format!(
                    "STUFE 3b: IDO KUBISCH FPU (OverSamp = 3 / fFPUCubic vs. {})",
                    wav_cub.path
                ),
                &wav_cub,
                3u8,
                false,
            ),
        ] {
            let mut hdd_settings = SoundSettings::default();
            hdd_settings.device_id = DEV_HDD;
            hdd_settings.samplerate = ref_wav.sample_rate as i32;
            hdd_settings.stereo = 2;
            hdd_settings.bits = 16;
            hdd_settings.oversamp = oversamp;
            hdd_settings.panning = true;
            hdd_settings.surround = false;
            hdd_settings.pre_amp = 0x80 + 40;

            let mut search_seq = Sequencer::new(loaded_module.clone(), hdd_settings);
            let found_pre_amp = search_seq.run_exact_peak_search();
            println!(
                "\n[PEAK SEARCH (DEV_HDD, OverSamp={})]: Ermittelter PreAmp = 0x{:02X} (raw={}, {:+} dB), VolMaster = {}",
                oversamp,
                found_pre_amp,
                found_pre_amp & 0x7F,
                (found_pre_amp & 0x7F) as i32 - 20,
                search_seq.module.state.vol_master
            );
            let exact = run_stage_comparison(
                &stage_title,
                &loaded_module,
                ref_wav,
                oversamp,
                found_pre_amp,
                detail,
            );
            if detail && !exact {
                all_exact = false;
            }
        }
        return Ok(all_exact);
    }

    let cub_path = wav_paths.last().ok_or("Keine Referenz-WAV angegeben")?;
    let wav_cub = load_wav_i16(cub_path, mod_file_arg, ffmpeg_bin_arg)?;
    let frames = wav_cub.samples.len() / 2;
    let fnz = first_nonzero_frame(&wav_cub.samples);
    println!(
        "[IDO Cubic] {} | {} Hz, {}-Bit, {} Ch | {} Stereo-Frames ({:.2}s) | Erster Non-Zero Frame: {:?}",
        wav_cub.path,
        wav_cub.sample_rate,
        wav_cub.bits_per_sample,
        wav_cub.channels,
        frames,
        frames as f64 / wav_cub.sample_rate as f64,
        fnz
    );

    let mut hdd_settings = SoundSettings::default();
    hdd_settings.device_id = DEV_HDD;
    hdd_settings.samplerate = wav_cub.sample_rate as i32;
    hdd_settings.stereo = 2;
    hdd_settings.bits = 16;
    hdd_settings.oversamp = 3;
    hdd_settings.panning = true;
    hdd_settings.surround = false;
    hdd_settings.pre_amp = 0x80 + 40;

    let mut search_seq = Sequencer::new(loaded_module.clone(), hdd_settings);
    let found_pre_amp = search_seq.run_exact_peak_search();
    let db_val = (found_pre_amp & 0x7F) as i32 - 20;
    println!(
        "\n[PEAK SEARCH (DEV_HDD, OverSamp=3 FPU Cubic)]: Ermittelter PreAmp = 0x{:02X} (raw={}, {:+} dB), VolMaster = {}",
        found_pre_amp,
        found_pre_amp & 0x7F,
        db_val,
        search_seq.module.state.vol_master
    );

    let mut cpu_hdd_settings = hdd_settings;
    cpu_hdd_settings.oversamp = 2;
    let mut cpu_search_seq = Sequencer::new(loaded_module.clone(), cpu_hdd_settings);
    let found_pre_amp_cpu = cpu_search_seq.run_exact_peak_search();
    println!(
        "[PEAK SEARCH (DEV_HDD, OverSamp=2 CPU Cubic)]: Ermittelter PreAmp = 0x{:02X} (raw={}, {:+} dB), VolMaster = {}",
        found_pre_amp_cpu,
        found_pre_amp_cpu & 0x7F,
        (found_pre_amp_cpu & 0x7F) as i32 - 20,
        cpu_search_seq.module.state.vol_master
    );

    let exact_a = run_stage_comparison(
        "VERGLEICH A: IDO KUBISCH (CPU / UI-Einstellung OverSamp = 2 / fCPUCubic vs. PAHGIANA_44_16_PAN_IDO-CUB.WAV)",
        &loaded_module,
        &wav_cub,
        2,
        found_pre_amp_cpu,
        true,
    );

    let exact_b = run_stage_comparison(
        "VERGLEICH B: IDO KUBISCH (FPU / OverSamp = 3 / fFPUCubic vs. PAHGIANA_44_16_PAN_IDO-CUB.WAV)",
        &loaded_module,
        &wav_cub,
        3,
        found_pre_amp,
        false,
    );

    Ok(exact_a || exact_b)
}

pub fn run_ffmpeg_stage_comparison_to_string(
    loaded_module: &LoadedModule,
    ref_wav: &WavData,
    oversamp: u8,
    pre_amp: u8,
    panning: bool,
) -> (bool, String) {
    let mut out = String::with_capacity(4096);
    let ido_label = match oversamp {
        0 => "IDO 0 (M4W Off)",
        1 => "IDO 1 (M4W Linear)",
        2 => "IDO 2 (M4W CPU Cubic)",
        3 => "IDO 3 (M4W FPU Cubic)",
        4 => "IT2 4 (No-Interp + Filter)",
        5 => "IT2 5 (Linear + Filter)",
        _ => "IT2 6 (Cubic + Filter)",
    };
    let sr = ref_wav.sample_rate.max(8000);
    let ref_frames = ref_wav.samples.len() / 2;

    let _ = writeln!(
        out,
        "\n----------------------------------------------------------------------"
    );
    let _ = writeln!(
        out,
        "FFMPEG / LIBOPENMPT REFERENZ-ANALYSE: {} vs. {}",
        ido_label, ref_wav.path
    );
    let _ = writeln!(
        out,
        "Referenz:    {} Hz | {}-Bit | {} Ch | {} Stereo-Frames ({:.2}s)",
        sr,
        ref_wav.bits_per_sample,
        ref_wav.channels,
        ref_frames,
        ref_frames as f64 / sr as f64
    );
    let _ = writeln!(
        out,
        "Mod4Win:     OverSamp={} | Panning={} | PreAmp=0x{:02X} ({:+} dB)",
        oversamp,
        panning,
        pre_amp,
        (pre_amp & 0x7F) as i32 - 20
    );
    let _ = writeln!(
        out,
        "----------------------------------------------------------------------"
    );

    let mut settings = SoundSettings::default();
    settings.device_id = DEV_HDD;
    settings.samplerate = sr as i32;
    settings.stereo = 2;
    settings.bits = 16;
    settings.oversamp = oversamp;
    settings.panning = panning;
    settings.surround = false;
    settings.pre_amp = pre_amp;

    let mut seq = Sequencer::new(loaded_module.clone(), settings);
    seq.module.state.repeat_song = false;

    let mut m4w_samples: Vec<i16> = Vec::with_capacity(ref_wav.samples.len() + sr as usize * 2);
    let mut order_starts: Vec<(usize, usize, usize, String)> = Vec::new();
    let mut last_ord = usize::MAX;
    let mut ticks = 0usize;
    let max_m4w_frames = (ref_frames * 2).max(sr as usize * 600);

    while m4w_samples.len() / 2 < max_m4w_frames && ticks < 300_000 {
        if seq.is_it_module() {
            let (cur_ord, cur_pat) = {
                let eng = seq.it_engine.as_ref().unwrap();
                let ord = eng.curr_order;
                let pat = seq
                    .module
                    .it_module
                    .as_ref()
                    .and_then(|it| it.orders.get(ord).copied())
                    .unwrap_or(255) as usize;
                (ord, pat)
            };
            let Some(mix_out) = seq.render_it_tick(None) else {
                break;
            };
            if cur_pat < 254 && cur_ord != last_ord {
                let eng = seq.it_engine.as_ref().unwrap();
                let active_v = eng.voices.iter().filter(|v| v.active).count();
                let ch_dump: Vec<String> = (0..64)
                    .filter(|&c| eng.host_channels[c].fg_voice.is_some())
                    .take(10)
                    .map(|c| {
                        let hc = &eng.host_channels[c];
                        let fv = hc.fg_voice.map(|vi| eng.voices[vi].fvol).unwrap_or(0);
                        format!(
                            "C{:02}(s={},v={},pan={},fv={})",
                            c + 1,
                            hc.last_smp,
                            hc.vol,
                            hc.cpan,
                            fv
                        )
                    })
                    .collect();
                order_starts.push((
                    cur_ord,
                    cur_pat,
                    m4w_samples.len() / 2,
                    format!(
                        "gvol={} spd={} tmp={} act_v={} | {}",
                        eng.global_vol,
                        eng.speed,
                        eng.tempo,
                        active_v,
                        ch_dump.join(" ")
                    ),
                ));
                last_ord = cur_ord;
            }
            if let MixOutputBuffer::Pcm16(pcm) = mix_out {
                m4w_samples.extend_from_slice(&pcm);
            }
            if seq.it_engine.as_ref().unwrap().song_finished || seq.module.state.player_repeats {
                break;
            }
        } else {
            if seq.module.state.curr_vblanc == 0 {
                seq.process_row();
                if seq.module.state.player_repeats {
                    break;
                }
            }
            seq.update_channels_for_mixing();
            if seq.module.state.player_repeats {
                break;
            }
            let cur_ord = seq.module.state.curr_pattern as usize;
            let cur_pat = seq.module.state.p_arrangement[cur_ord.min(255)] as usize;
            if cur_pat < 254 && cur_ord != last_ord {
                order_starts.push((
                    cur_ord,
                    cur_pat,
                    m4w_samples.len() / 2,
                    format!(
                        "spd={} bpm={}",
                        seq.module.state.speed, seq.module.state.bpm_speed
                    ),
                ));
                last_ord = cur_ord;
            }
            seq.module.state.buf_samples = seq.module.state.num_samples.max(1);
            let am_repeating = seq.module.state.player_repeats && !seq.module.state.repeat_song;
            let mix_out = process_sample_block(
                &mut seq.module.state,
                &mut seq.mix_mem,
                &seq.module.sample_mem,
                am_repeating,
            );
            if let MixOutputBuffer::Pcm16(pcm) = mix_out {
                m4w_samples.extend_from_slice(&pcm);
            }
            seq.advance_tick();
            if seq.module.state.player_repeats {
                break;
            }
        }
        ticks += 1;
    }

    let m4w_frames = m4w_samples.len() / 2;
    let common_frames = ref_frames.min(m4w_frames);
    let ref_secs = ref_frames as f64 / sr as f64;
    let m4w_secs = m4w_frames as f64 / sr as f64;

    let _ = writeln!(
        out,
        "Dauer/Sync:  {} Ticks | Mod4Win: {} Frames ({:.2}s) vs. Ref: {} Frames ({:.2}s) | Delta: {:+} Frames ({:+.2}s)",
        ticks,
        m4w_frames,
        m4w_secs,
        ref_frames,
        ref_secs,
        m4w_frames as i64 - ref_frames as i64,
        m4w_secs - ref_secs
    );

    if common_frames == 0 {
        let _ = writeln!(
            out,
            "FEHLER: Keine gemeinsamen Frames zum Vergleich vorhanden!"
        );
        return (false, out);
    }

    let mut tot_ref_sq = 0.0f64;
    let mut tot_m4w_sq = 0.0f64;
    for i in 0..common_frames {
        let rl = ref_wav.samples[i * 2] as f64;
        let rr = ref_wav.samples[i * 2 + 1] as f64;
        let ml = m4w_samples[i * 2] as f64;
        let mr = m4w_samples[i * 2 + 1] as f64;
        tot_ref_sq += rl * rl + rr * rr;
        tot_m4w_sq += ml * ml + mr * mr;
    }
    let rms_ref_all = (tot_ref_sq / (2.0 * common_frames as f64)).sqrt();
    let rms_m4w_all = (tot_m4w_sq / (2.0 * common_frames as f64)).sqrt();
    let raw_ratio_all = if rms_ref_all > 1.0 {
        rms_m4w_all / rms_ref_all
    } else {
        1.0
    };
    let norm_scale = if raw_ratio_all > 0.05 {
        1.0 / raw_ratio_all
    } else {
        1.0
    };

    let _ = writeln!(
        out,
        "Gesamt-RMS:  Ref={:.1} LSB | Mod4Win={:.1} LSB | Raw-Ratio={:.3}x (Norm-Faktor={:.3}x)\n",
        rms_ref_all, rms_m4w_all, raw_ratio_all, norm_scale
    );

    let _ = writeln!(
        out,
        "  Ord (Pat) [  Zeitspanne  ] | Ref RMS (  L  ,   R  ) | M4W RMS (  L  ,   R  ) | RawRat | NormRat | PanDiff | Status"
    );
    let _ = writeln!(
        out,
        "  ---------------------------+------------------------+------------------------+--------+---------+---------+-------"
    );

    let mut orders_checked = 0usize;
    let mut orders_ok = 0usize;
    let mut anomalies: Vec<(usize, usize, f64, f64, String)> = Vec::new();

    for w in 0..order_starts.len() {
        let (ord, pat, s_f, ref info) = order_starts[w];
        let e_f = if w + 1 < order_starts.len() {
            order_starts[w + 1].2
        } else {
            m4w_frames
        };
        let e_clamped = e_f.min(common_frames);
        if e_clamped <= s_f {
            continue;
        }
        let n = (e_clamped - s_f) as f64;
        let mut s_rl = 0.0f64;
        let mut s_rr = 0.0f64;
        let mut s_ml = 0.0f64;
        let mut s_mr = 0.0f64;
        for i in s_f..e_clamped {
            let rl = ref_wav.samples[i * 2] as f64;
            let rr = ref_wav.samples[i * 2 + 1] as f64;
            let ml = m4w_samples[i * 2] as f64;
            let mr = m4w_samples[i * 2 + 1] as f64;
            s_rl += rl * rl;
            s_rr += rr * rr;
            s_ml += ml * ml;
            s_mr += mr * mr;
        }
        let rl_rms = (s_rl / n).sqrt();
        let rr_rms = (s_rr / n).sqrt();
        let ml_rms = (s_ml / n).sqrt();
        let mr_rms = (s_mr / n).sqrt();
        let r_tot = ((s_rl + s_rr) / (2.0 * n)).sqrt();
        let m_tot = ((s_ml + s_mr) / (2.0 * n)).sqrt();
        let raw_rat = if r_tot > 1.0 { m_tot / r_tot } else { 1.0 };
        let norm_rat = raw_rat * norm_scale;

        let ref_pan = if rl_rms + rr_rms > 10.0 {
            rr_rms / (rl_rms + rr_rms)
        } else {
            0.5
        };
        let m4w_pan = if ml_rms + mr_rms > 10.0 {
            mr_rms / (ml_rms + mr_rms)
        } else {
            0.5
        };
        let pan_diff = (m4w_pan - ref_pan).abs();

        let status = if r_tot < 50.0 && m_tot < 50.0 {
            "SILENT"
        } else {
            orders_checked += 1;
            if (0.72..=1.35).contains(&norm_rat) && pan_diff <= 0.25 {
                orders_ok += 1;
                "OK"
            } else {
                anomalies.push((ord, pat, norm_rat, pan_diff, info.clone()));
                "WARN"
            }
        };

        let _ = writeln!(
            out,
            "  {:03} ({:03}) [{:6.2}..{:6.2}s] | ({:7.1}, {:7.1}) | ({:7.1}, {:7.1}) | {:5.3}x |  {:5.3}x |  {:5.3}  | {}",
            ord,
            pat,
            s_f as f64 / sr as f64,
            e_clamped as f64 / sr as f64,
            rl_rms,
            rr_rms,
            ml_rms,
            mr_rms,
            raw_rat,
            norm_rat,
            pan_diff,
            status
        );
    }

    let _ = writeln!(
        out,
        "----------------------------------------------------------------------"
    );
    let _ = writeln!(
        out,
        "ZUSAMMENFASSUNG: {}/{} aktive Orders im Toleranzband | Dauer-Verhältnis: {:.3}x",
        orders_ok,
        orders_checked,
        if ref_secs > 0.1 {
            m4w_secs / ref_secs
        } else {
            1.0
        }
    );
    if !anomalies.is_empty() {
        let _ = writeln!(out, "\nKANAL-/VOICE-DIAGNOSE BEI AUFFÄLLIGEN ORDERS:");
        for (ord, pat, nrat, pdiff, info) in &anomalies {
            let _ = writeln!(
                out,
                "  -> Ord {:02} (Pat {:02}) [NormRatio={:.3}x, PanDiff={:.3}]: {}",
                ord, pat, nrat, pdiff, info
            );
        }
    }
    let _ = writeln!(
        out,
        "======================================================================\n"
    );

    let dur_ok = (ref_secs - m4w_secs).abs() <= 2.5
        || (0.90..=1.10).contains(&(m4w_secs / ref_secs.max(0.1)));
    let passed = dur_ok
        && rms_m4w_all > 10.0
        && (orders_checked == 0 || orders_ok * 10 >= orders_checked * 8);
    (passed, out)
}

pub fn run_stage_comparison(
    stage_title: &str,
    loaded_module: &LoadedModule,
    ref_wav: &WavData,
    oversamp: u8,
    pre_amp: u8,
    detailed_order_report: bool,
) -> bool {
    let (max_abs_diff, frame_delta, total_compared_samples, txt) = run_stage_comparison_to_string(
        stage_title,
        loaded_module,
        ref_wav,
        oversamp,
        pre_amp,
        detailed_order_report,
    );
    print!("{}", txt);
    max_abs_diff == 0 && frame_delta == 0 && total_compared_samples > 0
}

pub fn run_stage_comparison_to_string(
    stage_title: &str,
    loaded_module: &LoadedModule,
    ref_wav: &WavData,
    oversamp: u8,
    pre_amp: u8,
    detailed_order_report: bool,
) -> (i32, i64, usize, String) {
    let mut out = String::with_capacity(4096);
    let _ = writeln!(
        out,
        "\n----------------------------------------------------------------------"
    );
    let _ = writeln!(out, "{}", stage_title);
    let _ = writeln!(
        out,
        "----------------------------------------------------------------------"
    );

    let mut settings = SoundSettings::default();
    settings.device_id = DEV_HDD;
    settings.samplerate = ref_wav.sample_rate as i32;
    settings.stereo = 2;
    settings.bits = 16;
    settings.oversamp = oversamp;
    settings.panning = true;
    settings.surround = false;
    settings.pre_amp = pre_amp;

    let mut seq = Sequencer::new(loaded_module.clone(), settings);
    seq.module.state.repeat_song = false;

    let ref_frames = ref_wav.samples.len() / 2;
    let mut rust_frame_cursor = 0usize;
    let mut tick_idx = 0usize;

    let mut first_nonzero_rust: Option<(usize, i16, i16, u16, u8, u16, u16)> = None;
    let mut first_diff_gt0: Option<(usize, u16, u8, u16, u16, i16, i16, i16, i16)> = None;
    let mut first_diff_gt1: Option<(usize, u16, u8, u16, u16, i16, i16, i16, i16, String)> = None;
    let mut first_diff_gt16: Option<(usize, u16, u8, u16, u16, i16, i16, i16, i16, String)> = None;

    let mut max_abs_diff: i32 = 0;
    let mut sum_abs_diff: u64 = 0;
    let mut exact_match_samples: usize = 0;
    let mut within_1lsb_samples: usize = 0;
    let mut total_compared_samples: usize = 0;

    #[derive(Default, Clone)]
    struct OrderStats {
        phys_pat: u8,
        ticks: usize,
        frames: usize,
        max_diff: i32,
        sum_diff: u64,
        exact_samples: usize,
        within_1lsb: usize,
        first_gt16_row_tick: Option<(u16, u16, i32)>,
    }

    let num_orders = (seq.module.state.mi.num_patterns as usize).max(1);
    let mut order_stats = vec![OrderStats::default(); num_orders];

    while !seq.module.state.player_repeats && tick_idx < 500_000 {
        if seq.module.state.curr_vblanc == 0 {
            seq.process_row();
            if seq.module.state.player_repeats {
                break;
            }
        }
        seq.update_channels_for_mixing();
        if seq.module.state.player_repeats {
            break;
        }

        let order = seq.module.state.curr_pattern;
        let phys_pat = seq.module.state.p_arrangement[order as usize];
        let row = seq.module.state.curr_note;
        let tick = seq.module.state.curr_vblanc;

        seq.module.state.buf_samples = seq.module.state.num_samples.max(1);
        let am_repeating = seq.module.state.player_repeats && !seq.module.state.repeat_song;

        let mix_out = process_sample_block(
            &mut seq.module.state,
            &mut seq.mix_mem,
            &seq.module.sample_mem,
            am_repeating,
        );

        let pcm = match mix_out {
            MixOutputBuffer::Pcm16(s) => s,
            _ => Vec::new(),
        };
        let tick_frames = pcm.len() / 2;

        if (order as usize) < order_stats.len() {
            let os = &mut order_stats[order as usize];
            os.phys_pat = phys_pat;
            os.ticks += 1;
            os.frames += tick_frames;
        }

        for f in 0..tick_frames {
            let global_frame = rust_frame_cursor + f;
            let rl = pcm[f * 2];
            let rr = pcm[f * 2 + 1];

            if first_nonzero_rust.is_none() && (rl != 0 || rr != 0) {
                first_nonzero_rust = Some((global_frame, rl, rr, order, phys_pat, row, tick));
            }

            if global_frame < ref_frames {
                let wl = ref_wav.samples[global_frame * 2];
                let wr = ref_wav.samples[global_frame * 2 + 1];
                let dl = (rl as i32 - wl as i32).abs();
                let dr = (rr as i32 - wr as i32).abs();
                let dmax = dl.max(dr);

                total_compared_samples += 2;
                sum_abs_diff += (dl + dr) as u64;
                if dl == 0 {
                    exact_match_samples += 1;
                }
                if dr == 0 {
                    exact_match_samples += 1;
                }
                if dl <= 1 {
                    within_1lsb_samples += 1;
                }
                if dr <= 1 {
                    within_1lsb_samples += 1;
                }
                if dmax > max_abs_diff {
                    max_abs_diff = dmax;
                }

                if (order as usize) < order_stats.len() {
                    let os = &mut order_stats[order as usize];
                    if dmax > os.max_diff {
                        os.max_diff = dmax;
                    }
                    os.sum_diff += (dl + dr) as u64;
                    if dl == 0 {
                        os.exact_samples += 1;
                    }
                    if dr == 0 {
                        os.exact_samples += 1;
                    }
                    if dl <= 1 {
                        os.within_1lsb += 1;
                    }
                    if dr <= 1 {
                        os.within_1lsb += 1;
                    }
                    if dmax > 16 && os.first_gt16_row_tick.is_none() {
                        os.first_gt16_row_tick = Some((row, tick, dmax));
                    }
                }

                if dmax > 0 && first_diff_gt0.is_none() {
                    first_diff_gt0 = Some((global_frame, order, phys_pat, row, tick, rl, rr, wl, wr));
                }
                if dmax > 1 && first_diff_gt1.is_none() {
                    let ch_dump = dump_active_channels(&seq);
                    first_diff_gt1 = Some((
                        global_frame,
                        order,
                        phys_pat,
                        row,
                        tick,
                        rl,
                        rr,
                        wl,
                        wr,
                        ch_dump,
                    ));
                }
                if dmax > 16 && first_diff_gt16.is_none() {
                    let ch_dump = dump_active_channels(&seq);
                    first_diff_gt16 = Some((
                        global_frame,
                        order,
                        phys_pat,
                        row,
                        tick,
                        rl,
                        rr,
                        wl,
                        wr,
                        ch_dump,
                    ));
                }
            }
        }

        rust_frame_cursor += tick_frames;
        seq.advance_tick();
        tick_idx += 1;
    }

    let frame_delta = rust_frame_cursor as i64 - ref_frames as i64;
    let _ = writeln!(
        out,
        "Tick-Timing: {} Ticks gerendert | Rust Stereo-Frames: {} vs. WAV Stereo-Frames: {} (Delta: {} Frames)",
        tick_idx, rust_frame_cursor, ref_frames, frame_delta
    );
    if let Some((f, rl, rr, ord, pat, row, tck)) = first_nonzero_rust {
        let _ = writeln!(
            out,
            "Erster Non-Zero Frame (Rust): Frame {} ({}, {}) bei Order {:02} (UI-Pat {:02}, PhysPat {:02}) | Row {:02} | Tick {}",
            f,
            rl,
            rr,
            ord,
            ord + 1,
            pat,
            row,
            tck
        );
    }
    let mae = if total_compared_samples > 0 {
        sum_abs_diff as f64 / total_compared_samples as f64
    } else {
        0.0
    };
    let exact_pct = if total_compared_samples > 0 {
        100.0 * exact_match_samples as f64 / total_compared_samples as f64
    } else {
        0.0
    };
    let lsb1_pct = if total_compared_samples > 0 {
        100.0 * within_1lsb_samples as f64 / total_compared_samples as f64
    } else {
        0.0
    };
    let one_lsb_only = within_1lsb_samples.saturating_sub(exact_match_samples);
    let gt1_lsb = total_compared_samples.saturating_sub(within_1lsb_samples);
    let _ = writeln!(
        out,
        "Gesamt-Statistik: Exact={:.6}% ({}/{}) | 1-LSB={} | >1-LSB={} | <=1 LSB={:.6}% | MAE={:.6} LSB | MaxDiff={} LSB",
        exact_pct,
        exact_match_samples,
        total_compared_samples,
        one_lsb_only,
        gt1_lsb,
        lsb1_pct,
        mae,
        max_abs_diff
    );

    if let Some((f, ord, pat, row, tck, rl, rr, wl, wr)) = first_diff_gt0 {
        let _ = writeln!(
            out,
            "Erste Abweichung > 0 LSB: Frame {} (Order {:02} / UI-Pat {:02} [Phys {:02}] | Row {:02} | Tick {}) -> Rust=({}, {}) vs WAV=({}, {})",
            f,
            ord,
            ord + 1,
            pat,
            row,
            tck,
            rl,
            rr,
            wl,
            wr
        );
    }
    if let Some((f, ord, pat, row, tck, rl, rr, wl, wr, dump)) = first_diff_gt1 {
        let _ = writeln!(
            out,
            "Erste Abweichung > 1 LSB: Frame {} (Order {:02} / UI-Pat {:02} [Phys {:02}] | Row {:02} | Tick {}) -> Rust=({}, {}) vs WAV=({}, {})",
            f,
            ord,
            ord + 1,
            pat,
            row,
            tck,
            rl,
            rr,
            wl,
            wr
        );
        if detailed_order_report {
            let _ = write!(out, "  Aktive Kanäle bei erster Abweichung > 1 LSB:\n{}", dump);
        }
    }
    if let Some((f, ord, pat, row, tck, rl, rr, wl, wr, dump)) = first_diff_gt16 {
        let _ = writeln!(
            out,
            "\nErste Abweichung > 16 LSB: Frame {} (Order {:02} / UI-Pat {:02} [Phys {:02}] | Row {:02} | Tick {}) -> Rust=({}, {}) vs WAV=({}, {})",
            f,
            ord,
            ord + 1,
            pat,
            row,
            tck,
            rl,
            rr,
            wl,
            wr
        );
        if detailed_order_report {
            let _ = write!(
                out,
                "  Aktive Kanäle bei erster Abweichung > 16 LSB:\n{}",
                dump
            );
        }
    }

    if detailed_order_report {
        let _ = writeln!(
            out,
            "\n\nDetail-Bericht pro Order (Fokus: Order 0..5, Pattern 44-50 [Order 43..49] & Ende 100..108):"
        );
        let _ = writeln!(
            out,
            "  Ord (UI-Pat) [Phys] | Ticks | Frames | <=1 LSB % | MAE (LSB) | MaxDiff | Erste Abw >16 (Row,Tick,Diff)"
        );
        for (ord, os) in order_stats.iter().enumerate() {
            if os.frames == 0 {
                continue;
            }
            let tot_smp = os.frames * 2;
            let pct1 = 100.0 * os.within_1lsb as f64 / tot_smp as f64;
            let ord_mae = os.sum_diff as f64 / tot_smp as f64;
            let show = ord <= 5 || (42..=51).contains(&ord) || ord >= 99 || os.max_diff > 32;
            if show {
                let _ = writeln!(
                    out,
                    "  {:03} (Pat {:03}) [{:03}] | {:5} | {:6} | {:8.2}% | {:9.3} | {:7} | {:?}",
                    ord,
                    ord + 1,
                    os.phys_pat,
                    os.ticks,
                    os.frames,
                    pct1,
                    ord_mae,
                    os.max_diff,
                    os.first_gt16_row_tick
                );
            }
        }
    }

    (max_abs_diff, frame_delta, total_compared_samples, out)
}

pub fn dump_active_channels(seq: &Sequencer) -> String {
    let mut out = String::new();
    let num_tracks = (seq.module.state.mi.num_tracks as usize).min(32);
    for ch in 0..num_tracks {
        let c = seq.module.state.channels[ch];
        let inst = c.inst;
        let smp_num = c.smp_num;
        let note = c.note;
        let rest = c.rest as u16;
        let iend = c.iend;
        let lln = c.lln;
        let tonh = c.tonh;
        let phi = c.phi;
        let plo = c.plo;
        let vol = c.vol;
        let env_vol = c.env_vol;
        let fade_vol = c.fade_vol;
        let pan_val = c.pan_val;
        let l_vol = c.l_vol;
        let r_vol = c.r_vol;
        let effkt = c.effkt;
        let opnd = c.opnd;
        let jmp_flags = c.jmp_flags;
        if note != 0 || l_vol != 0 || r_vol != 0 || effkt != 0 {
            out.push_str(&format!(
                "    Ch{:02}: inst={} smp={} note(esi)={} rest={} iend={} lln={} tonh={} phi={}:{} vol={} env_vol={} fade={} pan={} l/r={}/{} eff={}:0x{:02X} flags=0x{:02X}\n",
                ch,
                inst,
                smp_num,
                note,
                rest,
                iend,
                lln,
                tonh,
                phi,
                plo,
                vol,
                env_vol,
                fade_vol,
                pan_val,
                l_vol,
                r_vol,
                effkt,
                opnd,
                jmp_flags
            ));
        }
    }
    out
}

