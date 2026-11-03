//! Historische CPU- & Puffer-Simulation (Paket 6: Retro-Telemetrie & optionaler Authentic Stutter).
//!
//! Simuliert die Prozessorlast (`Main_Percnt` + `PBar`) und die Pufferzeit (`Main_BuffTime`)
//! klassischer 90er-Jahre-CPUs anhand der tatsächlich aktiven Mixer-Kanäle, Samplerate,
//! Bittiefe, Stereo/Surround, Panning und IDO-Stufe (inkl. getrennter Integer- vs. FPU-Leistung).

use crate::engine::types::SoundSettings;

/// Wählbare historische CPU-Profile (1988–1997) sowie native Echtzeit-Messung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CpuProfile {
    /// 0: Echte gemessene Callback-Zeit auf moderner Hardware (~0–1 %)
    #[default]
    Native = 0,
    /// 1: Intel 386 SX/16 (1988 – kämpft schon bei 4 Kanälen & 22 kHz Mono, keine Hardware-FPU)
    I386Sx16 = 1,
    /// 2: Intel 386 DX/33 (1989 – schafft 4Ch bei 44.1 kHz ohne Filter; IDO überlastet sofort)
    I386Dx33 = 2,
    /// 3: Intel 486 DX/33 (1990 – 4–8 Kanäle mit 44.1 kHz Stereo-Surround oder IDO Linear bei ~65–80 %)
    I486Dx33 = 3,
    /// 4: Intel 486 DX2/66 (1992 – 8–16 Kanäle mit IDO Linear machbar, schwitzt bei 20Ch XM + Panning)
    I486Dx2_66 = 4,
    /// 5: Intel 486 DX4/100 (1994 – 16–20 Kanäle mit IDO-CPU bei ~75–90 %)
    I486Dx4_100 = 5,
    /// 6: Intel Pentium 90 (1994 – starke Pipelined-FPU, auch IDO-FPU gut nutzbar)
    Pentium90 = 6,
    /// 7: AMD K5 PR133 (1996 – sehr starke Integer-Leistung bei IDO-CPU, aber schwache FPU bei IDO-FPU)
    AmdK5Pr133 = 7,
    /// 8: Intel Pentium 133 (1995 – Referenz aus M4W_GER.HLP: 32Ch, 48 kHz, 16 Bit, Stereo, Pan, Surr & IDO ~100 %)
    Pentium133 = 8,
    /// 9: AMD K6-200 (1997 – zieht 32 Kanäle in IDO-CPU locker durch, FPU etwas schwächer als Integer)
    AmdK6_200 = 9,
    /// 10: AMD K6-2 450 (1999 – ~2,25x Integer- & FPU-Leistung des K6-200, stärkstes Retro-Referenzprofil)
    AmdK6_2_450 = 10,
}

impl CpuProfile {
    pub const ALL: [CpuProfile; 11] = [
        CpuProfile::Native,
        CpuProfile::I386Sx16,
        CpuProfile::I386Dx33,
        CpuProfile::I486Dx33,
        CpuProfile::I486Dx2_66,
        CpuProfile::I486Dx4_100,
        CpuProfile::Pentium90,
        CpuProfile::AmdK5Pr133,
        CpuProfile::Pentium133,
        CpuProfile::AmdK6_200,
        CpuProfile::AmdK6_2_450,
    ];

    pub fn from_index(idx: u8) -> Self {
        match idx {
            0 => Self::Native,
            1 => Self::I386Sx16,
            2 => Self::I386Dx33,
            3 => Self::I486Dx33,
            4 => Self::I486Dx2_66,
            5 => Self::I486Dx4_100,
            6 => Self::Pentium90,
            7 => Self::AmdK5Pr133,
            8 => Self::Pentium133,
            9 => Self::AmdK6_200,
            10 => Self::AmdK6_2_450,
            _ => Self::Native,
        }
    }

    pub fn from_u8(idx: u8) -> Self {
        Self::from_index(idx)
    }

