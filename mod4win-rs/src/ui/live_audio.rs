//! Live-Audio-Pipeline (`cpal`), Tick-synchroner Parameter-Wechsel, Resampling (`seq_rate -> cpal_rate`),
//! Mono/8-Bit-Aufbereitung und Anti-Click-Rampen bei `Stop` / `Pause`.
//!
//! Löst die Live-Wiedergabe sauber von der festen Hardware-Samplerate des `cpal`-Ausgabegeräts,
//! ohne die bit-exakte Offline-Render-Pipeline (`--diff-suite` / `DEV_HDD`) in `mixer.rs` oder
//! `sequencer.rs` zu verändern.

use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::app::{InstrumentInfo, PendingPeakSearch, UiSharedState};
use super::archive_mol::read_module_bytes_universal;
use super::ini_config::{export_module_to_wav_file, Mod4WinIniConfig};
use crate::engine::loader::load_module_with_ext;
use crate::engine::mixer::{process_sample_block, MixOutputBuffer};
use crate::engine::sequencer::Sequencer;
use crate::engine::types::{
    mixer_flags, ModType, SoundSettings, DEV_DAC, MAX_M4W_CHANNELS, MAX_MIX_BUF_SAMPLES,
    MIX_BUFFER_OFS, SURR_LEN, SUR_BUFFER_OFS,
};

/// Wandelt ein vorzeichenloses 8-Bit-PCM-Sample (`0..=255`, Stille = `128`) verlustfrei
/// in ein vorzeichenbehaftetes 16-Bit-PCM-Sample (`-32768..=32512`, Stille = `0`) um.
#[inline(always)]
pub fn pcm8_to_i16(b: u8) -> i16 {
    ((b as i16) - 128) << 8
}

