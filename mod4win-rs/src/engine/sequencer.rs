//! Tracker-Sequencer und Tick-/Row-Engine der MOD4WIN 2.40 Beta Audio-Engine.
//!
//! Portiert aus:
//! - `legacy_src/PLAYER/play.inc` (`KanalInit`, `CalcVolMaster`, `SetVolTable`, `DoPhiPlo`,
//!   `VolAdjust1`, `VolAdjust`, `GetTonh`, `NoteToVal`, `InitEnvelope`, `InstrAdjust`,
//!   `ProcessEnvelope`, `ProcessAutoVibrato`, `DoSurround`, `ProcessNewNote`, `ProcessFirstTick`,
//!   `Play_the_Module`)
//! - `legacy_src/PLAYER/effects.inc` (`DoSetSpeed`, `DoFineFarTempo`, `IncTime`)

use crate::engine::loader::{bitset_contains, bitset_include, LoadedModule};
use crate::engine::mixer::{
    calc_sample_positions, process_sample_block, DacMixMemory, MixOutputBuffer,
};
use crate::engine::tables::{
    build_vol_table, DEF_MOD_TUNING_NTSC, DEF_MOD_TUNING_PAL, F_NTSC, F_PAL, HALF_NOTE,
    HALF_NOTE_1, MAX_AMIGA_PERIOD, MIN_AMIGA_PERIOD, PAN_CENTER, PAN_CENTER_SNAP_MAX,
    PAN_CENTER_SNAP_MIN, PITCH_TABLE, RAMP_DN_TABLE, RAMP_UP_TABLE, SIN_TABLE, SQR_TABLE,
    TUNE_TABLE, WAVE_RAMP_DN, WAVE_RAMP_UP, WAVE_SINE, WAVE_SQUARE,
};
use crate::engine::types::{
    effect_ids, event_flags, mixer_flags, EnvPoint, ModType, Note, SoundSettings, DEF_CH_VOL,
    DEV_HDD, MAX_M4W_CHANNELS, MAX_MIX_BUF_SAMPLES, MAX_PATTERNS, MIX_BUFFER_OFS, SURR_LEN,
    SUR_BUFFER_OFS,
};

/// Untere Periodengrenze für ScreamTracker 3 (`gen_lolimit = 14` in `playdefs.pas`).
pub const GEN_LOLIMIT: u16 = MIN_AMIGA_PERIOD;
/// Obere Periodengrenze (`gen_hilimit = 1712 * 32 = 54784` in `effects.inc`).
pub const GEN_HILIMIT: u16 = MAX_AMIGA_PERIOD;

/// Hauptstruktur des Tracker-Sequencers.
///
/// Verbindet das geladene Modul ([`LoadedModule`], inklusive [`EngineState`], entpackten Patterns,
/// Sample-Speicher und FT2-Laufzeitdaten) mit dem 32-Bit-Mischspeicher ([`DacMixMemory`]) aus `mixer.rs`
/// und stellt die blockweise Audio-Synthese über [`Sequencer::fill_audio_buffer`] bereit.
#[derive(Clone)]
pub struct Sequencer {
    /// Geladenes Modul inklusive [`EngineState`], Patterns, `sample_mem` und FT2-Strukturen.
    pub module: LoadedModule,
    /// Entpackter, mutabler Laufzeit-Pattern-Puffer (`Pattern^[1..NumTracks][0..255]` in `play.inc`),
    /// der durch `GetPattern(CurrPattern, 256)` beim Pattern-Wechsel und `GetPattern(CurrPattern, CurrNote)`
    /// bei `PATTRN_LOOP` (`E6x`/`SBx`) beschrieben und von `ProcessNewNote`/`ProcessFirstTick` in-place mutiert wird.
    pub active_pattern: Box<[[Note; 256]; MAX_M4W_CHANNELS]>,
    /// Laufzeit-Mischspeicher (`mix_buffer`, `sur_buffer`, `surr_ring`, `vol_table`).
    pub mix_mem: DacMixMemory,
    /// Zwischenspeicher für überhängende Stereo-/Mono-PCM-Samples aus dem letzten Tick-Block,
    /// falls `output.len()` in `fill_audio_buffer` nicht exakt auf eine Tick-Grenze fällt.
    pub pending_pcm: Vec<i16>,
    /// Lese-Index innerhalb von `pending_pcm`.
    pub pending_read_idx: usize,
    /// Gibt an, ob der aktuelle Tick (vor dem Mischen) bereits für den Mixer vorbereitet wurde.
    pub tick_prepared: bool,
    /// Korrekturwert für Farandole-Tempo (`FarSpeedCorr` in `effects.inc` Zeile 759).
    pub far_speed_corr: i16,
    /// Gibt an, ob ein einzelnes Pattern im Loop gespielt wird (`OnlyOnePattern` in `play.inc`).
    pub only_one_pattern: bool,
    /// Parallele Impulse-Tracker-Engine (`Some` nur bei `ModType::IT`, sonst `None`).
    pub it_engine: Option<Box<crate::engine::it_engine::ItEngine>>,
}

impl Sequencer {
    /// Erstellt und initialisiert einen neuen [`Sequencer`] aus einem geladenen [`LoadedModule`]
    /// und den gewünschten [`SoundSettings`].
    ///
    /// Entspricht `ActivateSound` + `KanalInit` in `legacy_src/PLAYER/play.inc` (Zeilen 2070–2200).
    pub fn new(module: LoadedModule, settings: SoundSettings) -> Self {
        let it_engine = if module.it_module.is_some() {
            Some(Box::new(crate::engine::it_engine::ItEngine::default()))
        } else {
            None
        };
        let mut seq = Self {
            module,
            active_pattern: Box::new([[Note::default(); 256]; MAX_M4W_CHANNELS]),
            mix_mem: DacMixMemory::default(),
            pending_pcm: Vec::with_capacity(MAX_MIX_BUF_SAMPLES * 2),
            pending_read_idx: 0,
            tick_prepared: false,
            far_speed_corr: 0,
            only_one_pattern: false,
            it_engine,
        };
        seq.module.state.sound_settings = settings;
        seq.get_total_time();
        seq.kanal_init(false);
        seq
    }

    /// Gibt `true` zurück, wenn ein Impulse-Tracker-Modul (`.IT`) geladen ist.
    #[inline]
    pub fn is_it_module(&self) -> bool {
        self.module.it_module.is_some()
    }