    pub fn as_index(self) -> u8 {
        self as u8
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn short_code(self) -> &'static str {
        match self {
            Self::Native => "NATIVE",
            Self::I386Sx16 => "386SX/16",
            Self::I386Dx33 => "386DX/33",
            Self::I486Dx33 => "486DX/33",
            Self::I486Dx2_66 => "486DX2/66",
            Self::I486Dx4_100 => "486DX4/100",
            Self::Pentium90 => "PENTIUM 90",
            Self::AmdK5Pr133 => "K5 PR133",
            Self::Pentium133 => "PENTIUM 133",
            Self::AmdK6_200 => "K6-200",
            Self::AmdK6_2_450 => "K6-2 450",
        }
    }

    /// Kompaktes 20-Zeichen-Banner inkl. Stutter-Indikator für `Main_Info`.
    pub fn lcd_banner_with_stutter(self, authentic_stutter: bool) -> String {
        if authentic_stutter {
            format!("CPU:{} [ST]", self.short_code())
        } else {
            self.lcd_banner().to_string()
        }
    }

    pub fn next(self) -> Self {
        Self::from_index((self.as_index() + 1) % 11)
    }

    pub fn prev(self) -> Self {
        Self::from_index((self.as_index() + 10) % 11)
    }

    /// Vollständiger Name für Fenstertitel, Log und INI.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Native => "Native (Modern CPU)",
            Self::I386Sx16 => "386 SX/16",
            Self::I386Dx33 => "386 DX/33",
            Self::I486Dx33 => "486 DX/33",
            Self::I486Dx2_66 => "486 DX2/66",
            Self::I486Dx4_100 => "486 DX4/100",
            Self::Pentium90 => "Pentium 90",
            Self::AmdK5Pr133 => "AMD K5 PR133",
            Self::Pentium133 => "Pentium 133",
            Self::AmdK6_200 => "AMD K6-200",
            Self::AmdK6_2_450 => "AMD K6-2 450",
        }
    }

    /// Kompaktes 20-Zeichen-Banner für das `FONT_16X20`-Hauptdisplay (`Main_Info`).
    pub fn lcd_banner(self) -> &'static str {
        match self {
            Self::Native => "CPU: NATIVE MODERN",
            Self::I386Sx16 => "CPU: 386 SX/16",
            Self::I386Dx33 => "CPU: 386 DX/33",
            Self::I486Dx33 => "CPU: 486 DX/33",
            Self::I486Dx2_66 => "CPU: 486 DX2/66",
            Self::I486Dx4_100 => "CPU: 486 DX4/100",
            Self::Pentium90 => "CPU: PENTIUM 90",
            Self::AmdK5Pr133 => "CPU: AMD K5 PR133",
            Self::Pentium133 => "CPU: PENTIUM 133",
            Self::AmdK6_200 => "CPU: AMD K6-200",
            Self::AmdK6_2_450 => "CPU: AMD K6-2 450",
        }
    }

    /// Parst einen CLI-Parameter (`--cpu-sim <name>`) oder INI-Wert flexibel in ein `CpuProfile`.
    pub fn parse_str(s: &str) -> Option<Self> {
        let norm: String = s
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if norm.is_empty() {
            return None;
        }
        match norm.as_str() {
            "0" | "native" | "modern" | "nativemoderncpu" | "off" => Some(Self::Native),
            "1" | "386sx" | "386sx16" | "i386sx16" => Some(Self::I386Sx16),
            "2" | "386" | "386dx" | "386dx33" | "i386dx33" => Some(Self::I386Dx33),
            "3" | "486" | "486dx" | "486dx33" | "i486dx33" => Some(Self::I486Dx33),
            "4" | "486dx2" | "486dx266" | "dx266" | "48666" | "i486dx266" => Some(Self::I486Dx2_66),
            "5" | "486dx4" | "486dx4100" | "dx4100" | "486100" | "i486dx4100" => {
                Some(Self::I486Dx4_100)
            }
            "6" | "p90" | "pentium90" => Some(Self::Pentium90),
            "7" | "k5" | "amdk5" | "k5pr133" | "amdk5pr133" | "k5133" => Some(Self::AmdK5Pr133),
            "8" | "p133" | "pentium133" | "pentium" => Some(Self::Pentium133),
            "9" | "k6" | "amdk6" | "k6200" | "amdk6200" => Some(Self::AmdK6_200),
            "10" | "k62" | "amdk62" | "k62450" | "amdk62450" | "k6450" => Some(Self::AmdK6_2_450),
            _ => None,
        }
    }

    /// Liefert `(int_capacity, fpu_capacity)` des historischen Prozessors.
    ///
    /// Normiert auf die Arbeitslast-Formel:
    /// `W = active_ch * (sr / 44100) * stereo_mult * surr_mult * bits_mult * pan_mult * ido_mult`
    pub fn capacities(self) -> (f32, f32) {
        match self {
            Self::Native => (100_000.0, 100_000.0),
            // 386 SX/16 (1988): kämpft schon bei 4Ch & 22 kHz Mono (W = 2.0 -> ~83%), keine FPU
            Self::I386Sx16 => (2.4, 0.45),
            // 386 DX/33 (1989): schafft 4Ch @ 44.1 kHz ohne Filter (W = 5.98 -> ~66%), IDO überlastet sofort
            Self::I386Dx33 => (9.0, 2.2),
            // 486 DX/33 (1990): 4-8Ch @ 44.1 kHz Stereo-Surround oder IDO Linear bei ~65-80%
            Self::I486Dx33 => (31.0, 22.0),
            // 486 DX2/66 (1992): 8-16Ch mit IDO Linear machbar, schwitzt bei 20Ch XM + Panning
            Self::I486Dx2_66 => (82.0, 58.0),
            // 486 DX4/100 (1994): 16-20Ch mit IDO-CPU bei ~75-90%
            Self::I486Dx4_100 => (175.0, 120.0),
            // Pentium 90 (1994): starke Pipelined-FPU, auch IDO-FPU gut nutzbar
            Self::Pentium90 => (225.0, 240.0),
            // AMD K5 PR133 (1996): sehr starke Integer-Einheit (stärker als P133), aber schwache FPU bei IDO-FPU!
            Self::AmdK5Pr133 => (355.0, 165.0),
            // Pentium 133 (1995): Referenz aus M4W_GER.HLP (32Ch, 48 kHz, 16-Bit, Stereo, Pan, Surr, IDO -> ~101%)
            Self::Pentium133 => (320.0, 335.0),
            // AMD K6-200 (1997): zieht 32Ch in IDO-CPU locker durch (~62%), FPU etwas schwächer als Integer
            Self::AmdK6_200 => (520.0, 310.0),
            // AMD K6-2 450 (1999): ~2,25x Integer- und FPU-Leistung des K6-200
            Self::AmdK6_2_450 => (1170.0, 697.5),
        }
    }
}

