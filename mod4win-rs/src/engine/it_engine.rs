//! Impulse Tracker (`.IT`) Sequencer, NNA Virtual-Voice-Architektur (64 Host-Kanäle × 256 Voices)
//! und duale Audio-/Mixer-Engine (`MOD4WIN IDO 0..3` vs. `IT2 Native` inkl. resonanter 2-Pole-Filter).
//!
//! Referenzen:
//! - Jeffrey Lim: Impulse Tracker 2.14 (`ITTECH.TXT`, `M32BITI.ASM`, `M12BIT.ASM`)
//! - Olav Sørensen (`8bitbubsy`): `it2play` (NNA/DCT/DCA, Envelopes, Pitch/Vol Slides, 2-Pole Resonant Filter)
//!
//! ### WICHTIG (Zero Regression)
//! Dieses Modul wird ausschließlich für [`ModType::IT`] instanziiert (`Sequencer::it_engine = Some(...)`).
//! Alle klassischen Formate (`.mod`, `.s3m`, `.xm`, `.mtm`, `.669`, `.stm`, `.far`, `.okt`, `.wow`)
//! durchlaufen weiterhin zu 100 % unverändert ihren originalen `PLAYER32.DLL`-Pfad.

use std::f64::consts::PI;

use crate::engine::formats::it::{
    it_env_flags, it_flags, it_smp_flags, map_it_command_to_m4w_effect,
    map_it_vol_cmd_to_m4w_effect, ItEnvelope, ItModuleData, ItNoteCell, ItSample,
};
use crate::engine::mixer::{
    apply_click_removal_slide, apply_post_filters, mix_out_and_clip, process_surround_buffer,
    DacMixMemory, MixOutputBuffer, StereoSample32,
};
use crate::engine::types::{EngineState, MAX_M4W_CHANNELS, MAX_MIX_BUF_SAMPLES, MAX_PATTERNS};

/// Maximale Anzahl der Host-Kanäle in Impulse Tracker (`64`).
pub const MAX_IT_HOST_CHANNELS: usize = 64;

/// Maximale Anzahl gleichzeitiger virtueller Misch-Stimmen (`256` Voices in IT 2.14).
pub const MAX_IT_VIRTUAL_VOICES: usize = 256;

/// Feinskalierte Sinus-Tabelle (`-64..+64`, 256 Einträge) für IT-Vibrato, Tremolo, Panbrello und Auto-Vibrato.
pub const IT_FINE_SINE_TABLE: [i8; 256] = [
    0, 2, 3, 5, 6, 8, 9, 11, 12, 14, 16, 17, 19, 20, 22, 23, 24, 26, 27, 29, 30, 32, 33, 34, 36,
    37, 38, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 56, 57, 58, 59, 59,
    60, 60, 61, 61, 62, 62, 62, 63, 63, 63, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 63, 63, 63,
    62, 62, 62, 61, 61, 60, 60, 59, 59, 58, 57, 56, 56, 55, 54, 53, 52, 51, 50, 49, 48, 47, 46, 45,
    44, 43, 42, 41, 39, 38, 37, 36, 34, 33, 32, 30, 29, 27, 26, 24, 23, 22, 20, 19, 17, 16, 14, 12,
    11, 9, 8, 6, 5, 3, 2, 0, -2, -3, -5, -6, -8, -9, -11, -12, -14, -16, -17, -19, -20, -22, -23,
    -24, -26, -27, -29, -30, -32, -33, -34, -36, -37, -38, -39, -41, -42, -43, -44, -45, -46, -47,
    -48, -49, -50, -51, -52, -53, -54, -55, -56, -56, -57, -58, -59, -59, -60, -60, -61, -61, -62,
    -62, -62, -63, -63, -63, -64, -64, -64, -64, -64, -64, -64, -64, -64, -64, -64, -63, -63, -63,
    -62, -62, -62, -61, -61, -60, -60, -59, -59, -58, -57, -56, -56, -55, -54, -53, -52, -51, -50,
    -49, -48, -47, -46, -45, -44, -43, -42, -41, -39, -38, -37, -36, -34, -33, -32, -30, -29, -27,
    -26, -24, -23, -22, -20, -19, -17, -16, -14, -12, -11, -9, -8, -6, -5, -3, -2,
];

/// Laufzeitzustand einer einzelnen Hüllkurve (Volume, Panning oder Pitch/Filter) auf einer Virtual Voice.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItEnvState {
    pub enabled: bool,
    pub tick: i32,
    pub value: i32, // 8.8 Fixed-Point (Knotenwert << 8)
    pub finished: bool,
}

impl ItEnvState {
    pub fn reset(&mut self, env: &ItEnvelope, default_val: i32) {
        self.enabled = (env.flags & it_env_flags::ON) != 0 && env.num_nodes > 0;
        self.tick = 0;
        self.finished = false;
        if self.enabled {
            self.value = (env.nodes[0].val as i32) << 8;
        } else {
            self.value = default_val << 8;
        }
    }

    /// Aktualisiert den Hüllkurven-Wert beim aktuellen `tick` und rückt `tick` für den nächsten Tick um 1 vor.
    pub fn step(&mut self, env: &ItEnvelope, note_off: bool, default_val: i32) {
        if !self.enabled || env.num_nodes == 0 {
            self.value = default_val << 8;
            return;
        }

        let sus_active = !note_off && (env.flags & it_env_flags::SUS_LOOP) != 0;
        let loop_active = (env.flags & it_env_flags::LOOP) != 0;

        let n = (env.num_nodes as usize).min(25);
        if n == 1 {
            self.value = (env.nodes[0].val as i32) << 8;
            self.finished = !sus_active && !loop_active;
            return;
        }

        // Loop-Prüfung (Sustain-Loop hat Vorrang solange !note_off)
        if sus_active {
            let slb = (env.sus_loop_begin as usize).min(n - 1);
            let sle = (env.sus_loop_end as usize).min(n - 1);
            let end_tick = env.nodes[sle].tick as i32;
            if self.tick >= end_tick {
                self.tick = env.nodes[slb].tick as i32;
                if slb == sle {
                    self.value = (env.nodes[slb].val as i32) << 8;
                    self.finished = false;
                    return;
                }
            }
        } else if loop_active {
            let lb = (env.loop_begin as usize).min(n - 1);
            let le = (env.loop_end as usize).min(n - 1);
            let end_tick = env.nodes[le].tick as i32;
            if self.tick >= end_tick {
                self.tick = env.nodes[lb].tick as i32;
                if lb == le {
                    self.value = (env.nodes[lb].val as i32) << 8;
                    self.finished = false;
                    return;
                }
            }
        }

        let last_tick = env.nodes[n - 1].tick as i32;
        if self.tick >= last_tick {
            self.value = (env.nodes[n - 1].val as i32) << 8;
            self.finished = !sus_active && !loop_active;
            return;
        }
        self.finished = false;

        // Finde das aktuelle Segment `nodes[idx] .. nodes[idx + 1]`
        let mut seg_idx = 0usize;
        for i in 0..(n - 1) {
            if self.tick >= (env.nodes[i].tick as i32)
                && self.tick < (env.nodes[i + 1].tick as i32)
            {
                seg_idx = i;
                break;
            }
        }

        let t0 = env.nodes[seg_idx].tick as i32;
        let t1 = env.nodes[seg_idx + 1].tick as i32;
        let v0 = (env.nodes[seg_idx].val as i32) << 8;
        let v1 = (env.nodes[seg_idx + 1].val as i32) << 8;
        let dt = (t1 - t0).max(1);
        self.value = v0 + ((v1 - v0) * (self.tick - t0)) / dt;

        self.tick += 1;
    }
}

/// 2-Pole Resonanzfilter-Zustand einer Virtual Voice (`it2play` / `IT214`).
#[derive(Clone, Copy, Debug)]
pub struct ItVoiceFilter {
    pub cutoff: u8,
    pub resonance: u8,
    pub env_cutoff: u8,
    pub highpass: bool,
    pub ever_active: bool,
    pub active: bool,
    pub fg: f32,
    pub fb0: f32,
    pub fb1: f32,
    pub y1_l: f32,
    pub y2_l: f32,
    pub y1_r: f32,
    pub y2_r: f32,
    last_calc_cut: u8,
    last_calc_res: u8,
    last_calc_sr: i32,
    last_calc_hp: bool,
}

impl Default for ItVoiceFilter {
    fn default() -> Self {
        Self {
            cutoff: 127,
            resonance: 0,
            env_cutoff: 255,
            highpass: false,
            ever_active: false,
            active: false,
            fg: 1.0,
            fb0: 0.0,
            fb1: 0.0,
            y1_l: 0.0,
            y2_l: 0.0,
            y1_r: 0.0,
            y2_r: 0.0,
            last_calc_cut: 255,
            last_calc_res: 255,
            last_calc_sr: 0,
            last_calc_hp: false,
        }
    }
}

impl ItVoiceFilter {
    /// Berechnet die `it2play`-Koeffizienten (`fg`, `fb0`, `fb1`) bei Änderung von Cutoff,
    /// Resonanz oder Samplerate.
    pub fn update_coefficients(&mut self, sample_rate: i32) {
        let eff_cut = ((self.cutoff as u32 * self.env_cutoff as u32) / 255).min(127) as u8;
        let res = self.resonance.min(127);

        if eff_cut < 127 || self.highpass {
            self.ever_active = true;
        }
        // In Impulse Tracker 2.14 / it2play / OpenMPT bleibt ein Low-Pass-Filter bei
        // eff_cut == 127 inaktiv, solange auf dieser Stimme noch nie eff_cut < 127 aktiv war.
        if !self.highpass && eff_cut == 127 && (res == 0 || !self.ever_active) {
            self.active = false;
            return;
        }
        self.active = true;

        if eff_cut == self.last_calc_cut
            && res == self.last_calc_res
            && sample_rate == self.last_calc_sr
            && self.highpass == self.last_calc_hp
        {
            return;
        }
        self.last_calc_cut = eff_cut;
        self.last_calc_res = res;
        self.last_calc_sr = sample_rate;
        self.last_calc_hp = self.highpass;

        let sr = (sample_rate.max(4000)) as f64;
        // Original it2play / IT 2.14 Frequenz-Formel:
        // f_c = 110 Hz * 2^((Cutoff - 128) / 24 + 5.5833) = 110 Hz * 2^(0.25 + Cutoff / 24)
        let mut fc = 110.0 * 2.0f64.powf(0.25 + (eff_cut as f64) / 24.0);
        let nyquist = sr * 0.49;
        if fc > nyquist {
            fc = nyquist;
        }

        let r = sr / (2.0 * PI * fc);
        // Resonanz-Dämpfung p = 10^((-resonance * 24) / (128 * 20))
        let p = 10.0f64.powf(-((res as f64) * 24.0) / (128.0 * 20.0));
        let d = 2.0 * p * r + p - 1.0;
        let e = r * r;
        let inv_denom = 1.0 / (1.0 + d + e);

        self.fg = inv_denom as f32;
        self.fb0 = ((d + e + e) * inv_denom) as f32;
        self.fb1 = (-e * inv_denom) as f32;
    }

    #[inline(always)]
    pub fn process_left(&mut self, x: f32) -> f32 {
        if !self.active {
            return x;
        }
        let y = self.fg * x + self.fb0 * self.y1_l + self.fb1 * self.y2_l;
        let y_clamped = y.clamp(-2_000_000_000.0, 2_000_000_000.0);
        self.y2_l = self.y1_l;
        self.y1_l = y_clamped;
        if self.highpass {
            x - y_clamped
        } else {
            y_clamped
        }
    }

    #[inline(always)]
    pub fn process_right(&mut self, x: f32) -> f32 {
        if !self.active {
            return x;
        }
        let y = self.fg * x + self.fb0 * self.y1_r + self.fb1 * self.y2_r;
        let y_clamped = y.clamp(-2_000_000_000.0, 2_000_000_000.0);
        self.y2_r = self.y1_r;
        self.y1_r = y_clamped;
        if self.highpass {
            x - y_clamped
        } else {
            y_clamped
        }
    }
}

/// Virtuelle Misch-Stimme (`ItVirtualVoice`, bis zu 256 gleichzeitig aktiv).
#[derive(Clone, Copy, Debug, Default)]
pub struct ItVirtualVoice {
    pub active: bool,
    pub background: bool,
    pub host_chn: u8,
    pub note: u8,       // 0..119
    pub orig_note: u8,  // 0..119 (vor Keymap)
    pub instrument: u8, // 1..255
    pub sample: u8,     // 1..255
    pub sample_pos: i32,
    pub sample_frac: u32, // 16-Bit gebrochene Phase (0..65535)
    pub reverse: bool,
    pub freq: u32,      // Aktuelle Sample-Frequenz in Hz (inkl. Vibrato/Arp/PitchEnv)
    pub base_freq: u32, // Basis-Frequenz in Hz (wird durch Portamento/Slides permanent verschoben)
    pub note_off: bool,
    pub note_fade: bool,
    pub fadeout_val: i32, // 0..=1024
    pub vol: i32,         // 0..=64 (Note/Sample-Lautstärke)
    pub fvol: i32,        // 0..=16384 (berechnete End-Lautstärke)
    pub pan: i32,         // 0..=64 oder 100 (Surround)
    pub l_vol: i32,       // Linke Misch-Lautstärke (0..=16384)
    pub r_vol: i32,       // Rechte Misch-Lautstärke (0..=16384)
    pub surround: bool,
    pub nna: u8,
    pub dct: u8,
    pub dca: u8,
    pub vol_env: ItEnvState,
    pub pan_env: ItEnvState,
    pub pitch_env: ItEnvState,
    pub auto_vib_pos: u8,
    pub auto_vib_depth: u32, // 8.8 Sweep-Akkumulator
    pub filter: ItVoiceFilter,
}

/// Host-/Pattern-Kanal (`ItHostChannel`, 64 Kanäle).
#[derive(Clone, Copy, Debug)]
pub struct ItHostChannel {
    pub fg_voice: Option<usize>,
    pub muted: bool,
    pub cvol: i32, // 0..=64 Channel Volume
    pub cpan: i32, // 0..=64 oder 100 (Surround)
    pub last_note: u8,
    pub last_inst: u8,
    pub last_smp: u8,
    pub vol: i32, // 0..=64 aktuelle Kanal-Notenlautstärke
    pub cell: ItNoteCell,
    pub porta_target_freq: u32,
    // Effekt-Speicher (Memory)
    pub mem_vol_slide: u8,     // D / K / L
    pub mem_porta_up_dn: u8,   // E / F
    pub mem_tone_porta: u8,    // G
    pub mem_vibrato_spd: u8,   // H / U
    pub mem_vibrato_dep: u8,   // H / U
    pub mem_tremor: u8,        // I
    pub mem_arpeggio: u8,      // J
    pub mem_cvol_slide: u8,    // N
    pub mem_sample_offset: u8, // O
    pub mem_high_offset: u8,   // SAx
    pub mem_pan_slide: u8,     // P
    pub mem_retrig: u8,        // Q
    pub mem_tremolo: u8,       // R
    pub mem_special: u8,       // S
    pub mem_tempo: u8,         // T
    pub mem_gvol_slide: u8,    // W
    pub mem_panbrello: u8,     // Y
    pub mem_vol_col_slide: u8, // Vol-Column a/b/c/d
    pub mem_vol_col_porta: u8, // Vol-Column e/f/g
    // Oszillator-Zustände
    pub vib_pos: u8,
    pub vib_wave: u8,
    pub trem_pos: u8,
    pub trem_wave: u8,
    pub panb_pos: u8,
    pub panb_wave: u8,
    pub tremor_cnt: u8,
    pub retrig_cnt: u8,
    pub pat_loop_row: u16,
    pub pat_loop_cnt: u8,
    // Aktives MIDI-Macro (SFx) & Kanal-Filter
    pub active_sfx_macro: u8,
    pub cutoff: u8,
    pub resonance: u8,
    pub highpass: bool,
    // Temporäre Modifikatoren für den aktuellen Tick
    pub tick_freq_delta: i32, // in 1/64-Halbton-Einheiten
    pub tick_vol_delta: i32,  // -64..+64 (Tremolo)
    pub tick_pan_delta: i32,  // -32..+32 (Panbrello)
    pub tremor_mute: bool,
}