    /// Berechnet vorab die Gesamtdauer (`mi.total_time`), die Pattern-Startzeiten (`pattern_times`)
    /// sowie die Loop-Punkte (`mi.loop_from`, `mi.loop_to`, `loop_to_time`) exakt wie `GetTotalTime`
    /// in `legacy_src/PLAYER/PLAYER32.PAS` (Zeilen 348–470).
    pub fn get_total_time(&mut self) {
        if let Some(ref it_mod) = self.module.it_module {
            crate::engine::it_engine::ItEngine::calc_total_time(it_mod, &mut self.module.state);
            return;
        }
        let num_patterns = self.module.state.mi.num_patterns;
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        if num_patterns == 0 || num_tracks == 0 {
            return;
        }

        self.module.state.row_played = [[0u8; 32]; MAX_PATTERNS];
        self.module.state.pattern_times = [-1; MAX_PATTERNS];
        for k in 0..num_tracks {
            self.module.state.channels[k].pattern_loops = 0;
        }

        let init_bpm = self.module.state.init_bpm_speed.clamp(32, 255) as u8;
        let init_spd = self.module.state.init_speed;
        self.module.state.force_bpm = true;
        self.do_set_speed(init_bpm);
        self.module.state.force_bpm = false;
        self.do_set_speed(init_spd);
        if self.module.state.mi.file_type == ModType::FFar {
            self.do_fine_far_tempo(0);
        }
        self.module.state.far_vib_depth = 4;
        self.module.state.play_time = 0;
        self.module.state.time_step_rest = 0;

        if !matches!(
            self.module.state.mi.file_type,
            ModType::F669 | ModType::UNIS | ModType::FFar | ModType::FT2
        ) {
            self.module.state.mi.loop_to = -1;
            self.module.state.mi.loop_from = -1;
        }

        let mut loop_from_time: i32 = -1;
        self.module.state.patterndelay = 0;
        let mut breakto: u8 = 0;
        self.module.state.glob_vol = self.module.state.init_glob_vol;

        let has_pat_loop =
            bitset_contains(&self.module.state.mi.effects_used_set, effect_ids::PATTRN_LOOP);
        let file_type = self.module.state.mi.file_type;

        let mut i: usize = 0;
        let mut guard_steps: usize = 0;

        'pattern_loop: loop {
            if guard_steps > 500_000 {
                break;
            }
            guard_steps += 1;

            let mut skip_to_s3m_inc = false;
            if file_type == ModType::S3M {
                match self.module.state.p_arrangement[i.min(MAX_PATTERNS - 1)] {
                    0xFE => skip_to_s3m_inc = true,
                    0xFF => break 'pattern_loop,
                    _ => {}
                }
            }

            let mut did_pjump = false;

            if !skip_to_s3m_inc {
                let ord_idx = i.min(MAX_PATTERNS - 1);
                self.module.state.pattern_times[ord_idx] = self.module.state.play_time;
                for k in 0..num_tracks {
                    self.module.state.channels[k].pattern_loopst = 0;
                }
                let pat_tempo = self.module.state.ptrn_tempi[ord_idx];
                if pat_tempo > 0 {
                    self.do_set_speed(pat_tempo);
                }

                let mut j: u8 = breakto;
                'row_loop: loop {
                    if guard_steps > 500_000 {
                        break 'pattern_loop;
                    }
                    guard_steps += 1;

                    breakto = 0;
                    'new_pattern: loop {
                        let phys_pat = self.module.state.p_arrangement[ord_idx] as usize;
                        for k in 0..num_tracks {
                            let note = self
                                .module
                                .patterns
                                .get(phys_pat)
                                .and_then(|rows| rows.get(j as usize))
                                .map(|row| row[k])
                                .unwrap_or_default();
                            match note.effekt {
                                effect_ids::SET_GLOB_VOL => {
                                    self.module.state.glob_vol = note.operands;
                                }
                                effect_ids::SET_SPEED => {
                                    self.do_set_speed(note.operands);
                                }
                                effect_ids::S3M_BPM => {
                                    if note.operands >= 32 {
                                        self.module.state.force_bpm = true;
                                        self.do_set_speed(note.operands);
                                        self.module.state.force_bpm = false;
                                    }
                                }
                                effect_ids::FINE_TEMPO_UP => {
                                    self.do_fine_far_tempo(note.operands as i8);
                                }
                                effect_ids::FINE_TEMPO_DWN => {
                                    self.do_fine_far_tempo((note.operands as i8).wrapping_neg());
                                }
                                effect_ids::PATTERN_DELAY => {
                                    self.module.state.patterndelay = note.operands;
                                }
                                _ => {}
                            }
                        }

                        let cx_ticks = ((self.module.state.patterndelay as u16) + 1)
                            * (self.module.state.speed as u16);
                        for _ in 0..cx_ticks {
                            self.inc_time();
                        }
                        self.module.state.patterndelay = 0;

                        bitset_include(&mut self.module.state.row_played[ord_idx], j);

                        if has_pat_loop {
                            for k in (0..num_tracks).rev() {
                                let note = self
                                    .module
                                    .patterns
                                    .get(phys_pat)
                                    .and_then(|rows| rows.get(j as usize))
                                    .map(|row| row[k])
                                    .unwrap_or_default();
                                if note.effekt == effect_ids::PATTRN_LOOP {
                                    let chn = &mut self.module.state.channels[k];
                                    if note.operands == 0 {
                                        chn.pattern_loopst = j;
                                    } else {
                                        if chn.pattern_loops == 0 {
                                            if chn.pattern_loopst <= j {
                                                chn.pattern_loops = note.operands.wrapping_add(1);
                                            }
                                        } else if chn.pattern_loops == 1 {
                                            chn.pattern_loops = 0;
                                            chn.pattern_loopst = j.wrapping_add(1);
                                        }
                                        if chn.pattern_loops > 1 {
                                            j = chn.pattern_loopst;
                                            chn.pattern_loops -= 1;
                                            guard_steps += 1;
                                            if guard_steps > 500_000 {
                                                break 'pattern_loop;
                                            }
                                            continue 'new_pattern;
                                        }
                                    }
                                }
                            }
                        }
                        break 'new_pattern;
                    }

                    let mut pjump = false;
                    let mut pbreak = false;
                    let mut jumpto: usize = 0;
                    let phys_pat = self.module.state.p_arrangement[ord_idx] as usize;
                    let use_patt_brk_ops = self.module.state.use_patt_brk_ops;

                    for k in 0..num_tracks {
                        let note = self
                            .module
                            .patterns
                            .get(phys_pat)
                            .and_then(|rows| rows.get(j as usize))
                            .map(|row| row[k])
                            .unwrap_or_default();
                        match note.effekt {
                            effect_ids::POSITION_JUMP => {
                                if (note.operands as u16) < num_patterns {
                                    pjump = true;
                                    jumpto = note.operands as usize;
                                }
                            }
                            effect_ids::PATTERN_BREAK => {
                                pbreak = true;
                                if use_patt_brk_ops {
                                    breakto = 10 * (note.operands >> 4) + (note.operands & 0x0F);
                                }
                            }
                            _ => {}
                        }
                    }

                    if pjump {
                        let jt_idx = jumpto.min(MAX_PATTERNS - 1);
                        if self.module.state.ptrn_breaks[jt_idx] < breakto {
                            breakto = 0;
                        }
                        if !self.one_channel_loops()
                            && bitset_contains(&self.module.state.row_played[jt_idx], breakto)
                            && self.module.state.mi.loop_from == -1
                        {
                            self.module.state.mi.loop_from = (i as i16) + 1;
                            self.module.state.mi.loop_to = (jumpto as i16) + 1;
                            loop_from_time = self.module.state.play_time;
                        }
                        i = jumpto;
                        did_pjump = true;
                        break 'row_loop;
                    } else if pbreak {
                        break 'row_loop;
                    }

                    if j >= self.module.state.ptrn_breaks[ord_idx] {
                        break 'row_loop;
                    }
                    j = j.wrapping_add(1);
                }
            }

            if !did_pjump {
                // S3M_Inc:
                i += 1;
                if i >= num_patterns as usize {
                    i = 0;
                }
            }

            // JMP:
            let next_idx = i.min(MAX_PATTERNS - 1);
            if self.module.state.ptrn_breaks[next_idx] < breakto {
                breakto = 0;
            }
            if !self.one_channel_loops()
                && bitset_contains(&self.module.state.row_played[next_idx], breakto)
            {
                break 'pattern_loop;
            }
        }

        self.module.state.mi.total_time = self.module.state.play_time;
        if loop_from_time == -1 {
            let _ = loop_from_time;
        } else {
            self.module.state.mi.total_time = loop_from_time;
        }
        if self.module.state.mi.loop_to > -1 {
            let lt_idx =
                (self.module.state.mi.loop_to - 1).clamp(0, (MAX_PATTERNS - 1) as i16) as usize;
            self.module.state.loop_to_time = self.module.state.pattern_times[lt_idx];
        } else {
            self.module.state.loop_to_time = 0;
        }
        for k in 0..num_tracks {
            let dp = self.module.state.def_panning[k];
            if dp != 0 && dp != 255 {
                self.module.state.mi.panning_off = false;
            }
        }
        if self.module.state.init_glob_vol == 0 && self.module.state.glob_vol == 0 {
            self.module.state.init_glob_vol = 64;
        }
    }

    /// Initialisiert alle Kanäle, Tempi, Master-Lautstärken, Panning-Positionen und Mischpuffer.
    ///
    /// ### Entsprechende Pascal-Prozeduren
    /// - `KanalInit` (`legacy_src/PLAYER/play.inc` Zeilen 2131–2200)
    /// - `CalcVolMaster` & `SetVolTable` (`legacy_src/PLAYER/play.inc` Zeilen 2110–2129, 2202–2244)
    pub fn kanal_init(&mut self, pal_timing: bool) {
        let state = &mut self.module.state;

        state.def_mod_tuning = if pal_timing {
            (F_PAL / 2.0 / 428.0).trunc() as u16 // 8287 Hz (DEF_MOD_TUNING_PAL)
        } else {
            (F_NTSC / 2.0 / 428.0).trunc() as u16 // 8363 Hz (DEF_MOD_TUNING_NTSC)
        };
        let _ = (DEF_MOD_TUNING_NTSC, DEF_MOD_TUNING_PAL);

        state.note_stretch = 65536.0 * 8.0 * 428.0 * (state.def_mod_tuning as f64);
        state.pm_is_working = false;
        state.glob_vol = state.init_glob_vol;
        state.use_surround = state.sound_settings.surround;

        state.samples_remain = 0;
        state.buf_samples = 0;
        state.curr_pattern = 0;
        state.curr_note = 0;
        state.curr_vblanc = 0;
        state.play_time = 0;
        state.time_step_rest = 0;
        state.break_to = 0;
        state.delay_pattern = false;
        state.patterndelay = 0;
        state.player_repeats = false;
        state.global_fade_out = false;
        state.global_fade_out_time = 0;
        state.far_vib_depth = 4;
        self.far_speed_corr = 0;

        // CalcVolMaster & SetVolTable
        self.calc_vol_master_and_table();

        // ForceBPM := True; DoSetSpeed(InitBpmSpeed); ForceBPM := False; DoSetSpeed(InitSpeed);
        let init_bpm = self.module.state.init_bpm_speed.clamp(32, 255) as u8;
        let init_spd = self.module.state.init_speed;
        self.module.state.force_bpm = true;
        self.do_set_speed(init_bpm);
        self.module.state.force_bpm = false;
        self.do_set_speed(init_spd);
        if self.module.state.mi.file_type == ModType::FFar {
            self.do_fine_far_tempo(0);
        }

        let state = &mut self.module.state;
        state.row_played = [[0u8; 32]; MAX_PATTERNS];
        state.channels = [Default::default(); MAX_M4W_CHANNELS];

        let jmp_interp = match state.sound_settings.oversamp {
            1 => mixer_flags::F_OVERSAMP,
            2 => mixer_flags::F_CPU_CUBIC,
            3 => mixer_flags::F_FPU_CUBIC,
            _ => 0,
        };

        let num_tracks = (state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for i in 0..num_tracks {
            let chn = &mut state.channels[i];
            chn.pattern_loopst = 0;
            chn.pattern_loops = 0;
            chn.effkt = effect_ids::NO_EFFEKT;
            chn.v_table = WAVE_SINE;
            chn.t_table = WAVE_SINE;
            chn.glissando_on = false;
            chn.pan_val = state.def_panning[i];
            chn.play_channel = -1;
            chn.jmp_flags = jmp_interp;
            chn.mix_buf = MIX_BUFFER_OFS as i32;
        }

        // Surround-Ringpuffer leeren und Start/End-Zeiger setzen (play.inc Zeilen 2183–2189)
        self.mix_mem.surr_ring.fill(Default::default());
        self.mix_mem.reset_declick();
        state.buf_end = 0;
        let surr_start = (8i64 * (state.sound_settings.samplerate as i64) / 1000
            * (SURR_LEN as i64)) as u16;
        state.buf_start = surr_start;

        state.r_fade_out = 0;
        state.l_fade_out = 0;
        state.r_sur_fade_out = 0;
        state.l_sur_fade_out = 0;
        state.env_stack_val = 0xCF;
        state.hdd_buf_frames = 0;

        self.pending_pcm.clear();
        self.pending_read_idx = 0;
        self.tick_prepared = false;

        if let Some(ref it_mod) = self.module.it_module {
            if self.it_engine.is_none() {
                self.it_engine = Some(Box::new(crate::engine::it_engine::ItEngine::default()));
            }
            if let Some(ref mut it_eng) = self.it_engine {
                it_eng.reset(it_mod, &mut self.module.state);
            }
            return;
        } else {
            self.it_engine = None;
        }

        // Erstes Pattern wie am Anfang von Play_the_Module (play.inc Zeilen 1163–1171) entpacken
        self.skip_s3m_order_markers();
        let cp = (self.module.state.curr_pattern as usize).min(MAX_PATTERNS - 1);
        self.get_pattern(cp, 256);

        // Falls das Start-Pattern ein initiales 669-Pattern-Tempo besitzt, setzen (play.inc Zeile 1171)
        let p0_tempo = self.module.state.ptrn_tempi[cp];
        if p0_tempo > 0 {
            self.do_set_speed(p0_tempo);
        }
    }

    // ========================================================================
    // 1. CIA-Timer, BPM, Speed & Master-Volume Berechnung
    // ========================================================================

    /// Berechnet `VolMaster` und die 65x256 Volume-Lookup-Tabelle (`CalcVolMaster` & `SetVolTable`
    /// in `legacy_src/PLAYER/play.inc` Zeilen 2110–2129 und 2202–2244).
    pub fn calc_vol_master_and_table(&mut self) {
        let state = &mut self.module.state;
        let pre_amp_db = (state.sound_settings.pre_amp & 0x7F) as f64;
        let amp = if state.sound_settings.oversamp > 1 {
            // Cubic splines can overamp the original sample by 25% = 2 dB
            10.0_f64.powf((pre_amp_db - 22.0) / 20.0)
        } else {
            10.0_f64.powf((pre_amp_db - 20.0) / 20.0)
        };

        let num_tracks = (state.mi.num_tracks.max(1)) as f64;
        let mut mv = if state.mi.file_type != ModType::S3M && state.mi.num_tracks > 4 {
            (256.0 * 8.0) / (4.0 + (num_tracks - 4.0) * 0.2)
        } else {
            (256.0 * 8.0) / num_tracks
        };

        if state.mi.file_type == ModType::S3M {
            let s3m_mv = 8.0 * (state.master_vol as f64);
            if s3m_mv > mv {
                mv = s3m_mv;
            }
        }

        state.vol_master = if state.scontrol {
            (4.0 * mv * amp).round() as u16
        } else {
            (1.0 * mv * amp).round() as u16
        };

        self.mix_mem.vol_table = build_vol_table(state.vol_master);

        // In MOD4WIN liegt VolumeTable bei PlayMemSel[0..33280] (SetVolTable, play.inc Zeilen 2208–2240).
        // Für Module mit ungeladenen/abgeschnittenen Samples (Offset = 0, aber LoopEnd > 0 + Oxx wie 4WD.S3M Ins 21)
        // liest der Mixer direkt aus PlayMemSel[0..33280].
        if self.module.sample_mem.len() >= 33280 {
            for (si, row) in self.mix_mem.vol_table.iter().enumerate() {
                let base = si * 512;
                for (b, &val) in row.iter().enumerate() {
                    let le = val.to_le_bytes();
                    self.module.sample_mem[base + b * 2] = le[0];
                    self.module.sample_mem[base + b * 2 + 1] = le[1];
                }
            }
        }
    }

    /// Berechnet CIA-Timer-Ticklänge (`num_samples` = Samples pro Tick, `time_step`) sowie
    /// `speed` (Ticks pro Zeile) und `bpm_speed` (BPM).
    ///
    /// ### Entsprechende ASM/Pascal-Prozedur
    /// `DoSetSpeed(Ops: Byte)` in `legacy_src/PLAYER/effects.inc` (Zeilen 665–742).
    ///
    /// ### Exakte Formeln aus `DoSetSpeed`
    /// - Für `Ops == 0`:
    ///   - Nur bei `ModType::UNIS`: `Speed = 1`, `BPMSpeed = (InitBPMSpeed * 3) as u8`.
    ///   - Sonst: Ignorieren (`Exit`).
    /// - Wenn `ForceBPM == true` oder (`Ops > 0x1F` und Format erlaubt BPM):
    ///   - `BPMSpeed = Ops.max(0x20)`
    ///   - `TimeStep = 0x09C4_0000 / BPMSpeed` (`2500 * 65536 / BPMSpeed` ms im 16.16-Format)
    ///   - `NumSamples = (samplerate * 2 + samplerate / 2) / BPMSpeed` (`Trunc(2.5 * samplerate / BPMSpeed)`)
    pub fn do_set_speed(&mut self, ops: u8) {
        let state = &mut self.module.state;
        let file_type = state.mi.file_type;

        let mut cx = ops as u16;
        if cx == 0 {
            if file_type != ModType::UNIS {
                return;
            }
            // UNIS-Speed 0: cl := byte(InitBPMSpeed * 3); Speed := 1; jmp @setbpm
            cx = ((state.init_bpm_speed.wrapping_mul(3)) & 0xFF).max(0x20);
            state.speed = 1;
            self.apply_bpm_hardware_timing(cx);
            return;
        }

        if state.force_bpm {
            self.apply_bpm_hardware_timing(cx.max(0x20));
            return;
        }

        if matches!(file_type, ModType::UNIS | ModType::F669) || state.sound_settings.no_bpm {
            // @NoBPM: Speed := cl; cl := InitBPMSpeed; jmp @SetBPM
            state.speed = cx as u8;
            let init_bpm = state.init_bpm_speed.max(0x20);
            self.apply_bpm_hardware_timing(init_bpm);
            return;
        }

        if file_type == ModType::S3M || cx <= 0x1F {
            // @setvblanc: Speed := cl
            state.speed = cx as u8;
            state.mi.curr_spd = state.speed;
        } else {
            // @setBPM
            self.apply_bpm_hardware_timing(cx.max(0x20));
        }
    }

    /// Berechnet `bpm_speed`, `time_step` und `num_samples` (`SamplesPerTick`) exakt nach den
    /// 32-Bit-Assembler-Instruktionen `@setBPM` in `effects.inc` (Zeilen 695–720).
    #[inline]
    fn apply_bpm_hardware_timing(&mut self, cl_bpm: u16) {
        let state = &mut self.module.state;
        let cx = cl_bpm.max(0x20);
        state.bpm_speed = cx;
        state.mi.curr_bpm = cx as u8;
        state.mi.curr_spd = state.speed;

        // TimeStep := $09C40000 DIV BPMSpeed (16-Bit Stufen-Division wie im Original-ASM):
        // xor dx, dx ; mov ax, 9C4h ; div cx ; mov word ptr TimeStep+2, ax
        // xor ax, ax ; div cx ; mov word ptr TimeStep, ax
        let hi_div = 0x09C4u32 / (cx as u32);
        let hi_rem = 0x09C4u32 % (cx as u32);
        let lo_div = (hi_rem << 16) / (cx as u32);
        state.time_step = ((hi_div << 16) | (lo_div & 0xFFFF)) as i32;

        // NumSamples := (samplerate * 2 + (samplerate >> 1)) DIV BPMSpeed
        // Wichtig: In effects.inc (Zeile 767) wird NumSamples NICHT auf MaxMixBufSamples (2000)
        // geclampt! Das Clamping für die Mischschleife erfolgt erst in ProcessSampleBlock
        // (dacmix.pas Zeile 371), während @SlideOut (Zeile 2461) durch das ungeclampte NumSamples teilt.
        let sr = (state.sound_settings.samplerate.max(1)) as u32;
        let eax = (sr << 1) + (sr >> 1);
        let num_samples = (eax / (cx as u32)) as u16;
        state.num_samples = num_samples.max(1);
    }

    /// Berechnet das feine Farandole-Tempo (`DoFineFarTempo` in `legacy_src/PLAYER/effects.inc` Zeilen 761–773).
    pub fn do_fine_far_tempo(&mut self, spd: i8) {
        if spd == 0 {
            self.far_speed_corr = 0;
        } else {
            self.far_speed_corr = self.far_speed_corr.wrapping_add(spd as i16);
        }
        let speed_i32 = self.module.state.speed as i32;
        let corr_i32 = self.far_speed_corr as i32;
        let far_bpm = (80 * (128 + speed_i32 * corr_i32)) / 128;
        let clamped_bpm = far_bpm.clamp(1, 255) as u8;
        self.module.state.force_bpm = true;
        self.do_set_speed(clamped_bpm);
        self.module.state.force_bpm = false;
    }

    /// Erhöht die Spielzeit `play_time` um einen Tick (`IncTime` in `legacy_src/PLAYER/effects.inc` Zeilen 775–790).
    #[inline]
    pub fn inc_time(&mut self) {
        let state = &mut self.module.state;
        let ts_lo = (state.time_step as u32) & 0xFFFF;
        let ts_hi = (state.time_step as u32) >> 16;
        let (new_rest, carry) = state.time_step_rest.overflowing_add(ts_lo as u16);
        state.time_step_rest = new_rest;
        state.play_time = state
            .play_time
            .wrapping_add(ts_hi as i32)
            .wrapping_add(if carry { 1 } else { 0 });
        state.mi.curr_time = state.play_time;
    }

    // ========================================================================
    // 2. Perioden-Lookup, Pitch (DoPhiPlo), Volume/Panning (VolAdjust) & Instrumente
    // ========================================================================

    /// Berechnet die Kanal-Periode `tonh` aus der Pattern-Note `ton`, `tuning` und `fine_tune`
    /// (`GetTonh` in `legacy_src/PLAYER/play.inc` Zeilen 232–278).
    pub fn get_tonh(&self, ton: u16, tuning: i32, fine_tune: i8) -> u16 {
        let state = &self.module.state;
        if state.use_linear_tune {
            // Linear freq. Table ==> Period := (10*12 - (Note - 1)) * 128 - FineTune
            let ax = (if (ton as i16) > 1 { ton } else { 1 }).wrapping_sub(1);
            let bx = (120u16.wrapping_sub(ax)) << 7;
            let ft = (fine_tune as i16) as u16;
            bx.wrapping_sub(ft)
        } else {
            if ton == 0 {
                return 0;
            }
            let mut ax = ton as u32;
            if state.mi.file_type == ModType::FT2 {
                let idx = (ton as usize).min(PITCH_TABLE.len() - 1);
                ax = PITCH_TABLE[idx] as u32;
            }
            let ecx = tuning as u32;
            if (ecx & 0xFFFF) == 0 {
                return ax as u16;
            }
            let bx = state.def_mod_tuning as u32;
            let prod = (ax as u64) * (bx as u64);
            let res = prod / (ecx as u64);
            if res >= 0xFFFF {
                0xFFFF
            } else {
                res as u16
            }
        }
    }

    /// Rechnet eine aktuelle Periode `note` in einen Noten-Anzeigeindex `1..145` um
    /// (`NoteToVal` in `legacy_src/PLAYER/play.inc` Zeilen 280–340).
    pub fn note_to_val(&self, note: u16, tune: i32, fine_tune: u8) -> u8 {
        let state = &self.module.state;
        if state.use_linear_tune {
            let bx = (note >> 1) as i16;
            let ft = (fine_tune as i8) as i16;
            let ax = ((ft.wrapping_add(bx).wrapping_add(bx)) >> 7)
                .wrapping_sub(10 * 12)
                .wrapping_neg()
                .wrapping_add(1);
            ax.clamp(0, 255) as u8
        } else {
            let mut eax = note as u32;
            if !matches!(
                state.mi.file_type,
                ModType::S3M | ModType::FT2 | ModType::Ftrk | ModType::WOW
            ) {
                eax <<= 2;
            }
            let ecx_tune = tune as u32;
            let prod = (eax as u64) * (ecx_tune as u64);
            let div = (state.def_mod_tuning.max(1)) as u64;
            let edx = prod / div;
            if edx == 0 {
                return 145;
            }
            if (edx >> 16) != 0 {
                return 0;
            }
            let dx = edx as u16;
            let mut cx = 145usize;
            while cx > 0 {
                if dx <= PITCH_TABLE[cx] {
                    return cx as u8;
                }
                cx -= 1;
            }
            0
        }
    }

    /// Berechnet die 16.16-Fixed-Point-Schrittweite (`plo`, `phi`, `fHiPitch`) für den Mixer
    /// aus der aktuellen Kanal-Periode `new_note` und `fine_tune`
    /// (`DoPhiPlo` in `legacy_src/PLAYER/play.inc` Zeilen 134–169).
    pub fn do_phi_plo(&mut self, ch_idx: usize, new_note: u16, fine_tune: i8) {
        let state = &mut self.module.state;
        if new_note == 0 {
            let chn = &mut state.channels[ch_idx];
            chn.note = 0;
            chn.event_chg |= event_flags::E_RESET;
            return;
        }

        let samplerate = (state.sound_settings.samplerate.max(1)) as f64;
        let tt_note: i32 = if state.use_linear_tune {
            // TTNote := Round(Exp(ln(2.0)*(9216.0-NewNote)/1536.0)*DefMODTuning/SoundSettings.SampleRate*65535.0)
            // In @FExp (Seg 4 + 0x0AEE) liegt [BP-4] exakt auf [SP_loop - 32] (Value in ProcessEnvelope):
            // Bei |x * log2(e)| > 0.5 (|9216 - NewNote| > 768) speichert FSTCW [BP-4] das FPU-CW (0x1F3F, Low-Byte 0x3F),
            // andernfalls bleibt FSTSW [BP-4] (Low-Byte 0x20 = PE) stehen.
            state.env_stack_val = if (9216 - (new_note as i32)).abs() > 768 {
                0x3F
            } else {
                0x20
            };
            let exp_part = (core::f64::consts::LN_2 * (9216.0 - (new_note as f64)) / 1536.0).exp();
            (exp_part * (state.def_mod_tuning as f64) / samplerate * 65535.0).round() as i32
        } else {
            let mut note_adj = new_note as i64;
            if fine_tune != 0 {
                let ft_idx = ((fine_tune as u8) & 0x0F) as usize;
                let tune_mul = TUNE_TABLE[ft_idx] as i64;
                note_adj = (tune_mul * note_adj) >> 16; // HiWord(tunetable[FineTune AND $0F] * TTNote)
            }
            if note_adj <= 0 {
                0
            } else {
                (state.note_stretch / samplerate / (note_adj as f64)).round() as i32
            }
        };

        let chn = &mut state.channels[ch_idx];
        chn.mod_event.pitch = tt_note;
        if tt_note > 30000 {
            chn.jmp_flags |= mixer_flags::F_HI_PITCH;
        } else {
            chn.jmp_flags &= !mixer_flags::F_HI_PITCH;
        }
        // ASM: mov di, oplo ; mov eax, TTNote ; mov [di], eax
        // Beschreibt `plo` (Low-Word) und die unteren 16 Bit von `phi` (High-Word):
        chn.plo = (tt_note as u32 & 0xFFFF) as u16;
        chn.phi = (tt_note >> 16) as i32;
    }

    /// Berechnet `avg_vol`, `l_vol`, `r_vol` sowie die Panning-Routing-Flags (`F_MONO`, `F_LEFT`,
    /// `F_RIGHT`, `F_PANNED`) für einen einzelnen Kanal `ch_idx` (0-basiert)
    /// (`VolAdjust1` in `legacy_src/PLAYER/play.inc` Zeilen 172–224).
    pub fn vol_adjust_1(&mut self, ch_idx: usize) {
        let state = &mut self.module.state;
        if ch_idx >= MAX_M4W_CHANNELS {
            return;
        }
        let is_ft2 = state.mi.file_type == ModType::FT2;
        let glob_vol = state.glob_vol as i32;
        let scontrol = state.scontrol;
        let panning_enabled = state.sound_settings.panning;
        let device_id = state.sound_settings.device_id;
        let vol_master = state.vol_master as i32;

        let chn = &mut state.channels[ch_idx];
        if chn.vol > 64 {
            chn.vol = 64;
        }

        let mut br: u16 = if is_ft2 {
            // BR := Longint(BR)*GlobVol*EnvVol DIV 64*FadeVol DIV (64*65536)
            // Hinweis: In play.inc Zeile 181 wird BR NICHT auf 64 geclampt (nur lVol/rVol in Zeilen 220–221).
            let num = (chn.vol as i64) * (glob_vol as i64) * (chn.env_vol as i64) / 64
                * (chn.fade_vol as i64);
            (num / (64 * 65536)).max(0) as u16
        } else {
            (((chn.vol as u16) * (glob_vol as u16)) / 64).min(64)
        };

        chn.avg_vol = (br & 0xFF) as u8;
        if chn.avg_vol != 0 {
            if scontrol {
                if chn.s_cont != 0 {
                    match chn.s_cont.wrapping_sub(1) {
                        0 => {
                            chn.avg_vol /= 4;
                            chn.pan_val = 0;
                        }
                        1 => {
                            chn.avg_vol /= 4;
                            chn.pan_val = 255;
                        }
                        2 => {
                            chn.avg_vol /= 2;
                            chn.pan_val = 0;
                        }
                        3 => {
                            chn.avg_vol /= 2;
                            chn.pan_val = 255;
                        }
                        4 => {
                            chn.avg_vol = (3 * (chn.avg_vol as u16) / 4) as u8;
                            chn.pan_val = 85;
                        }
                        5 => {
                            chn.avg_vol = (3 * (chn.avg_vol as u16) / 4) as u8;
                            chn.pan_val = 170;
                        }
                        6 => {
                            chn.avg_vol /= 2;
                            chn.pan_val = 128;
                        }
                        7 => {
                            chn.pan_val = 128;
                        }
                        _ => {}
                    }
                } else {
                    chn.avg_vol /= 4;
                }
                br = chn.avg_vol as u16;
            }

            let clean_mode = state.sound_settings.clean_mode();
            let mut hb = chn.pan_val as i32;
            if is_ft2 {
                let env_pan_i32 = if clean_mode {
                    (chn.env_pan as i32).clamp(0, 64)
                } else {
                    chn.env_pan as i32
                };
                hb += ((env_pan_i32 - 32) * (128 - (hb - 128).abs())) / 32;
            }
            let mut hb_u16 = if clean_mode {
                hb.clamp(0, 255) as u16
            } else {
                hb as u16
            };
            if (PAN_CENTER_SNAP_MIN as u16..=PAN_CENTER_SNAP_MAX as u16).contains(&hb_u16) {
                hb_u16 = PAN_CENTER as u16;
            }
            if !panning_enabled {
                if hb_u16 > 128 {
                    hb_u16 = 255;
                } else if hb_u16 < 128 {
                    hb_u16 = 0;
                }
            }

            let (bl, br_pan) = if hb_u16 != 128 {
                (br.wrapping_mul(255u16.wrapping_sub(hb_u16)), br.wrapping_mul(hb_u16))
            } else {
                (128u16.wrapping_mul(br), 128u16.wrapping_mul(br))
            };

            chn.l_vol = ((bl / 255) as u8).min(64);
            chn.r_vol = ((br_pan / 255) as u8).min(64);
        } else {
            chn.l_vol = 0;
            chn.r_vol = 0;
        }

        let vl = ((chn.l_vol as i32) * vol_master) / 2;
        let vr = ((chn.r_vol as i32) * vol_master) / 2;
        chn.mod_event.l_volume = vl.clamp(0, 65535) as u16;
        chn.mod_event.r_volume = vr.clamp(0, 65535) as u16;
        state.env_stack_val = 0xCF;

        if device_id >= DEV_HDD {
            chn.jmp_flags &= !mixer_flags::F_PANNED;
            if chn.r_vol == chn.l_vol {
                chn.jmp_flags |= mixer_flags::F_MONO;
            } else if chn.l_vol == 0 {
                chn.jmp_flags |= mixer_flags::F_RIGHT;
            } else if chn.r_vol == 0 {
                chn.jmp_flags |= mixer_flags::F_LEFT;
            } else {
                chn.jmp_flags |= mixer_flags::F_PANNED;
            }
        }
    }

    /// Aktualisiert Lautstärke und Panning eines Kanals (inkl. gekoppeltem Oktalyzer-Kanal)
    /// (`VolAdjust` in `legacy_src/PLAYER/play.inc` Zeilen 226–230).
    pub fn vol_adjust(&mut self, ch_idx: usize) {
        self.vol_adjust_1(ch_idx);
        if self.module.state.mi.file_type == ModType::Oktalyz {
            let rel = self.module.state.related_chn[ch_idx] as usize;
            if rel > 0 && rel <= MAX_M4W_CHANNELS {
                self.vol_adjust_1(rel - 1);
            }
        }
    }

    /// Initialisiert die FastTracker-II Lautstärke-/Panning-Hüllkurven, das Auto-Vibrato und
    /// das Fadeout für Kanal `ch_idx` und Instrument `inst_num` (1-basiert)
    /// (`InitEnvelope` in `legacy_src/PLAYER/play.inc` Zeilen 342–379).
    pub fn init_envelope(&mut self, ch_idx: usize, inst_num: u8) {
        let inst_idx = (inst_num as usize).min(self.module.ft2_instruments.len().saturating_sub(1));
        let ft2_inst = self.module.ft2_instruments[inst_idx];
        let chn = &mut self.module.state.channels[ch_idx];

        // Volume Envelope
        if (ft2_inst.ext.voltype & 1) != 0 {
            chn.env_vol_pos = 1;
            chn.env_vol = ft2_inst.ext.volenvpts[0].value as u8;
            chn.event_chg |= event_flags::E_VOL;
        } else {
            chn.env_vol_pos = 0;
            chn.env_vol = 64;
        }
        chn.env_vol_tck = 0;

        // Panning Envelope
        if (ft2_inst.ext.pantype & 1) != 0 {
            chn.env_pan_pos = 1;
            chn.env_pan = ft2_inst.ext.panenvpts[0].value as u8;
            chn.event_chg |= event_flags::E_VOL;
        } else {
            chn.env_pan_pos = 0;
            chn.env_pan = 32;
        }
        chn.env_pan_tck = 0;

        // Auto-Vibrato
        chn.vib_tck = 0;
        chn.vib_tbl = match ft2_inst.ext.vibrtype {
            0 => WAVE_SINE,
            1 => WAVE_SQUARE,
            2 => WAVE_RAMP_UP,
            _ => WAVE_RAMP_DN,
        };
        chn.vib_ptr = 0;

        // Fadeout
        chn.fade_vol = 65536;
        chn.do_fade_out = false;
    }

    /// Lädt das vorgewählte Instrument (`nxt_inst`) in die Mixer-Register des Kanals `ch_idx`
    /// und berechnet die Ziel-Periode `tonh` aus `new_ton`
    /// (`InstrAdjust` in `legacy_src/PLAYER/play.inc` Zeilen 382–455).
    pub fn instr_adjust(&mut self, ch_idx: usize, mut new_ton: u16, pat_inst: u8) {
        let file_type = self.module.state.mi.file_type;
        let is_ft2 = file_type == ModType::FT2;
        let effkt = self.module.state.channels[ch_idx].effkt;
        let old_chn_vol = self.module.state.channels[ch_idx].old_chn_vol;

        if effkt == effect_ids::FAR_PORTAMENTO
            || effkt == effect_ids::TONE_PORTAMENTO
            || effkt == effect_ids::PORT_VOLSLIDE
            || (is_ft2 && (old_chn_vol & 0xF0) == 0xF0)
        {
            if is_ft2 {
                // FT2 Bug! Retrigger note for TonePortamento...
                let smp_num = self.module.state.channels[ch_idx].smp_num as usize;
                let smp_vol = self
                    .module
                    .ft2_samples
                    .get(smp_num)
                    .map(|s| s.hdr.volume)
                    .unwrap_or(0);
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.note == 0 {
                    chn.note = chn.i_offset;
                }
                if !matches!(old_chn_vol, 0x70..=0x7F | 0x90..=0x9F) && chn.vol == 0 {
                    chn.vol = smp_vol;
                }
                if chn.fade_vol != 65536 {
                    // play.inc Zeile 404: IF FadeVol<>65536 THEN InitEnvelope(N,SmpNum);
                    self.init_envelope(ch_idx, smp_num as u8);
                }
                return;
            } else if self.module.state.channels[ch_idx].inst
                == self.module.state.channels[ch_idx].nxt_inst
            {
                return;
            }
            new_ton = 0;
        }

        let nxt_inst = self.module.state.channels[ch_idx].nxt_inst;
        if is_ft2 {
            let inst_idx =
                (nxt_inst as usize).min(self.module.ft2_instruments.len().saturating_sub(1));
            let ft2_inst = self.module.ft2_instruments[inst_idx];
            if ft2_inst.num_samples > 0 {
                // In Pascal: SampleNos: ARRAY[1..96] OF Byte -> SampleNos[NewTon] ist 1-basiert!
                let note_idx = (new_ton.saturating_sub(1) as usize).min(95);
                let smp_no = ft2_inst.ext.samplenos[note_idx] as u16;
                self.module.state.channels[ch_idx].smp_num = ft2_inst.sample_st.wrapping_add(smp_no);
            } else {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.note = 0;
                chn.event_chg |= event_flags::E_RESET;
                return;
            }

            if nxt_inst != 0 {
                let smp_idx = (self.module.state.channels[ch_idx].smp_num as usize)
                    .min(self.module.ft2_samples.len().saturating_sub(1));
                let smp_hdr = self.module.ft2_samples[smp_idx].hdr;
                if pat_inst != 0 {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.pan_val = smp_hdr.panning;
                    chn.vol = smp_hdr.volume;
                }
                self.init_envelope(ch_idx, nxt_inst);
                self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
            }
        } else {
            self.module.state.channels[ch_idx].smp_num = nxt_inst as u16;
        }

        // Instrument neu anspielen (play.inc Zeilen 422–453)
        let def_mod_tuning = self.module.state.def_mod_tuning as f64;
        let use_linear_tune = self.module.state.use_linear_tune;
        let smp_num = self.module.state.channels[ch_idx].smp_num;

        let mut rel_note: i16 = 0;
        let mut new_env_stack_val: Option<u8> = None;
        {
            let chn = &mut self.module.state.channels[ch_idx];
            chn.inst = nxt_inst;
            chn.jmp_flags &= !mixer_flags::F_16BIT;

            if is_ft2 {
                let smp_idx =
                    (smp_num as usize).min(self.module.ft2_samples.len().saturating_sub(1));
                let ft2_smp = self.module.ft2_samples[smp_idx];
                chn.note = ft2_smp.samp_offset;
                chn.iend = ft2_smp.hdr.samplelen;
                chn.lend = ft2_smp.hdr.loopend;
                chn.i_offset = ft2_smp.samp_offset;
                chn.i_fin_tun = ft2_smp.hdr.finetune as u8;
                chn.i_tune = (def_mod_tuning
                    * (core::f64::consts::LN_2 * (ft2_smp.hdr.relnote as f64) / 12.0).exp())
                .round() as i32;
                new_env_stack_val = Some(if ft2_smp.hdr.relnote < 0 { 0xFF } else { 0x00 });
                chn.lln = ft2_smp.hdr.loopstart;
                if (ft2_smp.hdr.sampletype & 0x10) != 0 {
                    chn.jmp_flags |= mixer_flags::F_16BIT;
                }
                rel_note = ft2_smp.hdr.relnote as i16;
            } else if nxt_inst > 0 {
                // MI.Instruments ist in Pascal 1..256 indiziert -> in Rust [nxt_inst - 1]
                let inst = self.module.state.mi.instruments[(nxt_inst - 1) as usize];
                chn.note = inst.offset;
                chn.iend = inst.iend;
                chn.lend = inst.loop_end;
                if (inst.bits & 0x1F) == 16 {
                    chn.jmp_flags |= mixer_flags::F_16BIT;
                }
                chn.i_fin_tun = inst.fine_tune;
                chn.i_offset = inst.offset;
                chn.i_tune = inst.tuning as i32;
                chn.lln = inst.loop_start;
            } else {
                chn.note = 0;
                chn.iend = 0;
                chn.lend = 0;
                chn.lln = 0;
            }

            if chn.lend != 0 && chn.lend > chn.lln {
                chn.iend = chn.lend;
                if chn.lln != 0 {
                    chn.lln = chn.iend.wrapping_sub(chn.lln);
                } else {
                    chn.lln = chn.iend.wrapping_add(65536);
                }
            } else {
                chn.lend = 0;
                chn.lln = chn.iend.wrapping_add(65536);
            }
            chn.iend = chn.iend.wrapping_sub(1);
            if chn.lend > 0 {
                chn.lend = chn.lend.wrapping_sub(1);
            }
            chn.rest = 0;
            chn.mod_event.inst = smp_num;
            chn.event_chg |= event_flags::E_INST;
        }
        if let Some(sv) = new_env_stack_val {
            self.module.state.env_stack_val = sv;
        }

        if new_ton != 0 {
            let i_tune = self.module.state.channels[ch_idx].i_tune;
            let i_fin_tun = self.module.state.channels[ch_idx].i_fin_tun as i8;
            let adj_ton = if use_linear_tune {
                ((new_ton as i32) + (rel_note as i32)).max(0) as u16
            } else {
                new_ton
            };
            let tonh = self.get_tonh(adj_ton, i_tune, i_fin_tun);
            self.module.state.channels[ch_idx].tonh = tonh;
        }
    }

    // ========================================================================
    // 3. FT2 Hüllkurven, Auto-Vibrato & Surround-Erkennung
    // ========================================================================

    /// Berechnet einen einzelnen Schritt einer FT2-Hüllkurve (Lautstärke oder Panning)
    /// (`ProcessEnvelope` in `legacy_src/PLAYER/play.inc` Zeilen 76–113).
    #[allow(clippy::too_many_arguments)]
    pub fn process_envelope(
        new_val: &mut u8,
        curr_tick: &mut u16,
        curr_pos: &mut u8,
        max_pos: u8,
        sustain_pos: u8,
        loop_start_pos: u8,
        loop_end_pos: u8,
        do_fade_out: bool,
        env_points: &[EnvPoint; 12],
    ) -> bool {
        let mut stack_val = 0x8Fu8;
        Self::process_envelope_with_pt0(
            new_val,
            curr_tick,
            curr_pos,
            max_pos,
            sustain_pos,
            loop_start_pos,
            loop_end_pos,
            do_fade_out,
            env_points,
            EnvPoint::default(),
            &mut stack_val,
            false,
        )
    }

    /// Interne `ProcessEnvelope`-Portierung inklusive 1-basiertem Index-0-Zugriff (`EnvPoints[0]`)
    /// und Stack-Slot `[bp-2]` (`env_stack_val`) bei doppelten Hüllkurven-Ticks (`play.inc` Zeilen 78–107).
    #[allow(clippy::too_many_arguments)]
    fn process_envelope_with_pt0(
        new_val: &mut u8,
        curr_tick: &mut u16,
        curr_pos: &mut u8,
        max_pos: u8,
        sustain_pos: u8,
        loop_start_pos: u8,
        loop_end_pos: u8,
        do_fade_out: bool,
        env_points: &[EnvPoint; 12],
        env_point_zero: EnvPoint,
        env_stack_val: &mut u8,
        clean_mode: bool,
    ) -> bool {
        if *curr_pos >= max_pos {
            return false;
        }
        let get_pt = |idx_1based: u8| -> EnvPoint {
            if idx_1based == 0 {
                env_point_zero
            } else {
                env_points
                    .get((idx_1based - 1) as usize)
                    .copied()
                    .unwrap_or_default()
            }
        };

        let sustain_hold = !do_fade_out
            && (!clean_mode || sustain_pos > 0)
            && sustain_pos == *curr_pos
            && *curr_tick == get_pt(sustain_pos).tick;
        if sustain_hold {
            return false;
        }

        let next_pt = get_pt(curr_pos.wrapping_add(1));
        let value: u8;
        if *curr_tick == next_pt.tick {
            value = next_pt.value as u8;
            *env_stack_val = value;
            *curr_pos = curr_pos.wrapping_add(1);
            if *curr_pos <= max_pos {
                if *curr_pos == loop_end_pos {
                    *curr_pos = loop_start_pos;
                    if loop_start_pos > 1 {
                        *curr_tick = get_pt(loop_start_pos).tick;
                    } else {
                        *curr_tick = 0;
                    }
                }
                if do_fade_out || *curr_pos != sustain_pos {
                    *curr_tick = curr_tick.wrapping_add(1);
                }
            }
        } else {
            let cur_pt = get_pt(*curr_pos);
            let t0 = cur_pt.tick as i32;
            let t1 = next_pt.tick as i32;
            let v0 = cur_pt.value as i32;
            let v1 = next_pt.value as i32;
            if t1 > t0 {
                let interp = (*curr_tick as i32 - t0).wrapping_mul(v1 - v0) / (t1 - t0) + v0;
                value = (interp & 0xFF) as u8;
                *env_stack_val = value;
            } else if clean_mode {
                value = (*new_val).min(64);
            } else {
                value = *env_stack_val;
            }
            *curr_tick = curr_tick.wrapping_add(1);
        }

        let changed = value != *new_val;
        *new_val = value;
        changed
    }

    /// Berechnet das FT2 Instrumenten-Auto-Vibrato für Kanal `ch_idx`
    /// (`ProcessAutoVibrato` in `legacy_src/PLAYER/play.inc` Zeilen 37–74).
    pub fn process_auto_vibrato(&mut self, ch_idx: usize) {
        let state = &mut self.module.state;
        let speed = (state.speed.max(1)) as i32;
        let curr_vblanc = state.curr_vblanc as i32;
        let inst_num = state.channels[ch_idx].inst as usize;
        if inst_num == 0 || inst_num >= self.module.ft2_instruments.len() {
            return;
        }
        let ft2_inst = self.module.ft2_instruments[inst_num];
        let vibr_depth = ft2_inst.ext.vibrdepth;
        let vibr_rate = ft2_inst.ext.vibrrate;
        let vibr_sweep = ft2_inst.ext.vibrsweep;

        let chn = &mut state.channels[ch_idx];
        if (chn.new_tonh != 0 || chn.tonh != 0) && vibr_depth != 0 && vibr_rate != 0 {
            let new_depth = if vibr_sweep != 0 && chn.vib_tck < vibr_sweep {
                let d = ((vibr_depth as u16) * (chn.vib_tck as u16) / (vibr_sweep as u16)) as u8;
                chn.vib_tck = chn.vib_tck.wrapping_add(1);
                d
            } else {
                vibr_depth
            };

            let step = ((curr_vblanc + 1) * (speed - 1) * (vibr_rate as i32) * 4) / 3 / speed;
            let new_ptr = chn.vib_ptr.wrapping_add(step as u8);
            if (chn.event_chg & event_flags::E_PITCH) == 0 || chn.new_tonh == 0 {
                chn.new_tonh = chn.tonh;
            }

            let wave_val: i8 = match chn.vib_tbl {
                WAVE_SINE => SIN_TABLE[new_ptr as usize],
                WAVE_SQUARE => SQR_TABLE[new_ptr as usize],
                WAVE_RAMP_UP => RAMP_UP_TABLE[new_ptr as usize],
                _ => RAMP_DN_TABLE[new_ptr as usize],
            };

            let new_val: i16 = ((wave_val as i16) * (new_depth as i16)) / 42;
            chn.new_tonh = (chn.new_tonh as i16).wrapping_add(new_val) as u16;
            if curr_vblanc + 1 == speed {
                chn.vib_ptr = new_ptr;
            }
            chn.event_chg |= event_flags::E_PITCH;
        }
    }

    /// Prüft, ob ein stark gepannter Kanal auf den ProLogic-Surround-Mischpuffer (`SurBufferOfs`)
    /// geroutet werden darf (`DoSurround` in `legacy_src/PLAYER/play.inc` Zeilen 2–14).
    pub fn do_surround(&self, a_inst: u8, a_pan_val: u8, a_ton_h: u16) -> bool {
        let state = &self.module.state;
        let num_tracks = (state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for k in 0..num_tracks {
            let chn = &state.channels[k];
            if (chn.event_chg & event_flags::E_INST) != 0
                && chn.inst == a_inst
                && chn.new_tonh == a_ton_h
                && ((chn.pan_val as i16) - (a_pan_val as i16)).abs() > 15
            {
                return false;
            }
        }
        true
    }

    /// Prüft, ob mindestens ein Kanal aktuell in einem Pattern-Loop (`E6x`) aktiv ist
    /// (`OneChannelLoops` in `legacy_src/PLAYER/play.inc` Zeilen 16–22).
    pub fn one_channel_loops(&self) -> bool {
        let state = &self.module.state;
        let num_tracks = (state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for k in 0..num_tracks {
            if state.channels[k].pattern_loops > 0 {
                return true;
            }
        }
        false
    }

    // ========================================================================
    // 4. Vollständige Effekt-Verarbeitung (effects.inc & play.inc)
    // ========================================================================

    /// Erhöht die Tonhöhe um `steps` Halbtöne (verkleinert die Periode `voice`)
    /// (`DoVoiceHigher` in `legacy_src/PLAYER/effects.inc` Zeilen 6–38).
    #[inline]
    pub fn do_voice_higher(&self, voice: u16, steps: u8) -> u16 {
        if steps == 0 {
            return voice;
        }
        if self.module.state.use_linear_tune {
            // Linear: 128 * steps abziehen
            voice.wrapping_sub((steps as u16) << 7)
        } else {
            // Amiga: voice * 2^(-steps/12) über iterative 16-Bit-Multiplikation mit HalfNote ($F1A2)
            let bx = HALF_NOTE as u32;
            let mut ax: u32 = 0xFFFF;
            for _ in 0..steps {
                let prod = ax * bx;
                ax = prod >> 16;
            }
            let prod = ax * (voice as u32);
            (prod >> 16) as u16
        }
    }

    /// Verringert die Tonhöhe um `steps` Halbtöne (vergrößert die Periode `voice`)
    /// (`DoVoiceLower` in `legacy_src/PLAYER/effects.inc` Zeilen 40–79).
    #[inline]
    pub fn do_voice_lower(&self, voice: u16, steps: u8) -> u16 {
        if steps == 0 {
            return voice;
        }
        if self.module.state.use_linear_tune {
            // Linear: 128 * steps addieren
            voice.wrapping_add((steps as u16) << 7)
        } else {
            // Amiga: voice * 2^(steps/12) über iterative 32-Bit-Multiplikation mit HalfNote1 ($10F39)
            let ebx = HALF_NOTE_1 as u64;
            let mut eax: u64 = 0x0000_FFFF;
            for _ in 0..steps {
                let prod = eax * ebx;
                eax = (prod >> 16) & 0xFFFF_FFFF;
            }
            let prod = eax * (voice as u64);
            if (prod >> 32) != 0 {
                GEN_HILIMIT
            } else {
                (prod >> 16) as u16
            }
        }
    }

    /// Klassisches Tracker-Arpeggio `0xy` (`DoArpeggio` in `legacy_src/PLAYER/effects.inc` Zeilen 82–113).
    pub fn do_arpeggio(&mut self, ch_idx: usize, ops: u8, base_ton: u16) -> u16 {
        if ops == 0 {
            return base_ton;
        }
        let mut cl = self.module.state.channels[ch_idx].acnt.wrapping_sub(1);
        if cl >= 3 {
            cl = 2;
        }
        self.module.state.channels[ch_idx].acnt = cl;

        let shift = cl << 2;
        let steps = (((ops as u16) >> shift) & 0x0F) as u8;
        if steps == 0 {
            base_ton
        } else {
            self.do_voice_higher(base_ton, steps)
        }
    }

    /// Oktalyzer-Arpeggio Up/Down (`DoArpeggio1` in `legacy_src/PLAYER/effects.inc` Zeilen 115–149).
    pub fn do_arpeggio_1(&mut self, ch_idx: usize, ops: u8, base_ton: u16) -> u16 {
        if ops == 0 {
            return base_ton;
        }
        let mut cl = self.module.state.channels[ch_idx].acnt.wrapping_add(1);
        if cl >= 4 {
            cl = 0;
        }
        self.module.state.channels[ch_idx].acnt = cl;

        match cl {
            0 => self.do_voice_higher(base_ton, ops >> 4),
            2 => self.do_voice_lower(base_ton, ops & 0x0F),
            _ => base_ton,
        }
    }

    /// Oktalyzer-Arpeggio Down/Up (`DoArpeggio2` in `legacy_src/PLAYER/effects.inc` Zeilen 151–185).
    pub fn do_arpeggio_2(&mut self, ch_idx: usize, ops: u8, base_ton: u16) -> u16 {
        if ops == 0 {
            return base_ton;
        }
        let mut cl = self.module.state.channels[ch_idx].acnt.wrapping_add(1);
        if cl >= 4 {
            cl = 0;
        }
        self.module.state.channels[ch_idx].acnt = cl;

        match cl {
            0 => self.do_voice_lower(base_ton, ops >> 4),
            2 => self.do_voice_higher(base_ton, ops & 0x0F),
            _ => base_ton,
        }
    }

    /// Berechnet die Perioden-Differenz für Portamento-Effekte (`GetDeltaPeriod` in `legacy_src/PLAYER/effects.inc` Zeilen 188–218).
    ///
    /// Gibt `(delta, overflow)` zurück (`overflow == true` entspricht `ZF == 0` im Original-ASM).
    #[inline]
    pub fn get_delta_period(&self, ops: u8) -> (u16, bool) {
        let speed = self.module.state.speed;
        if speed == 0 {
            return (0, false);
        }
        let spd_m1_x8 = ((speed.wrapping_sub(1) as u16) << 3) as u32;
        let curr_tck_p1 = (self.module.state.curr_vblanc.wrapping_add(1)) as u32;
        let num = spd_m1_x8 * (ops as u32) * curr_tck_p1;
        let eax = num / (speed as u32);
        if (eax >> 16) != 0 {
            (0xFFFF, true)
        } else {
            (eax as u16, false)
        }
    }

    /// Portamento Up (`1xx`) (`DoPortamento_up` in `legacy_src/PLAYER/effects.inc` Zeilen 221–248).
    pub fn do_portamento_up(&mut self, ch_idx: usize, ops: u8) -> i16 {
        let orig_ton = self.module.state.channels[ch_idx].tonh;
        if ops == 0 {
            return 0;
        }
        let (ax, overflow) = self.get_delta_period(ops);
        let cx = if overflow || orig_ton <= ax {
            GEN_LOLIMIT
        } else {
            orig_ton - ax
        };

        if self.module.state.curr_vblanc.wrapping_add(1) == (self.module.state.speed as u16) {
            self.module.state.channels[ch_idx].tonh = cx;
        }
        let current_ton = self.module.state.channels[ch_idx].tonh;
        (cx as i16).wrapping_sub(current_ton as i16)
    }

    /// Portamento Down (`2xx`) (`DoPortamento_down` in `legacy_src/PLAYER/effects.inc` Zeilen 251–277).
    pub fn do_portamento_down(&mut self, ch_idx: usize, ops: u8) -> i16 {
        let orig_ton = self.module.state.channels[ch_idx].tonh;
        if ops == 0 {
            return 0;
        }
        let (ax, overflow) = self.get_delta_period(ops);
        let cx = if overflow {
            GEN_HILIMIT
        } else {
            // effects.inc Zeilen 249–253:
            // add cx, ax; jnc @1; mov cx, gen_hilimit -> Clamping nur bei 16-Bit Carry!
            let (sum, carry) = orig_ton.overflowing_add(ax);
            if carry {
                GEN_HILIMIT
            } else {
                sum
            }
        };

        if self.module.state.curr_vblanc.wrapping_add(1) == (self.module.state.speed as u16) {
            self.module.state.channels[ch_idx].tonh = cx;
        }
        let current_ton = self.module.state.channels[ch_idx].tonh;
        (cx as i16).wrapping_sub(current_ton as i16)
    }

    /// Tone Portamento (`3xx`) inklusive Glissando (`E3x`)
    /// (`DoTone_portamento` in `legacy_src/PLAYER/effects.inc` Zeilen 280–392).
    pub fn do_tone_portamento(&mut self, ch_idx: usize, ops: u8) -> i16 {
        let orig_ton = self.module.state.channels[ch_idx].tonh;
        let dx_dest = self.module.state.channels[ch_idx].tpton;
        let gliss = self.module.state.channels[ch_idx].glissando_on;

        if ops == 0 || dx_dest == 0 || dx_dest == orig_ton {
            return 0;
        }

        let (mut ax, overflow) = self.get_delta_period(ops);
        let mut cx: u16;

        if overflow {
            cx = dx_dest;
        } else {
            cx = if gliss {
                self.module.state.channels[ch_idx].tp_gl_ton
            } else {
                orig_ton
            };

            if cx >= dx_dest {
                // Verringern (Pitch höher)
                if cx <= ax {
                    cx = dx_dest;
                } else {
                    cx -= ax;
                    if gliss {
                        ax = self.do_voice_higher(orig_ton, 1);
                        if cx <= ax {
                            cx = ax;
                            self.module.state.channels[ch_idx].tonh = cx;
                            if cx < dx_dest {
                                cx = dx_dest;
                            }
                        } else {
                            ax = cx;
                            cx = orig_ton;
                        }
                    } else if cx < dx_dest {
                        cx = dx_dest;
                    }
                }
            } else {
                // Erhöhen (Pitch tiefer)
                let (sum, carry) = cx.overflowing_add(ax);
                if carry {
                    cx = dx_dest;
                } else {
                    cx = sum;
                    if gliss {
                        ax = self.do_voice_lower(orig_ton, 1);
                        if cx >= ax {
                            cx = ax;
                            self.module.state.channels[ch_idx].tonh = cx;
                            if cx > dx_dest {
                                cx = dx_dest;
                            }
                        } else {
                            ax = cx;
                            cx = orig_ton;
                        }
                    } else if cx > dx_dest {
                        cx = dx_dest;
                    }
                }
            }
        }

        if self.module.state.curr_vblanc.wrapping_add(1) == (self.module.state.speed as u16) {
            if gliss {
                self.module.state.channels[ch_idx].tp_gl_ton = ax;
            } else {
                self.module.state.channels[ch_idx].tonh = cx;
            }
        }

        let current_ton = self.module.state.channels[ch_idx].tonh;
        (cx as i16).wrapping_sub(current_ton as i16)
    }

    /// 669 / UNIS Frequenz-Portamento Up (`AdjustFrequencyUp` in `legacy_src/PLAYER/effects.inc` Zeilen 395–434).
    pub fn adjust_frequency_up(&self, toneh: &mut u16, ops: u8) {
        if ops == 0 || *toneh == 0 {
            return;
        }
        let freq = 0xFFFF_FFFFu32 / (*toneh as u32);
        let scale: u32 = if self.module.state.mi.file_type == ModType::F669 {
            1592 * 4
        } else {
            1522 * 4
        };
        let add_freq = ((ops as u32) * 2 - 1).wrapping_mul(scale);
        let (new_freq, carry) = freq.overflowing_add(add_freq);
        if carry || new_freq == 0 {
            *toneh = GEN_LOLIMIT;
            return;
        }
        let new_period = 0xFFFF_FFFFu32 / new_freq;
        if (new_period >> 16) != 0 || new_period > (GEN_HILIMIT as u32) {
            *toneh = GEN_HILIMIT;
        } else {
            *toneh = new_period as u16;
        }
    }

    /// 669 / UNIS Frequenz-Portamento Down (`AdjustFrequencyDown` in `legacy_src/PLAYER/effects.inc` Zeilen 437–476).
    pub fn adjust_frequency_down(&self, toneh: &mut u16, ops: u8) {
        if ops == 0 || *toneh == 0 {
            return;
        }
        let freq = 0xFFFF_FFFFu32 / (*toneh as u32);
        let scale: u32 = if self.module.state.mi.file_type == ModType::F669 {
            1592 * 4
        } else {
            1522 * 4
        };
        let sub_freq = ((ops as u32) * 2 - 1).wrapping_mul(scale);
        if freq <= sub_freq {
            *toneh = GEN_HILIMIT;
            return;
        }
        let new_freq = freq - sub_freq;
        let new_period = 0xFFFF_FFFFu32 / new_freq;
        if (new_period >> 16) != 0 || new_period > (GEN_HILIMIT as u32) {
            *toneh = GEN_HILIMIT;
        } else {
            *toneh = new_period as u16;
        }
    }

    /// Berechnet den neuen Wellenform-Tabellenindex für Vibrato und Tremolo
    /// (`GetNewPointer` in `legacy_src/PLAYER/effects.inc` Zeilen 479–527).
    #[inline]
    pub fn get_new_pointer(&self, ops: u8) -> u8 {
        let state = &self.module.state;
        let vblank_p1 = (state.curr_vblanc.wrapping_add(1)) as u32;
        if matches!(state.mi.file_type, ModType::F669 | ModType::UNIS) {
            ((vblank_p1 << 6) & 0xFF) as u8
        } else {
            let x_times_4 = (((ops >> 2) & 0x3C) as u32) * vblank_p1;
            if state.mi.file_type == ModType::MTM {
                let bpm = (state.bpm_speed.max(1)) as u32;
                (((x_times_4 * 125) / bpm) & 0xFF) as u8
            } else {
                let speed = (state.speed.max(1)) as u32;
                (((x_times_4 * speed.saturating_sub(1)) / speed) & 0xFF) as u8
            }
        }
    }

    /// Liest einen Wert aus der über `table_id` gewählten Wellenform-Tabelle (`SinTable`,
    /// `RampUpTable`, `RampDnTable` oder `SqrTable`).
    #[inline]
    pub fn lookup_wave_table(table_id: u16, idx: u8) -> i8 {
        match table_id {
            WAVE_SINE => SIN_TABLE[idx as usize],
            WAVE_RAMP_UP => RAMP_UP_TABLE[idx as usize],
            WAVE_RAMP_DN => RAMP_DN_TABLE[idx as usize],
            WAVE_SQUARE => SQR_TABLE[idx as usize],
            _ => SIN_TABLE[idx as usize],
        }
    }

    /// Berechnet Vibrato (`4xy`) bzw. Fine Vibrato (`Uxy`)
    /// (`DoVibrato` in `legacy_src/PLAYER/effects.inc` Zeilen 530–578).
    pub fn do_vibrato(&mut self, ch_idx: usize, ops: u8, base_ton: u16, fine: bool) -> u16 {
        let step = self.get_new_pointer(ops);
        let vtini = self.module.state.channels[ch_idx].vtini;
        let al_idx = step.wrapping_add(vtini);

        if self.module.state.curr_vblanc.wrapping_add(1) == (self.module.state.speed as u16) {
            self.module.state.channels[ch_idx].vtini = al_idx;
        }

        let v_table = self.module.state.channels[ch_idx].v_table;
        let wave_val = Self::lookup_wave_table(v_table, al_idx) as i16;
        let depth = (ops & 0x0F) as i16;
        let mut ax = (wave_val * depth) >> 3;
        if fine {
            ax >>= 2;
        }
        if self.module.state.mi.file_type == ModType::MTM {
            ax >>= 1;
        }

        let res = (base_ton as i32) - (ax as i32);
        if self.module.state.sound_settings.clean_mode() {
            res.clamp(GEN_LOLIMIT as i32, GEN_HILIMIT as i32) as u16
        } else if ax < 0 {
            // sub cx, ax (addiert positiven Betrag; jc @Check -> nur bei 16-bit Überlauf >= 65536 wird GEN_HILIMIT gesetzt!)
            if res >= 65536 {
                GEN_HILIMIT
            } else {
                res as u16
            }
        } else if res < 0 {
            // @down: sub cx, ax; jnc @Check; mov cx, gen_lolimit
            GEN_LOLIMIT
        } else {
            res as u16
        }
    }

    /// Berechnet Tremolo (`7xy`) (`DoTremolo` in `legacy_src/PLAYER/effects.inc` Zeilen 580–621).
    pub fn do_tremolo(&mut self, ch_idx: usize, ops: u8) {
        let step = self.get_new_pointer(ops);
        let vtini = self.module.state.channels[ch_idx].vtini;
        let al_idx = step.wrapping_add(vtini);

        if self.module.state.curr_vblanc.wrapping_add(1) == (self.module.state.speed as u16) {
            self.module.state.channels[ch_idx].vtini = al_idx;
        }

        let t_table = self.module.state.channels[ch_idx].t_table;
        let wave_val = Self::lookup_wave_table(t_table, al_idx) as i16;
        let depth = (ops & 0x0F) as i16;
        let mut ax = (wave_val * depth) >> 6;
        if self.module.state.mi.file_type == ModType::MTM {
            ax >>= 1;
        }

        let volsld = self.module.state.channels[ch_idx].volsld as i16;
        let new_vol = (volsld - ax).clamp(0, 64) as u8;
        self.module.state.channels[ch_idx].vol = new_vol;
    }

    /// Berechnet Volume Slide (`Axy`, `5xy`, `6xy`) für einen Lautstärkewert (`vol` oder `glob_vol`)
    /// (`DoVolumeSlide` in `legacy_src/PLAYER/effects.inc` Zeilen 624–662).
    #[inline]
    pub fn apply_volume_slide_value(
        curr_vblanc: u16,
        fast_slides: bool,
        file_type: ModType,
        ops: u8,
        vol: &mut u8,
    ) {
        if curr_vblanc != 0 || fast_slides {
            let slide_up = if matches!(file_type, ModType::STM | ModType::S3M) {
                (ops & 0x0F) == 0
            } else {
                (ops & 0xF0) != 0
            };

            if slide_up {
                let up = ops >> 4;
                *vol = vol.saturating_add(up).min(64);
            } else {
                let dn = ops & 0x0F;
                *vol = vol.saturating_sub(dn);
            }
        }
    }

    /// Führt Volume Slide auf dem Kanalvolumen `channels[ch_idx].vol` aus.
    #[inline]
    pub fn do_volume_slide_channel(&mut self, ch_idx: usize, ops: u8) {
        let curr_vblanc = self.module.state.curr_vblanc;
        let fast_slides = self.module.state.fast_slides;
        let file_type = self.module.state.mi.file_type;
        let chn = &mut self.module.state.channels[ch_idx];
        Self::apply_volume_slide_value(curr_vblanc, fast_slides, file_type, ops, &mut chn.vol);
    }

    /// FührtPanning Slide (`Pxy` / FT2 VolColumn `$Dx`/`$Ex`) auf `channels[ch_idx].pan_val` aus
    /// (`DoPanningSlide` in `legacy_src/PLAYER/effects.inc` Zeilen 665–696).
    #[inline]
    pub fn do_panning_slide(&mut self, ch_idx: usize, ops: u8) {
        let state = &mut self.module.state;
        if state.curr_vblanc != 0 || state.fast_slides {
            let chn = &mut state.channels[ch_idx];
            if (ops & 0xF0) != 0 {
                let right = ops >> 4;
                chn.pan_val = chn.pan_val.saturating_add(right);
            } else {
                let left = ops & 0x0F;
                chn.pan_val = chn.pan_val.saturating_sub(left);
            }
        }
    }

    /// Oktalyzer Slide Up (`DoOktSldUp` in `legacy_src/PLAYER/effects.inc` Zeilen 698–708).
    #[inline]
    pub fn do_okt_sld_up(&mut self, ch_idx: usize, ops: u8) -> u16 {
        let tonh = self.module.state.channels[ch_idx].tonh;
        let new_ton = self.do_voice_higher(tonh, ops);
        self.module.state.channels[ch_idx].tonh = new_ton;
        new_ton
    }

    /// Oktalyzer Slide Down (`DoOktSldDwn` in `legacy_src/PLAYER/effects.inc` Zeilen 710–720).
    #[inline]
    pub fn do_okt_sld_dwn(&mut self, ch_idx: usize, ops: u8) -> u16 {
        let tonh = self.module.state.channels[ch_idx].tonh;
        let new_ton = self.do_voice_lower(tonh, ops);
        self.module.state.channels[ch_idx].tonh = new_ton;
        new_ton
    }

    /// Sample-Offset `9xx` (`DoPlayOffset` in `legacy_src/PLAYER/effects.inc` Zeilen 722–760).
    pub fn do_play_offset(&mut self, ch_idx: usize) {
        let chn = &mut self.module.state.channels[ch_idx];
        if chn.iend <= 0 {
            return;
        }
        chn.rest &= !0xFFFF; // mov word ptr chn.Rest, 0
        let ofs_add = (chn.opnd as i32) << 8;

        let mut ax = if chn.note == 0 {
            chn.i_offset.wrapping_add(ofs_add)
        } else {
            chn.note.wrapping_add(ofs_add)
        };

        if ax > chn.iend {
            let dx = if chn.lend != 0 {
                chn.lln
            } else {
                chn.iend.wrapping_sub(chn.i_offset)
            };
            if dx > 0 {
                while ax >= chn.iend {
                    ax = ax.wrapping_sub(dx);
                }
            } else {
                ax = chn.i_offset;
            }
        }
        chn.note = ax;
    }

    /// Setzt die Position einer FT2-Hüllkurve (`Lxx` / `SetEnvPos` in `legacy_src/PLAYER/play.inc` Zeilen 115–132).
    pub fn set_env_pos(
        new_val: &mut u8,
        new_pos: u16,
        env_pos: &mut u8,
        env_tck: &mut u16,
        env_pts: &[EnvPoint; 12],
        num_pts: u8,
        loop_end: u8,
    ) {
        let np = (num_pts as usize).min(12);
        if np > 1 {
            let mut tck = new_pos;
            let max_tck = env_pts[np - 1].tick;
            if tck > max_tck {
                tck = max_tck;
            }
            if loop_end > 0 && (loop_end as usize) <= np {
                let le_tck = env_pts[(loop_end - 1) as usize].tick;
                if tck > le_tck {
                    tck = le_tck;
                }
            }
            *env_tck = tck;
            let mut pos = 1u8;
            for i in 2..=np {
                if env_pts[i - 1].tick > tck {
                    pos = (i - 1) as u8;
                    break;
                }
            }
            *env_pos = pos;
            let p_idx = (pos - 1) as usize;
            let n_idx = (pos as usize).min(np - 1);
            let t0 = env_pts[p_idx].tick as i32;
            let t1 = env_pts[n_idx].tick as i32;
            let v0 = env_pts[p_idx].value as i32;
            let v1 = env_pts[n_idx].value as i32;
            if t1 > t0 {
                let val = (((tck as i32) - t0) * (v1 - v0)) / (t1 - t0) + v0;
                *new_val = val.clamp(0, 64) as u8;
            } else {
                *new_val = v0.clamp(0, 64) as u8;
            }
        }
    }

    /// Wertet die Effekt-Initialisierung und das Effect-Memory auf Tick 0 innerhalb von
    /// `ProcessNewNote` aus (`CASE Effekt OF` in `legacy_src/PLAYER/play.inc` Zeilen 800–977).
    pub fn process_tick0_effects(&mut self, ch_idx: usize, note: &mut Note) {
        let file_type = self.module.state.mi.file_type;
        let is_s3m_or_stm = matches!(file_type, ModType::S3M | ModType::STM);
        let is_s3m_stm_okt_ft2 = matches!(
            file_type,
            ModType::S3M | ModType::STM | ModType::Oktalyz | ModType::FT2
        );
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

        match note.effekt {
            effect_ids::NO_EFFEKT => {
                let def_ch_vol = if file_type == ModType::FT2 {
                    0
                } else {
                    DEF_CH_VOL
                };
                let chn = &self.module.state.channels[ch_idx];
                if chn.effkt == effect_ids::FAR_PORTAMENTO {
                    note.effekt = effect_ids::FAR_PORTAMENTO;
                    note.operands = chn.opnd;
                } else if chn.effkt == effect_ids::VOLUME_PORT
                    && chn.vol != chn.vol_prt
                    && note.chn_vol == def_ch_vol
                {
                    note.effekt = effect_ids::VOLUME_PORT;
                    note.operands = chn.opnd;
                } else if chn.effkt == effect_ids::SLOT_RETRIGGER {
                    note.effekt = effect_ids::SLOT_RETRIGGER;
                    note.operands = chn.opnd;
                }
            }
            effect_ids::SET_GLOB_VOL => {
                let gv = note.operands.min(64);
                self.module.state.glob_vol = gv;
                for i in 0..num_tracks {
                    self.module.state.channels[i].event_chg |= event_flags::E_VOL;
                }
            }
            effect_ids::XTRA_F_SLD_UP | effect_ids::PORTAMENTO_UP => {
                if is_s3m_stm_okt_ft2 {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if note.operands == 0 {
                        note.operands = chn.prt_ops;
                    } else {
                        chn.prt_ops = note.operands;
                    }
                    if is_s3m_or_stm && note.operands >= 0xE0 {
                        if note.operands >= 0xF0 {
                            note.effekt = effect_ids::F_SLD_UP;
                        } else {
                            note.effekt = effect_ids::XTRA_F_SLD_UP;
                        }
                        note.operands &= 0x0F;
                    }
                }
            }
            effect_ids::XTRA_F_SLD_DWN | effect_ids::PORTAMENTO_DOWN => {
                if is_s3m_stm_okt_ft2 {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if note.operands == 0 {
                        note.operands = chn.prt_ops;
                    } else {
                        chn.prt_ops = note.operands;
                    }
                    if is_s3m_or_stm && note.operands >= 0xE0 {
                        if note.operands >= 0xF0 {
                            note.effekt = effect_ids::F_SLD_DWN;
                        } else {
                            note.effekt = effect_ids::XTRA_F_SLD_DWN;
                        }
                        note.operands &= 0x0F;
                    }
                }
            }
            effect_ids::FINE_VOL_UP | effect_ids::FINE_VOL_DWN | effect_ids::VOLUME_SLIDE => {
                // Wichtig: In PLAYER32.DLL (play.inc Zeilen 799–816) teilen sich FineVolUp,
                // FineVolDwn und VolumeSlide in allen Modulformaten das Effektgedächtnis `VSldOps`
                // (der zweite CASE-Zweig mit fVuOps/fVdOps in play.inc Zeilen 962–967 wird von
                // Borland Pascal 7.0 durch den früheren CASE-Treffer nie erreicht).
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.vsld_ops;
                } else {
                    chn.vsld_ops = note.operands;
                }
                if is_s3m_or_stm {
                    if (note.operands & 0x0F) == 0x0F && (note.operands & 0xF0) != 0 {
                        note.effekt = effect_ids::FINE_VOL_UP;
                        note.operands >>= 4;
                    } else if (note.operands & 0xF0) == 0xF0 && (note.operands & 0x0F) != 0 {
                        note.effekt = effect_ids::FINE_VOL_DWN;
                        note.operands &= 0x0F;
                    }
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::GLOB_VOL_SLD => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.gv_sld_ops;
                } else {
                    chn.gv_sld_ops = note.operands;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::PAN_SLD => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.p_sld_ops;
                } else {
                    chn.p_sld_ops = note.operands;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::ARPEGGIO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.effkt != effect_ids::ARPEGGIO {
                    chn.acnt = 2;
                }
                if note.operands == 0 {
                    note.operands = chn.arp_ops;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.arp_ops = note.operands;
                }
            }
            effect_ids::SET_SPEED => {
                self.do_set_speed(note.operands);
            }
            effect_ids::S3M_BPM => {
                if note.operands >= 32 {
                    self.module.state.force_bpm = true;
                    self.do_set_speed(note.operands);
                    self.module.state.force_bpm = false;
                }
            }
            effect_ids::TONE_PORTAMENTO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if file_type != ModType::FT2
                    && chn.effkt != effect_ids::TONE_PORTAMENTO
                    && note.ton == 0
                {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
                if note.operands == 0 {
                    note.operands = chn.prt_n_ops;
                } else {
                    chn.prt_n_ops = note.operands;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::FAR_PORTAMENTO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.ton != 0 {
                    chn.tpton = note.ton;
                    chn.fp_tonh = chn.tonh;
                    if note.operands == 0 {
                        note.effekt = effect_ids::NO_EFFEKT;
                    } else {
                        chn.fp_vblanc = 1;
                    }
                } else if chn.effkt != effect_ids::FAR_PORTAMENTO {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::VOLUME_PORT => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.chn_vol != DEF_CH_VOL && note.operands != 0 {
                    chn.vol_prt = note.chn_vol;
                    note.chn_vol = DEF_CH_VOL;
                    chn.volsld = chn.vol;
                    chn.vp_vblanc = 1;
                } else if chn.effkt != effect_ids::VOLUME_PORT {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::PORT_VOLSLIDE => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.vsld_ops;
                } else {
                    chn.vsld_ops = note.operands;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::FINE_VIBRATO | effect_ids::VIBRATO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if !matches!(chn.effkt, effect_ids::VIBRATO | effect_ids::FINE_VIBRATO) {
                    chn.vtini = if file_type == ModType::MTM { 128 } else { 0 };
                }
                if (note.operands & 0xF0) == 0 {
                    note.operands |= chn.v_ops & 0xF0;
                }
                if (note.operands & 0x0F) == 0 {
                    note.operands |= chn.v_ops & 0x0F;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.v_ops = note.operands;
                }
            }
            effect_ids::VIBRA_VOLSLID => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.vsld_ops;
                } else {
                    chn.vsld_ops = note.operands;
                }
            }
            effect_ids::VIBRATO_DEPTH => {
                self.module.state.far_vib_depth = note.operands;
            }
            effect_ids::VIBRATO_SUST | effect_ids::FAR_VIBRATO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if !matches!(
                    chn.effkt,
                    effect_ids::VIBRATO_SUST | effect_ids::FAR_VIBRATO
                ) {
                    chn.vtini = 0;
                }
                if note.effekt == effect_ids::VIBRATO_SUST && note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                }
            }
            effect_ids::TREMOLO => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.effkt != effect_ids::TREMOLO {
                    chn.vtini = 0;
                    chn.volsld = chn.vol;
                }
                if note.chn_vol != DEF_CH_VOL {
                    if file_type != ModType::FT2 {
                        chn.volsld = note.chn_vol;
                    } else if note.chn_vol < 0x60 && note.chn_vol >= 0x10 {
                        chn.volsld = note.chn_vol - 0x10;
                    }
                }
                if (note.operands & 0xF0) == 0 {
                    note.operands |= chn.t_ops & 0xF0;
                }
                if (note.operands & 0x0F) == 0 {
                    note.operands |= chn.t_ops & 0x0F;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.t_ops = note.operands;
                }
            }
            effect_ids::OKT_ARPG1 => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.effkt != effect_ids::OKT_ARPG1 {
                    chn.acnt = 0;
                }
                if note.operands == 0 {
                    note.operands = chn.arp1_ops;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.arp1_ops = note.operands;
                }
            }
            effect_ids::OKT_ARPG2 => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.effkt != effect_ids::OKT_ARPG2 {
                    chn.acnt = 0;
                }
                if note.operands == 0 {
                    note.operands = chn.arp2_ops;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.arp2_ops = note.operands;
                }
            }
            effect_ids::OKT_SLD_UP => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.okt_su_ops;
                    if note.operands == 0 {
                        note.effekt = effect_ids::NO_EFFEKT;
                    }
                } else {
                    chn.okt_su_ops = note.operands;
                }
            }
            effect_ids::OKT_SLD_DWN => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.okt_sd_ops;
                    if note.operands == 0 {
                        note.effekt = effect_ids::NO_EFFEKT;
                    }
                } else {
                    chn.okt_sd_ops = note.operands;
                }
            }
            effect_ids::OKT_SLD1_UP => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.okt_su1_ops;
                    if note.operands == 0 {
                        note.effekt = effect_ids::NO_EFFEKT;
                    }
                } else {
                    chn.okt_su1_ops = note.operands;
                }
            }
            effect_ids::OKT_SLD1_DWN => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.okt_sd1_ops;
                    if note.operands == 0 {
                        note.effekt = effect_ids::NO_EFFEKT;
                    }
                } else {
                    chn.okt_sd1_ops = note.operands;
                }
            }
            effect_ids::FINE_TEMPO_UP => {
                self.do_fine_far_tempo(note.operands as i8);
            }
            effect_ids::FINE_TEMPO_DWN => {
                self.do_fine_far_tempo((note.operands as i8).wrapping_neg());
            }
            effect_ids::FAR_RETRIG => {
                if note.operands > 0 {
                    let speed = self.module.state.speed;
                    note.operands = 1u8.wrapping_add(speed / note.operands);
                }
            }
            effect_ids::RETRIG_VOL_SLD => {
                let chn = &mut self.module.state.channels[ch_idx];
                if (note.operands & 0x0F) == 0 {
                    note.operands |= chn.retr_ops & 0x0F;
                }
                if (note.operands & 0xF0) == 0 {
                    note.operands |= chn.retr_ops & 0xF0;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.retr_ops = note.operands;
                }
                if chn.effkt != effect_ids::RETRIG_VOL_SLD {
                    chn.acnt = 0;
                }
            }
            effect_ids::SLOT_RETRIGGER => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else if chn.old_volume == 0 {
                    note.operands /= 2;
                    chn.retr_ops = 0;
                    chn.old_volume = 4;
                }
            }
            effect_ids::TREMOR => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.tremor_ops;
                } else {
                    chn.tremor_ops = note.operands;
                }
                if chn.effkt != effect_ids::TREMOR {
                    chn.acnt = 0;
                }
            }
            effect_ids::VIBRATO_WAVE => {
                let chn = &mut self.module.state.channels[ch_idx];
                match note.operands & 0x03 {
                    0 => chn.v_table = WAVE_SINE,
                    1 => chn.v_table = WAVE_RAMP_UP,
                    2 => chn.v_table = WAVE_SQUARE,
                    _ => {}
                }
                if (note.operands & 0x04) == 0 {
                    chn.vtini = 0;
                }
            }
            effect_ids::TREMOLO_WAVE => {
                let chn = &mut self.module.state.channels[ch_idx];
                match note.operands & 0x03 {
                    0 => chn.t_table = WAVE_SINE,
                    1 => chn.t_table = WAVE_RAMP_UP,
                    2 => chn.t_table = WAVE_SQUARE,
                    _ => {}
                }
                if (note.operands & 0x04) == 0 {
                    chn.vtini = 0;
                }
            }
            effect_ids::PATTERN_DELAY => {
                let chn = &mut self.module.state.channels[ch_idx];
                if note.operands == 0 {
                    note.operands = chn.p_del_ops;
                }
                if note.operands == 0 {
                    note.effekt = effect_ids::NO_EFFEKT;
                } else {
                    chn.p_del_ops = note.operands;
                    self.module.state.delay_pattern = true;
                    self.module.state.patterndelay = note.operands;
                }
            }
            effect_ids::GLISSANDO_CTRL => {
                self.module.state.channels[ch_idx].glissando_on = note.operands != 0;
            }
            _ => {}
        }
    }

    /// Wertet die FT2 Volume-Column-Effektgedächtnisse innerhalb von `ProcessNewNote` aus
    /// (`CASE ChnVol OF` in `legacy_src/PLAYER/play.inc` Zeilen 1004–1060).
    pub fn process_ft2_vol_column_memory(&mut self, ch_idx: usize, note: &mut Note) {
        let chn = &mut self.module.state.channels[ch_idx];
        match note.chn_vol {
            0x00..=0x5F => {}
            0xA1..=0xAF => {
                // Set Vibrato Speed
                if !matches!(note.effekt, effect_ids::VIBRATO | effect_ids::FINE_VIBRATO) {
                    chn.v_ops = (chn.v_ops & 0x0F) | ((note.chn_vol << 4) & 0xF0);
                }
            }
            0x80..=0x8F => {
                // FineVolSldDn (bei FT2 ist DefChVol = 0, siehe PLAYER32.PAS Zeile 553)
                if note.effekt != effect_ids::FINE_VOL_DWN {
                    if note.chn_vol == 0x80 {
                        note.chn_vol = 0x80 | (chn.f_vsld_d_ops & 0x0F);
                    } else {
                        chn.f_vsld_d_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0x80 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0x90..=0x9F => {
                // FineVolSldUp
                if note.effekt != effect_ids::FINE_VOL_UP {
                    if note.chn_vol == 0x90 {
                        note.chn_vol = 0x90 | (chn.f_vsld_u_ops & 0x0F);
                    } else {
                        chn.f_vsld_u_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0x90 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0x60..=0x6F => {
                // VolSldDn
                if note.effekt != effect_ids::VOLUME_SLIDE {
                    if note.chn_vol == 0x60 {
                        note.chn_vol = 0x60 | (chn.v_sld_d_ops & 0x0F);
                    } else {
                        chn.v_sld_d_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0x60 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0x70..=0x7F => {
                // VolSldUp
                if note.effekt != effect_ids::VOLUME_SLIDE {
                    if note.chn_vol == 0x70 {
                        note.chn_vol = 0x70 | (chn.v_sld_u_ops & 0x0F);
                    } else {
                        chn.v_sld_u_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0x70 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0xD0..=0xDF => {
                // PanSlide Left
                if note.effekt != effect_ids::PAN_SLD {
                    if note.chn_vol == 0xD0 {
                        note.chn_vol = 0xD0 | (chn.p_sld_l_ops & 0x0F);
                    } else {
                        chn.p_sld_l_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0xD0 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0xE0..=0xEF => {
                // PanSlide Right
                if note.effekt != effect_ids::PAN_SLD {
                    if note.chn_vol == 0xE0 {
                        note.chn_vol = 0xE0 | (chn.p_sld_r_ops & 0x0F);
                    } else {
                        chn.p_sld_r_ops = note.chn_vol;
                    }
                    if note.chn_vol == 0xE0 {
                        note.chn_vol = 0;
                    }
                } else {
                    note.chn_vol = 0;
                }
            }
            0xB0..=0xBF => {
                // Vibrato
                if !matches!(note.effekt, effect_ids::VIBRATO | effect_ids::FINE_VIBRATO) {
                    if !matches!(chn.old_chn_vol, 0xB0..=0xBF) {
                        chn.vtini = 0;
                    }
                    if note.chn_vol == 0xB0 {
                        note.chn_vol |= chn.v_ops & 0x0F;
                    }
                    if note.chn_vol == 0xB0 {
                        note.chn_vol = 0;
                    } else {
                        chn.v_ops = (chn.v_ops & 0xF0) | (note.chn_vol & 0x0F);
                    }
                    if (chn.v_ops & 0xF0) == 0 {
                        note.chn_vol = 0;
                    }
                }
            }
            0xF0..=0xFF => {
                // Tone Portamento
                if !matches!(
                    note.effekt,
                    effect_ids::TONE_PORTAMENTO
                        | effect_ids::PORT_VOLSLIDE
                        | effect_ids::PORTAMENTO_UP
                        | effect_ids::PORTAMENTO_DOWN
                ) {
                    if note.chn_vol == 0xF0 {
                        note.chn_vol = 0xF0 | (chn.prt_n_ops >> 4);
                    } else {
                        chn.prt_n_ops = (note.chn_vol & 0x0F) << 4;
                    }
                    if note.chn_vol == 0xF0 {
                        note.chn_vol = 0;
                    }
                }
            }
            _ => {}
        }
    }

    /// Führt die Effekte aus, die ausschließlich beim ersten Tick einer Zeile (auch während
    /// `PatternDelay`) greifen (`ProcessFirstTick` in `legacy_src/PLAYER/play.inc` Zeilen 1106–1147).
    pub fn process_first_tick(&mut self, ch_idx: usize) {
        let file_type = self.module.state.mi.file_type;
        let use_linear_tune = self.module.state.use_linear_tune;
        let effkt = self.module.state.channels[ch_idx].effkt;
        let opnd = self.module.state.channels[ch_idx].opnd;

        match effkt {
            effect_ids::NO_EFFEKT => {}
            effect_ids::PANNING => {
                let chn = &mut self.module.state.channels[ch_idx];
                if file_type == ModType::FT2 {
                    chn.pan_val = opnd;
                } else if opnd < 128 {
                    chn.pan_val = opnd << 1;
                } else if opnd > 128 {
                    chn.pan_val = 128;
                } else {
                    chn.pan_val = 255;
                }
                chn.event_chg |= event_flags::E_VOL;
            }
            effect_ids::SET_VOLUME => {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.vol = opnd.min(64);
                chn.old_volume = chn.vol;
                chn.event_chg |= event_flags::E_VOL;
            }
            effect_ids::OKT_OLD_VOL => {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.vol = chn.old_volume;
                chn.event_chg |= event_flags::E_VOL;
            }
            effect_ids::STEREO_CONTROL if matches!(file_type, ModType::S3M | ModType::STM) => {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.s_cont = opnd.wrapping_add(1);
                chn.event_chg |= event_flags::E_VOL;
            }
            effect_ids::FINE_TUNE => {
                let chn = &mut self.module.state.channels[ch_idx];
                if use_linear_tune {
                    chn.new_fine_tune = opnd << 4;
                    let ift = (chn.i_fin_tun as i8) as i16;
                    let nft = (chn.new_fine_tune as i8) as i16;
                    chn.tonh = (chn.tonh as i16).wrapping_sub(ift).wrapping_add(nft) as u16;
                    if chn.new_tonh != 0 {
                        chn.new_tonh =
                            (chn.new_tonh as i16).wrapping_sub(ift).wrapping_add(nft) as u16;
                    }
                } else {
                    chn.new_fine_tune = opnd;
                }
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::E_PANNING => {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.pan_val = (opnd & 0x0F) * 17;
                chn.event_chg |= event_flags::E_VOL;
            }
            effect_ids::SET_ENVELOPE_POS => {
                let inst_idx = (self.module.state.channels[ch_idx].inst as usize)
                    .min(self.module.ft2_instruments.len().saturating_sub(1));
                let ft2_inst = self.module.ft2_instruments[inst_idx];
                let chn = &mut self.module.state.channels[ch_idx];
                let mut env_vol = chn.env_vol;
                let mut env_vol_pos = chn.env_vol_pos;
                let mut env_vol_tck = chn.env_vol_tck;
                let vol_pts = ft2_inst.ext.volenvpts;
                Self::set_env_pos(
                    &mut env_vol,
                    opnd as u16,
                    &mut env_vol_pos,
                    &mut env_vol_tck,
                    &vol_pts,
                    ft2_inst.ext.numvolpts,
                    ft2_inst.ext.volloopend,
                );
                chn.env_vol = env_vol;
                chn.env_vol_pos = env_vol_pos;
                chn.env_vol_tck = env_vol_tck;
            }
            effect_ids::KEY_OFF => {
                let row_idx = (self.module.state.curr_note as usize).min(255);
                self.active_pattern[ch_idx][row_idx].ton = 0;
                let chn = &mut self.module.state.channels[ch_idx];
                if file_type == ModType::FT2 && chn.env_vol_pos != 0 {
                    chn.do_fade_out = true;
                } else {
                    chn.note = 0;
                    chn.event_chg |= event_flags::E_RESET;
                }
            }
            _ => {}
        }
    }

    /// Wertet die Vor-Kernel-Tick-Effekte (`NoteDelay`, `Arpeggio`, `Tremor`, `RetrigNote`,
    /// `RetrigVolSlide`, `SlotRetrigger`, `NoteCut`) vor dem Aufruf des Mixers aus
    /// (`Play_the_Module` Zeilen 1344–1411 in `legacy_src/PLAYER/play.inc`).
    pub fn process_pre_kernel_tick_effects(&mut self, ch_idx: usize) {
        let curr_vblanc = self.module.state.curr_vblanc;
        let file_type = self.module.state.mi.file_type;
        let use_linear_tune = self.module.state.use_linear_tune;
        let speed = self.module.state.speed.max(1);

        if (self.module.state.channels[ch_idx].event_chg & event_flags::E_PITCH) == 0 {
            self.module.state.channels[ch_idx].new_tonh = self.module.state.channels[ch_idx].tonh;
        }

        // NoteDelay (EDx): triggert die Note, sobald CurrVBlanc + 1 == Operands
        let pat_note = self.get_current_pattern_note(ch_idx);
        if pat_note.effekt == effect_ids::NOTE_DELAY
            && curr_vblanc.wrapping_add(1) == (pat_note.operands as u16)
            && (pat_note.ton != 0 || pat_note.instrument != 0)
        {
            self.process_new_note(ch_idx);
        }

        let effkt = self.module.state.channels[ch_idx].effkt;
        let opnd = self.module.state.channels[ch_idx].opnd;

        match effkt {
            effect_ids::NO_EFFEKT => {}
            effect_ids::ARPEGGIO => {
                if use_linear_tune {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.acnt == 0 {
                        let sub = ((opnd & 0x0F) as u16) << 7;
                        chn.new_tonh = chn.new_tonh.wrapping_sub(sub);
                    } else if chn.acnt == 1 {
                        let sub = ((opnd >> 4) as u16) << 7;
                        chn.new_tonh = chn.new_tonh.wrapping_sub(sub);
                    }
                    if chn.acnt != 0 {
                        chn.acnt -= 1;
                    } else {
                        chn.acnt = 2;
                    }
                } else {
                    let base_ton = self.module.state.channels[ch_idx].new_tonh;
                    let new_ton = self.do_arpeggio(ch_idx, opnd, base_ton);
                    self.module.state.channels[ch_idx].new_tonh = new_ton;
                }
                self.module.state.channels[ch_idx].event_chg |= event_flags::E_PITCH;
            }
            effect_ids::OKT_ARPG1 => {
                let base_ton = self.module.state.channels[ch_idx].tonh;
                let new_ton = self.do_arpeggio_1(ch_idx, opnd, base_ton);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = new_ton;
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::OKT_ARPG2 => {
                let base_ton = self.module.state.channels[ch_idx].tonh;
                let new_ton = self.do_arpeggio_2(ch_idx, opnd, base_ton);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = new_ton;
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::TREMOR => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.vol > 0 {
                    if chn.acnt >= (opnd >> 4) {
                        chn.t_vol = chn.vol;
                        chn.vol = 0;
                        chn.event_chg |= event_flags::E_VOL;
                        chn.acnt = 0;
                    }
                } else if chn.acnt >= (opnd & 0x0F) {
                    chn.vol = chn.t_vol;
                    chn.event_chg |= event_flags::E_VOL;
                    chn.acnt = 0;
                }
                chn.acnt = chn.acnt.wrapping_add(1);
            }
            effect_ids::FAR_RETRIG => {
                if curr_vblanc == (opnd as u16) {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.note = chn.i_offset;
                    chn.event_chg |= event_flags::E_INST;
                    chn.opnd = chn.opnd.wrapping_add(pat_note.operands);
                }
            }
            effect_ids::RETRIG_NOTE => {
                if opnd != 0 && curr_vblanc != 0 && (curr_vblanc % (opnd as u16)) == 0 {
                    let smp_num = self.module.state.channels[ch_idx].smp_num;
                    let do_fade_out = self.module.state.channels[ch_idx].do_fade_out;
                    self.module.state.channels[ch_idx].note =
                        self.module.state.channels[ch_idx].i_offset;
                    if file_type == ModType::FT2 && smp_num != 0 && !do_fade_out {
                        // play.inc Zeile 1248: InitEnvelope(II, SmpNum);
                        self.init_envelope(ch_idx, smp_num as u8);
                    }
                    self.module.state.channels[ch_idx].event_chg |= event_flags::E_INST;
                }
            }
            effect_ids::RETRIG_VOL_SLD => {
                let acnt = self.module.state.channels[ch_idx].acnt;
                if acnt >= (opnd & 0x0F) {
                    let smp_num = self.module.state.channels[ch_idx].smp_num;
                    let do_fade_out = self.module.state.channels[ch_idx].do_fade_out;
                    self.module.state.channels[ch_idx].note =
                        self.module.state.channels[ch_idx].i_offset;
                    if file_type == ModType::FT2 && smp_num != 0 && !do_fade_out {
                        // play.inc Zeile 1255: InitEnvelope(II, SmpNum);
                        self.init_envelope(ch_idx, smp_num as u8);
                    }
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.event_chg |= event_flags::E_INST;
                    let mut v = chn.vol as i16;
                    match opnd >> 4 {
                        0 | 8 => {}
                        1 => {
                            v = (v - 1).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        2 => {
                            v = (v - 2).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        3 => {
                            v = (v - 4).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        4 => {
                            v = (v - 8).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        5 => {
                            v = (v - 16).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        6 => {
                            v = ((2 * v) / 3).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        7 => {
                            v = (v / 2).max(0);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        9 => {
                            v = (v + 1).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        10 => {
                            v = (v + 2).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        11 => {
                            v = (v + 4).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        12 => {
                            v = (v + 8).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        13 => {
                            v = (v + 16).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        14 => {
                            v = ((3 * v) / 2).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        15 => {
                            v = (2 * v).min(64);
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        _ => {}
                    }
                    chn.vol = v as u8;
                    chn.acnt = 0;
                }
                self.module.state.channels[ch_idx].acnt =
                    self.module.state.channels[ch_idx].acnt.wrapping_add(1);
            }
            effect_ids::SLOT_RETRIGGER => {
                let chn = &mut self.module.state.channels[ch_idx];
                if curr_vblanc == (chn.retr_ops as u16) {
                    chn.note = chn.i_offset;
                    chn.event_chg |= event_flags::E_INST;
                    chn.retr_ops = chn.retr_ops.wrapping_add(opnd) % speed;
                    chn.old_volume = chn.old_volume.saturating_sub(1);
                    if chn.old_volume == 0 {
                        chn.effkt = effect_ids::NO_EFFEKT;
                    }
                }
            }
            effect_ids::NOTE_CUT => {
                if curr_vblanc == (opnd as u16) {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if matches!(file_type, ModType::STM | ModType::S3M) {
                        chn.note = 0;
                        chn.event_chg |= event_flags::E_RESET;
                    } else {
                        chn.vol = 0;
                        chn.event_chg |= event_flags::E_VOL;
                    }
                }
            }
            _ => {}
        }
    }

    /// Kontinuierliche Effekt-Auswertung nach dem Mischen eines Ticks (`Play_the_Module`
    /// Zeilen 1525–1800 in `legacy_src/PLAYER/play.inc`).
    ///
    /// Umfasst:
    /// 1. FT2 Volume-Column-Effekte (`$61..$FF`)
    /// 2. First-Tick Fine-Slides (`CurrVBlanc == 0`: `FSldUp`, `FSldDwn`, `XtraFSldUp`, `XtraFSldDwn`,
    ///    `FineVolUp`, `FineVolDwn`, `FinePanLeft`, `FinePanRight`, `OktSld1Up`, `OktSld1Dwn`,
    ///    `FarVolUp`, `FarVolDwn`, `FarSldUp`, `FarSldDwn`, `FrequAdjust`)
    /// 3. Normale kontinuierliche Effekte (`Portamento_up`, `Portamento_down`, `Tone_portamento`,
    ///    `Vibrato`, `FineVibrato`, `PortVolslide`, `VibraVolslid`, `VolumeSlide`, `GlobVolSld`,
    ///    `PanSld`, `Tremolo`, `OktSldUp`, `OktSldDwn`, `FARportamento`, `VolumePort`, `FarVibrato`)
    pub fn process_tick_n_effects(&mut self, ch_idx: usize) {
        let curr_vblanc = self.module.state.curr_vblanc;
        let file_type = self.module.state.mi.file_type;
        let is_669_or_unis = matches!(file_type, ModType::F669 | ModType::UNIS);
        let speed = (self.module.state.speed.max(1)) as i32;
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

        self.module.state.channels[ch_idx].new_tonh = self.module.state.channels[ch_idx].tonh;

        // 1. FT2 Volume-Column-Effekte (play.inc Zeilen 1529–1560)
        if file_type == ModType::FT2 {
            let old_chn_vol = self.module.state.channels[ch_idx].old_chn_vol;
            match old_chn_vol {
                0x61..=0x6F => {
                    self.do_volume_slide_channel(ch_idx, old_chn_vol & 0x0F);
                    self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
                }
                0x71..=0x7F => {
                    self.do_volume_slide_channel(ch_idx, old_chn_vol << 4);
                    self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
                }
                0x80..=0x8F => {
                    if curr_vblanc == 0 {
                        let chn = &mut self.module.state.channels[ch_idx];
                        chn.vol = chn.vol.saturating_sub(old_chn_vol & 0x0F);
                        chn.event_chg |= event_flags::E_VOL;
                    }
                }
                0x90..=0x9F => {
                    if curr_vblanc == 0 {
                        let chn = &mut self.module.state.channels[ch_idx];
                        chn.vol = chn.vol.saturating_add(old_chn_vol & 0x0F).min(64);
                        chn.event_chg |= event_flags::E_VOL;
                    }
                }
                0xD1..=0xDF => {
                    self.do_panning_slide(ch_idx, old_chn_vol & 0x0F);
                    self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
                }
                0xE1..=0xEF => {
                    self.do_panning_slide(ch_idx, old_chn_vol << 4);
                    self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
                }
                0xB1..=0xBF => {
                    let v_ops = self.module.state.channels[ch_idx].v_ops;
                    let base_ton = self.module.state.channels[ch_idx].new_tonh;
                    let new_ton = self.do_vibrato(ch_idx, v_ops, base_ton, false);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = new_ton;
                    chn.event_chg |= event_flags::E_PITCH;
                }
                0xF0..=0xFF => {
                    let prt_n_ops = self.module.state.channels[ch_idx].prt_n_ops;
                    let delta = self.do_tone_portamento(ch_idx, prt_n_ops);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = (chn.new_tonh as i16).wrapping_add(delta) as u16;
                    chn.event_chg |= event_flags::E_PITCH;
                }
                _ => {}
            }
        }

        let effkt = self.module.state.channels[ch_idx].effkt;
        let opnd = self.module.state.channels[ch_idx].opnd;

        // 2. First-Tick FX (CurrVBlanc == 0, play.inc Zeilen 1562–1635)
        if curr_vblanc == 0 {
            match effkt {
                effect_ids::F_SLD_UP => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 8 * (opnd as u16);
                        if chn.tonh > GEN_LOLIMIT + step {
                            chn.tonh -= step;
                            chn.new_tonh = chn.new_tonh.saturating_sub(step);
                        } else {
                            chn.tonh = GEN_LOLIMIT;
                            chn.new_tonh = GEN_LOLIMIT;
                        }
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::F_SLD_DWN => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 8 * (opnd as u16);
                        if chn.tonh < GEN_HILIMIT.saturating_sub(step) {
                            chn.tonh += step;
                            chn.new_tonh = chn.new_tonh.saturating_add(step);
                        } else {
                            chn.tonh = GEN_HILIMIT;
                            chn.new_tonh = GEN_HILIMIT;
                        }
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::XTRA_F_SLD_UP => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 2 * (opnd as u16);
                        if chn.tonh > GEN_LOLIMIT + step {
                            chn.tonh -= step;
                            chn.new_tonh = chn.new_tonh.saturating_sub(step);
                        } else {
                            chn.tonh = GEN_LOLIMIT;
                            chn.new_tonh = GEN_LOLIMIT;
                        }
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::XTRA_F_SLD_DWN => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 2 * (opnd as u16);
                        if chn.tonh < GEN_HILIMIT.saturating_sub(step) {
                            chn.tonh += step;
                            chn.new_tonh = chn.new_tonh.saturating_add(step);
                        } else {
                            chn.tonh = GEN_HILIMIT;
                            chn.new_tonh = GEN_HILIMIT;
                        }
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::FINE_VOL_UP => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.vol = chn.vol.saturating_add(opnd).min(64);
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::FINE_VOL_DWN => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.vol = chn.vol.saturating_sub(opnd);
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::FINE_PAN_LEFT => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.pan_val = chn.pan_val.saturating_sub(17);
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::FINE_PAN_RIGHT => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.pan_val = chn.pan_val.saturating_add(17);
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::OKT_SLD1_UP => {
                    let new_ton = self.do_okt_sld_up(ch_idx, opnd);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = new_ton;
                    chn.event_chg |= event_flags::E_PITCH;
                }
                effect_ids::OKT_SLD1_DWN => {
                    let new_ton = self.do_okt_sld_dwn(ch_idx, opnd);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = new_ton;
                    chn.event_chg |= event_flags::E_PITCH;
                }
                effect_ids::FAR_VOL_UP => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.vol = chn.vol.saturating_add(4u8.saturating_mul(opnd)).min(64);
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::FAR_VOL_DWN if file_type == ModType::FFar => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.vol = chn.vol.saturating_sub(4u8.saturating_mul(opnd));
                    chn.event_chg |= event_flags::E_VOL;
                }
                effect_ids::FAR_SLD_UP => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 48 * (opnd as u16);
                        chn.tonh = if chn.tonh > GEN_LOLIMIT + step {
                            chn.tonh - step
                        } else {
                            GEN_LOLIMIT
                        };
                        chn.new_tonh = chn.tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::FAR_SLD_DWN => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.tonh > 0 {
                        let step = 48 * (opnd as u16);
                        chn.tonh = if chn.tonh < GEN_HILIMIT.saturating_sub(step) {
                            chn.tonh + step
                        } else {
                            GEN_HILIMIT
                        };
                        chn.new_tonh = chn.tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                effect_ids::FREQU_ADJUST => {
                    let tpton = self.module.state.channels[ch_idx].tpton;
                    if tpton > 0 {
                        let mut tonh = tpton;
                        self.adjust_frequency_up(&mut tonh, opnd);
                        if tonh < GEN_LOLIMIT {
                            tonh = GEN_LOLIMIT;
                        }
                        let chn = &mut self.module.state.channels[ch_idx];
                        chn.tonh = tonh;
                        chn.new_tonh = tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                }
                _ => {}
            }
        }

        // 3. Normal FX (alle Ticks, play.inc Zeilen 1636–1800)
        match effkt {
            effect_ids::NO_EFFEKT => {}
            effect_ids::PORTAMENTO_UP => {
                if is_669_or_unis {
                    let mut tonh = self.module.state.channels[ch_idx].tonh;
                    if tonh > 0 {
                        self.adjust_frequency_up(&mut tonh, opnd);
                        let chn = &mut self.module.state.channels[ch_idx];
                        if tonh < GEN_LOLIMIT {
                            tonh = GEN_LOLIMIT;
                            chn.effkt = effect_ids::NO_EFFEKT;
                        }
                        chn.tonh = tonh;
                        chn.new_tonh = tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                } else {
                    let delta = self.do_portamento_up(ch_idx, opnd);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = (chn.new_tonh as i16).wrapping_add(delta) as u16;
                    chn.event_chg |= event_flags::E_PITCH;
                }
            }
            effect_ids::PORTAMENTO_DOWN => {
                if is_669_or_unis {
                    let mut tonh = self.module.state.channels[ch_idx].tonh;
                    if tonh > 0 {
                        self.adjust_frequency_down(&mut tonh, opnd);
                        let chn = &mut self.module.state.channels[ch_idx];
                        if tonh > GEN_HILIMIT {
                            tonh = GEN_HILIMIT;
                            chn.effkt = effect_ids::NO_EFFEKT;
                        }
                        chn.tonh = tonh;
                        chn.new_tonh = tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                } else {
                    let delta = self.do_portamento_down(ch_idx, opnd);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = (chn.new_tonh as i16).wrapping_add(delta) as u16;
                    chn.event_chg |= event_flags::E_PITCH;
                }
            }
            effect_ids::TONE_PORTAMENTO => {
                if is_669_or_unis {
                    let mut tonh = self.module.state.channels[ch_idx].tonh;
                    let tpton = self.module.state.channels[ch_idx].tpton;
                    if tonh > 0 {
                        if tonh < tpton {
                            self.adjust_frequency_down(&mut tonh, opnd);
                            let chn = &mut self.module.state.channels[ch_idx];
                            if tonh > tpton {
                                tonh = tpton;
                                chn.effkt = effect_ids::NO_EFFEKT;
                            }
                            chn.tonh = tonh;
                            chn.new_tonh = tonh;
                            chn.event_chg |= event_flags::E_PITCH;
                        } else if tonh > tpton {
                            self.adjust_frequency_up(&mut tonh, opnd);
                            let chn = &mut self.module.state.channels[ch_idx];
                            if tonh < tpton {
                                tonh = tpton;
                                chn.effkt = effect_ids::NO_EFFEKT;
                            }
                            chn.tonh = tonh;
                            chn.new_tonh = tonh;
                            chn.event_chg |= event_flags::E_PITCH;
                        }
                    }
                } else {
                    let delta = self.do_tone_portamento(ch_idx, opnd);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = (chn.new_tonh as i16).wrapping_add(delta) as u16;
                    chn.event_chg |= event_flags::E_PITCH;
                }
            }
            effect_ids::FINE_VIBRATO | effect_ids::VIBRATO => {
                if is_669_or_unis {
                    let tpton = self.module.state.channels[ch_idx].tpton;
                    if tpton > 0 {
                        let vtini = self.module.state.channels[ch_idx].vtini;
                        let mut tonh = tpton;
                        match vtini % 4 {
                            1 => self.adjust_frequency_up(&mut tonh, opnd),
                            3 => self.adjust_frequency_down(&mut tonh, opnd),
                            _ => {}
                        }
                        let chn = &mut self.module.state.channels[ch_idx];
                        chn.vtini = chn.vtini.wrapping_add(1);
                        chn.tonh = tonh;
                        chn.new_tonh = tonh;
                        chn.event_chg |= event_flags::E_PITCH;
                    }
                } else {
                    let base_ton = self.module.state.channels[ch_idx].new_tonh;
                    let is_fine = effkt == effect_ids::FINE_VIBRATO;
                    let new_ton = self.do_vibrato(ch_idx, opnd, base_ton, is_fine);
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.new_tonh = new_ton;
                    chn.event_chg |= event_flags::E_PITCH;
                }
            }
            effect_ids::PORT_VOLSLIDE => {
                let prt_n_ops = self.module.state.channels[ch_idx].prt_n_ops;
                let delta = self.do_tone_portamento(ch_idx, prt_n_ops);
                self.do_volume_slide_channel(ch_idx, opnd);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = (chn.new_tonh as i16).wrapping_add(delta) as u16;
                chn.event_chg |= event_flags::E_PITCH | event_flags::E_VOL;
            }
            effect_ids::VIBRA_VOLSLID => {
                let v_ops = self.module.state.channels[ch_idx].v_ops;
                let base_ton = self.module.state.channels[ch_idx].new_tonh;
                let new_ton = self.do_vibrato(ch_idx, v_ops, base_ton, false);
                self.do_volume_slide_channel(ch_idx, opnd);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = new_ton;
                chn.event_chg |= event_flags::E_PITCH | event_flags::E_VOL;
            }
            effect_ids::VOLUME_SLIDE => {
                self.do_volume_slide_channel(ch_idx, opnd);
                self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
            }
            effect_ids::GLOB_VOL_SLD => {
                let fast_slides = self.module.state.fast_slides;
                let mut gv = self.module.state.glob_vol;
                Self::apply_volume_slide_value(curr_vblanc, fast_slides, file_type, opnd, &mut gv);
                self.module.state.glob_vol = gv;
                for i in 0..num_tracks {
                    self.module.state.channels[i].event_chg |= event_flags::E_VOL;
                }
            }
            effect_ids::PAN_SLD => {
                self.do_panning_slide(ch_idx, opnd);
                self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
            }
            effect_ids::TREMOLO => {
                self.do_tremolo(ch_idx, opnd);
                self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
            }
            effect_ids::OKT_SLD_UP => {
                let new_ton = self.do_okt_sld_up(ch_idx, opnd);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = new_ton;
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::OKT_SLD_DWN => {
                let new_ton = self.do_okt_sld_dwn(ch_idx, opnd);
                let chn = &mut self.module.state.channels[ch_idx];
                chn.new_tonh = new_ton;
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::FAR_PORTAMENTO => {
                let chn = &mut self.module.state.channels[ch_idx];
                let div = ((opnd.max(1)) as i32) * speed;
                let fp_vb = chn.fp_vblanc as i32;
                let fp_tonh = chn.fp_tonh as i32;
                let tpton = chn.tpton as i32;
                if fp_tonh > (chn.tonh as i32) {
                    let next_ton = fp_tonh - (fp_vb * (fp_tonh - tpton)) / div;
                    if next_ton < tpton {
                        chn.effkt = effect_ids::NO_EFFEKT;
                        chn.tonh = chn.tpton;
                    } else {
                        chn.tonh = next_ton.clamp(0, 65535) as u16;
                    }
                } else {
                    let next_ton = fp_tonh + (fp_vb * (tpton - fp_tonh)) / div;
                    if next_ton > tpton {
                        chn.effkt = effect_ids::NO_EFFEKT;
                        chn.tonh = chn.tpton;
                    } else {
                        chn.tonh = next_ton.clamp(0, 65535) as u16;
                    }
                }
                chn.fp_vblanc = chn.fp_vblanc.wrapping_add(1);
                chn.new_tonh = chn.tonh;
                chn.event_chg |= event_flags::E_PITCH;
            }
            effect_ids::VOLUME_PORT => {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.vol == chn.vol_prt {
                    chn.effkt = effect_ids::NO_EFFEKT;
                } else {
                    let div = ((opnd.max(1)) as u16) * (speed as u16);
                    let vp_vb = chn.vp_vblanc as u16;
                    if chn.vol_prt > chn.volsld {
                        let diff = (chn.vol_prt - chn.volsld) as u16;
                        chn.vol = chn.volsld.saturating_add(((vp_vb * diff) / div) as u8).min(64);
                    } else {
                        let diff = (chn.volsld - chn.vol_prt) as u16;
                        chn.vol = chn.volsld.saturating_sub(((vp_vb * diff) / div) as u8);
                    }
                    chn.event_chg |= event_flags::E_VOL;
                    chn.vp_vblanc = chn.vp_vblanc.wrapping_add(1);
                }
            }
            effect_ids::VIBRATO_SUST | effect_ids::FAR_VIBRATO => {
                let far_vib_depth = self.module.state.far_vib_depth as i32;
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.tpton > 0 && far_vib_depth != 0 {
                    let tpton = chn.tpton as i32;
                    let sin_val = SIN_TABLE[chn.vtini as usize] as i32;
                    let delta = (tpton * sin_val * far_vib_depth) >> 16;
                    chn.tonh = (tpton - delta).clamp(0, 65535) as u16;
                    chn.new_tonh = chn.tonh;
                    chn.event_chg |= event_flags::E_PITCH;
                    chn.vtini = chn.vtini.wrapping_add(12u8.wrapping_mul(opnd));
                }
            }
            _ => {}
        }
    }

    // ========================================================================
    // 5. Tick 0 (process_row) & Tick N (process_tick) Sequencer-Logik
    // ========================================================================

    /// Entpackt das Pattern für Order-Index `ord_idx` mit `n_rows` Events pro Spur in
    /// `self.active_pattern` exakt wie `TrackWrite` (`loaders.pas` Zeilen 90–162) und
    /// `GetPattern(N, NRows)` (`legacy_src/PLAYER/play.inc` Zeilen 470–608 mit `{$DEFINE CompressTracks}`).
    pub fn get_pattern(&mut self, ord_idx: usize, n_rows: usize) {
        if n_rows == 0 {
            return;
        }
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        let def_ch_vol = if self.module.state.mi.file_type == ModType::FT2 {
            0
        } else {
            DEF_CH_VOL
        };
        let fill_rows = n_rows.min(256);
        let def_note = Note {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::NO_EFFEKT,
            operands: 0,
            chn_vol: def_ch_vol,
        };
        for ch in 0..num_tracks {
            for r in 0..fill_rows {
                self.active_pattern[ch][r] = def_note;
            }
        }

        if ord_idx >= MAX_PATTERNS {
            return;
        }
        let pat_num = self.module.state.p_arrangement[ord_idx] as usize;
        let Some(pat_rows) = self.module.patterns.get(pat_num) else {
            return;
        };
        let max_rows = pat_rows.len().min(256);
        if max_rows == 0 {
            return;
        }

        for ch in 0..num_tracks {
            // 1. TrackWrite-Kompression für Spur `ch` (loaders.pas Zeilen 90–160)
            let mut packed: Vec<(u8, u8, Note)> = Vec::with_capacity(max_rows);
            let mut empty_rows: u8 = 0;

            for (twj, row) in pat_rows.iter().take(max_rows).enumerate() {
                let src = row[ch];
                let mut flgs: u8 = 0;
                if src.instrument != 0 {
                    flgs |= 0x01;
                }
                if src.ton != 0 {
                    flgs |= 0x02;
                }
                if src.effekt != effect_ids::NO_EFFEKT {
                    flgs |= 0x04;
                    if src.operands != 0 {
                        flgs |= 0x08;
                    }
                }
                if src.chn_vol != def_ch_vol {
                    flgs |= 0x10;
                }

                if flgs == 0 && empty_rows < 7 && twj != 0 {
                    empty_rows += 1;
                } else {
                    if let Some(last) = packed.last_mut() {
                        last.1 = empty_rows;
                    }
                    packed.push((flgs, 0, src));
                    empty_rows = 0;
                }
            }

            // 2. @trackmove Entpack-Schleife (play.inc Zeilen 548–600):
            // Entpackt maximal `n_rows` gepackte Events und überschreibt nur die per Flag-Bit
            // markierten Felder in `self.active_pattern[ch][dst_row]`.
            let mut rows_left = n_rows;
            let mut dst_row = 0usize;
            for (flgs, empty_after, src) in packed {
                if dst_row < 256 {
                    let dst = &mut self.active_pattern[ch][dst_row];
                    if (flgs & 0x01) != 0 {
                        dst.instrument = src.instrument;
                    }
                    if (flgs & 0x02) != 0 {
                        dst.ton = src.ton;
                    }
                    if (flgs & 0x04) != 0 {
                        dst.effekt = src.effekt;
                    }
                    if (flgs & 0x08) != 0 {
                        dst.operands = src.operands;
                    }
                    if (flgs & 0x10) != 0 {
                        dst.chn_vol = src.chn_vol;
                    }
                }
                dst_row += 1 + (empty_after as usize);
                rows_left -= 1;
                if rows_left == 0 {
                    break;
                }
            }
        }
    }

    /// Holt die aktuelle [`Note`] aus dem entpackten Laufzeit-Pattern-Puffer `active_pattern`
    /// für `curr_note` (Zeilen-Index) auf Kanal `ch_idx` (0-basiert).
    #[inline]
    pub fn get_current_pattern_note(&self, ch_idx: usize) -> Note {
        let row_idx = (self.module.state.curr_note as usize).min(255);
        self.active_pattern[ch_idx][row_idx]
    }

    /// Verarbeitet eine neue Notenzeile auf Kanal `ch_idx` bei Tick 0:
    /// Noten-Trigger, KeyOff (`0xFFFF`), Instrument-Wechsel (`nxt_inst`), Standard-Lautstärke,
    /// Lautstärkespalte (`chn_vol`), Perioden-Lookup (`instr_adjust`) und Effekt-Übernahme.
    ///
    /// ### Entsprechende Pascal-Prozeduren
    /// `ProcessNewNote(Chn)` und `ProcessFirstTick(Chn)` in `legacy_src/PLAYER/play.inc` (Zeilen 758–1253).
    pub fn process_new_note(&mut self, ch_idx: usize) {
        let mut note = self.get_current_pattern_note(ch_idx);
        let file_type = self.module.state.mi.file_type;

        // Wenn im vorherigen Schritt Arpeggio aktiv war und jetzt ein anderer Effekt kommt,
        // Basis-Periode TonH wiederherstellen (play.inc Zeilen 762–766)
        {
            let chn = &mut self.module.state.channels[ch_idx];
            if matches!(
                chn.effkt,
                effect_ids::ARPEGGIO | effect_ids::OKT_ARPG1 | effect_ids::OKT_ARPG2
            ) && note.effekt != chn.effkt
            {
                chn.new_tonh = chn.tonh;
                chn.event_chg |= event_flags::E_PITCH;
            }
        }

        // Format-spezifische Vorbehandlung inkl. KeyOff ($FFFF) bei FT2, STM, S3M (play.inc Zeilen 767–798)
        match file_type {
            ModType::FFar => {
                let chn = &self.module.state.channels[ch_idx];
                if chn.effkt == effect_ids::VIBRATO_SUST && note.effekt == effect_ids::NO_EFFEKT {
                    note.effekt = chn.effkt;
                    note.operands = chn.opnd;
                }
            }
            ModType::F669 | ModType::UNIS => {
                let chn = &self.module.state.channels[ch_idx];
                if note.ton == 0
                    || (note.instrument == 0
                        && matches!(
                            chn.effkt,
                            effect_ids::PORTAMENTO_UP
                                | effect_ids::PORTAMENTO_DOWN
                                | effect_ids::TONE_PORTAMENTO
                                | effect_ids::VIBRATO
                                | effect_ids::FREQU_ADJUST
                        ))
                {
                    match note.effekt {
                        effect_ids::NO_EFFEKT => {
                            note.effekt = chn.effkt;
                            note.operands = chn.opnd;
                        }
                        effect_ids::PORTAMENTO_UP
                        | effect_ids::PORTAMENTO_DOWN
                        | effect_ids::TONE_PORTAMENTO
                        | effect_ids::VIBRATO => {
                            if note.operands == 0 {
                                note.effekt = effect_ids::NO_EFFEKT;
                            }
                        }
                        _ => {}
                    }
                }
            }
            ModType::FT2 | ModType::STM | ModType::S3M => {
                if note.ton == 0xFFFF {
                    note.ton = 0;
                    let chn = &mut self.module.state.channels[ch_idx];
                    if file_type == ModType::FT2 && chn.env_vol_pos != 0 {
                        chn.do_fade_out = true;
                        note.instrument = 0;
                    } else {
                        chn.note = 0;
                        chn.event_chg |= event_flags::E_RESET;
                    }
                }
            }
            _ => {}
        }

        // Tick-0 Effekt-Vorbereitung (Platzhalter für den nächsten Turn)
        self.process_tick0_effects(ch_idx, &mut note);

        // Falls NewFineTune oder NewTonH vom letzten Effekt verändert wurden, zurücksetzen (play.inc Zeilen 978–985)
        {
            let use_linear_tune = self.module.state.use_linear_tune;
            let chn = &mut self.module.state.channels[ch_idx];
            if chn.new_fine_tune != 0 || chn.new_tonh != chn.tonh {
                if chn.new_fine_tune != 0 && use_linear_tune {
                    let nf = (chn.new_fine_tune as i8) as i16;
                    let ift = (chn.i_fin_tun as i8) as i16;
                    chn.tonh = (chn.tonh as i16).wrapping_sub(nf).wrapping_add(ift) as u16;
                }
                chn.new_tonh = chn.tonh;
                chn.new_fine_tune = 0;
                chn.event_chg |= event_flags::E_PITCH;
            }
        }

        // Neues Instrument vorwählen und Standard-Lautstärke setzen (play.inc Zeilen 987–1000)
        if note.instrument > 0 {
            self.module.state.channels[ch_idx].nxt_inst = note.instrument;
            if file_type != ModType::FT2 {
                let inst_vol = self.module.state.mi.instruments[(note.instrument - 1) as usize].volume;
                self.module.state.channels[ch_idx].vol = inst_vol;
            } else {
                let smp_num = (self.module.state.channels[ch_idx].smp_num as usize)
                    .min(self.module.ft2_samples.len().saturating_sub(1));
                let smp_hdr = self.module.ft2_samples[smp_num].hdr;
                self.module.state.channels[ch_idx].pan_val = smp_hdr.panning;
                self.module.state.channels[ch_idx].vol = smp_hdr.volume;
                self.init_envelope(ch_idx, note.instrument);
            }
            self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
        }

        // Lautstärkespalte (ChnVol) setzen & FT2 VolColumn Memory (play.inc Zeilen 1002–1060)
        if file_type != ModType::FT2 {
            if note.chn_vol != DEF_CH_VOL {
                let chn = &mut self.module.state.channels[ch_idx];
                chn.vol = note.chn_vol;
                chn.event_chg |= event_flags::E_VOL;
            }
        } else {
            self.process_ft2_vol_column_memory(ch_idx, &mut note);
        }

        // Effekt und Operanden in den Kanal übernehmen (play.inc Zeilen 1054–1056)
        {
            let chn = &mut self.module.state.channels[ch_idx];
            chn.effkt = note.effekt;
            chn.opnd = note.operands;
            chn.old_chn_vol = note.chn_vol;
        }

        // Neuen Ton anspielen & Periode ermitteln (play.inc Zeilen 1058–1070)
        if note.ton > 0 {
            self.instr_adjust(ch_idx, note.ton, note.instrument);
            if file_type == ModType::FT2
                && note.effekt == effect_ids::PLAY_OFFSET
                && self.module.state.channels[ch_idx].iend > 0
            {
                let chn = &mut self.module.state.channels[ch_idx];
                if chn.opnd == 0 {
                    chn.opnd = chn.ofs_ops;
                } else {
                    chn.ofs_ops = chn.opnd;
                }
                chn.mod_event.offset = (chn.opnd as i32) << 8;
                self.do_play_offset(ch_idx);
            }
            let chn = &mut self.module.state.channels[ch_idx];
            chn.new_tonh = chn.tonh;
            chn.event_chg |= event_flags::E_PITCH;
        }

        // Sample Offset (9xx) für Nicht-FT2 Formate (play.inc Zeilen 1073–1078)
        if file_type != ModType::FT2
            && self.module.state.channels[ch_idx].effkt == effect_ids::PLAY_OFFSET
            && self.module.state.channels[ch_idx].iend > 0
        {
            let chn = &mut self.module.state.channels[ch_idx];
            if chn.opnd == 0 {
                chn.opnd = chn.ofs_ops;
            } else {
                chn.ofs_ops = chn.opnd;
            }
            chn.mod_event.offset = (chn.opnd as i32) << 8;
            self.do_play_offset(ch_idx);
        }

        // TonePortamento & PortVolSlide Zielperiode TpTon setzen (play.inc Zeilen 1079–1087)
        if matches!(
            note.effekt,
            effect_ids::TONE_PORTAMENTO | effect_ids::PORT_VOLSLIDE
        ) {
            if note.ton != 0 {
                let use_linear_tune = self.module.state.use_linear_tune;
                let smp_num = self.module.state.channels[ch_idx].smp_num as usize;
                let i_tune = self.module.state.channels[ch_idx].i_tune;
                let i_fin_tun = self.module.state.channels[ch_idx].i_fin_tun as i8;
                let adj_ton = if use_linear_tune {
                    let rel_note = self
                        .module
                        .ft2_samples
                        .get(smp_num)
                        .map(|s| s.hdr.relnote as i32)
                        .unwrap_or(0);
                    ((note.ton as i32) + rel_note).max(0) as u16
                } else {
                    note.ton
                };
                self.module.state.channels[ch_idx].tpton =
                    self.get_tonh(adj_ton, i_tune, i_fin_tun);
            }
            if !matches!(
                self.module.state.channels[ch_idx].effkt,
                effect_ids::TONE_PORTAMENTO | effect_ids::PORT_VOLSLIDE
            ) {
                self.module.state.channels[ch_idx].tp_gl_ton =
                    self.module.state.channels[ch_idx].tonh;
            }
        }

        // FT2 VolColumn SetVolume ($10..$50), SetPanning ($C0..$CF) & TonePortamento ($F0..$FF) (play.inc Zeilen 1089–1104)
        if file_type == ModType::FT2 {
            match note.chn_vol {
                0x10..=0x50 => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.vol = note.chn_vol - 0x10;
                    chn.event_chg |= event_flags::E_VOL;
                }
                0xC0..=0xCF => {
                    let chn = &mut self.module.state.channels[ch_idx];
                    chn.pan_val = (note.chn_vol & 0x0F) * 17;
                    chn.event_chg |= event_flags::E_VOL;
                }
                0xF0..=0xFF => {
                    if !matches!(
                        note.effekt,
                        effect_ids::TONE_PORTAMENTO
                            | effect_ids::PORT_VOLSLIDE
                            | effect_ids::PORTAMENTO_UP
                            | effect_ids::PORTAMENTO_DOWN
                    ) {
                        if note.ton != 0 {
                            let use_linear_tune = self.module.state.use_linear_tune;
                            let smp_num = self.module.state.channels[ch_idx].smp_num as usize;
                            let i_tune = self.module.state.channels[ch_idx].i_tune;
                            let i_fin_tun = self.module.state.channels[ch_idx].i_fin_tun as i8;
                            let adj_ton = if use_linear_tune {
                                let rel_note = self
                                    .module
                                    .ft2_samples
                                    .get(smp_num)
                                    .map(|s| s.hdr.relnote as i32)
                                    .unwrap_or(0);
                                ((note.ton as i32) + rel_note).max(0) as u16
                            } else {
                                note.ton
                            };
                            self.module.state.channels[ch_idx].tpton =
                                self.get_tonh(adj_ton, i_tune, i_fin_tun);
                        }
                        if !matches!(
                            self.module.state.channels[ch_idx].effkt,
                            effect_ids::TONE_PORTAMENTO | effect_ids::PORT_VOLSLIDE
                        ) {
                            self.module.state.channels[ch_idx].tp_gl_ton =
                                self.module.state.channels[ch_idx].tonh;
                        }
                    }
                }
                _ => {}
            }
        }

        // In-place Mutation von Pattern^[Chn][CurrNote] zurückschreiben (play.inc Zeile 760:
        // WITH Pattern^[Chn][CurrNote], Channels[Chn] DO)
        let row_idx = (self.module.state.curr_note as usize).min(255);
        self.active_pattern[ch_idx][row_idx] = note;
    }

    /// Verarbeitet den Beginn einer Pattern-Zeile (Tick 0) über alle Kanäle
    /// (`NewPattern`-Block in `Play_the_Module`, `legacy_src/PLAYER/play.inc` Zeilen 1380–1396).
    pub fn process_row(&mut self) {
        // S3M Marker ($FE = Skip Order, $FF = End of Song) überspringen, falls direkt am Pattern-Anfang
        if self.module.state.curr_note == 0 {
            self.skip_s3m_order_markers();
        }

        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        if !self.module.state.delay_pattern {
            for ch_idx in 0..num_tracks {
                let pat_note = self.get_current_pattern_note(ch_idx);
                if pat_note.effekt != effect_ids::NOTE_DELAY
                    || (pat_note.ton == 0 && pat_note.instrument == 0)
                {
                    self.process_new_note(ch_idx);
                } else {
                    self.module.state.channels[ch_idx].effkt = effect_ids::NO_EFFEKT;
                }
            }
        } else {
            self.module.state.patterndelay = self.module.state.patterndelay.saturating_sub(1);
            if self.module.state.patterndelay == 0 {
                self.module.state.delay_pattern = false;
            }
        }

        // interpret all effects that are executed only on the first tick of each line (play.inc Zeile 1395)
        for ch_idx in 0..num_tracks {
            self.process_first_tick(ch_idx);
        }

        self.module.state.curr_vblanc = 0;

        // UI-Spiegelregister aktualisieren
        let state = &mut self.module.state;
        state.mi.curr_pttrn = state.curr_pattern + 1;
        state.mi.curr_line = state.curr_note + 1;
        state.mi.curr_spd = state.speed;
        state.mi.curr_bpm = state.bpm_speed as u8;
    }

    /// Aktualisiert vor dem Mischen eines Ticks die Vor-Kernel-Tick-Effekte (`Arpeggio`, `Tremor`,
    /// `Retrig`, `NoteCut`, `NoteDelay`), die FT2-Hüllkurven, das Auto-Vibrato,
    /// die Kanal-Schrittweiten (`do_phi_plo`), die Kanal-Lautstärken (`vol_adjust`) sowie
    /// das ProLogic-Surround-Routing (`do_surround`) für alle Kanäle.
    ///
    /// Entspricht der Vor-Kernel-Schleife in `Play_the_Module` (`legacy_src/PLAYER/play.inc` Zeilen 1344–1467).
    pub fn update_channels_for_mixing(&mut self) {
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

        // GlobalFadeOut am Tick-Anfang (play.inc Zeilen 1192–1205)
        if self.module.state.global_fade_out {
            self.module.state.global_fade_out_time += 1;
            if self.module.state.global_fade_out_time % 8 == 0 {
                if self.module.state.glob_vol != 0 {
                    for ch_idx in 0..num_tracks {
                        self.module.state.channels[ch_idx].event_chg |= event_flags::E_VOL;
                    }
                    self.module.state.glob_vol -= 1;
                } else {
                    self.module.state.player_repeats = true;
                    self.module.state.global_fade_out = false;
                }
            }
        }

        for ch_idx in 0..num_tracks {
            self.process_pre_kernel_tick_effects(ch_idx);
        }

        let file_type = self.module.state.mi.file_type;
        let is_ft2 = file_type == ModType::FT2;
        let is_stm_or_s3m = matches!(file_type, ModType::STM | ModType::S3M);
        let use_linear_tune = self.module.state.use_linear_tune;
        let use_surround = self.module.state.use_surround;
        let clean_mode = self.module.state.sound_settings.clean_mode();

        for ch_idx in 0..num_tracks {
            // 1. FT2 Envelopes, Auto-Vibrato & Fadeout (play.inc Zeilen 1415–1436)
            if is_ft2 {
                let inst_idx = (self.module.state.channels[ch_idx].inst as usize)
                    .min(self.module.ft2_instruments.len().saturating_sub(1));
                if inst_idx > 0 {
                    let ft2_inst = self.module.ft2_instruments[inst_idx];
                    // In TFT2Instrument (playdefs.pas Zeilen 569–610) liegen unmittelbar vor
                    // VolEnvPts[1..12] die Bytes SampleNos[93..96], und unmittelbar vor
                    // PanEnvPts[1..12] liegt VolEnvPts[12]. Bei CurrPos=0 und SustainPos=0
                    // liest Pascal EnvPoints[0].Tick aus diesem Speicherwort (im Clean-Mode bereinigt).
                    let vol_pt0 = if clean_mode {
                        ft2_inst.ext.volenvpts[0]
                    } else {
                        EnvPoint {
                            tick: u16::from_le_bytes([
                                ft2_inst.ext.samplenos[92],
                                ft2_inst.ext.samplenos[93],
                            ]),
                            value: u16::from_le_bytes([
                                ft2_inst.ext.samplenos[94],
                                ft2_inst.ext.samplenos[95],
                            ]),
                        }
                    };
                    let pan_pt0 = if clean_mode {
                        ft2_inst.ext.panenvpts[0]
                    } else {
                        ft2_inst.ext.volenvpts[11]
                    };
                    {
                        let state = &mut self.module.state;
                        let chn = &mut state.channels[ch_idx];
                        let mut env_vol = chn.env_vol;
                        let mut env_vol_tck = chn.env_vol_tck;
                        let mut env_vol_pos = chn.env_vol_pos;
                        let do_fade_out = chn.do_fade_out;
                        let vol_env_pts = ft2_inst.ext.volenvpts;
                        if Self::process_envelope_with_pt0(
                            &mut env_vol,
                            &mut env_vol_tck,
                            &mut env_vol_pos,
                            ft2_inst.ext.numvolpts,
                            ft2_inst.ext.volsustain,
                            ft2_inst.ext.volloopst,
                            ft2_inst.ext.volloopend,
                            do_fade_out,
                            &vol_env_pts,
                            vol_pt0,
                            &mut state.env_stack_val,
                            clean_mode,
                        ) {
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        chn.env_vol = env_vol;
                        chn.env_vol_tck = env_vol_tck;
                        chn.env_vol_pos = env_vol_pos;

                        let mut env_pan = chn.env_pan;
                        let mut env_pan_tck = chn.env_pan_tck;
                        let mut env_pan_pos = chn.env_pan_pos;
                        let pan_env_pts = ft2_inst.ext.panenvpts;
                        if Self::process_envelope_with_pt0(
                            &mut env_pan,
                            &mut env_pan_tck,
                            &mut env_pan_pos,
                            ft2_inst.ext.numpanpts,
                            ft2_inst.ext.pansustain,
                            ft2_inst.ext.panloopst,
                            ft2_inst.ext.panloopend,
                            do_fade_out,
                            &pan_env_pts,
                            pan_pt0,
                            &mut state.env_stack_val,
                            clean_mode,
                        ) {
                            chn.event_chg |= event_flags::E_VOL;
                        }
                        chn.env_pan = env_pan;
                        chn.env_pan_tck = env_pan_tck;
                        chn.env_pan_pos = env_pan_pos;
                    }

                    self.process_auto_vibrato(ch_idx);

                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.do_fade_out && ft2_inst.ext.volfadeout != 0 {
                        chn.fade_vol -= 2 * (ft2_inst.ext.volfadeout as i32);
                        if chn.fade_vol < 0 {
                            chn.do_fade_out = false;
                            chn.fade_vol = 0;
                        }
                        chn.event_chg |= event_flags::E_VOL;
                    }
                }
            }

            // 2. Pitch-Änderungen (EPitch) verarbeiten (play.inc Zeilen 1438–1454)
            let e_pitch = (self.module.state.channels[ch_idx].event_chg & event_flags::E_PITCH) != 0;
            if e_pitch {
                if is_stm_or_s3m {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if chn.new_tonh <= GEN_LOLIMIT {
                        chn.new_tonh = 0;
                    } else if chn.new_tonh < 128 {
                        chn.new_tonh = 128;
                    }
                }

                let new_tonh = self.module.state.channels[ch_idx].new_tonh;
                let i_tune = self.module.state.channels[ch_idx].i_tune;
                let i_fin_tun = self.module.state.channels[ch_idx].i_fin_tun;
                let new_fine_tune = self.module.state.channels[ch_idx].new_fine_tune;
                let smp_num = self.module.state.channels[ch_idx].smp_num as usize;

                let mut note_val = self.note_to_val(new_tonh, i_tune, i_fin_tun);
                if use_linear_tune {
                    let rel_note = self
                        .module
                        .ft2_samples
                        .get(smp_num)
                        .map(|s| s.hdr.relnote)
                        .unwrap_or(0);
                    note_val = (note_val as i16 - rel_note as i16).clamp(0, 255) as u8;
                }
                self.module.state.channels[ch_idx].note_val = note_val;

                let ft_to_use = if new_fine_tune == 0 {
                    i_fin_tun as i8
                } else {
                    new_fine_tune as i8
                };
                self.do_phi_plo(ch_idx, new_tonh, ft_to_use);
            }

            // 3. Lautstärke- und Panning-Änderungen (EVol) verarbeiten (play.inc Zeile 1456)
            if (self.module.state.channels[ch_idx].event_chg & event_flags::E_VOL) != 0 {
                self.vol_adjust(ch_idx);
            }

            // 4. ProLogic-Surround-Puffer-Zuweisung bei Instrumenten-Trigger (play.inc Zeilen 1458–1466)
            let e_inst = (self.module.state.channels[ch_idx].event_chg & event_flags::E_INST) != 0;
            if use_surround && e_inst {
                let inst = self.module.state.channels[ch_idx].inst;
                let pan_val = self.module.state.channels[ch_idx].pan_val;
                let new_tonh = self.module.state.channels[ch_idx].new_tonh;

                let mut target_buf = MIX_BUFFER_OFS as i32;
                if matches!(pan_val, 0..=15 | 240..=255)
                    && self.do_surround(inst, pan_val, new_tonh)
                {
                    target_buf = SUR_BUFFER_OFS as i32;
                }
                if clean_mode && target_buf == SUR_BUFFER_OFS as i32 {
                    // Im Clean-Mode auch verzögerte Stereo-Paare (z. B. mit Note Delay EDx auf einem der beiden Kanäle)
                    // erkennen: Wenn ein anderer Kanal bereits dasselbe Instrument auf derselben Tonhöhe mit
                    // entgegengesetztem Panning spielt (oder in dieser Zeile per Note Delay noch triggern wird),
                    // dürfen beide Kanäle nicht in den ProLogic-Surround-Delay-Puffer geroutet werden (verhindert
                    // destruktive Phasenauslöschung / Kammfilter-Pumpen wie in PAHGIANA.XM auf Kanal 5/6 und 14/15).
                    for k in 0..num_tracks {
                        if k == ch_idx {
                            continue;
                        }
                        let other = &self.module.state.channels[k];
                        let other_tonh = if (other.event_chg & event_flags::E_PITCH) != 0 {
                            other.new_tonh
                        } else {
                            other.tonh
                        };
                        if other.note > 0
                            && other.avg_vol > 0
                            && other.inst == inst
                            && other_tonh == new_tonh
                            && ((other.pan_val as i16) - (pan_val as i16)).abs() > 15
                        {
                            target_buf = MIX_BUFFER_OFS as i32;
                            self.module.state.channels[k].mix_buf = MIX_BUFFER_OFS as i32;
                        }
                    }
                }
                self.module.state.channels[ch_idx].mix_buf = target_buf;
            }

            // UI-Statusarrays aktualisieren
            let chn = &self.module.state.channels[ch_idx];
            self.module.state.mi.curr_effects[ch_idx] =
                ((chn.effkt as u16) << 8) | (chn.opnd as u16);
            if chn.note > 0 && chn.avg_vol != 0 {
                self.module.state.mi.curr_instr[ch_idx] = chn.inst;
                self.module.state.mi.curr_vols[ch_idx] =
                    (chn.l_vol >> 3).wrapping_add((chn.r_vol << 1) & 0xF0);
                if chn.note_val != 0 {
                    self.module.state.mi.curr_notes[ch_idx] = chn.note_val;
                }
            } else {
                self.module.state.mi.curr_vols[ch_idx] = 0;
            }
        }
    }

    /// Schaltet nach dem Mischen eines Ticks den Tick-Zähler `curr_vblanc` weiter und führt
    /// bei Erreichen von `speed` die Zeilen-/Order-Weiterschaltung aus.
    ///
    /// Entspricht dem Nach-Kernel-Block in `Play_the_Module` (`legacy_src/PLAYER/play.inc` Zeilen 1525–1910).
    pub fn advance_tick(&mut self) {
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

        // In DEV_HDD (BufferSize = 32768 Bytes = 8192 Stereo-Frames) kehrt Play_the_Module
        // bei jedem Pufferüberlauf (> 8192 Frames) zu PlayOne / ProcessUpdate (WriteWaveFile)
        // zurück, wodurch der Stack-Slot [S - 32] (env_stack_val) auf 0 zurückgesetzt wird.
        let tick_frames = (self.module.state.num_samples.max(1)) as u32;
        self.module.state.hdd_buf_frames += tick_frames;
        if self.module.state.hdd_buf_frames > 8192 {
            self.module.state.hdd_buf_frames -= 8192;
            self.module.state.env_stack_val = 0;
        }

        // EventChg nach erfolgreichem Mix-Block leeren (play.inc Zeile 1511)
        for ch_idx in 0..num_tracks {
            self.module.state.channels[ch_idx].event_chg = 0;
            self.module.state.channels[ch_idx].new_tonh = self.module.state.channels[ch_idx].tonh;
        }

        // Laufende Tick-Effekte auswerten (Platzhalter für den nächsten Turn)
        for ch_idx in 0..num_tracks {
            self.process_tick_n_effects(ch_idx);
        }

        self.module.state.curr_vblanc = self.module.state.curr_vblanc.wrapping_add(1);
        self.inc_time();

        let speed = (self.module.state.speed.max(1)) as u16;
        if self.module.state.curr_vblanc >= speed {
            self.advance_row_and_order();
        }
    }

    /// Schaltet am Ende einer Zeile (`curr_vblanc >= speed`) `curr_note` (Row) und ggf.
    /// `curr_pattern` (Order) inklusive `PositionJump` (`Bxx`), `PatternBreak` (`Dxx`),
    /// `PatternLoop` (`E6x`) und Song-Loop-Reset weiter
    /// (`Play_the_Module` Zeilen 1806–1910 in `legacy_src/PLAYER/play.inc`).
    pub fn advance_row_and_order(&mut self) {
        let curr_pat = (self.module.state.curr_pattern as usize).min(MAX_PATTERNS - 1);
        let curr_row = self.module.state.curr_note as u8;
        bitset_include(&mut self.module.state.row_played[curr_pat], curr_row);

        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);

        if !self.module.state.delay_pattern {
            // 1. Prüfe PatternLoop (E6x) rückwärts über alle Kanäle (play.inc Zeilen 1809–1825)
            for ch_idx in (0..num_tracks).rev() {
                let pat_note = self.get_current_pattern_note(ch_idx);
                if pat_note.effekt == effect_ids::PATTRN_LOOP {
                    let chn = &mut self.module.state.channels[ch_idx];
                    if pat_note.operands == 0 {
                        chn.pattern_loopst = curr_row;
                    } else {
                        if chn.pattern_loops == 0 {
                            if chn.pattern_loopst <= curr_row {
                                chn.pattern_loops = pat_note.operands.wrapping_add(1);
                            }
                        } else if chn.pattern_loops == 1 {
                            chn.pattern_loops = 0;
                            chn.pattern_loopst = curr_row.wrapping_add(1);
                        }
                        if chn.pattern_loops > 1 {
                            let loop_target = chn.pattern_loopst as u16;
                            chn.pattern_loops -= 1;
                            // play.inc Zeile 1691: GetPattern(CurrPattern, CurrNote);
                            self.get_pattern(curr_pat, curr_row as usize);
                            self.module.state.curr_note = loop_target;
                            self.module.state.curr_vblanc = 0;
                            return;
                        }
                    }
                }
            }

            // 2. Prüfe PositionJump (Bxx) und PatternBreak (Dxx) (play.inc Zeilen 1826–1854)
            let mut pjump = false;
            let mut pbreak = false;
            let mut jumpto: u8 = 0;
            let mut breakto: u8 = self.module.state.break_to;
            let num_patterns = self.module.state.mi.num_patterns;
            let use_patt_brk_ops = self.module.state.use_patt_brk_ops;

            for ch_idx in 0..num_tracks {
                let chn = &self.module.state.channels[ch_idx];
                match chn.effkt {
                    effect_ids::POSITION_JUMP => {
                        if (chn.opnd as u16) < num_patterns {
                            pjump = true;
                            jumpto = chn.opnd;
                        }
                    }
                    effect_ids::PATTERN_BREAK => {
                        pbreak = true;
                        if use_patt_brk_ops {
                            breakto = 10 * (chn.opnd >> 4) + (chn.opnd & 0x0F);
                        }
                    }
                    _ => {}
                }
            }

            if pjump && !self.only_one_pattern {
                self.module.state.curr_pattern = jumpto as u16;
                self.module.state.break_to = breakto;
                self.finish_pattern_transition();
                return;
            } else if pbreak {
                self.module.state.break_to = breakto;
                self.advance_to_next_pattern();
                return;
            } else {
                self.module.state.curr_note = self.module.state.curr_note.wrapping_add(1);
            }
        }

        // Prüfe, ob das Ende des aktuellen Patterns (`PtrnBreaks[CurrPattern]`) erreicht wurde
        let pat_break_row = self.module.state.ptrn_breaks[curr_pat] as u16;
        if self.module.state.curr_note > pat_break_row {
            self.advance_to_next_pattern();
        } else {
            self.module.state.curr_vblanc = 0;
        }
    }

    /// Schaltet zur nächsten Position in der Pattern-Order-Tabelle (`S3M_Inc` in `play.inc` Zeilen 1858–1909).
    fn advance_to_next_pattern(&mut self) {
        if self.only_one_pattern {
            let cp = (self.module.state.curr_pattern as usize).min(MAX_PATTERNS - 1);
            self.module.state.play_time = self.module.state.pattern_times[cp];
            self.module.state.row_played[cp] = [0u8; 32];
        } else {
            self.module.state.curr_pattern = self.module.state.curr_pattern.wrapping_add(1);
        }

        self.skip_s3m_order_markers();

        let file_type = self.module.state.mi.file_type;
        let loop_from = self.module.state.mi.loop_from;
        let loop_to = self.module.state.mi.loop_to;

        if matches!(
            file_type,
            ModType::FT2 | ModType::FFar | ModType::UNIS | ModType::F669
        ) && loop_from > 0
            && (self.module.state.curr_pattern as i16) == loop_from
        {
            let lt_pat = (loop_to.saturating_sub(1)).max(0) as u16;
            self.module.state.curr_pattern = lt_pat;
        }

        if self.module.state.curr_pattern >= self.module.state.mi.num_patterns {
            self.reset_song_on_end_wrap();
        }

        self.finish_pattern_transition();
    }

    /// Überspringt S3M-Order-Marker (`0xFE` = Marker überspringen, `0xFF` = Songende)
    /// (`play.inc` Zeilen 1370–1373).
    fn skip_s3m_order_markers(&mut self) {
        if self.module.state.mi.file_type != ModType::S3M {
            return;
        }
        let num_patterns = self.module.state.mi.num_patterns.max(1);
        let mut guard = 0usize;
        while self.module.state.curr_pattern < num_patterns && guard < MAX_PATTERNS {
            let ord = self.module.state.p_arrangement[self.module.state.curr_pattern as usize];
            match ord {
                0xFE => {
                    self.module.state.curr_pattern =
                        self.module.state.curr_pattern.wrapping_add(1);
                    guard += 1;
                }
                0xFF => {
                    self.module.state.curr_pattern = num_patterns;
                    break;
                }
                _ => break,
            }
        }
    }

    /// Setzt am Song-Ende (`CurrPattern >= MI.NumPatterns`) die Kanäle, das globale Volumen
    /// und die Tempi zurück (`play.inc` Zeilen 1865–1896).
    fn reset_song_on_end_wrap(&mut self) {
        self.module.state.curr_pattern = 0;
        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        let init_glob_vol = self.module.state.init_glob_vol;
        let def_panning = self.module.state.def_panning;

        self.module.state.glob_vol = init_glob_vol;
        self.module.state.play_time = 0;

        for i in 0..num_tracks {
            let chn = &mut self.module.state.channels[i];
            chn.effkt = effect_ids::NO_EFFEKT;
            chn.glissando_on = false;
            chn.pan_val = def_panning[i];
            chn.tpton = 0;
            chn.tp_gl_ton = 0;
            chn.volsld = 0;
            chn.vtini = 0;
            chn.acnt = 0;
            chn.v_ops = 0;
            chn.t_ops = 0;
            chn.prt_ops = 0;
            chn.prt_n_ops = 0;
            chn.ofs_ops = 0;
            chn.arp_ops = 0;
            chn.vsld_ops = 0;
            chn.f_vu_ops = 0;
            chn.f_vd_ops = 0;
            chn.p_del_ops = 0;
            chn.retr_ops = 0;
            chn.v_table = WAVE_SINE;
            chn.t_table = WAVE_SINE;
            chn.arp1_ops = 0;
            chn.arp2_ops = 0;
            chn.okt_su_ops = 0;
            chn.okt_su1_ops = 0;
            chn.okt_sd_ops = 0;
            chn.okt_sd1_ops = 0;
            chn.old_volume = 0;
            chn.t_vol = 0;
            chn.tremor_ops = 0;
            chn.vol_prt = 0;
            chn.vp_vblanc = 0;
            chn.fp_vblanc = 0;
            chn.fp_tonh = 0;
            chn.s_cont = 0;
            chn.old_chn_vol = 0;
            chn.gv_sld_ops = 0;
            chn.v_sld_u_ops = 0;
            chn.v_sld_d_ops = 0;
            chn.f_vsld_u_ops = 0;
            chn.f_vsld_d_ops = 0;
            chn.p_sld_ops = 0;
            chn.p_sld_l_ops = 0;
            chn.p_sld_r_ops = 0;
            chn.vol = 0;
            chn.event_chg = event_flags::E_VOL;
        }

        let init_bpm = self.module.state.init_bpm_speed.clamp(32, 255) as u8;
        let init_spd = self.module.state.init_speed;
        self.module.state.force_bpm = true;
        self.do_set_speed(init_bpm);
        self.module.state.force_bpm = false;
        self.do_set_speed(init_spd);
        if self.module.state.mi.file_type == ModType::FFar {
            self.do_fine_far_tempo(0);
        }
        self.module.state.far_vib_depth = 4;
    }

    /// Schließt den Übergang in ein neues Pattern ab (`EPJMP` und Pattern-Kopf in `play.inc`
    /// Zeilen 1374–1378 und 1897–1910).
    fn finish_pattern_transition(&mut self) {
        self.skip_s3m_order_markers();
        let cp = (self.module.state.curr_pattern as usize).min(MAX_PATTERNS - 1);
        let mut breakto = self.module.state.break_to;
        if self.module.state.ptrn_breaks[cp] < breakto {
            breakto = 0;
        }

        if !self.one_channel_loops()
            && bitset_contains(&self.module.state.row_played[cp], breakto)
        {
            self.module.state.player_repeats = true;
            if self.module.state.curr_pattern == 0 {
                self.module.state.play_time = 0;
            } else if (self.module.state.curr_pattern as i16)
                == self.module.state.mi.loop_to.saturating_sub(1)
            {
                self.module.state.play_time = self.module.state.loop_to_time;
                if self.module.state.sound_settings.device_id == DEV_HDD
                    && !self.mix_mem.peak_search
                {
                    self.module.state.global_fade_out = true;
                    self.module.state.global_fade_out_time = 0;
                    self.module.state.player_repeats = false;
                }
            } else {
                self.module.state.play_time = self.module.state.pattern_times[cp];
            }
            self.module.state.row_played = [[0u8; 32]; MAX_PATTERNS];
        }

        // play.inc Zeile 1167: GetPattern(CurrPattern, 256);
        self.get_pattern(cp, 256);
        self.module.state.curr_note = breakto as u16;
        self.module.state.break_to = 0;
        self.module.state.curr_vblanc = 0;

        let num_tracks = (self.module.state.mi.num_tracks as usize).min(MAX_M4W_CHANNELS);
        for ch_idx in 0..num_tracks {
            self.module.state.channels[ch_idx].pattern_loopst = 0;
        }

        let pat_tempo = self.module.state.ptrn_tempi[cp];
        if pat_tempo > 0 {
            self.do_set_speed(pat_tempo);
        }
    }

    // ========================================================================
    // 6. Hauptfunktion: fill_audio_buffer (Sequencer + Mixer Kopplung)
    // ========================================================================

    /// Führt für `.IT`-Module genau einen Sequencer- und Mixer-Tick über [`crate::engine::it_engine::ItEngine`]
    /// aus. Gibt `None` zurück, wenn kein `.IT`-Modul geladen ist (damit alle klassischen Formate
    /// ihren unveränderten Codepfad durchlaufen).
    pub fn render_it_tick(
        &mut self,
        muted_channels: Option<&[bool; 32]>,
    ) -> Option<MixOutputBuffer> {
        let (Some(it_mod), Some(it_eng)) = (&self.module.it_module, &mut self.it_engine) else {
            return None;
        };
        if let Some(mutes) = muted_channels {
            for (ch, &m) in mutes.iter().enumerate() {
                it_eng.host_channels[ch].muted = m;
            }
        }
        it_eng.process_tick(it_mod, &mut self.module.state);
        if self.module.state.player_repeats && !self.module.state.repeat_song {
            return Some(MixOutputBuffer::Pcm16(Vec::new()));
        }
        let mix_out = it_eng.mix_tick_block(it_mod, &mut self.module.state, &mut self.mix_mem);
        Some(mix_out)
    }

    /// Berechnet genau einen Sequencer-Tick (bei `curr_vblanc == 0` inklusive Zeilen-Auswertung
    /// `process_row`), aktualisiert die Kanal-Register, ruft `process_sample_block` aus `mixer.rs`
    /// für `num_samples` Frames auf und schaltet anschließend den Tick weiter.
    pub fn render_one_tick(&mut self) {
        if let Some(mix_out) = self.render_it_tick(None) {
            self.pending_pcm.clear();
            self.pending_read_idx = 0;
            match mix_out {
                MixOutputBuffer::Pcm16(samples) => {
                    self.pending_pcm = samples;
                }
                MixOutputBuffer::Pcm8(bytes) => {
                    self.pending_pcm = bytes
                        .into_iter()
                        .map(|b| ((b as i16) - 128) << 8)
                        .collect();
                }
                MixOutputBuffer::PeakSearchOnly => {}
            }
            return;
        }

        if !self.tick_prepared {
            if self.module.state.curr_vblanc == 0 {
                self.process_row();
            }
            self.update_channels_for_mixing();
            self.tick_prepared = true;
        }

        self.module.state.buf_samples = self.module.state.num_samples.max(1);
        let am_repeating = self.module.state.player_repeats && !self.module.state.repeat_song;

        let mix_out = process_sample_block(
            &mut self.module.state,
            &mut self.mix_mem,
            &self.module.sample_mem,
            am_repeating,
        );

        self.pending_pcm.clear();
        self.pending_read_idx = 0;
        match mix_out {
            MixOutputBuffer::Pcm16(samples) => {
                self.pending_pcm = samples;
            }
            MixOutputBuffer::Pcm8(bytes) => {
                self.pending_pcm = bytes
                    .into_iter()
                    .map(|b| ((b as i16) - 128) << 8)
                    .collect();
            }
            MixOutputBuffer::PeakSearchOnly => {}
        }

        self.tick_prepared = false;
        self.advance_tick();
    }

    /// Füllt den übergebenen 16-Bit-Audio-Ausgabepuffer `output` nahtlos durch abwechselnde
    /// Tick-Berechnung im Sequencer und Block-Synthese im Mixer (`process_sample_block`).
    ///
    /// Bei Stereo (`sound_settings.stereo == 2`) enthält `output` abwechselnd `[Left, Right, Left, Right, ...]`,
    /// bei Mono (`sound_settings.stereo == 1`) aufeinanderfolgende Mono-Samples.
    pub fn fill_audio_buffer(&mut self, output: &mut [i16]) {
        let mut out_pos = 0usize;
        let out_len = output.len();

        while out_pos < out_len {
            // 1. Falls noch Samples aus dem vorherigen Tick-Block übrig sind, zuerst kopieren
            let avail = self
                .pending_pcm
                .len()
                .saturating_sub(self.pending_read_idx);
            if avail > 0 {
                let to_copy = avail.min(out_len - out_pos);
                output[out_pos..out_pos + to_copy].copy_from_slice(
                    &self.pending_pcm[self.pending_read_idx..self.pending_read_idx + to_copy],
                );
                self.pending_read_idx += to_copy;
                out_pos += to_copy;
                continue;
            }

            // 2. Nächsten Sequencer-Tick ausführen und über `mixer.rs` mischen
            self.render_one_tick();

            // Sicherheits-Abbruch, falls PeakSearchOnly aktiv ist und keine Samples erzeugt werden
            if self.pending_pcm.is_empty() {
                output[out_pos..].fill(0);
                break;
            }
        }
    }

    /// Springt gezielt zur angegebenen 0-basierten Order-Position `target_order` (`0..mi.num_patterns`),
    /// indem die Sequencer-Zustandsmaschine (inklusive `calc_sample_positions`, Tempi, Envelopes,
    /// Kanal-Effektgedächtnis und laufender Instrumente) ab Songbeginn exakt bis zum Beginn von
    /// `target_order` im Schnellvorlauf berechnet wird (`PatternSearch` / `search_time` in
    /// `legacy_src/PLAYER/PLAYER32.PAS` Zeilen 719–907).
    pub fn seek_to_order(&mut self, target_order: u16) {
        self.seek_to_order_and_row(target_order, 0);
    }

    /// Springt gezielt zur angegebenen 0-basierten Order-Position `target_order` und Zeile `target_row`
    /// (`search_time` mit `SearchPattern = True`, `SearchP`, `SearchL` in `PLAYER32.PAS` Zeilen 719–1025,
    /// u. a. beim Umschalten von `NoBPM` während der Wiedergabe).
    pub fn seek_to_order_and_row(&mut self, target_order: u16, target_row: u16) {
        let max_order = self.module.state.mi.num_patterns.saturating_sub(1);
        let clamped_order = target_order.min(max_order);
        let repeat_song = self.module.state.repeat_song;

        self.kanal_init(false);
        self.module.state.repeat_song = repeat_song;

        if let (Some(it_mod), Some(it_eng)) = (&self.module.it_module, &mut self.it_engine) {
            it_eng.seek_to_order(it_mod, &mut self.module.state, clamped_order.saturating_add(1));
            self.module.state.row_played = [[0u8; 32]; MAX_PATTERNS];
            let reached = it_eng.curr_order.min(MAX_PATTERNS);
            for ord in 0..reached {
                bitset_include(&mut self.module.state.row_played[ord], 0);
            }
            self.module.state.player_repeats = false;
            self.module.state.l_fade_out = 0;
            self.module.state.r_fade_out = 0;
            self.module.state.l_sur_fade_out = 0;
            self.module.state.r_sur_fade_out = 0;
            self.mix_mem.mix_buffer.fill(Default::default());
            self.mix_mem.sur_buffer.fill(Default::default());
            self.mix_mem.surr_ring.fill(Default::default());
            self.pending_pcm.clear();
            self.pending_read_idx = 0;
            self.tick_prepared = false;
            return;
        }

        if clamped_order > 0 || target_row > 0 {
            let max_ticks: u32 = 500_000;
            let mut ticks_run: u32 = 0;

            while !(self.module.state.curr_pattern == clamped_order
                && self.module.state.curr_note >= target_row)
                && ticks_run < max_ticks
            {
                if self.module.state.curr_vblanc == 0 {
                    if self.module.state.curr_pattern == clamped_order
                        && self.module.state.curr_note >= target_row
                    {
                        break;
                    }
                    self.process_row();
                    if self.module.state.curr_pattern == clamped_order && target_row == 0 {
                        break;
                    }
                }
                self.update_channels_for_mixing();
                let num_tracks = self.module.state.mi.num_tracks;
                let num_samples = self.module.state.num_samples.max(1);
                calc_sample_positions(&mut self.module.state.channels, num_tracks, num_samples);
                self.advance_tick();
                ticks_run += 1;

                // Falls der Song durch einen früheren Bxx-Sprung loopt, bevor clamped_order erreicht wird:
                if self.module.state.player_repeats {
                    self.module.state.player_repeats = false;
                    self.module.state.curr_pattern = clamped_order;
                    self.module.state.curr_note = target_row;
                    self.module.state.curr_vblanc = 0;
                    self.module.state.break_to = 0;
                    self.finish_pattern_transition();
                    self.module.state.curr_note =
                        target_row.min(self.module.state.ptrn_breaks[clamped_order as usize] as u16);
                    break;
                }
            }
        }

        // Gespielte Zeilen-Historie für alle Orders VOR `clamped_order` beibehalten
        // (wie in `search_time` in `PLAYER32.PAS` Zeilen 752–807), damit ein späterer
        // Rücksprung (`Bxx` / Song-Ende-Wrap auf Order 0) zuverlässig als `player_repeats`
        // erkannt wird, und nur ab `(clamped_order, target_row)..MAX_PATTERNS` leeren:
        let reached = (clamped_order as usize).min(MAX_PATTERNS);
        for ord in 0..reached {
            bitset_include(&mut self.module.state.row_played[ord], 0);
        }
        if reached < MAX_PATTERNS {
            let mut row_mask = [0u8; 32];
            for r in 0..(target_row.min(255) as u8) {
                bitset_include(&mut row_mask, r);
            }
            self.module.state.row_played[reached] = row_mask;
        }
        for ord in (reached + 1)..MAX_PATTERNS {
            self.module.state.row_played[ord] = [0u8; 32];
        }
        self.module.state.player_repeats = false;
        self.module.state.l_fade_out = 0;
        self.module.state.r_fade_out = 0;
        self.module.state.l_sur_fade_out = 0;
        self.module.state.r_sur_fade_out = 0;
        self.mix_mem.mix_buffer.fill(Default::default());
        self.mix_mem.sur_buffer.fill(Default::default());
        self.mix_mem.surr_ring.fill(Default::default());
        self.mix_mem.reset_declick();
        self.pending_pcm.clear();
        self.pending_read_idx = 0;
        self.tick_prepared = false;

        // UI-Spiegelregister aktualisieren
        let state = &mut self.module.state;
        state.mi.curr_pttrn = state.curr_pattern + 1;
        state.mi.curr_line = state.curr_note + 1;
        state.mi.curr_spd = state.speed;
        state.mi.curr_bpm = state.bpm_speed as u8;
    }

    /// Springt zur 1-basierten UI-Pattern-/Song-Position `pattern_1based` (`1..=mi.num_patterns`).
    ///
    /// Beispiel: `seek_to_pattern(44)` springt zu Order `43` (im UI/CLI angezeigt als Pattern `44`).
    pub fn seek_to_pattern(&mut self, pattern_1based: u16) {
        let target_order = pattern_1based.saturating_sub(1);
        self.seek_to_order(target_order);
    }

    /// Springt relativ um `delta` Pattern-Positionen vor (`delta > 0`) oder zurück (`delta < 0`).
    pub fn step_order(&mut self, delta: i32) {
        let num_patterns = (self.module.state.mi.num_patterns.max(1)) as i32;
        let current = self.module.state.curr_pattern as i32;
        let mut target = (current + delta).clamp(0, num_patterns - 1) as u16;
        if delta < 0 {
            if let Some(ref it_mod) = self.module.it_module {
                while target > 0
                    && it_mod
                        .orders
                        .get(target as usize)
                        .copied()
                        .unwrap_or(0xFF)
                        >= 0xFE
                {
                    target -= 1;
                }
            } else if self.module.state.mi.file_type == ModType::S3M {
                while target > 0 && self.module.state.p_arrangement[target as usize] >= 0xFE {
                    target -= 1;
                }
            }
        }
        self.seek_to_order(target);
    }

    /// Führt den exakten `PeakSearch`-Vorlauf aus (`PLAYER32.PAS` Zeilen 586–650 und
    /// `play.inc` Zeilen 1396–1413 für `DeviceID = DEV_HDD`).
    ///
    /// Startet mit `pre_amp = 0x80 + 40` (`+20 dB`), mischt jeden Tick des Songs mit
    /// `mix_mem.peak_search = true`, dekrementiert bei `max_level > 32767` den `pre_amp`
    /// (inklusive `RestoreChannels` und `SetVolTable` + Wiederholung des Ticks) bis zum
    /// ersten `player_repeats`, und initialisiert den Sequencer anschließend über `kanal_init(false)`
    /// mit dem ermittelten `pre_amp` neu. Gibt den ermittelten `pre_amp`-Wert zurück.
    pub fn run_exact_peak_search(&mut self) -> u8 {
        if self.module.it_module.is_some() {
            let orig_settings = self.module.state.sound_settings;
            let orig_repeat_song = self.module.state.repeat_song;
            self.module.state.sound_settings.bits = 16;
            self.module.state.sound_settings.pre_amp = 0x80 + 26; // Start bei +6 dB für .IT
            self.module.state.repeat_song = false;
            self.kanal_init(false);
            self.mix_mem.peak_search = true;

            let max_ticks: u32 = 300_000;
            let mut ticks_run: u32 = 0;
            while !self.module.state.player_repeats && ticks_run < max_ticks {
                let Some(ref it_mod) = self.module.it_module else {
                    break;
                };
                let Some(ref mut it_eng) = self.it_engine else {
                    break;
                };
                it_eng.process_tick(it_mod, &mut self.module.state);
                if self.module.state.player_repeats {
                    break;
                }
                self.module.state.max_level = 0;
                let _ = it_eng.mix_tick_block(it_mod, &mut self.module.state, &mut self.mix_mem);
                while self.module.state.max_level > 32767
                    && (self.module.state.sound_settings.pre_amp & 0x7F) > 0
                {
                    self.module.state.sound_settings.pre_amp =
                        self.module.state.sound_settings.pre_amp.wrapping_sub(1);
                    self.module.state.max_level =
                        (((self.module.state.max_level as i64) * 913) >> 10) as i32; // ~ -1.00 dB
                }
                ticks_run += 1;
            }
            let found_pre_amp = self.module.state.sound_settings.pre_amp;
            self.mix_mem.peak_search = false;
            self.module.state.sound_settings = orig_settings;
            self.module.state.sound_settings.pre_amp = found_pre_amp;
            self.kanal_init(false);
            self.module.state.repeat_song = orig_repeat_song;
            return found_pre_amp;
        }

        let orig_settings = self.module.state.sound_settings;
        let orig_repeat_song = self.module.state.repeat_song;

        // PLAYER32.PAS Zeilen 586–599:
        // SoundSettings.PreAmp := $80 + 40; PeakSearch := True;
        // Wichtig: Während des PeakSearch-Vorlaufs immer mit 16-Bit-Skalierung (32767 Vollausschlag)
        // messen, damit auch im 8-Bit-Modus exakt derselbe dB-PreAmp-Wert wie bei 16-Bit ermittelt wird!
        self.module.state.sound_settings.bits = 16;
        self.module.state.sound_settings.pre_amp = 0x80 + 40;
        self.module.state.repeat_song = false;
        self.kanal_init(false);
        self.mix_mem.peak_search = true;

        let search_time = self.module.state.mi.total_time;
        let max_ticks: u32 = 500_000;
        let mut ticks_run: u32 = 0;

        while !self.module.state.player_repeats && ticks_run < max_ticks {
            if self.module.state.curr_vblanc == 0 {
                self.process_row();
            }
            self.update_channels_for_mixing();
            if (search_time > 0 && self.module.state.play_time >= search_time)
                || self.module.state.player_repeats
            {
                break;
            }

            self.module.state.buf_samples = self.module.state.num_samples.max(1);
            self.module.state.max_level = 0;

            // SaveChannels (play.inc Zeile 1402)
            let saved_channels = self.module.state.channels;

            loop {
                let am_repeating =
                    self.module.state.player_repeats && !self.module.state.repeat_song;
                let _ = process_sample_block(
                    &mut self.module.state,
                    &mut self.mix_mem,
                    &self.module.sample_mem,
                    am_repeating,
                );

                if self.module.state.max_level > 32767
                    && (self.module.state.sound_settings.pre_amp & 0x7F) > 0
                {
                    // RestoreChannels; Dec(SoundSettings.PreAmp); SetVolTable; MaxLevel := 0; GOTO Kernel;
                    self.module.state.channels = saved_channels;
                    self.module.state.sound_settings.pre_amp =
                        self.module.state.sound_settings.pre_amp.wrapping_sub(1);
                    self.calc_vol_master_and_table();
                    self.module.state.max_level = 0;
                } else {
                    break;
                }
            }

            // play.inc Zeile 1415: IF ShowPeakSearch THEN SetPercent(100*PlayTime DIV Mi.TotalTime);
            // SetPercent (WaitBox.PAS) ruft bei jedem Tick Yield() auf und überschreibt
            // dadurch den uninitialisierten Stack-Slot [S - 32] (env_stack_val) mit 0.
            self.module.state.env_stack_val = 0;

            self.advance_tick();
            ticks_run += 1;
        }

        let found_pre_amp = self.module.state.sound_settings.pre_amp;

        // Zurücksetzen für den eigentlichen Wiedergabe-/Recording-Lauf (PLAYER32.PAS Zeilen 627–646)
        self.mix_mem.peak_search = false;
        self.module.state.sound_settings = orig_settings;
        self.module.state.sound_settings.pre_amp = found_pre_amp;
        self.kanal_init(false);
        self.module.state.repeat_song = orig_repeat_song;

        found_pre_amp
    }

    /// Bereitet einen isolierten Sequencer für kooperativen, tickweisen PeakSearch vor
    /// (für Single-Thread-Umgebungen wie Win32s unter Windows 3.11 ohne `CreateThread`).
    pub fn init_incremental_peak_search(&mut self) {
        self.module.state.sound_settings.bits = 16;
        self.module.state.sound_settings.pre_amp = if self.module.it_module.is_some() {
            0x80 + 26
        } else {
            0x80 + 40
        };
        self.module.state.repeat_song = false;
        self.kanal_init(false);
        self.mix_mem.peak_search = true;
    }

    /// Führt bis zu `max_step_ticks` Ticks des PeakSearch-Vorlaufs aus.
    /// Gibt `Some(found_pre_amp)` zurück, sobald der Suchlauf abgeschlossen ist, sonst `None`.
    pub fn step_incremental_peak_search(
        &mut self,
        max_step_ticks: u32,
        ticks_run: &mut u32,
    ) -> Option<u8> {
        if self.module.it_module.is_some() {
            let max_ticks: u32 = 300_000;
            let mut step = 0u32;
            while !self.module.state.player_repeats && *ticks_run < max_ticks && step < max_step_ticks
            {
                let Some(ref it_mod) = self.module.it_module else {
                    return Some(self.module.state.sound_settings.pre_amp);
                };
                let Some(ref mut it_eng) = self.it_engine else {
                    return Some(self.module.state.sound_settings.pre_amp);
                };
                it_eng.process_tick(it_mod, &mut self.module.state);
                if self.module.state.player_repeats {
                    return Some(self.module.state.sound_settings.pre_amp);
                }
                self.module.state.max_level = 0;
                let _ = it_eng.mix_tick_block(it_mod, &mut self.module.state, &mut self.mix_mem);
                while self.module.state.max_level > 32767
                    && (self.module.state.sound_settings.pre_amp & 0x7F) > 0
                {
                    self.module.state.sound_settings.pre_amp =
                        self.module.state.sound_settings.pre_amp.wrapping_sub(1);
                    self.module.state.max_level =
                        (((self.module.state.max_level as i64) * 913) >> 10) as i32;
                }
                *ticks_run += 1;
                step += 1;
            }
            if self.module.state.player_repeats || *ticks_run >= max_ticks {
                return Some(self.module.state.sound_settings.pre_amp);
            }
            return None;
        }

        let search_time = self.module.state.mi.total_time;
        let max_ticks: u32 = 500_000;
        let mut step = 0u32;
        while !self.module.state.player_repeats && *ticks_run < max_ticks && step < max_step_ticks {
            if self.module.state.curr_vblanc == 0 {
                self.process_row();
            }
            self.update_channels_for_mixing();
            if (search_time > 0 && self.module.state.play_time >= search_time)
                || self.module.state.player_repeats
            {
                return Some(self.module.state.sound_settings.pre_amp);
            }

            self.module.state.buf_samples = self.module.state.num_samples.max(1);
            self.module.state.max_level = 0;
            let saved_channels = self.module.state.channels;

            loop {
                let am_repeating =
                    self.module.state.player_repeats && !self.module.state.repeat_song;
                let _ = process_sample_block(
                    &mut self.module.state,
                    &mut self.mix_mem,
                    &self.module.sample_mem,
                    am_repeating,
                );

                if self.module.state.max_level > 32767
                    && (self.module.state.sound_settings.pre_amp & 0x7F) > 0
                {
                    self.module.state.channels = saved_channels;
                    self.module.state.sound_settings.pre_amp =
                        self.module.state.sound_settings.pre_amp.wrapping_sub(1);
                    self.calc_vol_master_and_table();
                    self.module.state.max_level = 0;
                } else {
                    break;
                }
            }

            self.module.state.env_stack_val = 0;
            self.advance_tick();
            *ticks_run += 1;
            step += 1;
        }

        if self.module.state.player_repeats
            || *ticks_run >= max_ticks
            || (search_time > 0 && self.module.state.play_time >= search_time)
        {
            Some(self.module.state.sound_settings.pre_amp)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tables::SIN_TABLE;
    use crate::engine::types::Instrument;

    #[test]
    fn test_sequencer_cia_timing_and_tick0_trigger_and_buffer_fill() {
        let mut module = LoadedModule::default();
        module.state.mi.file_type = ModType::ProTr;
        module.state.mi.num_tracks = 4;
        module.state.mi.num_patterns = 1;
        module.state.mi.num_instruments = 1;
        module.state.init_speed = 6;
        module.state.init_bpm_speed = 125;

        // Instrument 1: 256-Byte Sinus-Sample mit Loop
        let base_ofs: i32 = 16;
        module.sample_mem = vec![0u8; (base_ofs as usize) + 260];
        for i in 0..256 {
            module.sample_mem[(base_ofs as usize) + i] = SIN_TABLE[i] as u8;
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
            bits: 8,
            tuning: DEF_MOD_TUNING_NTSC,
        };

        // Pattern 0 mit 64 Zeilen: Zeile 0 triggert C-2 (Periode 428 * 8 = 3424) mit Instrument 1 auf Kanal 0
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

        let settings = SoundSettings::default(); // 44100 Hz, 16-Bit Stereo
        let mut seq = Sequencer::new(module, settings);

        // Prüfe CIA-Timer-Berechnung bei 44100 Hz und 125 BPM:
        // NumSamples = (44100 * 2 + 22050) / 125 = 110250 / 125 = 882 Samples pro Tick
        assert_eq!(seq.module.state.num_samples, 882);
        assert_eq!(seq.module.state.speed, 6);
        assert_eq!(seq.module.state.bpm_speed, 125);

        // Fülle einen Audio-Puffer über mehrere Ticks und Zeilen hinweg (z. B. 882 * 6 * 2 = 10584 i16-Werte = 1 volle Zeile à 6 Ticks)
        let mut audio_out = vec![0i16; 882 * 6 * 2];
        seq.fill_audio_buffer(&mut audio_out);

        // Nach 6 Ticks (Speed = 6) muss Row 0 abgeschlossen sein und curr_note auf Row 1 stehen
        assert_eq!(seq.module.state.curr_note, 1);
        assert_eq!(seq.module.state.curr_vblanc, 0);

        // Kanal 0 muss getriggert worden sein und ein nicht-stummes Audiosignal erzeugt haben
        let inst0 = seq.module.state.channels[0].inst;
        let vol0 = seq.module.state.channels[0].vol;
        let tonh0 = seq.module.state.channels[0].tonh;
        assert_eq!(inst0, 1);
        assert_eq!(vol0, 64);
        assert_eq!(tonh0, 3424);
        assert!(audio_out.iter().any(|&sample| sample != 0));
    }

    #[test]
    fn test_sequencer_effects_arpeggio_portamento_vibrato_volslide_offset_and_breaks() {
        let mut module = LoadedModule::default();
        module.state.mi.file_type = ModType::ProTr;
        module.state.mi.num_tracks = 4;
        module.state.mi.num_patterns = 2;
        module.state.mi.num_instruments = 1;
        module.state.init_speed = 4;
        module.state.init_bpm_speed = 125;

        let base_ofs: i32 = 16;
        module.sample_mem = vec![0u8; (base_ofs as usize) + 1024];
        for i in 0..1024 {
            module.sample_mem[(base_ofs as usize) + i] = SIN_TABLE[i & 0xFF] as u8;
        }
        module.state.mi.instruments[0] = Instrument {
            name_len: 4,
            name: [0; 40],
            offset: base_ofs,
            iend: base_ofs + 1024,
            loop_start: base_ofs,
            loop_end: base_ofs + 1024,
            volume: 64,
            fine_tune: 0,
            bits: 8,
            tuning: DEF_MOD_TUNING_NTSC,
        };

        let mut pat0 = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        // Row 0: C-2 (3424), Inst 1, Sample Offset 901 (+256 Bytes)
        pat0[0][0] = Note {
            instrument: 1,
            ton: 3424,
            effekt: effect_ids::PLAY_OFFSET,
            operands: 0x01,
            chn_vol: DEF_CH_VOL,
        };
        // Row 1: Portamento Up 104
        pat0[1][0] = Note {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::PORTAMENTO_UP,
            operands: 0x04,
            chn_vol: DEF_CH_VOL,
        };
        // Row 2: Tone Portamento 310 zu D-2 (3050) + Volume Slide
        pat0[2][0] = Note {
            instrument: 0,
            ton: 3050,
            effekt: effect_ids::TONE_PORTAMENTO,
            operands: 0x10,
            chn_vol: DEF_CH_VOL,
        };
        // Row 3: Vibrato 484 + Arpeggio 037 auf Kanal 1 + PatternBreak D05
        pat0[3][0] = Note {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::VIBRATO,
            operands: 0x84,
            chn_vol: DEF_CH_VOL,
        };
        pat0[3][1] = Note {
            instrument: 1,
            ton: 3424,
            effekt: effect_ids::ARPEGGIO,
            operands: 0x37,
            chn_vol: DEF_CH_VOL,
        };
        pat0[3][2] = Note {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::PATTERN_BREAK,
            operands: 0x05, // BCD 05 -> Zeile 5 im nächsten Pattern
            chn_vol: DEF_CH_VOL,
        };

        let pat1 = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        module.patterns.push(pat0);
        module.patterns.push(pat1);
        module.state.p_arrangement[0] = 0;
        module.state.p_arrangement[1] = 1;
        module.state.ptrn_breaks[0] = 63;
        module.state.ptrn_breaks[1] = 63;

        let mut seq = Sequencer::new(module, SoundSettings::default());

        // Row 0, Tick 0..3 (4 Ticks)
        for t in 0..4 {
            seq.render_one_tick();
            if t == 0 {
                // Sample Offset 901 hat Startposition um >= 256 verschoben
                let note_pos = seq.module.state.channels[0].note;
                assert!(note_pos > base_ofs + 256);
            }
        }
        assert_eq!(seq.module.state.curr_note, 1);

        // Row 1 (Portamento Up): Nach 4 Ticks muss TonH kleiner als 3424 sein
        for _ in 0..4 {
            seq.render_one_tick();
        }
        let tonh_after_porta = seq.module.state.channels[0].tonh;
        assert!(tonh_after_porta < 3424);
        assert_eq!(seq.module.state.curr_note, 2);

        // Row 2 (Tone Portamento Richtung 3050):
        for _ in 0..4 {
            seq.render_one_tick();
        }
        let tonh_after_tp = seq.module.state.channels[0].tonh;
        assert!(tonh_after_tp < tonh_after_porta && tonh_after_tp >= 3050);
        assert_eq!(seq.module.state.curr_note, 3);

        // Row 3 (Vibrato, Arpeggio & PatternBreak D05):
        for _ in 0..4 {
            seq.render_one_tick();
        }
        // Nach Row 3 muss der PatternBreak D05 in Pattern 1, Zeile 5 gesprungen sein!
        let curr_pat = seq.module.state.curr_pattern;
        let curr_row = seq.module.state.curr_note;
        assert_eq!(curr_pat, 1);
        assert_eq!(curr_row, 5);
    }

    #[test]
    fn test_xm_set_speed_and_pahgiana_timing() {
        // 1. Synthetischer XM-Test für Fxx (Speed < 32 und BPM >= 32) über Pattern-Grenzen
        let mut module = LoadedModule::default();
        module.state.mi.file_type = ModType::FT2;
        module.state.mi.num_tracks = 4;
        module.state.mi.num_patterns = 3;
        module.state.use_linear_tune = true;
        module.state.init_speed = 3;
        module.state.speed = 3;
        module.state.init_bpm_speed = 195;
        module.state.bpm_speed = 195;

        let mut pat0 = vec![[Note::default(); MAX_M4W_CHANNELS]; 2];
        pat0[0][0].effekt = effect_ids::SET_SPEED;
        pat0[0][0].operands = 8; // F08 -> Speed 8

        let pat1 = vec![[Note::default(); MAX_M4W_CHANNELS]; 2]; // Kein Fxx -> Speed 8 bleibt erhalten!

        let mut pat2 = vec![[Note::default(); MAX_M4W_CHANNELS]; 2];
        pat2[0][0].effekt = effect_ids::SET_SPEED;
        pat2[0][0].operands = 3; // F03 -> Speed zurück auf 3

        module.patterns = vec![pat0, pat1, pat2];
        module.state.p_arrangement[0] = 0;
        module.state.p_arrangement[1] = 1;
        module.state.p_arrangement[2] = 2;
        module.state.ptrn_breaks[0] = 1;
        module.state.ptrn_breaks[1] = 1;
        module.state.ptrn_breaks[2] = 1;

        let mut seq = Sequencer::new(module, SoundSettings::default());
        seq.process_row();
        assert_eq!(seq.module.state.speed, 8);

        // 2 Zeilen à 8 Ticks = 16 Ticks für Pattern 0
        for _ in 0..16 {
            seq.render_one_tick();
        }
        assert_eq!(seq.module.state.curr_pattern, 1);
        assert_eq!(seq.module.state.speed, 8);

        // 2 Zeilen à 8 Ticks = 16 Ticks für Pattern 1
        for _ in 0..16 {
            seq.render_one_tick();
        }
        assert_eq!(seq.module.state.curr_pattern, 2);

        // Erste Zeile in Pattern 2 setzt Speed wieder auf 3
        seq.render_one_tick();
        assert_eq!(seq.module.state.speed, 3);

        // 2. Falls PAHGIANA.XM im Workspace liegt, kompletten Durchlauf über Order 43..49 und 101..108 prüfen
        let xm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../PAHGIANA.XM");
        if let Ok(bytes) = std::fs::read(&xm_path) {
            let loaded = crate::engine::loader::load_xm(&bytes).expect("PAHGIANA.XM load failed");
            let mut seq = Sequencer::new(loaded, SoundSettings::default());
            assert_eq!(seq.module.state.speed, 3);
            assert_eq!(seq.module.state.bpm_speed, 195);

            let mut order_speeds = vec![0u8; seq.module.state.mi.num_patterns as usize];
            let mut last_order = u16::MAX;

            // Simuliere Tick-Schritte ohne PCM-Kernel bis zum ersten Song-Ende
            let mut ticks = 0usize;
            while !seq.module.state.player_repeats && ticks < 500_000 {
                if seq.module.state.curr_vblanc == 0 {
                    seq.process_row();
                    let ord = seq.module.state.curr_pattern as usize;
                    if seq.module.state.curr_pattern != last_order && ord < order_speeds.len() {
                        order_speeds[ord] = seq.module.state.speed;
                        last_order = seq.module.state.curr_pattern;
                    }
                }
                seq.update_channels_for_mixing();
                seq.advance_tick();
                ticks += 1;
            }

            // Orders 0..=42 (UI Pattern 1..=43): Speed 3
            for ord in 0..=42 {
                assert_eq!(order_speeds[ord], 3, "Order {} sollte Speed 3 haben", ord);
            }
            // Orders 43..=48 (UI Pattern 44..=49): F08 -> Speed 8 (ruhiger Zwischenteil!)
            for ord in 43..=48 {
                assert_eq!(order_speeds[ord], 8, "Order {} (UI {}) sollte Speed 8 haben", ord, ord + 1);
            }
            // Orders 49..=100 (UI Pattern 50..=101): F03 -> Speed 3
            for ord in 49..=100 {
                assert_eq!(order_speeds[ord], 3, "Order {} (UI {}) sollte wieder Speed 3 haben", ord, ord + 1);
            }
            // Orders 101..=108 (UI Pattern 102..=109, Outro): F08 -> Speed 8
            for ord in 101..=108 {
                assert_eq!(order_speeds[ord], 8, "Order {} (UI {}) sollte im Outro Speed 8 haben", ord, ord + 1);
            }
        }
    }

    #[test]
    fn test_xm_bidi_loop_unrolling_and_seek_pattern_in_pahgiana() {
        // Prüfe zunächst synthetisch, dass ein BiDi-Sample mit Post-Loop-Tail (SampleLen > LoopEnd)
        // exakt ab Offset + LoopEnd gespiegelt wird (und nicht hinter SampleLen!).
        let mut sample_mem = Vec::new();
        // Baue ein 1100-Byte Sample mit LoopStart = 200, LoopEnd = 1050, SampleLen = 1100
        let mut raw_pcm = vec![0u8; 1100];
        for i in 0..1050 {
            raw_pcm[i] = ((i % 64) as i8) as u8;
        }
        // Hinter LoopEnd (1050..1100) absichtlich extreme Ausreißer (-128) legen:
        for i in 1050..1100 {
            raw_pcm[i] = (-128i8) as u8;
        }
        let mut offset = 0i32;
        let mut iend = 1100i32;
        let mut loop_start = 200i32;
        let mut loop_end = 1050i32;
        let mut vol = 64u8;

        crate::engine::loader::load_instrument_into_sample_mem(
            &mut sample_mem,
            &raw_pcm,
            &mut offset,
            &mut iend,
            &mut loop_start,
            &mut loop_end,
            &mut vol,
            8,
            0, // 8-bit signed linear
            true,
        );

        // Am ursprünglichen LoopEnd (offset + 1050) muss jetzt das letzte Byte des Vorwärts-Loops
        // gespiegelt stehen (raw_pcm[1049]), NICHT der Post-Loop-Ausreißer (-128)!
        let mirrored_first = sample_mem[(offset + 1050) as usize] as i8;
        assert_eq!(mirrored_first, raw_pcm[1049] as i8);
        assert_ne!(mirrored_first, -128);

        // Und an den 4 Guard-Bytes hinter dem neuen LoopEnd (offset + 1050 + 850) muss LoopStart stehen:
        assert_eq!(loop_end, offset + 1050 + 850);
        assert_eq!(iend, loop_end);
        for g in 0..4 {
            assert_eq!(
                sample_mem[(loop_end as usize) + g],
                sample_mem[(loop_start as usize) + g]
            );
        }

        // Teste nun seek_to_pattern(44) und seek_to_pattern(102) auf PAHGIANA.XM (falls vorhanden)
        let candidate_paths = [
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../PAHGIANA.XM"),
            std::path::PathBuf::from(r"M:\Audio\MOD_MID_SID\MOD\XM\PAHGIANA.XM"),
        ];
        for path in &candidate_paths {
            if let Ok(bytes) = std::fs::read(path) {
                let loaded = crate::engine::loader::load_xm(&bytes).expect("PAHGIANA.XM load failed");
                let mut seq = Sequencer::new(loaded, SoundSettings::default());

                // Seek auf UI-Pattern 44 (Order 43, physisches Pattern 16)
                seq.seek_to_pattern(44);
                let cp44 = seq.module.state.mi.curr_pttrn;
                assert_eq!(seq.module.state.curr_pattern, 43);
                assert_eq!(cp44, 44);
                assert_eq!(seq.module.state.curr_note, 0);

                // Rendere 1 Zeile (8 Ticks) in Pattern 44: Speed muss auf 8 wechseln, Kanal 15 startet auf Vol 0
                seq.render_one_tick();
                assert_eq!(seq.module.state.speed, 8);
                assert_eq!(seq.module.state.channels[15].vol, 0);
                // Kanal 2 hat ED1 (NoteDelay 1 Tick): Auf Tick 0 noch nicht getriggert, auf Tick 1 wird Note 61 getriggert!
                seq.render_one_tick();
                assert_eq!(seq.module.state.channels[2].inst, 17);
                assert_eq!(seq.module.state.channels[2].pan_val, 255);

                // Step +1 Pattern -> UI-Pattern 45 (Order 44, physisches Pattern 11)
                seq.step_order(1);
                let cp45 = seq.module.state.mi.curr_pttrn;
                assert_eq!(seq.module.state.curr_pattern, 44);
                assert_eq!(cp45, 45);

                // Seek auf UI-Pattern 102 (Order 101, physisches Pattern 31)
                seq.seek_to_pattern(102);
                let cp102 = seq.module.state.mi.curr_pttrn;
                assert_eq!(seq.module.state.curr_pattern, 101);
                assert_eq!(cp102, 102);
                seq.render_one_tick();
                assert_eq!(seq.module.state.speed, 8);

                // Regressionstest A: Hin- und Herspringen (20 -> 102 -> 44 -> 105 -> 5 -> 101)
                // und anschließend bis zum Ende von Pattern 109 (Order 108) durchspielen:
                // `player_repeats` darf ERST am echten Song-Ende (Wrap von Order 108 -> Order 0)
                // ausgelöst werden (und im nächsten Durchlauf NICHT bei Pattern 101/102 abbrechen!).
                for &pat in &[20u16, 102, 44, 105, 5, 101] {
                    seq.seek_to_pattern(pat);
                }
                assert_eq!(seq.module.state.curr_pattern, 100); // UI-Pattern 101 = Order 100
                let mut ticks_to_end = 0usize;
                let mut last_ord_before_repeat = 0u16;
                while !seq.module.state.player_repeats && ticks_to_end < 100_000 {
                    last_ord_before_repeat = seq.module.state.curr_pattern;
                    if seq.module.state.curr_vblanc == 0 {
                        seq.process_row();
                    }
                    seq.update_channels_for_mixing();
                    seq.advance_tick();
                    ticks_to_end += 1;
                }
                assert!(seq.module.state.player_repeats);
                assert_eq!(
                    last_ord_before_repeat, 108,
                    "Song muss exakt nach Order 108 (UI-Pattern 109) enden"
                );
                assert_eq!(seq.module.state.curr_pattern, 0);

                // Regressionstest B: Seek auf Pattern 5 (Order 4) + Repeat 1:
                // Nach dem Ende von Pattern 109 (Order 108 -> Order 0) muss der nächste
                // komplette Durchlauf von Pattern 1 (Order 0) nahtlos über Pattern 4 und 5
                // hinweg bis zum Ende von Pattern 109 (Order 108) durchlaufen!
                seq.seek_to_pattern(5);
                assert_eq!(seq.module.state.curr_pattern, 4);
                let mut guard1 = 0usize;
                while !seq.module.state.player_repeats && guard1 < 100_000 {
                    if seq.module.state.curr_vblanc == 0 {
                        seq.process_row();
                    }
                    seq.update_channels_for_mixing();
                    seq.advance_tick();
                    guard1 += 1;
                }
                assert!(seq.module.state.player_repeats);
                assert_eq!(seq.module.state.curr_pattern, 0);
                seq.module.state.player_repeats = false;

                // Zweiter Durchlauf (ab Order 0): darf NICHT bei Order 3/4 (Pattern 4/5) neu starten,
                // sondern muss wieder exakt bis Order 108 (Pattern 109) durchspielen!
                let mut guard2 = 0usize;
                let mut last_ord_pass2 = 0u16;
                while !seq.module.state.player_repeats && guard2 < 100_000 {
                    last_ord_pass2 = seq.module.state.curr_pattern;
                    if seq.module.state.curr_vblanc == 0 {
                        seq.process_row();
                    }
                    seq.update_channels_for_mixing();
                    seq.advance_tick();
                    guard2 += 1;
                }
                assert!(seq.module.state.player_repeats);
                assert_eq!(
                    last_ord_pass2, 108,
                    "Zweiter Repeat-Durchlauf darf nicht bei Pattern 4/5 abbrechen, sondern muss bis Order 108 (Pattern 109) laufen"
                );
                break;
            }
        }
    }

    #[test]
    fn test_group1_dxx_wrap_and_group3a_ido1_old_smp_regressions() {
        let rerun_mods = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ModBatchRerun1/mods");
        if !rerun_mods.exists() {
            return;
        }

        // 1. Gruppe 1: Exakte Stereo-Frame-Länge bis zum Song-Ende bei Dxx-PatternBreak-Wrap
        let group1_cases: &[(&str, usize)] = &[
            ("BS123.MOD", 1_966_860),
            ("enigma-leveleffects-salvaged.mod", 7_910_328),
            ("fun tune.mod", 106_722),
            ("nru suger bat.xm", 6_104_160),
            ("trip to kauppa.xm", 2_878_848),
            ("2neksek netsoveh.mod", 336_924),
            ("aivan sanaton laulu.mod", 10_477_530),
        ];

        for &(filename, expected_frames) in group1_cases {
            let path = rerun_mods.join(filename);
            if !path.exists() {
                continue;
            }
            let bytes = std::fs::read(&path).expect("Modul lesen");
            let ext = path.extension().and_then(|e| e.to_str());
            let loaded = crate::engine::loader::load_module_with_ext(&bytes, ext).expect("Modul laden");
            let mut settings = SoundSettings::default();
            settings.device_id = DEV_HDD;
            settings.samplerate = 44100;
            settings.bits = 16;
            settings.stereo = 2;
            settings.panning = true;
            settings.oversamp = 0;

            let mut seq = Sequencer::new(loaded, settings);
            seq.module.state.repeat_song = false;

            let mut total_frames = 0usize;
            let mut guard = 0usize;
            while !seq.module.state.player_repeats && guard < 200_000 {
                if seq.module.state.curr_vblanc == 0 {
                    seq.process_row();
                }
                seq.update_channels_for_mixing();
                let num_tracks = seq.module.state.mi.num_tracks;
                let num_samples = seq.module.state.num_samples.max(1);
                total_frames += (num_samples as usize).min(crate::engine::types::MAX_MIX_BUF_SAMPLES);
                calc_sample_positions(&mut seq.module.state.channels, num_tracks, num_samples);
                seq.advance_tick();
                guard += 1;
            }
            assert_eq!(
                total_frames, expected_frames,
                "Gruppe 1 Song-Länge weicht für {} ab",
                filename
            );
        }

        // 2. Untergruppe 3A: Exakte Stereo-Sample-Werte an den früheren IDO-1-Loop-Wrap-Stellen
        let group3a_cases: &[(&str, u8, usize, (i16, i16))] = &[
            ("deep funk.xm", 0x9D, 402_342, (-100, -100)),
            ("macie baterie.xm", 0x94, 40_925, (617, 617)),
            ("four fancy martinis.xm", 0x9E, 4_746_114, (3773, 3895)),
            ("lunaressence.xm", 0x9F, 4_058_488, (5675, 5769)),
            ("light waves.xm", 0xA2, 5_296_194, (5458, 5523)),
        ];

        for &(filename, preamp, target_frame, expected_lr) in group3a_cases {
            let path = rerun_mods.join(filename);
            if !path.exists() {
                continue;
            }
            let bytes = std::fs::read(&path).expect("Modul lesen");
            let ext = path.extension().and_then(|e| e.to_str());
            let loaded = crate::engine::loader::load_module_with_ext(&bytes, ext).expect("Modul laden");
            let mut settings = SoundSettings::default();
            settings.device_id = DEV_HDD;
            settings.samplerate = 44100;
            settings.bits = 16;
            settings.stereo = 2;
            settings.panning = true;
            settings.surround = false;
            settings.oversamp = 1;
            settings.pre_amp = preamp;

            let mut seq = Sequencer::new(loaded, settings);
            seq.module.state.repeat_song = false;

            let mut curr_frame = 0usize;
            let mut found_sample: Option<(i16, i16)> = None;
            while !seq.module.state.player_repeats {
                seq.render_one_tick();
                let pcm = &seq.pending_pcm;
                let tick_frames = pcm.len() / 2;
                if target_frame >= curr_frame && target_frame < curr_frame + tick_frames {
                    let idx = (target_frame - curr_frame) * 2;
                    found_sample = Some((pcm[idx], pcm[idx + 1]));
                    break;
                }
                curr_frame += tick_frames;
            }
            assert_eq!(
                found_sample,
                Some(expected_lr),
                "Untergruppe 3A IDO-1 Sample bei Frame {} weicht für {} ab",
                target_frame,
                filename
            );
        }
    }
}