/// Berechnet die ungerundete CPU-Auslastung als Faktor (`1.0` = `100%`) für einen einzelnen Tick.
///
/// - Skaliert linear mit `SampleRate / 44100.0` und `active_channels`.
/// - Multiplikatoren laut Original-Dokumentation:
///   * `Stereo`: `x1.3`
///   * `Surround`: `x1.15`
///   * `16-Bit`: `x1.15`
///   * `Panning aktiv`: `x1.8`
/// - Interpolations-Kosten:
///   * `0 = IDO OFF`: `x1.0` (Integer)
///   * `1 = IDO Linear`: `x2.2` (Integer)
///   * `2 = IDO CPU-Cubic`: `x3.0` (Integer)
///   * `3 = IDO FPU-Cubic`: `x3.2` (FPU!)
pub fn compute_raw_cpu_load_ratio(
    profile: CpuProfile,
    active_channels: usize,
    settings: &SoundSettings,
    native_callback_ratio: f32,
) -> f32 {
    if profile == CpuProfile::Native {
        return native_callback_ratio.clamp(0.0, 1.0);
    }

    let sr_factor = (settings.samplerate.max(4000) as f32) / 44100.0;
    // Kleiner Grund-Overhead für Sequencer/Timer-Tick (~0.35 Kanäle Äquivalent)
    let eff_channels = (active_channels as f32) + 0.35;

    let stereo_mult = if settings.stereo >= 2 { 1.3 } else { 1.0 };
    let surr_mult = if settings.surround && settings.stereo >= 2 {
        1.15
    } else {
        1.0
    };
    let bits_mult = if settings.bits == 16 { 1.15 } else { 1.0 };
    let pan_mult = if settings.panning { 1.8 } else { 1.0 };

    let oversamp = settings.oversamp.min(3);
    let (ido_mult, uses_fpu) = match oversamp {
        0 => (1.0, false),
        1 => (2.2, false),
        2 => (3.0, false),
        _ => (3.2, true),
    };

    let workload = eff_channels * sr_factor * stereo_mult * surr_mult * bits_mult * pan_mult * ido_mult;
    let (int_cap, fpu_cap) = profile.capacities();
    let capacity = if uses_fpu { fpu_cap } else { int_cap };

    (workload / capacity.max(0.1)).max(0.0)
}