impl Default for ItHostChannel {
    fn default() -> Self {
        Self {
            fg_voice: None,
            muted: false,
            cvol: 64,
            cpan: 32,
            last_note: 60,
            last_inst: 0,
            last_smp: 0,
            vol: 64,
            cell: ItNoteCell::default(),
            porta_target_freq: 0,
            mem_vol_slide: 0,
            mem_porta_up_dn: 0,
            mem_tone_porta: 0,
            mem_vibrato_spd: 0,
            mem_vibrato_dep: 0,
            mem_tremor: 0,
            mem_arpeggio: 0,
            mem_cvol_slide: 0,
            mem_sample_offset: 0,
            mem_high_offset: 0,
            mem_pan_slide: 0,
            mem_retrig: 0,
            mem_tremolo: 0,
            mem_special: 0,
            mem_tempo: 0,
            mem_gvol_slide: 0,
            mem_panbrello: 0,
            mem_vol_col_slide: 0,
            mem_vol_col_porta: 0,
            vib_pos: 0,
            vib_wave: 0,
            trem_pos: 0,
            trem_wave: 0,
            panb_pos: 0,
            panb_wave: 0,
            tremor_cnt: 0,
            retrig_cnt: 0,
            pat_loop_row: 0,
            pat_loop_cnt: 0,
            active_sfx_macro: 0,
            cutoff: 127,
            resonance: 0,
            highpass: false,
            tick_freq_delta: 0,
            tick_vol_delta: 0,
            tick_pan_delta: 0,
            tremor_mute: false,
        }
    }
}

/// Komplette Laufzeit-Engine für Impulse Tracker (`.IT`).
#[derive(Clone)]
pub struct ItEngine {
    pub host_channels: Box<[ItHostChannel; MAX_IT_HOST_CHANNELS]>,
    pub voices: Vec<ItVirtualVoice>,
    pub curr_order: usize,
    pub curr_row: u16,
    pub curr_tick: u8,
    pub speed: u8,
    pub tempo: u16,
    pub global_vol: i32, // 0..=128
    pub row_delay: u8,   // SEx Pattern Delay (in Zeilen-Wiederholungen)
    pub tick_delay: u8,  // S6x Fine Pattern Delay (in Extra-Ticks)
    pub break_order: Option<usize>,
    pub break_row: Option<u16>,
    pub song_finished: bool,
    pub rng_seed: u32,
}

impl Default for ItEngine {
    fn default() -> Self {
        Self {
            host_channels: Box::new([ItHostChannel::default(); MAX_IT_HOST_CHANNELS]),
            voices: vec![ItVirtualVoice::default(); MAX_IT_VIRTUAL_VOICES],
            curr_order: 0,
            curr_row: 0,
            curr_tick: 0,
            speed: 6,
            tempo: 125,
            global_vol: 128,
            row_delay: 0,
            tick_delay: 0,
            break_order: None,
            break_row: None,
            song_finished: false,
            rng_seed: 0x1357_9BDF,
        }
    }
}

impl ItEngine {
    pub fn new(it: &ItModuleData, state: &mut EngineState) -> Self {
        let mut eng = Self::default();
        eng.reset(it, state);
        eng
    }