/// Übernimmt geänderte [`SoundSettings`] aus dem UI/Setup-Dialog sauber an einer Tick-Grenze
/// in den laufenden [`Sequencer`], ohne aktive Instrumenten-Flags (`F_16BIT`, `F_HI_PITCH`)
/// oder Sample-Zeiger (`note`, `iend`, `lln`, `lend`, `rest`) zu beschädigen.
pub fn apply_live_sound_settings(seq: &mut Sequencer, new_settings: SoundSettings) {
    let old_settings = seq.module.state.sound_settings;

    // 1. Kosmetische/historische Optionen, Clean-Mode & De-Click übernehmen (TimerMode, CleanMode, DeClick, nBuffers)
    seq.module.state.sound_settings.set_timer_mode(new_settings.timer_mode());
    seq.module.state.sound_settings.set_clean_mode(new_settings.clean_mode());
    seq.module.state.sound_settings.set_declick(new_settings.declick());
    seq.module.state.sound_settings.n_buffers = new_settings.n_buffers.clamp(1, 99);
    // Im Live-Soundkarten-Betrieb muss `device_id` immer `DEV_DAC` (0) sein, damit `process_sample_block`
    // keine 32-KB-HDD-Block-Teilung ausführt.
    seq.module.state.sound_settings.device_id = DEV_DAC;

    let new_sr = new_settings.samplerate.clamp(4000, 96000);
    let old_sr = old_settings.samplerate.clamp(4000, 96000);
    let new_bits = if new_settings.bits == 8 { 8 } else { 16 };
    let new_stereo = new_settings.stereo.clamp(1, 2);
    let new_surround = new_settings.surround && new_stereo == 2;
    let new_oversamp = new_settings.oversamp.min(6);
    let new_panning = new_settings.panning;
    let new_pre_amp = new_settings.pre_amp;
    let new_no_bpm = new_settings.no_bpm;

    let audio_unchanged = new_sr == old_sr
        && new_bits == old_settings.bits
        && new_stereo == old_settings.stereo
        && new_surround == seq.module.state.use_surround
        && new_oversamp == old_settings.oversamp
        && new_panning == old_settings.panning
        && new_pre_amp == old_settings.pre_amp
        && new_no_bpm == old_settings.no_bpm;

    // Wenn sich z. B. nur `TimerMode` (`Interrupt`/`Task`/`Timer`) geändert hat,
    // keinerlei Mixer- oder Kanalregister anfassen (100% unterbrechungsfrei!).
    if audio_unchanged {
        return;
    }

    // 2. Samplerate-Wechsel: `num_samples` (`buf_samples`) über aktuelle BPM neu berechnen
    //    und die 16.16-Phasen-Inkremente (`phi`/`plo`) aller aktiven Kanäle umskalieren,
    //    damit laufende Noten ihre exakte Tonhöhe beibehalten!
    if new_sr != old_sr {
        seq.module.state.sound_settings.samplerate = new_sr;
        let sr_u32 = new_sr as u32;
        let cx_bpm = (seq.module.state.bpm_speed.max(0x20)) as u32;
        let eax = (sr_u32 << 1) + (sr_u32 >> 1);
        let num_samples = (eax / cx_bpm).min(MAX_MIX_BUF_SAMPLES as u32) as u16;
        seq.module.state.num_samples = num_samples.max(1);
        seq.module.state.buf_samples = seq.module.state.num_samples;

        for chn in seq.module.state.channels.iter_mut() {
            let old_step = (((chn.phi as u32) << 16) | (chn.plo as u32)) as u64;
            if old_step > 0 {
                let new_step = ((old_step * (old_sr as u64) + (new_sr as u64 / 2))
                    / (new_sr as u64))
                    .min(0x7FFF_FFFF) as i32;
                chn.mod_event.pitch = new_step;
                if new_step > 30000 {
                    chn.jmp_flags |= mixer_flags::F_HI_PITCH;
                } else {
                    chn.jmp_flags &= !mixer_flags::F_HI_PITCH;
                }
                chn.plo = (new_step as u32 & 0xFFFF) as u16;
                chn.phi = new_step >> 16;
            }
        }

        // Surround-Verzögerungszeiger an neue Samplerate anpassen
        let surr_start =
            (((8i64 * (new_sr as i64) / 1000 * (SURR_LEN as i64)) as u16) & !7) & 0x1FFF;
        seq.module.state.buf_start = seq.module.state.buf_end.wrapping_add(surr_start) & 0x1FFF;
    }

    // 3. Bits, Stereo/Mono, Panning, IDO, PreAmp & NoBPM setzen
    seq.module.state.sound_settings.bits = new_bits;
    seq.module.state.sound_settings.stereo = new_stereo;
    seq.module.state.sound_settings.panning = new_panning;
    seq.module.state.sound_settings.oversamp = new_oversamp;
    seq.module.state.sound_settings.pre_amp = new_pre_amp;

    if new_no_bpm != old_settings.no_bpm {
        let saved_order = seq.module.state.curr_pattern;
        let saved_row = seq.module.state.curr_note;
        seq.module.state.sound_settings.no_bpm = new_no_bpm;
        seq.get_total_time();
        seq.seek_to_order_and_row(saved_order, saved_row);
    }

    // 4. Surround-Status sauber umschalten
    let old_surround = seq.module.state.use_surround;
    seq.module.state.sound_settings.surround = new_surround;
    seq.module.state.use_surround = new_surround;
    let num_tracks = (seq.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

    if new_surround != old_surround {
        if new_surround {
            seq.mix_mem.surr_ring.fill(Default::default());
            seq.module.state.l_sur_fade_out = 0;
            seq.module.state.r_sur_fade_out = 0;
            seq.module.state.buf_end = 0;
            let surr_start =
                (((8i64 * (new_sr as i64) / 1000 * (SURR_LEN as i64)) as u16) & !7) & 0x1FFF;
            seq.module.state.buf_start = surr_start;

            for ch_idx in 0..num_tracks {
                let inst = seq.module.state.channels[ch_idx].inst;
                let pan_val = seq.module.state.channels[ch_idx].pan_val;
                let tonh = seq.module.state.channels[ch_idx].tonh;
                let mut target_buf = MIX_BUFFER_OFS as i32;
                if matches!(pan_val, 0..=15 | 240..=255) && seq.do_surround(inst, pan_val, tonh) {
                    target_buf = SUR_BUFFER_OFS as i32;
                }
                seq.module.state.channels[ch_idx].mix_buf = target_buf;
            }
        } else {
            for chn in seq.module.state.channels.iter_mut() {
                chn.mix_buf = MIX_BUFFER_OFS as i32;
            }
            seq.module.state.l_sur_fade_out = 0;
            seq.module.state.r_sur_fade_out = 0;
        }
    }

    // 5. Kanal-Interpolationsflags (`F_INTERP_MASK = 0x0060`) aktualisieren,
    //    OHNE `F_16BIT (0x0010)`, `F_HI_PITCH (0x0008)` oder `F_PAN_MASK (0x0006)` zu überschreiben!
    let jmp_interp = match new_oversamp {
        1 | 5 => mixer_flags::F_OVERSAMP,
        2 | 6 => mixer_flags::F_CPU_CUBIC,
        3 => mixer_flags::F_FPU_CUBIC,
        _ => 0,
    };
    for chn in seq.module.state.channels.iter_mut() {
        chn.jmp_flags = (chn.jmp_flags & !mixer_flags::F_INTERP_MASK) | jmp_interp;
    }

    // 6. Master-Volume & Volume-Lookup-Tabelle neu berechnen und `vol_adjust_1` für alle Kanäle
    //    ausführen, damit `l_vol`, `r_vol` und die Panning-Routing-Bits (`F_MONO`/`F_LEFT`/`F_RIGHT`/`F_PANNED`)
    //    sofort konsistent sind.
    seq.calc_vol_master_and_table();
    for ch_idx in 0..num_tracks {
        seq.vol_adjust_1(ch_idx);
    }
}

/// Live-Audio-Prozessor für den `cpal`-Ausgabestream.
///
/// Entkoppelt die interne MOD4WIN-Samplerate (`11025..48000` Hz), Bittiefe (`8` / `16` Bit)
/// und Kanalzahl (`Mono` / `Stereo` / `Surround`) von der festen `cpal`-Hardware-Konfiguration
/// und verhindert Knacksen bei `Stop`, `Pause` und Parameter-Wechseln.
pub struct LiveAudioProcessor {
    /// Tatsächliche Hardware-Samplerate des `cpal`-Ausgabestreams (z. B. `48000` Hz).
    pub cpal_sample_rate: u32,
    /// Warteschlange bereits gerenderter Stereo-Frames `(Left, Right)` in der Sequencer-Samplerate.
    pub frame_queue: VecDeque<(i16, i16)>,
    /// Gebrochene 32.32-Fixed-Point-Resampling-Phase (`0 .. (1 << 32)`).
    pub resample_frac_fp: u64,
    /// Letztes an `cpal` ausgegebenes Stereo-Sample-Paar (für Anti-Click-Rampen).
    pub last_out_l: i16,
    pub last_out_r: i16,
    /// Gibt an, ob im vorherigen Callback aktiv abgespielt wurde (`is_playing && !is_paused`).
    pub was_playing: bool,
    /// Startwerte und Zähler für die ~2 ms Anti-Click-Fade-Out-Rampe bei `Stop` / `Pause`.
    pub fade_start_l: i16,
    pub fade_start_r: i16,
    pub fade_out_remaining: u32,
    pub fade_out_total: u32,
    /// Zähler für die ~1.5 ms Anti-Click-Fade-In-Rampe bei `Play` / `Resume`.
    pub fade_in_remaining: u32,
    pub fade_in_total: u32,
    /// Virtueller 1995er SoundBlaster-DMA-Ringpuffer für `--authentic-stutter`.
    pub dma_ring: Vec<(i16, i16)>,
    pub dma_pos: usize,
    pub stutter_tick: u32,
}

impl LiveAudioProcessor {
    /// Erstellt einen neuen [`LiveAudioProcessor`] für die gegebene `cpal`-Hardware-Samplerate.
    pub fn new(cpal_sample_rate: u32) -> Self {
        Self {
            cpal_sample_rate: cpal_sample_rate.max(1),
            frame_queue: VecDeque::with_capacity(MAX_MIX_BUF_SAMPLES * 2),
            resample_frac_fp: 0,
            last_out_l: 0,
            last_out_r: 0,
            was_playing: false,
            fade_start_l: 0,
            fade_start_r: 0,
            fade_out_remaining: 0,
            fade_out_total: 1,
            fade_in_remaining: 0,
            fade_in_total: 1,
            dma_ring: vec![(0, 0); 2048],
            dma_pos: 0,
            stutter_tick: 0,
        }
    }

    /// Leert die internen Frame-Warteschlangen nach einem Pattern-Seek oder Modul-Wechsel
    /// und startet eine kurze Anti-Click-Überblendung vom letzten Ausgangswert.
    pub fn reset_on_seek_or_load(&mut self, seq: &mut Sequencer) {
        self.frame_queue.clear();
        self.resample_frac_fp = 0;
        seq.pending_pcm.clear();
        seq.pending_read_idx = 0;
        seq.tick_prepared = false;

        if self.last_out_l != 0 || self.last_out_r != 0 {
            let fade_in_len = ((self.cpal_sample_rate * 3) / 2000).max(24);
            self.fade_start_l = self.last_out_l;
            self.fade_start_r = self.last_out_r;
            self.fade_in_total = fade_in_len;
            self.fade_in_remaining = fade_in_len;
        }
    }

    /// Rendert genau einen Sequencer-Tick und legt die erzeugten Samples als normalisierte
    /// 16-Bit-Stereo-Frames `(Left, Right)` in `self.frame_queue` ab:
    /// - Mono (`stereo == 1`) wird auf beide Kanäle `(m, m)` dupliziert (verhindert doppelte Geschwindigkeit!).
    /// - 8-Bit Unsigned PCM (`0..255`, Stille = `128`) wird über [`pcm8_to_i16`] (`((b - 128) << 8)`) konvertiert.
    pub fn render_one_tick_to_queue(
        &mut self,
        seq: &mut Sequencer,
        muted_channels: &[bool; 32],
    ) {
        // Sicherstellen, dass `device_id` im Live-Modus immer `DEV_DAC` (0) ist
        seq.module.state.sound_settings.device_id = DEV_DAC;

        if let Some(mix_out) = seq.render_it_tick(Some(muted_channels)) {
            let is_stereo = seq.module.state.sound_settings.stereo >= 2;
            match mix_out {
                MixOutputBuffer::Pcm16(samples) => {
                    if is_stereo {
                        for pair in samples.as_chunks::<2>().0 {
                            self.frame_queue.push_back((pair[0], pair[1]));
                        }
                    } else {
                        for &m in &samples {
                            self.frame_queue.push_back((m, m));
                        }
                    }
                }
                MixOutputBuffer::Pcm8(bytes) => {
                    if is_stereo {
                        for pair in bytes.as_chunks::<2>().0 {
                            let l = pcm8_to_i16(pair[0]);
                            let r = pcm8_to_i16(pair[1]);
                            self.frame_queue.push_back((l, r));
                        }
                    } else {
                        for &b in &bytes {
                            let m = pcm8_to_i16(b);
                            self.frame_queue.push_back((m, m));
                        }
                    }
                }
                MixOutputBuffer::PeakSearchOnly => {
                    let n = seq.module.state.buf_samples.max(1) as usize;
                    for _ in 0..n {
                        self.frame_queue.push_back((0, 0));
                    }
                }
            }
            return;
        }

        if !seq.tick_prepared {
            if seq.module.state.curr_vblanc == 0 {
                seq.process_row();
            }
            seq.update_channels_for_mixing();
            seq.tick_prepared = true;
        }

        // Stummgeschaltete Kanäle vor dem Mischen auf Lautstärke 0 setzen, ohne `note` zu löschen
        // (`@VOL0` in `mixer.rs` schaltet die Sample-Position stumm weiter).
        let num_tracks = (seq.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for (i, &muted) in muted_channels.iter().enumerate().take(num_tracks) {
            if muted {
                seq.module.state.channels[i].avg_vol = 0;
                seq.module.state.channels[i].l_vol = 0;
                seq.module.state.channels[i].r_vol = 0;
            }
        }

        seq.module.state.buf_samples = seq.module.state.num_samples.max(1);
        let am_repeating = seq.module.state.player_repeats && !seq.module.state.repeat_song;
        let is_stereo = seq.module.state.sound_settings.stereo >= 2;

        let mix_out = process_sample_block(
            &mut seq.module.state,
            &mut seq.mix_mem,
            &seq.module.sample_mem,
            am_repeating,
        );

        match mix_out {
            MixOutputBuffer::Pcm16(samples) => {
                if is_stereo {
                    for pair in samples.as_chunks::<2>().0 {
                        self.frame_queue.push_back((pair[0], pair[1]));
                    }
                } else {
                    for &m in &samples {
                        self.frame_queue.push_back((m, m));
                    }
                }
            }
            MixOutputBuffer::Pcm8(bytes) => {
                if is_stereo {
                    for pair in bytes.as_chunks::<2>().0 {
                        let l = pcm8_to_i16(pair[0]);
                        let r = pcm8_to_i16(pair[1]);
                        self.frame_queue.push_back((l, r));
                    }
                } else {
                    for &b in &bytes {
                        let m = pcm8_to_i16(b);
                        self.frame_queue.push_back((m, m));
                    }
                }
            }
            MixOutputBuffer::PeakSearchOnly => {
                let n = seq.module.state.buf_samples.max(1) as usize;
                for _ in 0..n {
                    self.frame_queue.push_back((0, 0));
                }
            }
        }

        seq.tick_prepared = false;
        seq.advance_tick();
    }

    /// Füllt `out_frames` mit Stereo-Frames `(Left, Right)` in der `cpal`-Hardware-Samplerate:
    /// - Führt bei `Stop` oder `Pause` eine sanfte ~2 ms Anti-Click-Rampe gegen `0` aus und füllt danach exakt `0`.
    /// - Übernimmt ausstehende `pending_settings` exakt an der Tick-Grenze vor dem nächsten Tick.
    /// - Resamplet von `seq.module.state.sound_settings.samplerate` auf `self.cpal_sample_rate`.
    pub fn fill_cpal_stereo_frames(
        &mut self,
        seq: &mut Sequencer,
        out_frames: &mut [(i16, i16)],
        is_playing: bool,
        is_paused: bool,
        stutter_active: bool,
        muted_channels: &[bool; 32],
        pending_settings: &mut Option<SoundSettings>,
    ) {
        let active_play = is_playing && !is_paused;

        if !active_play {
            if self.was_playing {
                self.was_playing = false;
                self.fade_in_remaining = 0;
                if self.last_out_l != 0 || self.last_out_r != 0 {
                    let fade_len = ((self.cpal_sample_rate * 2) / 1000).max(32);
                    self.fade_start_l = self.last_out_l;
                    self.fade_start_r = self.last_out_r;
                    self.fade_out_total = fade_len;
                    self.fade_out_remaining = fade_len;
                }
                self.dma_ring.fill((0, 0));
                self.stutter_tick = 0;
            }

            // Bei `Stop` (`!is_playing && !is_paused`) zusätzlich alle Restpuffer & Fade-Register leeren
            if !is_playing && !is_paused {
                self.frame_queue.clear();
                self.resample_frac_fp = 0;
                seq.pending_pcm.clear();
                seq.pending_read_idx = 0;
                seq.tick_prepared = false;
                seq.module.state.l_fade_out = 0;
                seq.module.state.r_fade_out = 0;
                seq.module.state.l_sur_fade_out = 0;
                seq.module.state.r_sur_fade_out = 0;
            }

            if let Some(new_settings) = pending_settings.take() {
                apply_live_sound_settings(seq, new_settings);
            }

            for (out_l, out_r) in out_frames.iter_mut() {
                if self.fade_out_remaining > 0 {
                    self.fade_out_remaining -= 1;
                    let rem = self.fade_out_remaining as i32;
                    let tot = (self.fade_out_total.max(1)) as i32;
                    let l = ((self.fade_start_l as i32) * rem / tot) as i16;
                    let r = ((self.fade_start_r as i32) * rem / tot) as i16;
                    self.last_out_l = l;
                    self.last_out_r = r;
                    *out_l = l;
                    *out_r = r;
                } else {
                    self.last_out_l = 0;
                    self.last_out_r = 0;
                    *out_l = 0;
                    *out_r = 0;
                }
            }
            return;
        }

        // Übergang von Stop/Pause -> Play: kurze ~1.5 ms Anti-Click-Fade-In-Rampe starten
        if !self.was_playing {
            self.was_playing = true;
            self.fade_out_remaining = 0;
            let fade_in_len = ((self.cpal_sample_rate * 3) / 2000).max(24);
            self.fade_start_l = self.last_out_l;
            self.fade_start_r = self.last_out_r;
            self.fade_in_total = fade_in_len;
            self.fade_in_remaining = fade_in_len;
        }

        // Optionaler 1995er SoundBlaster-DMA-Ringpuffer-Stutter (`--authentic-stutter` + Puffer = 00.0s)
        let ring_len = self.dma_ring.len().max(1);
        if stutter_active {
            self.stutter_tick = self.stutter_tick.wrapping_add(1);
            if self.stutter_tick % 4 != 1 {
                for (out_l, out_r) in out_frames.iter_mut() {
                    let (dl, dr) = self.dma_ring[self.dma_pos % ring_len];
                    self.dma_pos = (self.dma_pos + 1) % ring_len;
                    self.last_out_l = dl;
                    self.last_out_r = dr;
                    *out_l = dl;
                    *out_r = dr;
                }
                return;
            }
        } else {
            self.stutter_tick = 0;
        }

        let out_rate = (self.cpal_sample_rate.max(1)) as u64;

        for (out_l, out_r) in out_frames.iter_mut() {
            // Neue Sequencer-Ticks nachrendern, sobald weniger als 2 Stützpunkte in der Queue liegen.
            // Ausstehende Parameter-Änderungen (`pending_settings`) werden exakt hier an der Tick-Grenze übernommen!
            let mut guard = 0usize;
            while self.frame_queue.len() < 2 && guard < 16 {
                if let Some(new_settings) = pending_settings.take() {
                    apply_live_sound_settings(seq, new_settings);
                }
                self.render_one_tick_to_queue(seq, muted_channels);
                guard += 1;
            }

            if self.frame_queue.is_empty() {
                *out_l = 0;
                *out_r = 0;
                self.last_out_l = 0;
                self.last_out_r = 0;
                continue;
            }

            let seq_rate = (seq.module.state.sound_settings.samplerate.clamp(4000, 96000)) as u64;
            let (l0, r0) = self.frame_queue[0];
            let (l1, r1) = if self.frame_queue.len() >= 2 {
                self.frame_queue[1]
            } else {
                (l0, r0)
            };

            let (raw_l, raw_r) = if seq_rate == out_rate && self.resample_frac_fp == 0 {
                self.frame_queue.pop_front();
                (l0, r0)
            } else {
                let alpha = (self.resample_frac_fp >> 16) as i32; // 0..=65535
                let l = (l0 as i32 + (((l1 as i32 - l0 as i32) * alpha) >> 16))
                    .clamp(-32768, 32767) as i16;
                let r = (r0 as i32 + (((r1 as i32 - r0 as i32) * alpha) >> 16))
                    .clamp(-32768, 32767) as i16;

                let step_fp = (seq_rate << 32) / out_rate;
                self.resample_frac_fp = self.resample_frac_fp.wrapping_add(step_fp);
                let whole_steps = (self.resample_frac_fp >> 32) as usize;
                self.resample_frac_fp &= 0xFFFF_FFFF;
                for _ in 0..whole_steps {
                    if !self.frame_queue.is_empty() {
                        self.frame_queue.pop_front();
                    }
                }
                (l, r)
            };

            let (final_l, final_r) = if self.fade_in_remaining > 0 {
                self.fade_in_remaining -= 1;
                let rem = self.fade_in_remaining as i32;
                let tot = (self.fade_in_total.max(1)) as i32;
                let inv = tot - rem;
                let l = (((self.fade_start_l as i32) * rem + (raw_l as i32) * inv) / tot) as i16;
                let r = (((self.fade_start_r as i32) * rem + (raw_r as i32) * inv) / tot) as i16;
                (l, r)
            } else {
                (raw_l, raw_r)
            };

            *out_l = final_l;
            *out_r = final_r;
            self.last_out_l = final_l;
            self.last_out_r = final_r;
            self.dma_ring[self.dma_pos % ring_len] = (final_l, final_r);
            self.dma_pos = (self.dma_pos + 1) % ring_len;
        }
    }
}

/// Prüft im Audio-Thread/Callback auf Liedende (`player_repeats`) und `15s IntroScan` (`play_time >= 15000`),
/// wertet Repeat (`0=Off, 1=All, 2=One`) & Shuffle (`number_list`) aus und synchronisiert UI-Kommandos.
pub fn handle_audio_cycle_and_sync(
    ui: &mut UiSharedState,
    seq: &mut Sequencer,
    live_proc: &mut LiveAudioProcessor,
    pending_settings: &mut Option<SoundSettings>,
    no_gui: bool,
    finished_clone: &Arc<AtomicBool>,
) {
    sync_ui_commands(ui, seq, live_proc, pending_settings);

    let intro_scan_expired = ui.is_playing
        && !ui.is_paused
        && ui.intro_scan
        && seq.module.state.play_time >= 15_000;

    if seq.module.state.player_repeats || intro_scan_expired {
        seq.module.state.player_repeats = false;
        if no_gui {
            finished_clone.store(true, Ordering::SeqCst);
            return;
        }
        ui.on_auto_advance(intro_scan_expired);
        sync_ui_commands(ui, seq, live_proc, pending_settings);
    }
}

/// Synchronisiert ausstehende UI-Kommandos (Pattern-Seek, Settings, Mutes, Song-Load, WAV-Export)
/// mit dem laufenden [`Sequencer`] und [`LiveAudioProcessor`].
pub fn sync_ui_commands(
    ui: &mut UiSharedState,
    seq: &mut Sequencer,
    live_proc: &mut LiveAudioProcessor,
    pending_settings: &mut Option<SoundSettings>,
) {
    if let Some(target_pat) = ui.pending_seek_pattern.take() {
        seq.seek_to_pattern(target_pat);
        seq.module.state.player_repeats = false;
        live_proc.reset_on_seek_or_load(seq);
        if !ui.is_playing && !ui.is_paused {
            seq.module.state.play_time = 0;
            ui.elapsed_secs = 0;
        }
    } else if ui.pending_seek_delta != 0 {
        seq.step_order(ui.pending_seek_delta);
        ui.pending_seek_delta = 0;
        seq.module.state.player_repeats = false;
        live_proc.reset_on_seek_or_load(seq);
    }

    if ui.settings_dirty {
        ui.settings_dirty = false;
        *pending_settings = Some(ui.sound_settings);
    }

    if ui.channel_muted_dirty {
        ui.channel_muted_dirty = false;
        let num_tracks = (seq.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for ch_idx in 0..num_tracks {
            if !ui.channel_muted[ch_idx] {
                seq.vol_adjust_1(ch_idx);
            }
        }
    }

    if let Some(new_path) = ui.pending_load_file.take()
        && let Ok((file_data, ext_opt)) = read_module_bytes_universal(&new_path)
            && let Ok(new_mod) = load_module_with_ext(&file_data, ext_opt.as_deref()) {
                let mod_name_len = new_mod.state.mi.mod_name_len as usize;
                let mod_name = String::from_utf8_lossy(&new_mod.state.mi.mod_name[..mod_name_len.min(40)])
                    .trim()
                    .to_string();
                let num_instruments = new_mod.state.mi.num_instruments as usize;
                let mut inst_names = Vec::new();
                let mut inst_infos = Vec::new();
                for i in 1..=num_instruments.min(255) {
                    let inst = &new_mod.state.mi.instruments[i];
                    let len = (inst.name_len as usize).min(40);
                    let name_str = String::from_utf8_lossy(&inst.name[..len]).trim().to_string();
                    inst_names.push(name_str.clone());
                    let length = (inst.iend - inst.offset).max(0);
                    let (loop_start, loop_length) = if inst.loop_end > 0 {
                        (
                            (inst.loop_end - inst.loop_start - inst.offset).max(0),
                            inst.loop_start.max(0),
                        )
                    } else {
                        (0, 0)
                    };
                    inst_infos.push(InstrumentInfo {
                        index: i,
                        name: name_str,
                        length,
                        loop_start,
                        loop_length,
                        fine_tune: inst.fine_tune as i8,
                        volume: inst.volume,
                        is_16bit: (inst.bits & 0x10) != 0,
                        c4_freq: inst.tuning as i32,
                    });
                }
                ui.title = mod_name;
                ui.filename = new_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_uppercase();
                ui.current_file_path = Some(new_path.clone());
                ui.format_str = format!("{:?}", new_mod.state.mi.file_type);
                ui.num_channels = (new_mod.state.mi.num_tracks as usize).min(32);
                ui.num_patterns = new_mod.state.mi.num_patterns as usize;
                ui.num_orders = new_mod.state.mi.num_patterns as usize;
                ui.num_unique_patterns = new_mod.state.mi.diff_patterns as usize;
                ui.file_size_kb = (file_data.len() / 1024).max(1);
                ui.song_mem_bytes = new_mod.state.mi.song_mem as usize;
                ui.total_time_secs = (new_mod.state.mi.total_time / 1000).max(0) as u32;
                ui.instrument_names = inst_names;
                ui.instruments = inst_infos;
                ui.effects_used_set = new_mod.state.mi.effects_used_set;
                ui.channel_muted = [false; 32];

                if new_mod.state.mi.file_type != ModType::IT && ui.sound_settings.oversamp >= 4 {
                    ui.sound_settings.oversamp = match ui.sound_settings.oversamp {
                        4 => 0,
                        5 => 1,
                        _ => 2,
                    };
                    ui.ini_config.over_sampling = ui.sound_settings.oversamp;
                }
                let mut cur_settings = ui.sound_settings;
                seq.module = new_mod;
                seq.module.state.sound_settings = cur_settings;
                seq.kanal_init(false);
                if ui.ini_config.auto_preamp_peaksearch {
                    let pks_path = Mod4WinIniConfig::default_pks_path();
                    let f_size = file_data.len() as u32;
                    if ui.ini_config.state_saver {
                        if let Some(cached_peak) =
                            ui.ini_config.find_peak(f_size, &ui.title, &pks_path)
                        {
                            let pa = (cached_peak & 0x7F).min(40);
                            cur_settings.pre_amp = (cur_settings.pre_amp & 0x80) | pa;
                            ui.sound_settings.pre_amp = cur_settings.pre_amp;
                            ui.ini_config.pre_amplification = pa;
                            seq.module.state.sound_settings = cur_settings;
                        } else {
                            ui.pending_initial_peak_search = Some(PendingPeakSearch(
                                seq.module.clone(),
                                cur_settings,
                                f_size,
                                ui.title.clone(),
                            ));
                        }
                    } else {
                        ui.pending_initial_peak_search = Some(PendingPeakSearch(
                            seq.module.clone(),
                            cur_settings,
                            f_size,
                            ui.title.clone(),
                        ));
                    }
                }
                let total_ms = compute_module_total_time_ms_on_seq(seq);
                ui.total_time_secs = ((total_ms + 500) / 1000).max(0) as u32;
                seq.module.state.repeat_song = false;
                live_proc.reset_on_seek_or_load(seq);
                ui.cpu_telemetry.reset_buffer(ui.ini_config.output_buffers);
                if ui.intro_scan {
                    ui.is_playing = true;
                }
                ui.is_paused = false;
            }

    if let Some(wav_out_path) = ui.pending_export_wav.take()
        && let Some(ref mod_path) = ui.current_file_path
            && let Ok(file_data) = fs::read(mod_path) {
                let ext = mod_path.extension().and_then(|e| e.to_str());
                if let Ok(export_mod) = load_module_with_ext(&file_data, ext) {
                    ui.is_exporting_wav = true;
                    match export_module_to_wav_file(
                        export_mod,
                        file_data.len() as u32,
                        &mut ui.ini_config,
                        &wav_out_path,
                    ) {
                        Ok(frames) => {
                            let fname = wav_out_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("OUT.WAV");
                            println!(
                                "[DEV_HDD] Direct-to-Disk WAV-Export abgeschlossen: {} ({} Frames)",
                                wav_out_path.display(),
                                frames
                            );
                            ui.title = format!("SAVED {}", fname);
                            ui.export_status_msg = Some(format!("WAV Saved: {}", fname));
                        }
                        Err(e) => {
                            eprintln!("[DEV_HDD] Fehler beim WAV-Export: {}", e);
                            ui.title = "WAV EXPORT ERROR".to_string();
                        }
                    }
                    ui.is_exporting_wav = false;
                }
            }
}

/// Berechnet die exakte Gesamtdauer (`mi.total_time`), die Pattern-Startzeiten (`pattern_times`)
/// sowie die Loop-Punkte (`mi.loop_from`, `mi.loop_to`, `loop_to_time`) exakt wie `GetTotalTime`
/// in `legacy_src/PLAYER/PLAYER32.PAS` (Zeilen 348–470) und setzt anschließend den Sequencer
/// über `kanal_init(false)` sauber auf den Songanfang zurück.
#[inline(never)]
pub fn compute_module_total_time_ms_on_seq(seq: &mut Sequencer) -> i32 {
    if seq.module.state.mi.num_patterns == 0 || seq.module.state.mi.num_tracks == 0 {
        return 0;
    }
    let saved_repeat = seq.module.state.repeat_song;
    seq.get_total_time();
    let total_ms = seq.module.state.mi.total_time;
    seq.kanal_init(false);
    seq.module.state.repeat_song = saved_repeat;
    seq.module.state.mi.total_time = total_ms;
    total_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::loader::LoadedModule;
    use crate::engine::tables::{DEF_MOD_TUNING_NTSC, SIN_TABLE};
    use crate::engine::types::{effect_ids, Instrument, ModType, Note, DEF_CH_VOL};

    fn create_test_sine_module(is_16bit_inst: bool) -> LoadedModule {
        let mut module = LoadedModule::default();
        module.state.mi.file_type = ModType::ProTr;
        module.state.mi.num_tracks = 4;
        module.state.mi.num_patterns = 1;
        module.state.mi.num_instruments = 1;
        module.state.init_speed = 6;
        module.state.init_bpm_speed = 125;

        let base_ofs: i32 = 16;
        if is_16bit_inst {
            module.sample_mem = vec![0u8; ((base_ofs as usize) + 260) * 2];
            for i in 0..256 {
                let val_i16 = ((SIN_TABLE[i] as i16) * 200).clamp(-32000, 32000);
                let bytes = val_i16.to_le_bytes();
                let byte_idx = ((base_ofs as usize) + i) * 2;
                module.sample_mem[byte_idx] = bytes[0];
                module.sample_mem[byte_idx + 1] = bytes[1];
            }
        } else {
            module.sample_mem = vec![0u8; (base_ofs as usize) + 260];
            for i in 0..256 {
                module.sample_mem[(base_ofs as usize) + i] = SIN_TABLE[i] as u8;
            }
        }

        module.state.mi.instruments[0] = Instrument {
            name_len: 4,
            name: [0; 40],
            offset: base_ofs,
            iend: base_ofs + 256,
            loop_start: base_ofs,
            loop_end: base_ofs + 256,
            volume: 64,
            fine_tune: 0,
            bits: if is_16bit_inst { 16 } else { 8 },
            tuning: DEF_MOD_TUNING_NTSC,
        };

        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        rows[0][0] = Note {
            instrument: 1,
            ton: 428 * 8, // 3424 (C-2)
            effekt: effect_ids::NO_EFFEKT,
            operands: 0,
            chn_vol: DEF_CH_VOL,
        };
        module.patterns.push(rows);
        module.state.p_arrangement[0] = 0;
        module.state.ptrn_breaks[0] = 63;
        module
    }

    #[test]
    fn test_pcm8_to_i16_conversion() {
        assert_eq!(pcm8_to_i16(128), 0);
        assert_eq!(pcm8_to_i16(0), -32768);
        assert_eq!(pcm8_to_i16(255), 32512);
    }

    #[test]
    fn test_mono_mode_duplicates_to_stereo_at_1x_speed() {
        let module = create_test_sine_module(false);
        let mut settings = SoundSettings::default();
        settings.samplerate = 44100;
        settings.stereo = 1; // Mono
        settings.bits = 16;

        let mut seq = Sequencer::new(module, settings);
        let mut proc = LiveAudioProcessor::new(44100);
        let muted = [false; 32];

        // Bei 44100 Hz und 125 BPM hat 1 Tick exakt 882 Samples.
        // Im Mono-Modus müssen 882 Stereo-Frames `(m, m)` in `frame_queue` landen (NICHT 441!).
        proc.render_one_tick_to_queue(&mut seq, &muted);
        assert_eq!(proc.frame_queue.len(), 882);
        assert!(proc.frame_queue.iter().any(|&(l, _)| l != 0));
        for &(l, r) in &proc.frame_queue {
            assert_eq!(l, r, "Mono muss identisch auf Links und Rechts liegen");
        }
    }

    #[test]
    fn test_8bit_mode_and_live_option_changes_preserve_16bit_instrument_flag() {
        let module = create_test_sine_module(true); // 16-Bit Instrument im Modul!
        let mut settings = SoundSettings::default();
        settings.samplerate = 44100;
        settings.stereo = 2;
        settings.bits = 16;
        settings.oversamp = 2; // CPU-Cubic

        let mut seq = Sequencer::new(module, settings);
        let mut proc = LiveAudioProcessor::new(48000);
        let muted = [false; 32];

        // Tick 0 rendern -> Kanal 0 triggert das 16-Bit-Instrument (`F_16BIT = 0x10`)
        proc.render_one_tick_to_queue(&mut seq, &muted);
        assert_ne!(
            seq.module.state.channels[0].jmp_flags & mixer_flags::F_16BIT,
            0,
            "16-Bit-Instrument muss F_16BIT gesetzt haben"
        );
        assert_eq!(
            seq.module.state.channels[0].jmp_flags & mixer_flags::F_INTERP_MASK,
            mixer_flags::F_CPU_CUBIC
        );

        // Jetzt während der Wiedergabe auf 8-Bit, Mono, IDO FPU-Cubic und Samplerate 11025 Hz wechseln:
        let step_before = ((seq.module.state.channels[0].phi as u64) << 16)
            | (seq.module.state.channels[0].plo as u64);
        assert!(step_before > 0);

        let mut new_settings = settings;
        new_settings.bits = 8;
        new_settings.stereo = 1;
        new_settings.oversamp = 3; // FPU-Cubic
        new_settings.samplerate = 11025;
        new_settings.panning = false;

        apply_live_sound_settings(&mut seq, new_settings);

        // 1. `F_16BIT` des geladenen Instruments darf NICHT gelöscht worden sein!
        assert_ne!(
            seq.module.state.channels[0].jmp_flags & mixer_flags::F_16BIT,
            0,
            "F_16BIT muss beim Ändern der Ausgabeparameter erhalten bleiben"
        );
        // 2. `F_INTERP_MASK` muss sauber auf `F_FPU_CUBIC` stehen:
        assert_eq!(
            seq.module.state.channels[0].jmp_flags & mixer_flags::F_INTERP_MASK,
            mixer_flags::F_FPU_CUBIC
        );
        // 3. `num_samples` muss für 11025 Hz / 125 BPM = 220 Samples betragen:
        assert_eq!(seq.module.state.num_samples, 220);
        // 4. Phasen-Inkrement `(phi, plo)` muss um Faktor `44100 / 11025 = 4` skaliert sein:
        let step_after = ((seq.module.state.channels[0].phi as u64) << 16)
            | (seq.module.state.channels[0].plo as u64);
        assert_eq!(step_after, step_before * 4);
    }

    #[test]
    fn test_resampling_11025_to_48000_and_stop_pause_fade_out() {
        let module = create_test_sine_module(false);
        let mut settings = SoundSettings::default();
        settings.samplerate = 11025;
        settings.stereo = 2;
        settings.bits = 8;

        let mut seq = Sequencer::new(module, settings);
        let mut proc = LiveAudioProcessor::new(48000);
        let muted = [false; 32];
        let mut pending = None;

        // 1 Sekunde Audio bei 48000 Hz anfordern (48000 Stereo-Frames)
        let mut out_48k = vec![(0i16, 0i16); 48000];
        proc.fill_cpal_stereo_frames(
            &mut seq,
            &mut out_48k,
            true,
            false,
            false,
            &muted,
            &mut pending,
        );

        // Bei 125 BPM (50 Ticks/Sekunde) muss nach 1 Sekunde (48000 Hardware-Frames)
        // die Sequencer-Spielzeit `play_time` bei ~1000 ms (50..51 Ticks) liegen,
        // obwohl intern mit 11025 Hz gemischt wurde!
        let play_time_ms = seq.module.state.play_time;
        assert!(
            (980..=1060).contains(&play_time_ms),
            "Nach 48000 Frames bei 48kHz muss play_time ~1000ms betragen, war aber {}ms",
            play_time_ms
        );

        // Erzwinge einen definierten letzten Sample-Pegel vor Pause/Stop
        proc.last_out_l = 16000;
        proc.last_out_r = -16000;
        proc.was_playing = true;

        // Jetzt Pause auslösen -> erste ~96 Frames (2 ms bei 48 kHz) rampen sanft auf 0, danach strikt 0!
        let mut pause_buf = vec![(999i16, 999i16); 256];
        proc.fill_cpal_stereo_frames(
            &mut seq,
            &mut pause_buf,
            false,
            true,
            false,
            &muted,
            &mut pending,
        );

        assert!(pause_buf[0].0 > 0 && pause_buf[0].0 < 16000);
        assert!(pause_buf[0].1 < 0 && pause_buf[0].1 > -16000);
        for &(l, r) in &pause_buf[120..] {
            assert_eq!((l, r), (0, 0), "Nach 2ms Fade-Out muss strikt 0 ausgegeben werden");
        }
    }

    #[test]
    fn test_live_no_bpm_toggle_recomputes_speed_bpm_and_total_time() {
        // Simuliert ein klassisches Amiga-VBlank-MOD (wie KlissjePa.MOD aus der Hilfe-FAQ),
        // das in Row 0 `F20` (0x20 = 32) als VBlank-Speed und in Row 1 wieder `F06` (Speed 6) setzt.
        let mut module = create_test_sine_module(false);
        module.patterns[0][0][0].effekt = crate::engine::types::effect_ids::SET_SPEED;
        module.patterns[0][0][0].operands = 0x20; // F20 (32)
        module.patterns[0][1][0].effekt = crate::engine::types::effect_ids::SET_SPEED;
        module.patterns[0][1][0].operands = 0x06; // F06 (6)

        let mut settings = SoundSettings::default();
        settings.no_bpm = false; // Standard: BPM EIN

        let mut seq = Sequencer::new(module, settings);
        let total_time_with_bpm = seq.module.state.mi.total_time;

        // 1. Mit BPM EIN (no_bpm = false) bis Row 2 vorrücken:
        // Row 0 (`F20`) setzt `bpm_speed = 32` (und lässt `speed = 6`),
        // Row 1 (`F06`) setzt `speed = 6`, während `bpm_speed` bei 32 hängenbleibt ("schleppend")!
        for _ in 0..12 {
            seq.render_one_tick();
        }
        assert_eq!(seq.module.state.curr_note, 2);
        assert_eq!(seq.module.state.speed, 6);
        assert_eq!(seq.module.state.bpm_speed, 32);

        // 2. Jetzt live im Hauptfenster `BPM` ausschalten (`no_bpm = true`):
        // Muss wie in PLAYER32.PAS `GetTotalTime` + `search_time` bis (Order 0, Row 2) neu auswerten:
        // `F20` war nur 1 Zeile lang VBlank-Speed 32, danach hat Row 1 (`F06`) wieder Speed 6 bei 125 BPM gesetzt!
        let mut no_bpm_settings = settings;
        no_bpm_settings.no_bpm = true;
        apply_live_sound_settings(&mut seq, no_bpm_settings);

        assert_eq!(seq.module.state.curr_note, 2);
        assert_eq!(seq.module.state.speed, 6);
        assert_eq!(seq.module.state.bpm_speed, 125);
        let total_time_no_bpm = seq.module.state.mi.total_time;
        assert!(
            total_time_no_bpm < total_time_with_bpm / 2,
            "TotalTime muss ohne BPM-Fehlinterpretation deutlich kürzer sein ({} vs {})",
            total_time_no_bpm,
            total_time_with_bpm
        );

        // 3. Und beim Wiedereinschalten von `BPM` (`no_bpm = false`) wird exakt der CIA-BPM-Zustand wiederhergestellt:
        apply_live_sound_settings(&mut seq, settings);
        let total_time_restored = seq.module.state.mi.total_time;
        assert_eq!(seq.module.state.curr_note, 2);
        assert_eq!(seq.module.state.speed, 6);
        assert_eq!(seq.module.state.bpm_speed, 32);
        assert_eq!(total_time_restored, total_time_with_bpm);
    }
}