/// Laufzeitzustand der CPU- und Puffer-Telemetrie inkl. Glättung, Timer-Jitter und DMA-Stutter.
#[derive(Clone, Debug)]
pub struct CpuTelemetryState {
    /// Geglättete CPU-Last als Verhältnis (`1.0` = 100 %)
    pub smoothed_load_ratio: f32,
    /// Angezeigte 3-stellige CPU-Last in Prozent (`0..=999`, gedämpft alle ~450 ms aktualisiert)
    pub display_cpu_percent: u32,
    /// Aktueller interner Füllstand des virtuellen Audio-Puffers in Sekunden (`0.0 .. setup_buffs * 0.2`)
    pub virtual_buffer_secs: f32,
    /// Gedämpft angezeigte Pufferzeit in Sekunden (alle ~450 ms aktualisiert)
    pub displayed_buffer_secs: f32,
    /// Maximale Pufferzeit (`setup_buffs * 0.2`)
    pub max_buffer_secs: f32,
    /// Gibt an, ob der virtuelle Puffer wegen CPU-Überlastung (`>= 100%`) auf `00.0s` leergelaufen ist
    pub buffer_underrun: bool,
    /// Deterministischer LCG-Zustand für realistischen ±1–2 % Timer-Jitter
    jitter_seed: u32,
    /// Verbleibende Stutter-Frames im aktuellen DMA-Underrun-Zyklus (`--authentic-stutter`)
    pub stutter_frames_remaining: usize,
    /// Cooldown-Frames zwischen zwei Stutter-Hängern, damit der Sound typisch „Hack... Hack...“ stottert
    pub stutter_cooldown_frames: usize,
    /// Zähler für Blink-Animation bei `00.0s` Buffer-Underrun
    pub tick_counter: u32,
    /// Akkumulierte Zeit seit dem letzten Display-Refresh (~0.45 s Intervall für ruhige 7-Segment-Ziffern)
    pub display_accum_secs: f32,
}

impl Default for CpuTelemetryState {
    fn default() -> Self {
        Self {
            smoothed_load_ratio: 0.0,
            display_cpu_percent: 0,
            virtual_buffer_secs: 0.0,
            displayed_buffer_secs: 0.0,
            max_buffer_secs: 3.6,
            buffer_underrun: false,
            jitter_seed: 0x1995_0420,
            stutter_frames_remaining: 0,
            stutter_cooldown_frames: 0,
            tick_counter: 0,
            display_accum_secs: 0.5,
        }
    }
}

impl CpuTelemetryState {
    pub fn new(n_buffers: u8) -> Self {
        let mut s = Self::default();
        s.reset_buffer(n_buffers);
        s
    }
    /// Setzt den virtuellen Puffer bei Wiedergabestart/Songwechsel/Stopp wie im Original auf `00.0s` zurück,
    /// sodass er sich während des Abspielens erst schrittweise bis `n_buffers * 0.2s` aufbaut!
    pub fn reset_buffer(&mut self, n_buffers: u8) {
        let max_s = (n_buffers.clamp(1, 99) as f32) * 0.2;
        self.max_buffer_secs = max_s;
        self.virtual_buffer_secs = 0.0;
        self.displayed_buffer_secs = 0.0;
        self.smoothed_load_ratio = 0.0;
        self.display_cpu_percent = 0;
        self.buffer_underrun = false;
        self.stutter_frames_remaining = 0;
        self.stutter_cooldown_frames = 0;
        self.display_accum_secs = 0.5;
    }