    #[inline]
    fn next_random(&mut self) -> i8 {
        self.rng_seed = self
            .rng_seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.rng_seed >> 24) & 0x7F) as i8 - 64
    }

    pub fn reset(&mut self, it: &ItModuleData, state: &mut EngineState) {
        self.voices.fill(ItVirtualVoice::default());
        for ch in 0..MAX_IT_HOST_CHANNELS {
            let mut hc = ItHostChannel::default();
            let raw_pan = it.chn_pan[ch];
            hc.muted = (raw_pan & 0x80) != 0;
            let p = raw_pan & 0x7F;
            hc.cpan = if p == 100 { 100 } else { p.min(64) as i32 };
            hc.cvol = it.chn_vol[ch].min(64) as i32;
            self.host_channels[ch] = hc;
        }

        self.curr_order = 0;
        while self.curr_order < it.orders.len() && it.orders[self.curr_order] == 0xFE {
            self.curr_order += 1;
        }
        self.curr_row = 0;
        self.curr_tick = 0;
        self.speed = it.initial_speed.max(1);
        self.tempo = (it.initial_tempo as u16).max(32);
        self.global_vol = it.global_vol.min(128) as i32;
        self.row_delay = 0;
        self.tick_delay = 0;
        self.break_order = None;
        self.break_row = None;
        self.song_finished = false;

        Self::update_tempo_samples(self.tempo, state);
        self.sync_ui_metadata(it, state);
    }

    pub fn update_tempo_samples(tempo: u16, state: &mut EngineState) {
        let sr = state.sound_settings.samplerate.max(4000) as u32;
        let bpm = (tempo.clamp(32, 255)) as u32;
        // 1 Tick = 2.5 / BPM Sekunden = (sr * 5) / (2 * BPM) Frames
        // Wichtig: Nicht auf MAX_MIX_BUF_SAMPLES (2000) clampen, da BPM < 55 (bis 32 BPM)
        // bei 44100/48000 Hz bis zu 3750 Frames pro Tick benötigt!
        let num_samples = ((sr * 5) / (2 * bpm)).clamp(1, 8192) as u16;
        state.num_samples = num_samples;
        state.buf_samples = num_samples;
        // 16.16 Fixed-Point Millisekunden pro Tick (0x09C4_0000 / BPM)
        state.time_step = (0x09C4_0000u32 / bpm) as i32;
        state.time_step_rest = 0;
        state.bpm_speed = bpm as u16;
    }

    /// Berechnet vorab die Gesamtlaufzeit (`state.mi.total_time`) und die Pattern-Startzeiten
    /// (`state.pattern_times`) eines `.IT`-Moduls durch schnellen Trockenlauf der Order-/Pattern-Struktur.
    pub fn calc_total_time(it: &ItModuleData, state: &mut EngineState) {
        state.pattern_times = [-1; MAX_PATTERNS];
        if it.orders.is_empty() {
            state.mi.total_time = 0;
            return;
        }

        let mut sim_speed = it.initial_speed.max(1) as u32;
        let mut sim_tempo = (it.initial_tempo.max(32)) as u32;
        let mut play_time_ms: i64 = 0;
        let mut time_rem: u32 = 0;

        let mut visited = vec![[0u8; 128]; it.orders.len().max(1)];
        let mut ord = 0usize;
        let mut row = 0u16;
        let mut pat_loop_row = [0u16; 64];
        let mut pat_loop_cnt = [0u8; 64];
        let mut mem_tempo = [0u8; 64];
        let mut guard = 0usize;

        while ord < it.orders.len() && guard < 300_000 {
            guard += 1;
            let pat_idx = it.orders[ord];
            if pat_idx == 0xFE {
                ord += 1;
                row = 0;
                continue;
            }
            if pat_idx == 0xFF {
                break;
            }
            if ord < MAX_PATTERNS && state.pattern_times[ord] < 0 {
                state.pattern_times[ord] = play_time_ms.min(i32::MAX as i64) as i32;
            }

            let Some(pat) = it.patterns.get(pat_idx as usize) else {
                ord += 1;
                row = 0;
                continue;
            };
            if row >= pat.num_rows {
                ord += 1;
                row = 0;
                pat_loop_row = [0u16; 64];
                pat_loop_cnt = [0u8; 64];
                continue;
            }

            let r_idx = (row as usize).min(1023);
            let mask_byte = r_idx >> 3;
            let mask_bit = 1u8 << (r_idx & 7);
            if (visited[ord][mask_byte] & mask_bit) != 0 {
                state.mi.loop_from = ord as i16;
                break;
            }
            visited[ord][mask_byte] |= mask_bit;

            let mut row_delay: u32 = 0;
            let mut tick_delay: u32 = 0;
            let mut jump_ord: Option<usize> = None;
            let mut break_row: Option<u16> = None;

            for ch in 0..MAX_IT_HOST_CHANNELS {
                let cell = pat.rows[row as usize][ch];
                match cell.command {
                    1 => {
                        // Axx: Set Speed
                        if cell.param > 0 {
                            sim_speed = cell.param as u32;
                        }
                    }
                    2 => {
                        // Bxx: Position Jump
                        jump_ord = Some(cell.param as usize);
                        if break_row.is_none() {
                            break_row = Some(0);
                        }
                    }
                    3 => {
                        // Cxx: Pattern Break
                        break_row = Some(cell.param as u16);
                    }
                    19 => {
                        // Sxy
                        let sub = cell.param >> 4;
                        let val = cell.param & 0x0F;
                        match sub {
                            0x06 => tick_delay += val as u32,
                            0x0B => {
                                if val == 0 {
                                    pat_loop_row[ch] = row;
                                } else if pat_loop_cnt[ch] == 0 {
                                    pat_loop_cnt[ch] = val;
                                    break_row = Some(pat_loop_row[ch]);
                                    jump_ord = Some(ord);
                                    // Erlaube erneutes Durchlaufen der geloopten Zeilen
                                    for r in pat_loop_row[ch]..=row {
                                        let rb = (r as usize).min(1023) >> 3;
                                        let rbit = 1u8 << ((r as usize) & 7);
                                        visited[ord][rb] &= !rbit;
                                    }
                                } else {
                                    pat_loop_cnt[ch] -= 1;
                                    if pat_loop_cnt[ch] > 0 {
                                        break_row = Some(pat_loop_row[ch]);
                                        jump_ord = Some(ord);
                                        for r in pat_loop_row[ch]..=row {
                                            let rb = (r as usize).min(1023) >> 3;
                                            let rbit = 1u8 << ((r as usize) & 7);
                                            visited[ord][rb] &= !rbit;
                                        }
                                    } else {
                                        pat_loop_row[ch] = row + 1;
                                    }
                                }
                            }
                            0x0E => {
                                if row_delay == 0 {
                                    row_delay = val as u32;
                                }
                            }
                            _ => {}
                        }
                    }
                    20 => {
                        // Txx: Set Tempo / Tempo Slide
                        if cell.param != 0 {
                            mem_tempo[ch] = cell.param;
                        }
                        let p = mem_tempo[ch];
                        if p >= 0x20 {
                            sim_tempo = p as u32;
                        } else {
                            let hi = (p >> 4) as i32;
                            let lo = (p & 0x0F) as i32;
                            let ticks = sim_speed.saturating_sub(1) as i32;
                            if hi == 0 {
                                sim_tempo =
                                    (sim_tempo as i32 - lo * ticks).clamp(32, 255) as u32;
                            } else if hi == 1 {
                                sim_tempo =
                                    (sim_tempo as i32 + lo * ticks).clamp(32, 255) as u32;
                            }
                        }
                    }
                    _ => {}
                }
            }

            let total_ticks = sim_speed * (1 + row_delay) + tick_delay;
            let num = total_ticks * 2500 + time_rem;
            play_time_ms += (num / sim_tempo) as i64;
            time_rem = num % sim_tempo;

            if let Some(jo) = jump_ord {
                if jo != ord {
                    pat_loop_row = [0u16; 64];
                    pat_loop_cnt = [0u8; 64];
                }
                ord = jo;
                row = break_row.unwrap_or(0);
            } else if let Some(br) = break_row {
                ord += 1;
                row = br;
                pat_loop_row = [0u16; 64];
                pat_loop_cnt = [0u8; 64];
            } else {
                row += 1;
                if row >= pat.num_rows {
                    ord += 1;
                    row = 0;
                    pat_loop_row = [0u16; 64];
                    pat_loop_cnt = [0u8; 64];
                }
            }
        }

        state.mi.total_time = play_time_ms.min(i32::MAX as i64) as i32;
    }

    /// Springt auf die 1-basierte Order-Position `pattern_1based` (`<<` / `>>` im Player).
    pub fn seek_to_order(&mut self, it: &ItModuleData, state: &mut EngineState, order_1based: u16) {
        if it.orders.is_empty() {
            return;
        }
        let last_valid = it
            .orders
            .iter()
            .rposition(|&o| o < 0xFE)
            .unwrap_or(0);
        let mut target = (order_1based.saturating_sub(1) as usize).min(last_valid);
        while target <= last_valid && it.orders[target] >= 0xFE {
            target += 1;
        }
        if target > last_valid || it.orders[target] >= 0xFE {
            target = last_valid;
        }

        // Übernehme Speed (Axx), Tempo (Txx) und Global Volume (Vxx) aus den vorangehenden Orders 0..target
        let mut sim_speed = it.initial_speed.max(1);
        let mut sim_tempo = (it.initial_tempo as u16).max(32);
        let mut sim_gvol = it.global_vol.min(128) as i32;
        for ord in 0..target {
            let pat_idx = it.orders[ord];
            if pat_idx >= 0xFE {
                continue;
            }
            if let Some(pat) = it.patterns.get(pat_idx as usize) {
                for r in 0..(pat.num_rows as usize) {
                    let mut row_break = false;
                    for ch in 0..MAX_IT_HOST_CHANNELS {
                        let cell = pat.rows[r][ch];
                        match cell.command {
                            1 if cell.param > 0 => sim_speed = cell.param,
                            2 | 3 => row_break = true,
                            20 if cell.param >= 0x20 => sim_tempo = cell.param as u16,
                            22 if cell.param <= 0x80 => sim_gvol = cell.param as i32,
                            _ => {}
                        }
                    }
                    if row_break {
                        break;
                    }
                }
            }
        }
        self.speed = sim_speed;
        self.tempo = sim_tempo;
        self.global_vol = sim_gvol;
        Self::update_tempo_samples(self.tempo, state);

        self.curr_order = target;
        self.curr_row = 0;
        self.curr_tick = 0;
        self.row_delay = 0;
        self.tick_delay = 0;
        self.break_order = None;
        self.break_row = None;
        self.voices.fill(ItVirtualVoice::default());
        for hc in self.host_channels.iter_mut() {
            hc.fg_voice = None;
        }
        if target < MAX_PATTERNS && state.pattern_times[target] >= 0 {
            state.play_time = state.pattern_times[target];
        }
        self.sync_ui_metadata(it, state);
    }

    #[inline]
    fn calc_note_freq(c5_speed: u32, note_0_119: u8) -> u32 {
        let diff = (note_0_119 as f64) - 60.0;
        ((c5_speed.max(1) as f64) * 2.0f64.powf(diff / 12.0))
            .round()
            .clamp(1.0, 2_000_000.0) as u32
    }

    #[inline]
    fn slide_freq(freq: u32, fine_units: i32, linear_slides: bool) -> u32 {
        if fine_units == 0 || freq == 0 {
            return freq;
        }
        if linear_slides {
            // 1 fine unit = 1/64 Halbton = 1/768 Oktave
            let factor = 2.0f64.powf((fine_units as f64) / 768.0);
            ((freq as f64) * factor).round().clamp(1.0, 2_000_000.0) as u32
        } else {
            // Amiga-Perioden-Raum: 1712 * 8363 = 14_317_456
            const AMIGA_CLOCK: f64 = 1712.0 * 8363.0;
            let period = (AMIGA_CLOCK / (freq as f64)) - (fine_units as f64);
            if period <= 1.0 {
                2_000_000
            } else {
                (AMIGA_CLOCK / period).round().clamp(1.0, 2_000_000.0) as u32
            }
        }
    }

    /// Führt NNA (New Note Action) auf der bisherigen Vordergrund-Stimme von `host_idx` aus
    /// und prüft DCT/DCA (Duplicate Check Type / Action) gegen laufende Hintergrund-Stimmen.
    fn apply_nna_and_dct(
        &mut self,
        it: &ItModuleData,
        host_idx: usize,
        new_note: u8,
        new_inst: u8,
        new_smp: u8,
    ) {
        let use_inst = (it.flags & it_flags::USE_INSTRUMENTS) != 0;

        // 1. Bisherige Vordergrund-Stimme über NNA behandeln
        if let Some(fg_idx) = self.host_channels[host_idx].fg_voice {
            if self.voices[fg_idx].active
                && !self.voices[fg_idx].background
                && self.voices[fg_idx].host_chn as usize == host_idx
            {
                let nna = self.voices[fg_idx].nna;
                match nna {
                    0 => {
                        // Cut
                        self.voices[fg_idx].active = false;
                        self.host_channels[host_idx].fg_voice = None;
                    }
                    1 => {
                        // Continue in background
                        self.voices[fg_idx].background = true;
                        self.host_channels[host_idx].fg_voice = None;
                    }
                    2 => {
                        // Note Off in background
                        self.voices[fg_idx].background = true;
                        self.trigger_voice_note_off(it, fg_idx);
                        self.host_channels[host_idx].fg_voice = None;
                    }
                    _ => {
                        // Note Fade in background
                        self.voices[fg_idx].background = true;
                        self.voices[fg_idx].note_fade = true;
                        self.host_channels[host_idx].fg_voice = None;
                    }
                }
            } else {
                self.host_channels[host_idx].fg_voice = None;
            }
        }

        // 2. Duplicate Check (DCT / DCA) im Instrument-Modus
        if use_inst && new_inst > 0 && (new_inst as usize) < it.instruments.len() {
            let inst = &it.instruments[new_inst as usize];
            if inst.dct != 0 {
                for v_idx in 0..MAX_IT_VIRTUAL_VOICES {
                    let v = &self.voices[v_idx];
                    if !v.active || !v.background || v.host_chn as usize != host_idx {
                        continue;
                    }
                    let is_dup = match inst.dct {
                        1 => v.instrument == new_inst && v.note == new_note, // Note
                        2 => v.sample == new_smp && new_smp > 0,             // Sample
                        3 => v.instrument == new_inst,                       // Instrument
                        _ => false,
                    };
                    if is_dup {
                        match inst.dca {
                            0 => {
                                // Cut
                                self.voices[v_idx].active = false;
                            }
                            1 => {
                                // Note Off
                                self.trigger_voice_note_off(it, v_idx);
                            }
                            _ => {
                                // Note Fade
                                self.voices[v_idx].note_fade = true;
                            }
                        }
                    }
                }
            }
        }
    }

    fn trigger_voice_note_off(&mut self, it: &ItModuleData, v_idx: usize) {
        let v = &mut self.voices[v_idx];
        v.note_off = true;
        let use_inst = (it.flags & it_flags::USE_INSTRUMENTS) != 0;
        if use_inst && v.instrument > 0 && (v.instrument as usize) < it.instruments.len() {
            let inst = &it.instruments[v.instrument as usize];
            // In IT startet bei Note-Off sofort das Fadeout, wenn die Vol-Hüllkurve aus ist oder einen Loop hat
            if !v.vol_env.enabled || (inst.vol_env.flags & it_env_flags::LOOP) != 0 {
                v.note_fade = true;
            }
        } else {
            v.note_fade = true;
        }
    }

    /// Allokiert eine freie Virtual Voice (oder übernimmt die leiseste Hintergrund-Stimme)
    /// und bereinigt eventuelle verwaiste `fg_voice`-Verweise auf diesen Slot.
    fn allocate_virtual_voice(&mut self) -> usize {
        let chosen = if let Some(idx) = self.voices.iter().position(|v| !v.active) {
            idx
        } else if let Some((idx, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.background)
            .min_by_key(|(_, v)| v.fvol)
        {
            idx
        } else {
            self.voices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.fvol)
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        };

        for hc in self.host_channels.iter_mut() {
            if hc.fg_voice == Some(chosen) {
                hc.fg_voice = None;
            }
        }
        chosen
    }

    /// Verarbeitet ein MIDI-/Filter-Macro (`Z00..Z7F` über `SFx` oder `Z80..ZFF` über feste `Zxx`-Tabelle).
    fn execute_zxx_filter_macro(&mut self, it: &ItModuleData, host_idx: usize, param: u8) {
        let macro_bytes: [u8; 32] = if param < 0x80 {
            let sfx_idx = (self.host_channels[host_idx].active_sfx_macro & 0x0F) as usize;
            it.midi_config.sfx[sfx_idx]
        } else {
            it.midi_config.zxx[(param - 0x80) as usize]
        };

        // Entferne Leerzeichen und werte IT-System-Macros "F0F000xx" (Cutoff) / "F0F001xx" (Resonance) aus
        let mut clean = [0u8; 32];
        let mut clen = 0usize;
        for &b in &macro_bytes {
            if b == 0 {
                break;
            }
            if !b.is_ascii_whitespace() {
                clean[clen] = b.to_ascii_uppercase();
                clen += 1;
            }
        }
        if clen < 7 || &clean[0..5] != b"F0F00" {
            // Standard-Fallback falls kein Macro-String hinterlegt ist:
            if param < 0x80 {
                let cut = param & 0x7F;
                self.host_channels[host_idx].cutoff = cut;
                for v in self.voices.iter_mut() {
                    if v.active && v.host_chn as usize == host_idx {
                        v.filter.cutoff = cut;
                    }
                }
            } else if param < 0x90 {
                let res = ((param - 0x80) * 8).min(127);
                self.host_channels[host_idx].resonance = res;
                for v in self.voices.iter_mut() {
                    if v.active && v.host_chn as usize == host_idx {
                        v.filter.resonance = res;
                    }
                }
            }
            return;
        }

        let mode = clean[5]; // '0' = Cutoff, '1' = Resonance, '2' = Filter Mode (OpenMPT: 00=LP, 10=HP)
        let val: u8 = if clean[6] == b'Z' {
            param & 0x7F
        } else if clen >= 8 {
            let hi = (clean[6] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (clean[7] as char).to_digit(16).unwrap_or(0) as u8;
            ((hi << 4) | lo).min(127)
        } else {
            param & 0x7F
        };

        if mode == b'0' {
            self.host_channels[host_idx].cutoff = val;
            for v in self.voices.iter_mut() {
                if v.active && v.host_chn as usize == host_idx {
                    v.filter.cutoff = val;
                }
            }
        } else if mode == b'1' {
            self.host_channels[host_idx].resonance = val;
            for v in self.voices.iter_mut() {
                if v.active && v.host_chn as usize == host_idx {
                    v.filter.resonance = val;
                }
            }
        } else if mode == b'2' {
            let hp = val != 0;
            self.host_channels[host_idx].highpass = hp;
            for v in self.voices.iter_mut() {
                if v.active && v.host_chn as usize == host_idx {
                    v.filter.highpass = hp;
                }
            }
        }
    }

    /// Verarbeitet einen einzelnen Tick (`Tick 0` = Row-Trigger oder `Tick 1..Speed-1` = Slide/Modulations-Tick).
    pub fn process_tick(&mut self, it: &ItModuleData, state: &mut EngineState) {
        if self.song_finished || it.orders.is_empty() {
            state.mi.playing = false;
            return;
        }

        // Überspringe `+++` (0xFE) Marker
        while self.curr_order < it.orders.len() && it.orders[self.curr_order] == 0xFE {
            self.curr_order += 1;
        }
        if self.curr_order >= it.orders.len() || it.orders[self.curr_order] == 0xFF {
            state.player_repeats = true;
            if state.repeat_song {
                self.curr_order = 0;
                while self.curr_order < it.orders.len() && it.orders[self.curr_order] == 0xFE {
                    self.curr_order += 1;
                }
                self.curr_row = 0;
                state.play_time = 0;
                state.row_played = [[0u8; 32]; MAX_PATTERNS];
            } else {
                self.song_finished = true;
                state.mi.playing = false;
                return;
            }
        }

        // Loop-Erkennung über `state.row_played` (wie in `finish_pattern_transition` in `sequencer.rs`),
        // sofern aktuell kein Pattern-Loop (`SBx`) aktiv ist:
        if self.curr_tick == 0 && self.row_delay == 0 && (self.curr_row as usize) < 256 {
            let in_pat_loop = self.host_channels.iter().any(|hc| hc.pat_loop_cnt > 0);
            let ord_idx = self.curr_order.min(MAX_PATTERNS - 1);
            let row_u8 = self.curr_row as u8;
            let byte_idx = (row_u8 >> 3) as usize;
            let bit_mask = 1u8 << (row_u8 & 7);
            if !in_pat_loop && (state.row_played[ord_idx][byte_idx] & bit_mask) != 0 {
                state.player_repeats = true;
                state.play_time = state.pattern_times[ord_idx].max(0);
                state.row_played = [[0u8; 32]; MAX_PATTERNS];
                if !state.repeat_song {
                    self.song_finished = true;
                    state.mi.playing = false;
                    return;
                }
            }
            state.row_played[ord_idx][byte_idx] |= bit_mask;
        }

        // Bereinige verwaiste fg_voice-Zeiger und setze temporäre Tick-Modifikatoren zurück
        for (ch, hc) in self.host_channels.iter_mut().enumerate() {
            if let Some(v_idx) = hc.fg_voice {
                if !self.voices[v_idx].active
                    || self.voices[v_idx].background
                    || self.voices[v_idx].host_chn as usize != ch
                {
                    hc.fg_voice = None;
                }
            }
            hc.tick_freq_delta = 0;
            hc.tick_vol_delta = 0;
            hc.tick_pan_delta = 0;
            hc.tremor_mute = false;
        }

        if self.curr_tick == 0 {
            if self.row_delay == 0 {
                self.process_row_tick0(it, state);
            } else {
                // Bei aktivem SEx Pattern-Delay laufen auf den Wiederholungs-Rows die Tick-0-Effekte ohne Re-Trigger
                self.process_effects_tick0_only(it, state);
            }
        } else {
            self.process_non_zero_tick(it, state);
        }

        // Aktualisiere alle aktiven Virtual Voices (Hüllkurven, Auto-Vibrato, Fadeout, Filter, Stereo-Panning)
        self.update_all_virtual_voices(it, state.sound_settings.samplerate);

        // Synchronisiere Kanal- und Song-Metadaten für die MOD4WIN-GUI (VU-Meter, Effekte-Fenster, LCDs)
        self.sync_ui_metadata(it, state);

        // Spielzeit vorrücken (2500 / BPM ms)
        let bpm = (self.tempo.clamp(32, 255)) as i32;
        let num = 2500 + (state.time_step_rest as i32);
        state.play_time += num / bpm;
        state.time_step_rest = (num % bpm) as u16;
        state.mi.curr_time = state.play_time;

        // Tick-/Row-Weiterschaltung für den nächsten Aufruf
        self.curr_tick += 1;
        let target_ticks = self.speed.saturating_add(self.tick_delay);
        if self.curr_tick >= target_ticks {
            self.curr_tick = 0;
            self.tick_delay = 0;

            if self.row_delay > 0 {
                self.row_delay -= 1;
            } else if self.break_order.is_some() || self.break_row.is_some() {
                let prev_ord = self.curr_order;
                let next_ord = self.break_order.take().unwrap_or(self.curr_order + 1);
                let next_row = self.break_row.take().unwrap_or(0);
                self.curr_order = next_ord;
                while self.curr_order < it.orders.len() && it.orders[self.curr_order] == 0xFE {
                    self.curr_order += 1;
                }
                self.curr_row = next_row;
                if self.curr_order != prev_ord {
                    for hc in self.host_channels.iter_mut() {
                        hc.pat_loop_row = 0;
                        hc.pat_loop_cnt = 0;
                    }
                }
            } else {
                self.curr_row += 1;
                let pat_idx = it.orders.get(self.curr_order).copied().unwrap_or(0) as usize;
                let num_rows = it.patterns.get(pat_idx).map(|p| p.num_rows).unwrap_or(64);
                if self.curr_row >= num_rows {
                    self.curr_row = 0;
                    self.curr_order += 1;
                    while self.curr_order < it.orders.len() && it.orders[self.curr_order] == 0xFE {
                        self.curr_order += 1;
                    }
                    for hc in self.host_channels.iter_mut() {
                        hc.pat_loop_row = 0;
                        hc.pat_loop_cnt = 0;
                    }
                }
            }
        }
    }

    fn process_row_tick0(&mut self, it: &ItModuleData, state: &mut EngineState) {
        let pat_idx = it.orders.get(self.curr_order).copied().unwrap_or(0) as usize;
        let Some(pat) = it.patterns.get(pat_idx) else {
            return;
        };
        if self.curr_row >= pat.num_rows {
            self.curr_row = 0;
        }
        let row_cells = pat.rows[self.curr_row as usize];

        for ch in 0..MAX_IT_HOST_CHANNELS {
            let cell = row_cells[ch];
            self.host_channels[ch].cell = cell;

            // Prüfe auf Note Delay (`SDx` mit `x > 0`):
            if cell.command == 19 && (cell.param >> 4) == 0x0D && (cell.param & 0x0F) > 0 {
                continue;
            }

            self.trigger_channel_cell(it, state, ch, cell, false);
        }
    }

    fn process_effects_tick0_only(&mut self, it: &ItModuleData, state: &mut EngineState) {
        for ch in 0..MAX_IT_HOST_CHANNELS {
            let cell = self.host_channels[ch].cell;
            self.apply_volume_column_tick0(it, ch, cell.vol_cmd, false);
            self.apply_command_tick0(it, state, ch, cell.command, cell.param, false);
        }
    }

    fn trigger_channel_cell(
        &mut self,
        it: &ItModuleData,
        state: &mut EngineState,
        ch: usize,
        cell: ItNoteCell,
        from_note_delay: bool,
    ) {
        let use_inst = (it.flags & it_flags::USE_INSTRUMENTS) != 0 && it.instruments.len() > 1;
        let is_tone_porta = cell.command == 7
            || cell.command == 12
            || (193..=202).contains(&cell.vol_cmd);

        // 1. Spezielle Note-Off / Note-Cut / Note-Fade Befehle in der Notenspalte
        if cell.note == 254 {
            // ^^^ Note Cut
            if let Some(v_idx) = self.host_channels[ch].fg_voice {
                self.voices[v_idx].active = false;
                self.host_channels[ch].fg_voice = None;
            }
        } else if cell.note == 255 {
            // === Note Off
            if let Some(v_idx) = self.host_channels[ch].fg_voice {
                self.trigger_voice_note_off(it, v_idx);
            }
        } else if cell.note == 253 {
            // ~~~ Note Fade
            if let Some(v_idx) = self.host_channels[ch].fg_voice {
                self.voices[v_idx].note_fade = true;
            }
        }

        let has_real_note = (1..=120).contains(&cell.note);
        let has_inst = cell.instrument > 0;

        if has_inst {
            self.host_channels[ch].last_inst = cell.instrument;
        }

        // 2. Neue Note oder Instrument-Trigger
        if has_real_note {
            let raw_note = cell.note - 1; // 0..119
            self.host_channels[ch].last_note = raw_note;
            let inst_idx = self.host_channels[ch].last_inst;

            let (mapped_note, smp_idx) = if use_inst {
                if let Some(ins) = it.instruments.get(inst_idx as usize) {
                    ins.note_sample_table
                        .get(raw_note as usize)
                        .copied()
                        .unwrap_or((raw_note, 0))
                } else {
                    (raw_note, 0)
                }
            } else {
                (raw_note, inst_idx)
            };

            if smp_idx > 0 {
                self.host_channels[ch].last_smp = smp_idx;
            }
            let active_smp = self.host_channels[ch].last_smp as usize;
            let c5_speed = it
                .samples
                .get(active_smp)
                .map(|s| s.c5_speed)
                .unwrap_or(8363);
            let target_freq = Self::calc_note_freq(c5_speed, mapped_note);

            let has_active_fg = self.host_channels[ch]
                .fg_voice
                .map(|vi| self.voices[vi].active && !self.voices[vi].background)
                .unwrap_or(false);

            if is_tone_porta && has_active_fg {
                // Tone Portamento (`Gxx`, `Lxy`, `g`): Kein neuer Voice-Trigger, nur Ziel-Frequenz & ggf. Default-Volume/Envelopes setzen
                self.host_channels[ch].porta_target_freq = target_freq;
                if has_inst {
                    if let Some(smp) = it.samples.get(active_smp) {
                        self.host_channels[ch].vol = smp.vol as i32;
                        if let Some(v_idx) = self.host_channels[ch].fg_voice {
                            let same_inst = self.voices[v_idx].instrument == inst_idx;
                            let is_modplug = it.cmwt == 0x0200 && it.cwtv >= 0x0212;
                            self.voices[v_idx].vol = smp.vol as i32;
                            self.voices[v_idx].note_off = false;
                            self.voices[v_idx].note_fade = false;
                            self.voices[v_idx].fadeout_val = 1024;
                            if use_inst && (inst_idx as usize) < it.instruments.len() {
                                let ins = &it.instruments[inst_idx as usize];
                                if (ins.vol_env.flags & it_env_flags::CARRY) == 0
                                    && !(is_modplug && same_inst && !self.voices[v_idx].vol_env.finished)
                                {
                                    self.voices[v_idx].vol_env.reset(&ins.vol_env, 64);
                                }
                                if (ins.pan_env.flags & it_env_flags::CARRY) == 0
                                    && !(is_modplug && same_inst && !self.voices[v_idx].pan_env.finished)
                                {
                                    self.voices[v_idx].pan_env.reset(&ins.pan_env, 0);
                                }
                                if (ins.pitch_env.flags & it_env_flags::CARRY) == 0
                                    && !(is_modplug && same_inst && !self.voices[v_idx].pitch_env.finished)
                                {
                                    self.voices[v_idx].pitch_env.reset(&ins.pitch_env, 0);
                                }
                            }
                        }
                    }
                }
            } else if active_smp > 0 && active_smp < it.samples.len() {
                // Carry-Hüllkurvenzustand der bisherigen Stimme vor NNA sichern
                let prev_env = self.host_channels[ch].fg_voice.and_then(|v_idx| {
                    let pv = &self.voices[v_idx];
                    if pv.active {
                        Some((pv.vol_env, pv.pan_env, pv.pitch_env))
                    } else {
                        None
                    }
                });

                self.apply_nna_and_dct(it, ch, raw_note, inst_idx, active_smp as u8);
                let v_idx = self.allocate_virtual_voice();

                let smp = &it.samples[active_smp];
                let mut nv = ItVirtualVoice::default();
                nv.active = (smp.flags & it_smp_flags::HAS_DATA) != 0 && smp.length > 0;
                nv.background = false;
                nv.host_chn = ch as u8;
                nv.note = mapped_note;
                nv.orig_note = raw_note;
                nv.instrument = inst_idx;
                nv.sample = active_smp as u8;
                nv.sample_pos = 0;
                nv.sample_frac = 0;
                nv.reverse = false;
                nv.base_freq = target_freq;
                nv.freq = target_freq;
                nv.note_off = false;
                nv.note_fade = false;
                nv.fadeout_val = 1024;
                nv.vol = if has_inst {
                    smp.vol as i32
                } else {
                    self.host_channels[ch].vol
                };
                nv.pan = self.host_channels[ch].cpan;

                if has_inst {
                    // 1. Instrument Default Pan (Bit 7 == 0 bedeutet aktiv!)
                    if use_inst && (inst_idx as usize) < it.instruments.len() {
                        let ins = &it.instruments[inst_idx as usize];
                        if (ins.dfp & 0x80) == 0 && nv.pan != 100 {
                            nv.pan = (ins.dfp & 0x7F).min(64) as i32;
                            self.host_channels[ch].cpan = nv.pan;
                        }
                    }

                    // 2. Sample Default Panning (Bit 7 gesetzt = aktiv; überschreibt Instrument Default Pan!)
                    if (smp.dfp & 0x80) != 0 {
                        nv.pan = (smp.dfp & 0x7F).min(64) as i32;
                        self.host_channels[ch].cpan = nv.pan;
                    }
                }

                // 3. Instrument-Einstellungen (NNA, DCT, DCA, Envelopes, PPS, IFC/IFR, Random Vol/Pan)
                if use_inst && (inst_idx as usize) < it.instruments.len() {
                    let ins = &it.instruments[inst_idx as usize];
                    nv.nna = ins.nna;
                    nv.dct = ins.dct;
                    nv.dca = ins.dca;

                    // Pitch-Pan Separation
                    if ins.pps != 0 && nv.pan != 100 {
                        let note_delta = (raw_note as i32) - (ins.ppc as i32);
                        nv.pan = (nv.pan + ((note_delta * (ins.pps as i32)) >> 3)).clamp(0, 64);
                    }
                    // Random Volume & Panning Variation (nur bei neuem Instrumenten-Trigger)
                    if has_inst {
                        if ins.rv > 0 {
                            let r = self.next_random() as i32; // -64..+63
                            let delta = (nv.vol * (ins.rv as i32) * r) >> 12;
                            nv.vol = (nv.vol + delta).clamp(0, 64);
                        }
                        if ins.rp > 0 && nv.pan != 100 {
                            let r = self.next_random() as i32;
                            let delta = ((ins.rp as i32) * r) >> 6;
                            nv.pan = (nv.pan + delta).clamp(0, 64);
                        }

                        // Initial Filter Cutoff (`IFC`) & Resonance (`IFR`)
                        if (ins.ifc & 0x80) != 0 {
                            self.host_channels[ch].cutoff = ins.ifc & 0x7F;
                        }
                        if (ins.ifr & 0x80) != 0 {
                            self.host_channels[ch].resonance = ins.ifr & 0x7F;
                        }
                    }

                    nv.vol_env.reset(&ins.vol_env, 64);
                    nv.pan_env.reset(&ins.pan_env, 0);
                    nv.pitch_env.reset(&ins.pitch_env, 0);

                    if let Some((p_vol, p_pan, p_pit)) = prev_env {
                        if (ins.vol_env.flags & it_env_flags::CARRY) != 0 && !p_vol.finished {
                            nv.vol_env = p_vol;
                        }
                        if (ins.pan_env.flags & it_env_flags::CARRY) != 0 && !p_pan.finished {
                            nv.pan_env = p_pan;
                        }
                        if (ins.pitch_env.flags & it_env_flags::CARRY) != 0 && !p_pit.finished {
                            nv.pitch_env = p_pit;
                        }
                    }
                } else {
                    nv.vol_env.enabled = false;
                    nv.vol_env.value = 64 << 8;
                    nv.pan_env.enabled = false;
                    nv.pitch_env.enabled = false;
                }

                nv.filter.cutoff = self.host_channels[ch].cutoff;
                nv.filter.resonance = self.host_channels[ch].resonance;
                nv.filter.highpass = self.host_channels[ch].highpass;
                nv.surround = nv.pan == 100;

                self.host_channels[ch].vol = nv.vol;
                self.host_channels[ch].porta_target_freq = target_freq;
                self.host_channels[ch].vib_pos = 0;
                self.host_channels[ch].trem_pos = 0;
                self.host_channels[ch].panb_pos = 0;
                self.voices[v_idx] = nv;
                self.host_channels[ch].fg_voice = Some(v_idx);
            }
        } else if has_inst {
            // Instrumentennummer OHNE Note: Setzt die Default-Lautstärke auf der aktiven Stimme
            let active_smp = if use_inst {
                let inst_idx = cell.instrument as usize;
                let rn = self.host_channels[ch].last_note as usize;
                it.instruments
                    .get(inst_idx)
                    .and_then(|ins| ins.note_sample_table.get(rn).map(|&(_, s)| s as usize))
                    .unwrap_or(0)
            } else {
                cell.instrument as usize
            };
            if let Some(smp) = it.samples.get(active_smp) {
                self.host_channels[ch].vol = smp.vol as i32;
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].vol = smp.vol as i32;
                }
            }
        }

        // 3. Volume-Column & Effekt-Spalte (Tick 0)
        self.apply_volume_column_tick0(it, ch, cell.vol_cmd, has_real_note);
        self.apply_command_tick0(it, state, ch, cell.command, cell.param, from_note_delay);
    }

    fn apply_volume_column_tick0(
        &mut self,
        it: &ItModuleData,
        ch: usize,
        vol_cmd: u8,
        _has_note: bool,
    ) {
        match vol_cmd {
            0..=64 => {
                // v00..v64: Set Volume
                let v = vol_cmd as i32;
                self.host_channels[ch].vol = v;
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].vol = v;
                }
            }
            65..=74 => {
                // a0..a9: Fine Volume Slide Up
                let p = vol_cmd - 65;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_slide = p;
                }
                let step = self.host_channels[ch].mem_vol_col_slide as i32;
                self.add_channel_voice_vol(ch, step);
            }
            75..=84 => {
                // b0..b9: Fine Volume Slide Down
                let p = vol_cmd - 75;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_slide = p;
                }
                let step = self.host_channels[ch].mem_vol_col_slide as i32;
                self.add_channel_voice_vol(ch, -step);
            }
            85..=94 => {
                // c0..c9: Volume Slide Up (Speicher auf Tick 0 setzen, Ausführung auf Tick 1+)
                let p = vol_cmd - 85;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_slide = p;
                }
            }
            95..=104 => {
                // d0..d9: Volume Slide Down
                let p = vol_cmd - 95;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_slide = p;
                }
            }
            105..=114 => {
                // e0..e9: Portamento Down (Skalierung * 4)
                let p = (vol_cmd - 105) * 4;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_porta = p;
                }
            }
            115..=124 => {
                // f0..f9: Portamento Up (Skalierung * 4)
                let p = (vol_cmd - 115) * 4;
                if p > 0 {
                    self.host_channels[ch].mem_vol_col_porta = p;
                }
            }
            128..=192 => {
                // p00..p64: Set Panning
                let pan = (vol_cmd - 128) as i32;
                self.host_channels[ch].cpan = pan;
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pan = pan;
                    self.voices[v_idx].surround = false;
                }
            }
            193..=202 => {
                // g0..g9: Tone Portamento (Tabelle: 0, 1, 4, 8, 16, 32, 64, 96, 128, 255)
                const VOL_PORTA_TABLE: [u8; 10] = [0, 1, 4, 8, 16, 32, 64, 96, 128, 255];
                let p = VOL_PORTA_TABLE[(vol_cmd - 193) as usize];
                if p > 0 {
                    if (it.flags & it_flags::LINK_EFFECT_G) != 0 {
                        self.host_channels[ch].mem_porta_up_dn = p;
                        self.host_channels[ch].mem_tone_porta = p;
                    } else {
                        self.host_channels[ch].mem_vol_col_porta = p;
                    }
                }
            }
            203..=212 => {
                // h0..h9: Vibrato Depth
                let dep = vol_cmd - 203;
                if dep > 0 {
                    self.host_channels[ch].mem_vibrato_dep = dep * 4;
                }
            }
            _ => {}
        }
    }

    fn apply_command_tick0(
        &mut self,
        it: &ItModuleData,
        state: &mut EngineState,
        ch: usize,
        command: u8,
        param: u8,
        from_note_delay: bool,
    ) {
        let linear_slides = (it.flags & it_flags::LINEAR_SLIDES) != 0;
        let old_effects = (it.flags & it_flags::OLD_EFFECTS) != 0;

        match command {
            1 => {
                // Axx: Set Speed
                if param > 0 {
                    self.speed = param;
                }
            }
            2 => {
                // Bxx: Jump to Order
                self.break_order = Some(param as usize);
                if self.break_row.is_none() {
                    self.break_row = Some(0);
                }
            }
            3 => {
                // Cxx: Break to Row
                self.break_row = Some(param as u16);
            }
            4 | 11 | 12 => {
                // Dxy (Volume Slide), Kxy (Vib + VolSlide), Lxy (Porta + VolSlide)
                if command == 11 || command == 12 {
                    // Kxy / Lxy nutzen für den Slide-Anteil den Dxy-Speicher
                    if param != 0 {
                        self.host_channels[ch].mem_vol_slide = param;
                    }
                } else if param != 0 {
                    self.host_channels[ch].mem_vol_slide = param;
                }
                let p = self.host_channels[ch].mem_vol_slide;
                let hi = p >> 4;
                let lo = p & 0x0F;
                if lo == 0x0F && hi != 0 {
                    // Fine Volume Slide Up (DF0 ist normaler Slide Down um 15!)
                    self.add_channel_voice_vol(ch, hi as i32);
                } else if hi == 0x0F && lo != 0 {
                    // Fine Volume Slide Down (D0F ist normaler Slide Up um 15! DFF ist Fine Slide Up um 15!)
                    self.add_channel_voice_vol(ch, -(lo as i32));
                } else if old_effects && (hi == 0 || lo == 0) {
                    // In Old-Effects oder bei D0F/DF0? Nein, normale D0x/Dx0 laufen auf Tick 1..N.
                }
            }
            5 => {
                // Exx: Portamento Down
                if param != 0 {
                    self.host_channels[ch].mem_porta_up_dn = param;
                    if (it.flags & it_flags::LINK_EFFECT_G) != 0 {
                        self.host_channels[ch].mem_tone_porta = param;
                    }
                }
                let p = self.host_channels[ch].mem_porta_up_dn;
                let hi = p >> 4;
                let lo = (p & 0x0F) as i32;
                if hi == 0x0F {
                    // EFx: Fine Portamento Down (Tick 0, * 4)
                    self.slide_channel_voice_freq(ch, -lo * 4, linear_slides);
                } else if hi == 0x0E {
                    // EEx: Extra-Fine Portamento Down (Tick 0, * 1)
                    self.slide_channel_voice_freq(ch, -lo, linear_slides);
                }
            }
            6 => {
                // Fxx: Portamento Up
                if param != 0 {
                    self.host_channels[ch].mem_porta_up_dn = param;
                    if (it.flags & it_flags::LINK_EFFECT_G) != 0 {
                        self.host_channels[ch].mem_tone_porta = param;
                    }
                }
                let p = self.host_channels[ch].mem_porta_up_dn;
                let hi = p >> 4;
                let lo = (p & 0x0F) as i32;
                if hi == 0x0F {
                    // FFx: Fine Portamento Up (Tick 0, * 4)
                    self.slide_channel_voice_freq(ch, lo * 4, linear_slides);
                } else if hi == 0x0E {
                    // FEx: Extra-Fine Portamento Up (Tick 0, * 1)
                    self.slide_channel_voice_freq(ch, lo, linear_slides);
                }
            }
            7 => {
                // Gxx: Tone Portamento
                if param != 0 {
                    self.host_channels[ch].mem_tone_porta = param;
                    if (it.flags & it_flags::LINK_EFFECT_G) != 0 {
                        self.host_channels[ch].mem_porta_up_dn = param;
                    }
                }
            }
            8 | 21 => {
                // Hxy (Vibrato) / Uxy (Fine Vibrato)
                let spd = param >> 4;
                let dep = param & 0x0F;
                if spd > 0 {
                    self.host_channels[ch].mem_vibrato_spd = spd;
                }
                if dep > 0 {
                    self.host_channels[ch].mem_vibrato_dep = if command == 21 {
                        dep
                    } else if old_effects {
                        dep * 8
                    } else {
                        dep * 4
                    };
                }
                if !old_effects {
                    self.do_channel_vibrato(ch);
                }
            }
            9 => {
                // Ixy: Tremor
                if param != 0 {
                    self.host_channels[ch].mem_tremor = param;
                }
                self.do_channel_tremor(ch, old_effects);
            }
            10 => {
                // Jxy: Arpeggio
                if param != 0 {
                    self.host_channels[ch].mem_arpeggio = param;
                }
            }
            13 => {
                // Mxx: Set Channel Volume (0..64)
                if param <= 64 {
                    self.host_channels[ch].cvol = param as i32;
                }
            }
            14 => {
                // Nxy: Channel Volume Slide
                if param != 0 {
                    self.host_channels[ch].mem_cvol_slide = param;
                }
                let p = self.host_channels[ch].mem_cvol_slide;
                let hi = p >> 4;
                let lo = p & 0x0F;
                if lo == 0x0F && hi != 0 {
                    self.host_channels[ch].cvol =
                        (self.host_channels[ch].cvol + hi as i32).clamp(0, 64);
                } else if hi == 0x0F && lo != 0 {
                    self.host_channels[ch].cvol =
                        (self.host_channels[ch].cvol - lo as i32).clamp(0, 64);
                }
            }
            15 => {
                // Oxx: Set Sample Offset
                if param != 0 {
                    self.host_channels[ch].mem_sample_offset = param;
                }
                let has_note = (1..=120).contains(&self.host_channels[ch].cell.note);
                if has_note {
                    let ofs = ((self.host_channels[ch].mem_high_offset as i32) << 16)
                        | ((self.host_channels[ch].mem_sample_offset as i32) << 8);
                    if let Some(v_idx) = self.host_channels[ch].fg_voice {
                        let s_idx = self.voices[v_idx].sample as usize;
                        if let Some(smp) = it.samples.get(s_idx) {
                            let len = smp.length as i32;
                            if ofs < len {
                                self.voices[v_idx].sample_pos = ofs;
                                self.voices[v_idx].sample_frac = 0;
                            } else if old_effects
                                && (smp.flags & it_smp_flags::LOOP) != 0
                                && smp.loop_end > smp.loop_begin
                            {
                                self.voices[v_idx].sample_pos = smp.loop_begin as i32;
                                self.voices[v_idx].sample_frac = 0;
                            }
                        }
                    }
                }
            }
            16 => {
                // Pxy: Panning Slide (Hi = Left = -Pan, Lo = Right = +Pan!)
                if param != 0 {
                    self.host_channels[ch].mem_pan_slide = param;
                }
                let p = self.host_channels[ch].mem_pan_slide;
                let hi = p >> 4;
                let lo = p & 0x0F;
                if lo == 0x0F && hi != 0 {
                    // Fine Pan Slide Left
                    self.add_channel_pan(ch, -(hi as i32));
                } else if hi == 0x0F && lo != 0 {
                    // Fine Pan Slide Right
                    self.add_channel_pan(ch, lo as i32);
                }
            }
            17 => {
                // Qxy: Retrig Note
                if param != 0 {
                    self.host_channels[ch].mem_retrig = param;
                }
                if !from_note_delay {
                    self.do_channel_retrig(ch);
                }
            }
            18 => {
                // Rxy: Tremolo
                if param != 0 {
                    self.host_channels[ch].mem_tremolo = param;
                }
                if !old_effects {
                    self.do_channel_tremolo(ch);
                }
            }
            19 => {
                // Sxy: Special Subcommands
                if param != 0 {
                    self.host_channels[ch].mem_special = param;
                }
                let p = self.host_channels[ch].mem_special;
                let sub = p >> 4;
                let val = p & 0x0F;
                match sub {
                    0x03 => {
                        if val <= 3 {
                            self.host_channels[ch].vib_wave = val;
                        }
                    }
                    0x04 => {
                        if val <= 3 {
                            self.host_channels[ch].trem_wave = val;
                        }
                    }
                    0x05 => {
                        if val <= 3 {
                            self.host_channels[ch].panb_wave = val;
                        }
                    }
                    0x06 => {
                        // S6x: Fine Pattern Delay (x Extra-Ticks)
                        self.tick_delay = self.tick_delay.saturating_add(val);
                    }
                    0x07 => {
                        // S7x: NNA / Past-Note / Instrument Envelope Control
                        self.apply_s7x_subcommand(it, ch, val);
                    }
                    0x08 => {
                        // S8x: Set Coarse Panning (0..15 -> 0..64)
                        let pan = (((val as i32) * 64 + 7) / 15).clamp(0, 64);
                        self.host_channels[ch].cpan = pan;
                        if let Some(v_idx) = self.host_channels[ch].fg_voice {
                            self.voices[v_idx].pan = pan;
                            self.voices[v_idx].surround = false;
                        }
                    }
                    0x09 => {
                        // S90: Surround Off | S91: Surround On | S9E: Play Forward | S9F: Play Backward
                        match val {
                            0x00 => {
                                if self.host_channels[ch].cpan == 100 {
                                    self.host_channels[ch].cpan = 32;
                                }
                                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                                    if self.voices[v_idx].pan == 100 {
                                        self.voices[v_idx].pan = 32;
                                    }
                                    self.voices[v_idx].surround = false;
                                }
                            }
                            0x01 => {
                                self.host_channels[ch].cpan = 100;
                                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                                    self.voices[v_idx].pan = 100;
                                    self.voices[v_idx].surround = true;
                                }
                            }
                            0x0E => {
                                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                                    self.voices[v_idx].reverse = false;
                                }
                            }
                            0x0F => {
                                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                                    self.voices[v_idx].reverse = true;
                                    let has_note =
                                        (1..=120).contains(&self.host_channels[ch].cell.note);
                                    if has_note && self.voices[v_idx].sample_pos == 0 {
                                        let s_idx = self.voices[v_idx].sample as usize;
                                        if let Some(smp) = it.samples.get(s_idx) {
                                            let end_pos = if (smp.flags & it_smp_flags::LOOP) != 0
                                                && smp.loop_end > smp.loop_begin
                                            {
                                                (smp.loop_end as i32) - 1
                                            } else {
                                                (smp.length as i32) - 1
                                            };
                                            self.voices[v_idx].sample_pos = end_pos.max(0);
                                            self.voices[v_idx].sample_frac = 0xFFFF;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    0x0A => {
                        // SAx: Set High Sample Offset
                        self.host_channels[ch].mem_high_offset = val;
                    }
                    0x0B => {
                        // SBx: Pattern Loop
                        if val == 0 {
                            self.host_channels[ch].pat_loop_row = self.curr_row;
                        } else if self.host_channels[ch].pat_loop_cnt == 0 {
                            self.host_channels[ch].pat_loop_cnt = val;
                            self.break_row = Some(self.host_channels[ch].pat_loop_row);
                            self.break_order = Some(self.curr_order);
                        } else {
                            self.host_channels[ch].pat_loop_cnt -= 1;
                            if self.host_channels[ch].pat_loop_cnt > 0 {
                                self.break_row = Some(self.host_channels[ch].pat_loop_row);
                                self.break_order = Some(self.curr_order);
                            } else {
                                self.host_channels[ch].pat_loop_row = self.curr_row + 1;
                            }
                        }
                    }
                    0x0C => {
                        // SCx: Note Cut at Tick x (SC0 = Tick 1 in IT!)
                        if val == 0 {
                            // wird auf Tick 1 geschnitten
                        }
                    }
                    0x0E => {
                        // SEx: Pattern Delay (in Zeilen)
                        if self.row_delay == 0 {
                            self.row_delay = val;
                        }
                    }
                    0x0F => {
                        // SFx: Select Parameterized MIDI Macro
                        self.host_channels[ch].active_sfx_macro = val;
                    }
                    _ => {}
                }
            }
            20 => {
                // Txx: Set Tempo / Tempo Slide
                if param != 0 {
                    self.host_channels[ch].mem_tempo = param;
                }
                let p = self.host_channels[ch].mem_tempo;
                if p >= 0x20 {
                    self.tempo = p as u16;
                    Self::update_tempo_samples(self.tempo, state);
                }
            }
            22 => {
                // Vxx: Set Global Volume (0..128)
                if param <= 128 {
                    self.global_vol = param as i32;
                }
            }
            23 => {
                // Wxy: Global Volume Slide
                if param != 0 {
                    self.host_channels[ch].mem_gvol_slide = param;
                }
                let p = self.host_channels[ch].mem_gvol_slide;
                let hi = p >> 4;
                let lo = p & 0x0F;
                if lo == 0x0F && hi != 0 {
                    self.global_vol = (self.global_vol + hi as i32).clamp(0, 128);
                } else if hi == 0x0F && lo != 0 {
                    self.global_vol = (self.global_vol - lo as i32).clamp(0, 128);
                }
            }
            24 => {
                // Xxx: Set Fine Panning (0..255 -> 0..64)
                let pan = (((param as i32) + 2) >> 2).clamp(0, 64);
                self.host_channels[ch].cpan = pan;
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pan = pan;
                    self.voices[v_idx].surround = false;
                }
            }
            25 => {
                // Yxy: Panbrello
                if param != 0 {
                    self.host_channels[ch].mem_panbrello = param;
                }
                self.do_channel_panbrello(ch);
            }
            26 => {
                // Zxx: Filter Cutoff / Resonance / MIDI Macro
                self.execute_zxx_filter_macro(it, ch, param);
            }
            _ => {}
        }
    }

    fn apply_s7x_subcommand(&mut self, it: &ItModuleData, ch: usize, val: u8) {
        match val {
            0x00 => {
                // S70: Past Note Cut
                for v in self.voices.iter_mut() {
                    if v.active && v.background && v.host_chn as usize == ch {
                        v.active = false;
                    }
                }
            }
            0x01 => {
                // S71: Past Note Off
                for v_idx in 0..MAX_IT_VIRTUAL_VOICES {
                    if self.voices[v_idx].active
                        && self.voices[v_idx].background
                        && self.voices[v_idx].host_chn as usize == ch
                    {
                        self.trigger_voice_note_off(it, v_idx);
                    }
                }
            }
            0x02 => {
                // S72: Past Note Fade
                for v in self.voices.iter_mut() {
                    if v.active && v.background && v.host_chn as usize == ch {
                        v.note_fade = true;
                    }
                }
            }
            0x03..=0x06 => {
                // S73..S76: Set NNA to Cut (0), Continue (1), Note Off (2), Note Fade (3)
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].nna = val - 0x03;
                }
            }
            0x07 => {
                // S77: Volume Envelope Off
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].vol_env.enabled = false;
                }
            }
            0x08 => {
                // S78: Volume Envelope On
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].vol_env.enabled = true;
                }
            }
            0x09 => {
                // S79: Panning Envelope Off
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pan_env.enabled = false;
                }
            }
            0x0A => {
                // S7A: Panning Envelope On
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pan_env.enabled = true;
                }
            }
            0x0B => {
                // S7B: Pitch/Filter Envelope Off
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pitch_env.enabled = false;
                }
            }
            0x0C => {
                // S7C: Pitch/Filter Envelope On
                if let Some(v_idx) = self.host_channels[ch].fg_voice {
                    self.voices[v_idx].pitch_env.enabled = true;
                }
            }
            _ => {}
        }
    }

    fn process_non_zero_tick(&mut self, it: &ItModuleData, state: &mut EngineState) {
        let linear_slides = (it.flags & it_flags::LINEAR_SLIDES) != 0;
        let old_effects = (it.flags & it_flags::OLD_EFFECTS) != 0;

        for ch in 0..MAX_IT_HOST_CHANNELS {
            let cell = self.host_channels[ch].cell;

            // 1. Prüfe Note Delay (`SDx`) auf dem Ziel-Tick:
            if cell.command == 19 && (cell.param >> 4) == 0x0D {
                let d_tick = (cell.param & 0x0F).max(1);
                if self.curr_tick == d_tick {
                    self.trigger_channel_cell(it, state, ch, cell, true);
                }
            }

            // 2. Volume-Column Effekte auf Tick 1..N (`c`, `d`, `e`, `f`, `g`, `h`)
            match cell.vol_cmd {
                85..=94 => {
                    // c0..c9: Volume Slide Up
                    let step = self.host_channels[ch].mem_vol_col_slide as i32;
                    self.add_channel_voice_vol(ch, step);
                }
                95..=104 => {
                    // d0..d9: Volume Slide Down
                    let step = self.host_channels[ch].mem_vol_col_slide as i32;
                    self.add_channel_voice_vol(ch, -step);
                }
                105..=114 => {
                    // e0..e9: Portamento Down (mem_vol_col_porta ist bereits * 4 skaliert)
                    let step = self.host_channels[ch].mem_vol_col_porta as i32;
                    self.slide_channel_voice_freq(ch, -step, linear_slides);
                }
                115..=124 => {
                    // f0..f9: Portamento Up (mem_vol_col_porta ist bereits * 4 skaliert)
                    let step = self.host_channels[ch].mem_vol_col_porta as i32;
                    self.slide_channel_voice_freq(ch, step, linear_slides);
                }
                193..=202 => {
                    // g0..g9: Tone Portamento
                    let step = if (it.flags & it_flags::LINK_EFFECT_G) != 0 {
                        self.host_channels[ch].mem_tone_porta
                    } else {
                        self.host_channels[ch].mem_vol_col_porta
                    };
                    self.do_channel_tone_porta(ch, (step as i32) * 4, linear_slides);
                }
                203..=212 => {
                    // h0..h9: Vibrato
                    self.do_channel_vibrato(ch);
                }
                _ => {}
            }

            // 3. Haupt-Effektspalte auf Tick 1..N
            match cell.command {
                4 => {
                    // Dxy: Volume Slide
                    self.do_channel_vol_slide_tickn(ch);
                }
                5 => {
                    // Exx: Portamento Down (nur wenn hi < 0x0E)
                    let p = self.host_channels[ch].mem_porta_up_dn;
                    if (p >> 4) < 0x0E {
                        self.slide_channel_voice_freq(ch, -(p as i32) * 4, linear_slides);
                    }
                }
                6 => {
                    // Fxx: Portamento Up (nur wenn hi < 0x0E)
                    let p = self.host_channels[ch].mem_porta_up_dn;
                    if (p >> 4) < 0x0E {
                        self.slide_channel_voice_freq(ch, (p as i32) * 4, linear_slides);
                    }
                }
                7 => {
                    // Gxx: Tone Portamento
                    let step = (self.host_channels[ch].mem_tone_porta as i32) * 4;
                    self.do_channel_tone_porta(ch, step, linear_slides);
                }
                8 | 21 => {
                    // Hxy / Uxy: Vibrato
                    self.do_channel_vibrato(ch);
                }
                9 => {
                    // Ixy: Tremor
                    self.do_channel_tremor(ch, old_effects);
                }
                10 => {
                    // Jxy: Arpeggio
                    let arp = self.host_channels[ch].mem_arpeggio;
                    let semis = match self.curr_tick % 3 {
                        1 => (arp >> 4) as i32,
                        2 => (arp & 0x0F) as i32,
                        _ => 0,
                    };
                    self.host_channels[ch].tick_freq_delta += semis * 64;
                }
                11 => {
                    // Kxy: Vibrato + Volume Slide
                    self.do_channel_vibrato(ch);
                    self.do_channel_vol_slide_tickn(ch);
                }
                12 => {
                    // Lxy: Tone Portamento + Volume Slide
                    let step = (self.host_channels[ch].mem_tone_porta as i32) * 4;
                    self.do_channel_tone_porta(ch, step, linear_slides);
                    self.do_channel_vol_slide_tickn(ch);
                }
                14 => {
                    // Nxy: Channel Volume Slide
                    let p = self.host_channels[ch].mem_cvol_slide;
                    let hi = p >> 4;
                    let lo = p & 0x0F;
                    if lo == 0 {
                        self.host_channels[ch].cvol =
                            (self.host_channels[ch].cvol + hi as i32).clamp(0, 64);
                    } else if hi == 0 {
                        self.host_channels[ch].cvol =
                            (self.host_channels[ch].cvol - lo as i32).clamp(0, 64);
                    }
                }
                16 => {
                    // Pxy: Panning Slide
                    let p = self.host_channels[ch].mem_pan_slide;
                    let hi = p >> 4;
                    let lo = p & 0x0F;
                    if lo == 0 {
                        self.add_channel_pan(ch, -(hi as i32));
                    } else if hi == 0 {
                        self.add_channel_pan(ch, lo as i32);
                    }
                }
                17 => {
                    // Qxy: Retrig Note
                    self.do_channel_retrig(ch);
                }
                18 => {
                    // Rxy: Tremolo
                    self.do_channel_tremolo(ch);
                }
                19 => {
                    // Sxy: SCx Note Cut
                    let p = self.host_channels[ch].mem_special;
                    if (p >> 4) == 0x0C {
                        let cut_tick = (p & 0x0F).max(1);
                        if self.curr_tick == cut_tick {
                            if let Some(v_idx) = self.host_channels[ch].fg_voice {
                                self.voices[v_idx].active = false;
                                self.host_channels[ch].fg_voice = None;
                            }
                        }
                    }
                }
                20 => {
                    // T0x / T1x: Tempo Slide Down / Up
                    let p = self.host_channels[ch].mem_tempo;
                    if p < 0x20 {
                        let hi = p >> 4;
                        let lo = (p & 0x0F) as i32;
                        if hi == 0 {
                            self.tempo = ((self.tempo as i32) - lo).clamp(32, 255) as u16;
                            Self::update_tempo_samples(self.tempo, state);
                        } else if hi == 1 {
                            self.tempo = ((self.tempo as i32) + lo).clamp(32, 255) as u16;
                            Self::update_tempo_samples(self.tempo, state);
                        }
                    }
                }
                23 => {
                    // Wxy: Global Volume Slide
                    let p = self.host_channels[ch].mem_gvol_slide;
                    let hi = p >> 4;
                    let lo = p & 0x0F;
                    if lo == 0 {
                        self.global_vol = (self.global_vol + hi as i32).clamp(0, 128);
                    } else if hi == 0 {
                        self.global_vol = (self.global_vol - lo as i32).clamp(0, 128);
                    }
                }
                25 => {
                    // Yxy: Panbrello
                    self.do_channel_panbrello(ch);
                }
                _ => {}
            }
        }
    }

    #[inline]
    fn add_channel_voice_vol(&mut self, ch: usize, delta: i32) {
        let nv = (self.host_channels[ch].vol + delta).clamp(0, 64);
        self.host_channels[ch].vol = nv;
        if let Some(v_idx) = self.host_channels[ch].fg_voice {
            self.voices[v_idx].vol = (self.voices[v_idx].vol + delta).clamp(0, 64);
        }
    }

    #[inline]
    fn add_channel_pan(&mut self, ch: usize, delta: i32) {
        if self.host_channels[ch].cpan == 100 {
            self.host_channels[ch].cpan = 32;
        }
        let np = (self.host_channels[ch].cpan + delta).clamp(0, 64);
        self.host_channels[ch].cpan = np;
        if let Some(v_idx) = self.host_channels[ch].fg_voice {
            self.voices[v_idx].pan = np;
            self.voices[v_idx].surround = false;
        }
    }

    #[inline]
    fn slide_channel_voice_freq(&mut self, ch: usize, fine_units: i32, linear_slides: bool) {
        if let Some(v_idx) = self.host_channels[ch].fg_voice {
            let nf = Self::slide_freq(self.voices[v_idx].base_freq, fine_units, linear_slides);
            self.voices[v_idx].base_freq = nf;
            self.voices[v_idx].freq = nf;
        }
    }

    fn do_channel_vol_slide_tickn(&mut self, ch: usize) {
        let p = self.host_channels[ch].mem_vol_slide;
        let hi = p >> 4;
        let lo = p & 0x0F;
        if lo == 0 {
            // Dx0: Slide Up
            self.add_channel_voice_vol(ch, hi as i32);
        } else if hi == 0 {
            // D0x: Slide Down
            self.add_channel_voice_vol(ch, -(lo as i32));
        }
    }

    fn do_channel_tone_porta(&mut self, ch: usize, fine_units: i32, linear_slides: bool) {
        let target = self.host_channels[ch].porta_target_freq;
        let Some(v_idx) = self.host_channels[ch].fg_voice else {
            return;
        };
        if target == 0 || fine_units == 0 {
            return;
        }
        let cur = self.voices[v_idx].base_freq;
        if cur < target {
            let nf = Self::slide_freq(cur, fine_units, linear_slides).min(target);
            self.voices[v_idx].base_freq = nf;
            self.voices[v_idx].freq = nf;
        } else if cur > target {
            let nf = Self::slide_freq(cur, -fine_units, linear_slides).max(target);
            self.voices[v_idx].base_freq = nf;
            self.voices[v_idx].freq = nf;
        }
    }

    fn eval_oscillator(&mut self, wave: u8, pos: u8) -> i32 {
        match wave & 3 {
            0 => IT_FINE_SINE_TABLE[pos as usize] as i32,
            1 => 64 - (((pos as i32) * 128) >> 8), // Ramp Down (+64 .. -64)
            2 => {
                if pos < 128 {
                    64
                } else {
                    -64
                }
            } // Square
            _ => self.next_random() as i32,        // Random
        }
    }

    fn do_channel_vibrato(&mut self, ch: usize) {
        let wave = self.host_channels[ch].vib_wave;
        let pos = self.host_channels[ch].vib_pos;
        let spd = self.host_channels[ch].mem_vibrato_spd;
        let dep = self.host_channels[ch].mem_vibrato_dep as i32;
        let val = self.eval_oscillator(wave, pos);
        // Skaliere auf 1/64-Halbton-Einheiten
        self.host_channels[ch].tick_freq_delta += (val * dep) >> 6;
        self.host_channels[ch].vib_pos = pos.wrapping_add(spd.wrapping_mul(4));
    }

    fn do_channel_tremolo(&mut self, ch: usize) {
        let wave = self.host_channels[ch].trem_wave;
        let pos = self.host_channels[ch].trem_pos;
        let p = self.host_channels[ch].mem_tremolo;
        let spd = p >> 4;
        let dep = (p & 0x0F) as i32;
        let val = self.eval_oscillator(wave, pos);
        self.host_channels[ch].tick_vol_delta += (val * dep) >> 6;
        self.host_channels[ch].trem_pos = pos.wrapping_add(spd.wrapping_mul(4));
    }

    fn do_channel_panbrello(&mut self, ch: usize) {
        let wave = self.host_channels[ch].panb_wave;
        let pos = self.host_channels[ch].panb_pos;
        let p = self.host_channels[ch].mem_panbrello;
        let spd = p >> 4;
        let dep = (p & 0x0F) as i32;
        let val = self.eval_oscillator(wave, pos);
        self.host_channels[ch].tick_pan_delta += (val * dep) >> 6;
        self.host_channels[ch].panb_pos = pos.wrapping_add(spd);
    }

    fn do_channel_tremor(&mut self, ch: usize, old_effects: bool) {
        let p = self.host_channels[ch].mem_tremor;
        let add = if old_effects { 1 } else { 0 };
        let on_ticks = ((p >> 4) + add).max(1);
        let off_ticks = ((p & 0x0F) + add).max(1);
        let cycle = on_ticks.saturating_add(off_ticks);
        let cnt = self.host_channels[ch].tremor_cnt % cycle;
        self.host_channels[ch].tremor_mute = cnt >= on_ticks;
        self.host_channels[ch].tremor_cnt = cnt.wrapping_add(1);
    }

    fn do_channel_retrig(&mut self, ch: usize) {
        let p = self.host_channels[ch].mem_retrig;
        let vol_cmd = p >> 4;
        let interval = (p & 0x0F).max(1);
        if self.host_channels[ch].retrig_cnt > 0 {
            self.host_channels[ch].retrig_cnt -= 1;
            return;
        }
        self.host_channels[ch].retrig_cnt = interval - 1;

        let Some(v_idx) = self.host_channels[ch].fg_voice else {
            return;
        };
        self.voices[v_idx].sample_pos = 0;
        self.voices[v_idx].sample_frac = 0;
        self.voices[v_idx].reverse = false;

        let mut v = self.voices[v_idx].vol;
        v = match vol_cmd {
            1 => v - 1,
            2 => v - 2,
            3 => v - 4,
            4 => v - 8,
            5 => v - 16,
            6 => (v * 2) / 3,
            7 => v / 2,
            9 => v + 1,
            10 => v + 2,
            11 => v + 4,
            12 => v + 8,
            13 => v + 16,
            14 => (v * 3) / 2,
            15 => v * 2,
            _ => v,
        }
        .clamp(0, 64);
        self.voices[v_idx].vol = v;
        self.host_channels[ch].vol = v;
    }

    /// Aktualisiert auf jedem Tick alle aktiven Virtual Voices (Hüllkurven, Fadeout, Auto-Vibrato,
    /// Pitch-Modulation, Stereo-Lautstärken und 2-Pole-Filterkoeffizienten).
    fn update_all_virtual_voices(&mut self, it: &ItModuleData, sample_rate: i32) {
        let use_inst = (it.flags & it_flags::USE_INSTRUMENTS) != 0 && it.instruments.len() > 1;
        let linear_slides = (it.flags & it_flags::LINEAR_SLIDES) != 0;
        let is_stereo = (it.flags & it_flags::STEREO) != 0;
        let pan_sep = it.pan_sep.min(128) as i32;

        for v_idx in 0..MAX_IT_VIRTUAL_VOICES {
            if !self.voices[v_idx].active {
                continue;
            }

            let host_idx = (self.voices[v_idx].host_chn as usize).min(MAX_IT_HOST_CHANNELS - 1);
            let is_fg = self.host_channels[host_idx].fg_voice == Some(v_idx);
            let inst_idx = self.voices[v_idx].instrument as usize;
            let smp_idx = self.voices[v_idx].sample as usize;

            let Some(smp) = it.samples.get(smp_idx) else {
                self.voices[v_idx].active = false;
                continue;
            };

            let (ins_gbv, ins_fadeout, is_filter_env) =
                if use_inst && inst_idx < it.instruments.len() {
                    let ins = &it.instruments[inst_idx];
                    let note_off = self.voices[v_idx].note_off;
                    self.voices[v_idx].vol_env.step(&ins.vol_env, note_off, 64);
                    self.voices[v_idx].pan_env.step(&ins.pan_env, note_off, 0);
                    self.voices[v_idx]
                        .pitch_env
                        .step(&ins.pitch_env, note_off, 0);

                    if self.voices[v_idx].vol_env.finished {
                        if self.voices[v_idx].vol_env.value <= 0 {
                            self.voices[v_idx].active = false;
                            if is_fg {
                                self.host_channels[host_idx].fg_voice = None;
                            }
                            continue;
                        }
                        self.voices[v_idx].note_fade = true;
                    }
                    (
                        ins.gbv as i32,
                        ins.fadeout as i32,
                        (ins.pitch_env.flags & it_env_flags::FILTER) != 0,
                    )
                } else {
                    (128, 0, false)
                };

            // Fadeout-Berechnung
            if self.voices[v_idx].note_fade {
                self.voices[v_idx].fadeout_val =
                    (self.voices[v_idx].fadeout_val - ins_fadeout).max(0);
                if self.voices[v_idx].fadeout_val <= 0 {
                    self.voices[v_idx].active = false;
                    if is_fg {
                        self.host_channels[host_idx].fg_voice = None;
                    }
                    continue;
                }
            }

            // Sample Auto-Vibrato
            let mut auto_vib_units: i32 = 0;
            if smp.vid > 0 && smp.vis > 0 {
                let target_depth = (smp.vid as u32) << 8;
                if smp.vir == 0 {
                    self.voices[v_idx].auto_vib_depth = target_depth;
                } else {
                    self.voices[v_idx].auto_vib_depth = (self.voices[v_idx].auto_vib_depth
                        + smp.vir as u32)
                        .min(target_depth);
                }
                let av_pos = self.voices[v_idx].auto_vib_pos;
                let av_wave = self.eval_oscillator(smp.vit, av_pos);
                let cur_dep = (self.voices[v_idx].auto_vib_depth >> 8) as i32;
                auto_vib_units = (av_wave * cur_dep) >> 6;
                self.voices[v_idx].auto_vib_pos = av_pos.wrapping_add(smp.vis);
            }

            // Pitch- / Filter-Envelope & Frequenz-Modulation
            let mut total_freq_units = auto_vib_units;
            if is_fg {
                total_freq_units += self.host_channels[host_idx].tick_freq_delta;
            }
            if self.voices[v_idx].pitch_env.enabled {
                let p_val_256 = self.voices[v_idx].pitch_env.value; // (-32..+32) << 8
                if is_filter_env {
                    // Filter-Envelope moduliert die effektive Cutoff-Skalierung (0..255)
                    let norm = (((p_val_256 >> 8) + 32) * 255 / 64).clamp(0, 255) as u8;
                    self.voices[v_idx].filter.env_cutoff = norm;
                } else {
                    // Pitch-Envelope: -32..+32 entspricht -16..+16 Halbtönen (1 Einheit = 1/64 Halbton -> * 32)
                    total_freq_units += p_val_256 >> 3;
                    self.voices[v_idx].filter.env_cutoff = 255;
                }
            } else {
                self.voices[v_idx].filter.env_cutoff = 255;
            }

            let base_f = self.voices[v_idx].base_freq;
            self.voices[v_idx].freq = Self::slide_freq(base_f, total_freq_units, linear_slides);

            // End-Lautstärke (`fvol` in 0..=16384)
            let hc_muted = self.host_channels[host_idx].muted
                || (is_fg && self.host_channels[host_idx].tremor_mute);
            if hc_muted {
                self.voices[v_idx].fvol = 0;
                self.voices[v_idx].l_vol = 0;
                self.voices[v_idx].r_vol = 0;
            } else {
                let mut v = self.voices[v_idx].vol;
                if is_fg {
                    v = (v + self.host_channels[host_idx].tick_vol_delta).clamp(0, 64);
                }
                let smp_gvl = smp.gvl as i32;
                let hc_vol = self.host_channels[host_idx].cvol;
                let g_vol = self.global_vol;
                let env_vol_256 = self.voices[v_idx].vol_env.value.clamp(0, 64 << 8);
                let fade = self.voices[v_idx].fadeout_val; // 0..=1024

                // v1 in 0..=4096 (64 * 64 * 128 >> 7 = 4096 = 2^12)
                let v1 = (v * smp_gvl * ins_gbv) >> 7;
                // v2 in 0..=4096 (4096 * 64 * 128 >> 13 = 4096 = 2^12)
                let v2 = (v1 * hc_vol * g_vol) >> 13;
                // fvol in 0..=16384 (2^12 * 2^14 * 2^10 = 2^36; >> 22 = 2^14 = 16384)
                let fvol = ((v2 as i64 * env_vol_256 as i64 * fade as i64) >> 22)
                    .clamp(0, 16384) as i32;
                self.voices[v_idx].fvol = fvol;

                // Panning & Stereo-Aufteilung
                let raw_pan = self.voices[v_idx].pan;
                if raw_pan == 100 || self.voices[v_idx].surround {
                    self.voices[v_idx].surround = true;
                    self.voices[v_idx].l_vol = fvol >> 1;
                    self.voices[v_idx].r_vol = fvol >> 1;
                } else {
                    self.voices[v_idx].surround = false;
                    let mut p = raw_pan.clamp(0, 64);
                    if is_fg {
                        p = (p + self.host_channels[host_idx].tick_pan_delta).clamp(0, 64);
                    }
                    if self.voices[v_idx].pan_env.enabled {
                        let pe = self.voices[v_idx].pan_env.value >> 8; // -32..+32
                        p = (p + ((pe * (32 - (p - 32).abs())) >> 5)).clamp(0, 64);
                    }
                    if !is_stereo {
                        p = 32;
                    } else {
                        p = (32 + (((p - 32) * pan_sep) >> 7)).clamp(0, 64);
                    }
                    self.voices[v_idx].l_vol = (fvol * (64 - p)) >> 6;
                    self.voices[v_idx].r_vol = (fvol * p) >> 6;
                }
            }

            // Resonanzfilter-Koeffizienten aktualisieren
            self.voices[v_idx].filter.update_coefficients(sample_rate);
        }
    }

    /// Synchronisiert die ersten 32 Host-Kanäle und die Song-Position in `EngineState`,
    /// damit alle MOD4WIN-Fenster (Hauptfenster, Effekte-Fenster `ana.pas`, Info-Fenster)
    /// während der `.IT`-Wiedergabe exakt animiert werden.
    pub fn sync_ui_metadata(&self, it: &ItModuleData, state: &mut EngineState) {
        state.mi.curr_pttrn = (self.curr_order + 1).min(u16::MAX as usize) as u16;
        state.mi.curr_line = self.curr_row + 1;
        state.mi.curr_spd = self.speed;
        state.mi.curr_bpm = self.tempo.clamp(32, 255) as u8;
        state.speed = self.speed;
        state.bpm_speed = self.tempo.clamp(32, 255);
        state.curr_pattern = self.curr_order.min(u16::MAX as usize) as u16;
        state.curr_note = self.curr_row;
        state.curr_vblanc = self.curr_tick as u16;

        for ch in 0..MAX_M4W_CHANNELS {
            let hc = &self.host_channels[ch];
            let v_opt = hc.fg_voice.and_then(|idx| {
                let v = &self.voices[idx];
                if v.active {
                    Some(v)
                } else {
                    None
                }
            });

            if let Some(v) = v_opt {
                let l64 = ((v.l_vol * 64) >> 14).clamp(0, 64) as u8;
                let r64 = ((v.r_vol * 64) >> 14).clamp(0, 64) as u8;
                let avg64 = ((v.fvol * 64) >> 14).clamp(0, 64) as u8;
                let inst_num = if (it.flags & it_flags::USE_INSTRUMENTS) != 0 {
                    v.instrument
                } else {
                    v.sample
                };
                let note_num = v.orig_note.saturating_add(1).min(96);
                state.channels[ch].l_vol = l64;
                state.channels[ch].r_vol = r64;
                state.channels[ch].avg_vol = avg64;
                state.channels[ch].note = if avg64 > 0 { 1 } else { 0 };
                state.channels[ch].inst = inst_num;
                state.channels[ch].note_val = note_num;
                state.mi.curr_vols[ch] = avg64;
                state.mi.curr_instr[ch] = inst_num;
                state.mi.curr_notes[ch] = note_num;
            } else {
                state.channels[ch].l_vol = 0;
                state.channels[ch].r_vol = 0;
                state.channels[ch].avg_vol = 0;
                state.channels[ch].note = 0;
                state.channels[ch].inst = 0;
                state.channels[ch].note_val = 0;
                state.mi.curr_vols[ch] = 0;
                state.mi.curr_notes[ch] = 0;
                state.mi.curr_instr[ch] = 0;
            }

            // Aktiver Effekt für das Effekte-Fenster (`ana.pas`)
            let eff_opt = map_it_command_to_m4w_effect(hc.cell.command, hc.cell.param)
                .map(|eid| (eid, hc.cell.param))
                .or_else(|| {
                    map_it_vol_cmd_to_m4w_effect(hc.cell.vol_cmd).map(|eid| (eid, hc.cell.vol_cmd))
                });
            if let Some((eid, op)) = eff_opt {
                state.mi.curr_effects[ch] = ((eid as u16) << 8) | (op as u16);
                state.channels[ch].effkt = eid;
                state.channels[ch].opnd = op;
            } else {
                state.mi.curr_effects[ch] = 0xFF00;
                state.channels[ch].effkt = 0xFF;
                state.channels[ch].opnd = 0;
            }
        }
    }

    // ========================================================================
    // Duale Audio-/Mixer-Architektur (`MOD4WIN IDO 0..3` vs. `IT2 Native 4..6`)
    // ========================================================================

    /// Mischt einen Tick aller aktiven Virtual Voices (bis zu 256) in den 32-Bit-Mischpuffer
    /// und wendet je nach `state.sound_settings.oversamp`:
    /// - **`0..=3` (`MOD4WIN IDO 0..3`)**: Das klassische MOD4WIN-Resampling (Non-Interp, Linear IDO,
    ///   CPU Cubic, FPU Cubic) inklusive Click-Removal-Rampe (`@SlideOut`), ProLogic-Surround-Ringpuffer
    ///   und `@MixOut`-Clipping an.
    /// - **`4..=6` (`IT2 Native`)**: Authentisches Impulse-Tracker-Resampling (`4` = IT Non-Interpolated,
    ///   `5` = IT Linear nach `M32BITI.ASM`, `6` = IT Cubic Spline nach `it2play`) ohne globales
    ///   MOD4WIN-Post-Filtering an.
    /// In **beiden** Welten durchläuft jede Stimme ihren eigenen 2-Pole-Resonanzfilter (`Zxx`/`IFC`/`IFR`/Filter-Env).
    pub fn mix_tick_block(
        &mut self,
        it: &ItModuleData,
        state: &mut EngineState,
        mem: &mut DacMixMemory,
    ) -> MixOutputBuffer {
        let mut remaining = state.buf_samples as usize;
        if remaining == 0 {
            return MixOutputBuffer::Pcm16(Vec::new());
        }
        if remaining <= MAX_MIX_BUF_SAMPLES {
            return self.mix_sub_block(it, state, mem, remaining);
        }

        let mut first_out: Option<MixOutputBuffer> = None;
        while remaining > 0 {
            let chunk = remaining.min(MAX_MIX_BUF_SAMPLES);
            remaining -= chunk;
            let part = self.mix_sub_block(it, state, mem, chunk);
            match (&mut first_out, part) {
                (None, p) => first_out = Some(p),
                (Some(MixOutputBuffer::Pcm16(acc)), MixOutputBuffer::Pcm16(more)) => {
                    acc.extend_from_slice(&more);
                }
                (Some(MixOutputBuffer::Pcm8(acc)), MixOutputBuffer::Pcm8(more)) => {
                    acc.extend_from_slice(&more);
                }
                _ => {}
            }
        }
        first_out.unwrap_or(MixOutputBuffer::Pcm16(Vec::new()))
    }

    fn mix_sub_block(
        &mut self,
        it: &ItModuleData,
        state: &mut EngineState,
        mem: &mut DacMixMemory,
        buf_samples: usize,
    ) -> MixOutputBuffer {
        let buf_samples = buf_samples.min(MAX_MIX_BUF_SAMPLES);
        if buf_samples == 0 {
            return MixOutputBuffer::Pcm16(Vec::new());
        }

        let oversamp_mode = state.sound_settings.oversamp;
        let is_m4w_ido_world = oversamp_mode <= 3;
        let total_frames = if is_m4w_ido_world {
            buf_samples + 1 // +1 Stützframe für @SlideOut Click-Removal in der MOD4WIN-Welt
        } else {
            buf_samples
        };

        for frame in mem.mix_buffer.iter_mut().take(total_frames) {
            *frame = StereoSample32::default();
        }
        if is_m4w_ido_world && state.sound_settings.surround {
            for frame in mem.sur_buffer.iter_mut().take(total_frames) {
                *frame = StereoSample32::default();
            }
        }

        let sr = state.sound_settings.samplerate.max(4000) as u64;
        let raw_preamp = (state.sound_settings.pre_amp & 0x7F).min(40) as i32;
        // PreAmp-Faktor (bei 20 = 0 dB = 1.0x, wie in CalcVolMaster)
        let preamp_factor = 10.0f64.powf(((raw_preamp - 20) as f64) / 20.0);
        // In Impulse Tracker 2.14 / it2play / libopenmpt entspricht MixVolume = 0..128
        // einer linearen Stimmen-Skalierung von mix_vol / 128.0.
        let mix_vol_scale = ((it.mix_vol.max(1) as f64) / 128.0) * preamp_factor;
        let master_scale = (mix_vol_scale * 256.0).round() as i64;

        for v_idx in 0..MAX_IT_VIRTUAL_VOICES {
            if !self.voices[v_idx].active {
                continue;
            }
            let smp_idx = self.voices[v_idx].sample as usize;
            let Some(smp) = it.samples.get(smp_idx) else {
                self.voices[v_idx].active = false;
                continue;
            };
            if smp.data_16.is_empty() || smp.length == 0 {
                self.voices[v_idx].active = false;
                continue;
            }

            let step_16_16 = (((self.voices[v_idx].freq as u64) << 16) / sr) as u32;
            let silent = self.voices[v_idx].fvol <= 0;

            if silent && (it.flags & it_flags::VOL0_OPTIMIZATIONS) != 0 {
                self.advance_voice_silent(v_idx, smp, step_16_16, buf_samples);
                continue;
            }

            let l_gain = ((self.voices[v_idx].l_vol as i64 * master_scale) >> 14) as i32;
            let r_gain = ((self.voices[v_idx].r_vol as i64 * master_scale) >> 14) as i32;
            let is_surr = self.voices[v_idx].surround;
            let route_m4w_surr = is_m4w_ido_world && state.sound_settings.surround && is_surr;

            let target_buf: &mut [StereoSample32] = if route_m4w_surr {
                &mut mem.sur_buffer[..total_frames]
            } else {
                &mut mem.mix_buffer[..total_frames]
            };

            let pcm_l = &smp.data_16;
            let pcm_r = if (smp.flags & it_smp_flags::STEREO) != 0 && !smp.data_16_right.is_empty()
            {
                Some(&smp.data_16_right)
            } else {
                None
            };

            for (f_idx, frame) in target_buf.iter_mut().enumerate() {
                if !self.voices[v_idx].active {
                    break;
                }
                let pos = self.voices[v_idx].sample_pos;
                let frac = self.voices[v_idx].sample_frac as i32; // 0..65535

                let raw_l = Self::resample_point(pcm_l, smp, pos, frac, oversamp_mode) as f32;
                let filt_l = self.voices[v_idx].filter.process_left(raw_l) as i32;

                let filt_r = if let Some(right_data) = pcm_r {
                    let raw_r =
                        Self::resample_point(right_data, smp, pos, frac, oversamp_mode) as f32;
                    self.voices[v_idx].filter.process_right(raw_r) as i32
                } else {
                    filt_l
                };

                if !silent {
                    let out_l = filt_l.wrapping_mul(l_gain);
                    let out_r = if is_surr && !route_m4w_surr {
                        // In IT Native Surround (S91) ist der rechte Kanal um 180° phasengedreht (`M32BITI.ASM`)
                        -filt_r.wrapping_mul(r_gain)
                    } else {
                        filt_r.wrapping_mul(r_gain)
                    };
                    frame.left = frame.left.wrapping_add(out_l);
                    frame.right = frame.right.wrapping_add(out_r);
                }

                // Beim Zusatz-Frame `buf_samples` (für @SlideOut) die Phase nicht ein weiteres Mal vorrücken
                if f_idx < buf_samples {
                    self.step_voice_phase(v_idx, smp, step_16_16);
                }
            }
        }

        if is_m4w_ido_world {
            // Klassische MOD4WIN Click-Removal-Rampe (`@SlideOut`) + ProLogic-Surround-Ringpuffer
            apply_click_removal_slide(
                &mut mem.mix_buffer,
                buf_samples,
                state.num_samples,
                &mut state.l_fade_out,
                &mut state.r_fade_out,
            );
            if state.sound_settings.surround {
                apply_click_removal_slide(
                    &mut mem.sur_buffer,
                    buf_samples,
                    state.num_samples,
                    &mut state.l_sur_fade_out,
                    &mut state.r_sur_fade_out,
                );
                process_surround_buffer(
                    &mut mem.mix_buffer,
                    &mut mem.sur_buffer,
                    &mut mem.surr_ring,
                    buf_samples,
                    &mut state.buf_start,
                    &mut state.buf_end,
                );
            }
            apply_post_filters(state, mem, buf_samples);
        }

        mix_out_and_clip(
            &mem.mix_buffer,
            buf_samples,
            state.sound_settings.bits,
            state.sound_settings.stereo,
            state.sound_settings.pre_amp | 0x80,
            mem.peak_search,
            &mut state.max_level,
        )
    }

    #[inline]
    fn get_clamped_sample(pcm: &[i16], smp: &ItSample, idx: i32) -> i32 {
        if pcm.is_empty() {
            return 0;
        }
        let len = pcm.len() as i32;
        if idx < 0 {
            return pcm[0] as i32;
        }
        if idx >= len {
            if (smp.flags & it_smp_flags::LOOP) != 0 && smp.loop_end > smp.loop_begin {
                let lb = (smp.loop_begin as i32).clamp(0, len - 1);
                let le = (smp.loop_end as i32).clamp(lb + 1, len);
                let wrap = lb + ((idx - lb).rem_euclid(le - lb));
                return pcm[wrap as usize] as i32;
            }
            return 0;
        }
        pcm[idx as usize] as i32
    }

    /// Wertet den Sample-Wert an Position `(pos, frac_0_65535)` gemäß der gewählten Resampling-Stufe aus:
    /// - `0` (`MOD4WIN IDO 0`) / `4` (`IT Native Off`): Zero-Order Hold (Non-Interpolated)
    /// - `1` (`MOD4WIN IDO 1`): Lineares MOD4WIN-IDO-Resampling (`dacmix.pas` `@S16o1`)
    /// - `2` (`MOD4WIN IDO 2`): 32-Bit Integer Cubic-Spline (`dacmix.pas` `@Cs16HiS`)
    /// - `3` (`MOD4WIN IDO 3`): x87 FPU Cubic-Spline (`dacmix.pas` `@fCs16HiS`)
    /// - `5` (`IT Native Linear`): Lineares 16-Bit IT2.14-Resampling (`M32BITI.ASM` `Get32Bit16Waveform`)
    /// - `6` (`IT Native Cubic`): 4-Tap Cubic-Spline Resampling nach `it2play`
    #[inline]
    fn resample_point(
        pcm: &[i16],
        smp: &ItSample,
        pos: i32,
        frac: i32,
        oversamp_mode: u8,
    ) -> i32 {
        match oversamp_mode {
            0 | 4 => Self::get_clamped_sample(pcm, smp, pos),
            1 => {
                // MOD4WIN IDO 1 (Linear IDO)
                let s0 = Self::get_clamped_sample(pcm, smp, pos);
                let s1 = Self::get_clamped_sample(pcm, smp, pos + 1);
                s0 + (((s1 - s0) * frac) >> 16)
            }
            2 => {
                // MOD4WIN IDO 2 (32-Bit Integer Cubic-Spline aus `@Cs16HiS` in `dacmix.pas`)
                let ym1 = Self::get_clamped_sample(pcm, smp, pos - 1);
                let y0 = Self::get_clamped_sample(pcm, smp, pos);
                let y1 = Self::get_clamped_sample(pcm, smp, pos + 1);
                let y2 = Self::get_clamped_sample(pcm, smp, pos + 2);

                let ebp_d = y0;
                let mut ebx = y0 - y1;
                let ecx_c = y1 - ym1;
                let mut eax = y2 + ebx * 2;
                ebx -= ym1;
                eax += ebx;
                ebx += eax;
                eax = (eax + 4) >> 3;
                eax = (eax * frac + 4096) >> 13;
                ebx += ebp_d;
                eax -= ebx;
                eax = (eax + 4) >> 3;
                eax = (eax * frac + 4096) >> 13;
                eax += ecx_c;
                eax = (eax + 4) >> 3;
                eax = (eax * frac + 8192) >> 14;
                eax + ebp_d
            }
            3 => {
                // MOD4WIN IDO 3 (x87 FPU Cubic-Spline aus `@fCs16HiS` in `dacmix.pas`)
                let ym1 = Self::get_clamped_sample(pcm, smp, pos - 1) as f64;
                let y0 = Self::get_clamped_sample(pcm, smp, pos) as f64;
                let y1 = Self::get_clamped_sample(pcm, smp, pos + 1) as f64;
                let y2 = Self::get_clamped_sample(pcm, smp, pos + 2) as f64;

                let a = (y0 - y1) * 3.0 - ym1 + y2;
                let b = y0 * 5.0 - y1 * 4.0 - ym1 * 2.0 + y2;
                let c = y1 - ym1;
                let x = (frac as f64) * (1.0 / 65536.0);
                let poly = (((a * x - b) * x + c) * x + y0 * 2.0) * 0.5;
                poly.round() as i32
            }
            5 => {
                // IT Native Linear (`Get32Bit16Waveform` aus `legacy_src/IT/M32BITI.ASM`)
                let s0 = Self::get_clamped_sample(pcm, smp, pos);
                let s1 = Self::get_clamped_sample(pcm, smp, pos + 1);
                s0 + (((s1 - s0) * (frac >> 1)) >> 15)
            }
            _ => {
                // IT Native Cubic Spline (Catmull-Rom / it2play 4-Tap Cubic)
                let ym1 = Self::get_clamped_sample(pcm, smp, pos - 1) as f64;
                let y0 = Self::get_clamped_sample(pcm, smp, pos) as f64;
                let y1 = Self::get_clamped_sample(pcm, smp, pos + 1) as f64;
                let y2 = Self::get_clamped_sample(pcm, smp, pos + 2) as f64;
                let t = (frac as f64) * (1.0 / 65536.0);

                let c0 = y0;
                let c1 = 0.5 * (y1 - ym1);
                let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
                let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
                (((c3 * t + c2) * t + c1) * t + c0).round() as i32
            }
        }
    }

    fn advance_voice_silent(
        &mut self,
        v_idx: usize,
        smp: &ItSample,
        step_16_16: u32,
        frames: usize,
    ) {
        for _ in 0..frames {
            if !self.voices[v_idx].active {
                break;
            }
            self.step_voice_phase(v_idx, smp, step_16_16);
        }
    }

    #[inline]
    fn step_voice_phase(&mut self, v_idx: usize, smp: &ItSample, step_16_16: u32) {
        let v = &mut self.voices[v_idx];
        let step_int = (step_16_16 >> 16) as i32;
        let step_frac = step_16_16 & 0xFFFF;

        if !v.reverse {
            let new_frac = v.sample_frac + step_frac;
            v.sample_pos += step_int + ((new_frac >> 16) as i32);
            v.sample_frac = new_frac & 0xFFFF;
        } else {
            let step_total = ((step_int as i64) << 16) | (step_frac as i64);
            let cur_total = ((v.sample_pos as i64) << 16) | (v.sample_frac as i64);
            let next_total = cur_total - step_total;
            v.sample_pos = (next_total >> 16) as i32;
            v.sample_frac = (next_total & 0xFFFF) as u32;
        }

        // Loop-Handling (Sustain-Loop hat Vorrang solange !v.note_off)
        let use_sus = !v.note_off
            && (smp.flags & it_smp_flags::SUS_LOOP) != 0
            && smp.sus_loop_end > smp.sus_loop_begin;
        let use_loop = (smp.flags & it_smp_flags::LOOP) != 0 && smp.loop_end > smp.loop_begin;

        if use_sus {
            let lb = smp.sus_loop_begin as i32;
            let le = smp.sus_loop_end as i32;
            let bidi = (smp.flags & it_smp_flags::PINGPONG_SUS_LOOP) != 0;
            Self::wrap_loop_bounds(v, lb, le, bidi);
        } else if use_loop {
            let lb = smp.loop_begin as i32;
            let le = smp.loop_end as i32;
            let bidi = (smp.flags & it_smp_flags::PINGPONG_LOOP) != 0;
            Self::wrap_loop_bounds(v, lb, le, bidi);
        } else if v.sample_pos >= (smp.length as i32) || v.sample_pos < 0 {
            v.active = false;
            let hc_idx = (v.host_chn as usize).min(MAX_IT_HOST_CHANNELS - 1);
            if self.host_channels[hc_idx].fg_voice == Some(v_idx) {
                self.host_channels[hc_idx].fg_voice = None;
            }
        }
    }

    #[inline]
    fn wrap_loop_bounds(v: &mut ItVirtualVoice, lb: i32, le: i32, bidi: bool) {
        let span = (le - lb).max(1);
        if bidi {
            if !v.reverse && v.sample_pos >= le {
                let over = v.sample_pos - le;
                v.sample_pos = (le - 1 - (over % span)).clamp(lb, le - 1);
                v.reverse = true;
            } else if v.reverse && v.sample_pos < lb {
                let under = lb - 1 - v.sample_pos;
                v.sample_pos = (lb + (under % span)).clamp(lb, le - 1);
                v.reverse = false;
            }
        } else if !v.reverse && v.sample_pos >= le {
            v.sample_pos = lb + ((v.sample_pos - lb) % span);
        } else if v.reverse && v.sample_pos < lb {
            let under = lb - 1 - v.sample_pos;
            v.sample_pos = (le - 1 - (under % span)).clamp(lb, le - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::formats::it::load_it;
    use crate::engine::sequencer::Sequencer;
    use crate::engine::types::SoundSettings;

    #[test]
    fn test_it_engine_nna_virtual_voices_filter_and_dual_mixer_modes() {
        let it_bytes = crate::engine::formats::it::tests::build_minimal_it_bytes();
        let loaded = load_it(&it_bytes).expect("Minimal .IT laden");

        // Teste alle 7 Interpolations-Stufen (0..=3 = MOD4WIN IDO, 4..=6 = IT2 Native)
        for oversamp in 0..=6u8 {
            let mut settings = SoundSettings::default();
            settings.samplerate = 44100;
            settings.bits = 16;
            settings.stereo = 2;
            settings.panning = true;
            settings.oversamp = oversamp;

            let mut seq = Sequencer::new(loaded.clone(), settings);
            assert!(seq.is_it_module());

            let mut non_zero = false;
            // Rendere 12 Ticks (2 Zeilen à Speed 6 -> Row 0 triggert C-5 mit Z60 Filter, Row 1 triggert E-5 mit NNA Continue)
            for tick in 0..12 {
                let out_buf = seq.render_it_tick(None).expect("IT-Tick muss Puffer liefern");
                match out_buf {
                    crate::engine::mixer::MixOutputBuffer::Pcm16(pcm) => {
                        assert!(!pcm.is_empty());
                        if pcm.iter().any(|&s| s != 0) {
                            non_zero = true;
                        }
                    }
                    crate::engine::mixer::MixOutputBuffer::Pcm8(pcm) => {
                        assert!(!pcm.is_empty());
                        if pcm.iter().any(|&s| s != 0x80) {
                            non_zero = true;
                        }
                    }
                    _ => {}
                }
                if tick == 6 {
                    // Auf Row 1 (Tick 6) muss dank NNA = Continue sowohl die Hintergrund-Voice (C-5)
                    // als auch die neue Vordergrund-Voice (E-5) gleichzeitig aktiv sein!
                    let it_eng = seq.it_engine.as_ref().unwrap();
                    let active_voices = it_eng.voices.iter().filter(|v| v.active).count();
                    assert!(
                        active_voices >= 2,
                        "NNA=Continue muss bei Row 1 mindestens 2 aktive Virtual Voices erzeugen (aktiv: {})",
                        active_voices
                    );
                }
            }
            assert!(
                non_zero,
                "IT-Mixer muss für oversamp={} ein hörbares Ausgangssignal erzeugen",
                oversamp
            );
        }
    }

    #[test]
    fn test_it_engine_fg_voice_lifecycle_fvol_scaling_and_oxx_offset() {
        use crate::engine::formats::it::{it_flags, it_smp_flags, ItNoteCell};

        let it_bytes = crate::engine::formats::it::tests::build_minimal_it_bytes();
        let mut loaded = load_it(&it_bytes).expect("Minimal .IT laden");
        let it = loaded.it_module.as_mut().unwrap();
        it.flags |= it_flags::OLD_EFFECTS;

        // Sample 1: Kurzes One-Shot-Sample (4 Samples lang), endet sofort im ersten Tick
        it.samples[1].flags &= !it_smp_flags::LOOP;
        it.samples[1].length = 4;
        it.samples[1].data_16 = vec![16384; 4];

        // Sample 2: Geloooptes Lead-Sample (64 Samples lang)
        let mut smp2 = it.samples[1].clone();
        smp2.flags |= it_smp_flags::LOOP;
        smp2.length = 64;
        smp2.loop_begin = 0;
        smp2.loop_end = 64;
        smp2.data_16 = vec![16384; 64];
        it.samples.push(smp2);
        it.instruments[1].note_sample_table.iter_mut().for_each(|(_, s)| *s = 1);
        let mut ins2 = it.instruments[1].clone();
        ins2.nna = 0; // NNA = Note Cut
        ins2.note_sample_table.iter_mut().for_each(|(_, s)| *s = 2);
        it.instruments.push(ins2);

        // Row 0: Ch 1 triggert kurzes One-Shot (belegt Voice 0 und endet in Tick 0)
        // Row 1: Ch 0 triggert gelooptes Lead-Sample mit v=16 (übernimmt freie Voice 0),
        //        Ch 1 triggert gleichzeitig eine neue Note mit v=64 (darf Ch 0 NICHT via altem fg_voice abschneiden!)
        it.patterns[0].rows[0][0] = ItNoteCell::default();
        it.patterns[0].rows[0][1] = ItNoteCell {
            note: 60,
            instrument: 1,
            vol_cmd: 64,
            command: 0,
            param: 0,
        };
        it.patterns[0].rows[1][0] = ItNoteCell {
            note: 60,
            instrument: 2,
            vol_cmd: 16, // 25% Lautstärke
            command: 15, // Oxx (Sample Offset)
            param: 0x9F, // Out-of-range Offset (0x9F00 > 64) bei OLD_EFFECTS
        };
        it.patterns[0].rows[1][1] = ItNoteCell {
            note: 64,
            instrument: 2,
            vol_cmd: 64, // 100% Lautstärke
            command: 0,
            param: 0,
        };

        let mut settings = SoundSettings::default();
        settings.samplerate = 44100;
        let mut seq = Sequencer::new(loaded, settings);

        // Rendere Row 0 (6 Ticks): One-Shot auf Ch 1 muss am Ende von Tick 0 sauber fg_voice freigeben
        for _ in 0..6 {
            seq.render_it_tick(None);
        }
        assert_eq!(
            seq.it_engine.as_ref().unwrap().host_channels[1].fg_voice,
            None,
            "Beendetes One-Shot-Sample muss fg_voice auf dem Host-Channel zurücksetzen"
        );

        // Rendere Tick 0 von Row 1: Ch 0 und Ch 1 müssen BEIDE eine aktive Vordergrund-Voice besitzen
        seq.render_it_tick(None);
        let eng = seq.it_engine.as_ref().unwrap();
        let v_ch0 = eng.host_channels[0].fg_voice.expect("Ch 0 muss aktive fg_voice haben");
        let v_ch1 = eng.host_channels[1].fg_voice.expect("Ch 1 muss aktive fg_voice haben");
        assert_ne!(v_ch0, v_ch1, "Ch 0 und Ch 1 dürfen sich nicht dieselbe Voice teilen");
        assert!(eng.voices[v_ch0].active, "Ch 0 Lead-Voice darf nicht durch Ch 1 NNA=Cut getötet werden");
        assert!(eng.voices[v_ch1].active, "Ch 1 Voice muss aktiv sein");

        // Prüfe lineare fvol-Skalierung (>> 22 statt >> 20): v=16 muss exakt 1/4 von v=64 sein
        let fvol_ch0 = eng.voices[v_ch0].fvol;
        let fvol_ch1 = eng.voices[v_ch1].fvol;
        assert!(
            fvol_ch0 > 0 && fvol_ch1 == fvol_ch0 * 4,
            "fvol muss für v=16 ({}) exakt 1/4 von v=64 ({}) betragen",
            fvol_ch0,
            fvol_ch1
        );
    }
}