    /// Aktualisiert CPU-Last und Pufferzeit für ein Zeitintervall `dt_secs`.
    pub fn step_telemetry(
        &mut self,
        profile: CpuProfile,
        is_playing: bool,
        active_channels: usize,
        settings: &SoundSettings,
        n_buffers: u8,
        native_callback_ratio: f32,
        dt_secs: f32,
    ) {
        let max_s = (n_buffers.clamp(1, 99) as f32) * 0.2;
        self.max_buffer_secs = max_s;
        if self.virtual_buffer_secs > max_s {
            self.virtual_buffer_secs = max_s;
        }
        if self.displayed_buffer_secs > max_s {
            self.displayed_buffer_secs = max_s;
        }

        if !is_playing {
            self.smoothed_load_ratio = 0.0;
            self.display_cpu_percent = 0;
            self.virtual_buffer_secs = 0.0;
            self.displayed_buffer_secs = 0.0;
            self.buffer_underrun = false;
            self.stutter_frames_remaining = 0;
            self.stutter_cooldown_frames = 0;
            self.tick_counter = 0;
            self.display_accum_secs = 0.5;
            return;
        }
        self.tick_counter = self.tick_counter.wrapping_add(1);

        let raw_ratio = compute_raw_cpu_load_ratio(
            profile,
            active_channels,
            settings,
            native_callback_ratio,
        );

        // Sanfte exponentielle Glättung
        let alpha = (dt_secs * 8.0).clamp(0.06, 0.35);
        if self.smoothed_load_ratio == 0.0 {
            self.smoothed_load_ratio = raw_ratio;
        } else {
            self.smoothed_load_ratio =
                self.smoothed_load_ratio * (1.0 - alpha) + raw_ratio * alpha;
        }

        // Leichtes realistisches Schwanken (±1–2 % Timer-Jitter) bei historischen Profilen
        let jitter_pct: f32 = if profile == CpuProfile::Native {
            0.0
        } else {
            self.jitter_seed = self
                .jitter_seed
                .wrapping_mul(1664525)
                .wrapping_add(1013904223);
            let r = ((self.jitter_seed >> 16) & 0xFF) as f32 / 255.0; // 0.0 .. 1.0
            (r - 0.5) * 3.0 // -1.5% .. +1.5%
        };

        // Virtuelle Puffer-Simulation:
        // - Startet bei 00.0s und füllt sich während des Abspielens abhängig von der freien CPU-Reserve `(1.0 - cpu_load)` bis `max_s`.
        // - Reicht die CPU-Leistung nicht (`>= 100%`), sinkt der Puffer bzw. verharrt bei `00.0s` (Buffer-Underrun).
        let dt = dt_secs.clamp(0.0005, 0.25);
        if self.smoothed_load_ratio >= 1.0 {
            let drain = (self.smoothed_load_ratio - 1.0).max(0.05) * 1.0 * dt;
            self.virtual_buffer_secs = (self.virtual_buffer_secs - drain).max(0.0);
        } else {
            let refill_rate = (1.0 - self.smoothed_load_ratio).max(0.05) * 1.4;
            self.virtual_buffer_secs = (self.virtual_buffer_secs + refill_rate * dt).min(max_s);
        }

        self.buffer_underrun = self.virtual_buffer_secs <= 0.001 && self.smoothed_load_ratio >= 1.0;

        // Ruhigere Refresh-Rate (~450 ms / ca. 2x pro Sekunde) für die 7-Segment-Zahlenanzeigen
        self.display_accum_secs += dt;
        if self.display_accum_secs >= 0.45 || self.buffer_underrun {
            self.display_accum_secs = 0.0;
            self.displayed_buffer_secs = self.virtual_buffer_secs;
            let noisy_pct = (self.smoothed_load_ratio * 100.0 + jitter_pct).max(0.0);
            self.display_cpu_percent = if profile == CpuProfile::Native {
                (noisy_pct.round() as u32).min(999)
            } else {
                (noisy_pct.round() as u32).clamp(1, 999)
            };
        }
    }

    /// Liefert `(btime_int, btime_tenths)` für die 2+1-stellige LCD-Anzeige `Main_BuffTime` (z.B. `08.0` -> `(8, 0)`).
    pub fn buffer_time_digits(&self) -> (u32, u32) {
        let total_tenths = (self.displayed_buffer_secs.max(0.0) * 10.0).round() as u32;
        let secs = (total_tenths / 10).min(99);
        let tenths = total_tenths % 10;
        (secs, tenths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_historical_cpu_profiles_and_fpu_vs_int_ratios() {
        let mut s = SoundSettings::default();
        s.samplerate = 48000;
        s.bits = 16;
        s.stereo = 2;
        s.surround = true;
        s.panning = true;
        s.oversamp = 2; // IDO CPU-Cubic

        // 1. Pentium 133 Referenz aus M4W_GER.HLP: 32 aktive Kanäle, 48 kHz, 16-Bit, Stereo, Panning, Surround & IDO -> ~100%
        let p133_load = compute_raw_cpu_load_ratio(CpuProfile::Pentium133, 32, &s, 0.0);
        assert!(
            (0.95..=1.10).contains(&p133_load),
            "Pentium 133 reference load should be around 100%, got {:.1}%",
            p133_load * 100.0
        );

        // 2. AMD K5 PR133 vs. Pentium 133 bei 20 Kanälen (44.1 kHz, 16-Bit, Stereo, Panning):
        //    In IDO-CPU (oversamp=2) ist der K5 schneller (geringere Last) als der P133,
        //    in IDO-FPU (oversamp=3) bricht die K5-FPU jedoch stark ein (>100% Last)!
        s.samplerate = 44100;
        s.surround = false;
        s.oversamp = 2; // IDO-CPU
        let k5_int = compute_raw_cpu_load_ratio(CpuProfile::AmdK5Pr133, 20, &s, 0.0);
        let p133_int = compute_raw_cpu_load_ratio(CpuProfile::Pentium133, 20, &s, 0.0);
        assert!(
            k5_int < p133_int,
            "AMD K5 PR133 integer performance on IDO-CPU must beat Pentium 133"
        );

        s.oversamp = 3; // IDO-FPU
        let k5_fpu = compute_raw_cpu_load_ratio(CpuProfile::AmdK5Pr133, 20, &s, 0.0);
        let p133_fpu = compute_raw_cpu_load_ratio(CpuProfile::Pentium133, 20, &s, 0.0);
        assert!(
            k5_fpu > 1.0 && k5_fpu > p133_fpu * 1.8,
            "AMD K5 PR133 must suffer heavy FPU load spikes on IDO-FPU ({:.1}% vs P133 {:.1}%)",
            k5_fpu * 100.0,
            p133_fpu * 100.0
        );

        // 3. AMD K6-200 zieht 32 Kanäle in IDO-CPU locker durch (< 70%), ist bei IDO-FPU aber deutlich höher ausgelastet
        s.samplerate = 48000;
        s.surround = true;
        s.oversamp = 2;
        let k6_int = compute_raw_cpu_load_ratio(CpuProfile::AmdK6_200, 32, &s, 0.0);
        s.oversamp = 3;
        let k6_fpu = compute_raw_cpu_load_ratio(CpuProfile::AmdK6_200, 32, &s, 0.0);
        let k6_450_int = compute_raw_cpu_load_ratio(CpuProfile::AmdK6_2_450, 32, &s, 0.0);
        assert!(k6_int < 0.70, "AMD K6-200 must handle 32ch IDO-CPU easily");
        assert!(k6_fpu > k6_int * 1.5, "AMD K6-200 FPU load must be higher than integer load");
        assert!(
            k6_450_int < k6_fpu * 0.5,
            "AMD K6-2 450 must have ~2.25x capacity of K6-200"
        );
    }

    #[test]
    fn test_buffer_meltdown_and_refill() {
        let mut tele = CpuTelemetryState::default();
        tele.reset_buffer(10); // 10 * 0.2s = 2.0s max Puffer, startet bei 00.0s!
        assert_eq!(tele.buffer_time_digits(), (0, 0));

        let mut s = SoundSettings::default();
        s.samplerate = 44100;
        s.bits = 16;
        s.stereo = 2;
        s.panning = true;
        s.oversamp = 3; // IDO-FPU auf 386 SX/16 -> massive Überlastung (> 999%)!

        for _ in 0..20 {
            tele.step_telemetry(CpuProfile::I386Sx16, true, 8, &s, 10, 0.0, 0.05);
        }
        assert!(tele.buffer_underrun, "Buffer must underrun under heavy CPU overload");
        assert_eq!(tele.buffer_time_digits(), (0, 0));
        assert!(tele.display_cpu_percent > 100 && tele.display_cpu_percent <= 999);

        // Schalte auf Native CPU -> Puffer füllt sich schrittweise bis 02.0s auf
        for _ in 0..50 {
            tele.step_telemetry(CpuProfile::Native, true, 8, &s, 10, 0.01, 0.05);
        }
        assert!(!tele.buffer_underrun);
        assert_eq!(tele.buffer_time_digits(), (2, 0));
    }
}
