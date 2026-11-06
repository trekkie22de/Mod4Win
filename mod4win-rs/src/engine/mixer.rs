//! Bit-identische Rust-Portierung der 16/32-Bit Turbo-Assembler-Mixing-Engine von MOD4WIN 2.40 Beta.
//!
//! Übersetzte Originaldateien aus `legacy_src/`:
//! 1. `legacy_src/PLAYER/dacmix.pas` (2718 Zeilen Inline-386/x87-Assembler):
//!    - `GetNewSmpVal`: Einzel-Sample-Synthese mit Volume-Scaling & IDO/Cubic-Spline für Oszilloskop/Metering
//!    - `CalcSamplePositions`: Schneller 64-Bit-Phasen-Vorlauf (`mul ecx` / `shrd eax, edx, 16`) beim Seeking/Scanning
//!    - `ProcessSampleBlock`: Haupt-Mixing-Engine über `@JmpTable` (64 Einträge für 8/16-Bit, Mono/Left/Right/Panned,
//!      Low/High-Pitch, Non-Interpolated, Linear IDO (`fOverSamp`), 32-Bit Integer Cubic-Spline (`fCPUCubic`) und
//!      x87 FPU Cubic-Spline (`fFPUCubic`)), inklusive Click-Removal (`@SlideOut` / `@SurFadeLoop`),
//!      ProLogic-Surround-Ringpuffer (`@SurrLoop`) und finalem Buffer-Clipping (`@MixOut`: `@2Q16S`..`@1Q8m`).
//! 2. `legacy_src/IT/M12BIT.ASM` (813 Zeilen 386er Turbo-Assembler):
//!    - 16-fach entrollte (`REPT 16`) 12-Bit Volume-Table Channel-Mixer (`Mix12Bit8*` / `Mix12Bit16*`)
//! 3. `legacy_src/IT/M32BITI.ASM` (818 Zeilen 386er Turbo-Assembler):
//!    - 16-fach entrollte (`REPT 16`) 32-Bit Linear-IDO Channel-Mixer (`Mix32BitI8*` / `Mix32BitI16*`)

use crate::engine::types::{
    event_flags, mixer_flags, Channel, EngineState, MixChannel, MAX_M4W_CHANNELS,
    MAX_MIX_BUF_SAMPLES, SURR_BUFFER_LEN, SUR_BUFFER_OFS,
};

// ============================================================================
// 1. Zentrale Puffer- und Speicher-Typen des Mixers (dacmix.pas, playdefs.pas)
// ============================================================================

/// Ein einzelner 32-Bit-Stereo-Akkumulationsframe (`[Left, Right]`, 8 Bytes)
/// im Mischpuffer (`MixBufferOfs` bzw. `SurBufferOfs` in `playdefs.pas` Zeilen 757–758).
///
/// Im Assembler (`dacmix.pas`) zeigt `ES:DI` auf ein Array solcher 8-Byte-Einträge:
/// - `es:[di]` (`+0`): Linker Kanal (`i32`)
/// - `es:[di+4]` (`+4`): Rechter Kanal (`i32`)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StereoSample32 {
    /// Linker 32-Bit-Akkumulationskanal (`es:[di]`).
    pub left: i32,
    /// Rechter 32-Bit-Akkumulationskanal (`es:[di+4]`).
    pub right: i32,
}

/// Anzahl der 32-Bit-Stereo-Frames im Surround-Verzögerungs-Ringpuffer
/// (`SurrBufferLen = 8192` Bytes / `8` Bytes pro Frame = `1024` Frames).
pub const SURR_RING_FRAMES: usize = SURR_BUFFER_LEN / 8;

/// Ausgangs-Format des finalen Ausgabepuffers (`SoundBuffer`) in `@MixOut` (`dacmix.pas` Zeilen 2560–2706).
#[derive(Debug, Clone)]
pub enum MixOutputBuffer {
    /// Kein Sound-Ausgabepuffer aktiv (`PeakSearch != 0`: `@2Q16c` / `@2Q16mc` aktualisiert nur `MaxLevel`).
    PeakSearchOnly,
    /// 16-Bit Signed PCM (`i16`, Stereo interleaved oder Mono).
    Pcm16(Vec<i16>),
    /// 8-Bit Unsigned PCM (`u8`, Mittenwert `0x80`, Stereo interleaved oder Mono).
    Pcm8(Vec<u8>),
}

/// Per-Kanal-Zustand für das optionale Mikro-De-Clicking (`CL+DC`), das Knackgeräusche
/// fehlerhafter Module (unsaubere Loop-Punkte, nicht-nulldurchgängige Sample-Starts,
/// 2-Byte-Amiga-Header/Tail-Artefakte, harte Note-Cuts/Retrigger) gezielt pro Kanal glättet.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelDeClickState {
    /// Letzter linker 32-Bit-Pegel des Kanals am Übergangspunkt (`chn_scratch[buf_samples].left`).
    pub prev_l: i32,
    /// Letzter rechter 32-Bit-Pegel des Kanals am Übergangspunkt (`chn_scratch[buf_samples].right`).
    pub prev_r: i32,
    /// Ob der Kanal zuletzt in den Surround-Puffer (`sur_buffer`) geroutet war.
    pub was_surround: bool,
    /// Ob der Kanal im vorangegangenen Subblock aktiv war (`note != 0 && avg_vol != 0`).
    pub was_active: bool,
    /// Erwarteter Sample-Zeiger (`chn.note`) zu Beginn des nächsten Subblocks bei nahtloser Wiedergabe.
    pub expected_next_esi: i32,
}

/// Laufzeit-Speicher der `dacmix.pas`-Mixing-Engine (`PlayMemSel`, `SurrPtr`, `SamplesPtr`).
///
/// Kapselt die in `PLAYER32.DLL` über 16:16/16:32-Selektoren adressierten Speicherbereiche:
/// - `mix_buffer`: 32-Bit-Stereo-Hauptmischpuffer (`MixBufferOfs`, `MAX_MIX_BUF_SAMPLES + 1` Frames)
/// - `sur_buffer`: 32-Bit-Stereo-Surround-Mischpuffer (`SurBufferOfs`, `MAX_MIX_BUF_SAMPLES + 1` Frames)
/// - `surr_ring`: 8192-Byte (1024 Stereo-Frames) ProLogic-Surround-Delay-Ringpuffer (`SurrPtr`)
/// - `vol_table`: 65x256 16-Bit-Volume-Lookup-Tabelle (`VolTableOfs1` / `VolTableOfs2`)
/// Zustand des authentischen Amiga-Paula-Tiefpassfilters (A500/A2000 4.42 kHz RC + ~3.275 kHz 12 dB/Okt Sallen-Key LED-Filter).
#[derive(Debug, Clone, Copy, Default)]
pub struct AmigaFilterState {
    pub rc_l: f64,
    pub rc_r: f64,
    pub svf_ic1_l: f64,
    pub svf_ic2_l: f64,
    pub svf_ic1_r: f64,
    pub svf_ic2_r: f64,
    pub active: bool,
}

/// Zustand einer phasenkompensierten 2x2-Pol TPT-SVF-Frequenzweichenstufe (`CompSvfStage`).
///
/// Realisiert die Übertragungsfunktion
/// $$H_{\text{HP,comp}}(\tilde{s}) = \frac{\tilde{s}^4 + 2k\tilde{s}^3}{(1 + k\tilde{s} + \tilde{s}^2)^2}, \qquad B(\tilde{s}) = 1 - H_{\text{HP,comp}}(\tilde{s})$$
/// mit:
/// - exakter algebraischer Rekonstruktion $B + H_{\text{HP,comp}} \equiv 1$,
/// - steiler `-18 dB/Oktave` ($O(\tilde{\omega}^3)$) Sperrdämpfung tiefer Grundtöne im Hochpasszweig, und
/// - verschwindendem Phasenfehler 1. Ordnung ($O(1/\tilde{\omega}^2)$) im Hochfrequenzbereich.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompSvfStage {
    pub ic1_a: f64,
    pub ic2_a: f64,
    pub ic1_b: f64,
    pub ic2_b: f64,
}

impl CompSvfStage {
    #[inline(always)]
    pub fn process(&mut self, u: f64, a1: f64, a2: f64, a3: f64, k: f64, c2: f64) -> (f64, f64) {
        // SVF A: 2-Pol Hochpass HP_a
        let v3a = u - self.ic2_a;
        let v1a = a1 * self.ic1_a + a2 * v3a;
        let v2a = self.ic2_a + a2 * self.ic1_a + a3 * v3a;
        self.ic1_a = 2.0 * v1a - self.ic1_a;
        self.ic2_a = 2.0 * v2a - self.ic2_a;
        let hp_a = u - k * v1a - v2a;

        // SVF B: angewendet auf HP_a -> Phasenkompensation HP_b + 2*k*BP_b + c2*LP_b
        let v3b = hp_a - self.ic2_b;
        let v1b = a1 * self.ic1_b + a2 * v3b;
        let v2b = self.ic2_b + a2 * self.ic1_b + a3 * v3b;
        self.ic1_b = 2.0 * v1b - self.ic1_b;
        self.ic2_b = 2.0 * v2b - self.ic2_b;
        let hp_b = hp_a - k * v1b - v2b;

        let u_hp = hp_b + 2.0 * k * v1b + c2 * v2b;
        let band_lp = u - u_hp;
        (band_lp, u_hp)
    }
}

/// Zustand des modernen adaptiven 5-Band-Wiener-/Spektral-Denoisers (`DeHiss`) mit Transienten-Schutz.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveDeHissState {
    /// 4 kaskadierte phasenkompensierte TPT-SVF-Frequenzweichenstufen (1000 Hz, 2100 Hz, 4200 Hz, 8400 Hz) für L & R.
    pub xover_l: [CompSvfStage; 4],
    pub xover_r: [CompSvfStage; 4],
    /// Grundton-/Tiefton-Hüllkurve (`< 1000 Hz`).
    pub env_low: f64,
    /// Schnelle Hüllkurven der 4 oberen Bänder (`1000..2100`, `2100..4200`, `4200..8400`, `> 8400 Hz`).
    pub env_fast: [f64; 4],
    /// Langsame Hüllkurven der 4 oberen Bänder (für Spectral-Flux-Transienten-Erkennung).
    pub env_slow: [f64; 4],
    /// Adaptiver Minimum-Statistics / MCRA Rauschpegel-Schätzer pro Band.
    pub noise_floor: [f64; 4],
    /// Geglättete Wiener-Filterverstärkung $G_b \in [G_{\min}, 1.0]$ pro Band.
    pub smooth_gain: [f64; 4],
    /// Transienten-Schutzfaktor $T \in [0.0, 1.0]$ für knackige Drums & Hi-Hats.
    pub transient_hold: f64,
    /// Ob der Denoiser im vorangegangenen Block aktiv war.
    pub active: bool,
}

impl Default for AdaptiveDeHissState {
    fn default() -> Self {
        Self {
            xover_l: [CompSvfStage::default(); 4],
            xover_r: [CompSvfStage::default(); 4],
            env_low: 0.0,
            env_fast: [0.0; 4],
            env_slow: [0.0; 4],
            noise_floor: [1200.0, 1000.0, 900.0, 800.0],
            smooth_gain: [0.50, 0.25, 0.18, 0.15],
            transient_hold: 0.0,
            active: false,
        }
    }
}

#[derive(Clone)]
pub struct DacMixMemory {
    /// Haupt-Mischpuffer (`MixBufferOfs = 33280`, `2001` Stereo-Frames à 8 Bytes).
    pub mix_buffer: [StereoSample32; MAX_MIX_BUF_SAMPLES + 1],
    /// Surround-Mischpuffer (`SurBufferOfs = 49288`, `2001` Stereo-Frames à 8 Bytes).
    pub sur_buffer: [StereoSample32; MAX_MIX_BUF_SAMPLES + 1],
    /// 8192-Byte Surround-Ringpuffer (`SurrPtr`, `1024` Stereo-Frames à 8 Bytes).
    pub surr_ring: [StereoSample32; SURR_RING_FRAMES],
    /// Vorberechnete 8-Bit-Volume-Lookup-Tabelle (`[0..=64][0..=255] -> i16`, aus `SetVolTable`).
    pub vol_table: [[i16; 256]; 65],
    /// Flag für Peak-Suche (`PeakSearch` in `playdefs.pas` / `dacmix.pas` Zeilen 2569, 2617).
    pub peak_search: bool,
    /// Temporärer Einzelkanal-Mischpuffer für das optionale Per-Channel-De-Clicking (`CL+DC`).
    pub chn_scratch: [StereoSample32; MAX_MIX_BUF_SAMPLES + 1],
    /// Per-Kanal-De-Click-Historie (`prev_l`, `prev_r`, `expected_next_esi`).
    pub chn_declick: [ChannelDeClickState; MAX_M4W_CHANNELS],
    /// Ob im vorangegangenen Block `declick` aktiv war (für nahtloses Live-Umschalten).
    pub declick_active: bool,
    /// Zustand des authentischen Amiga-Paula-Tiefpassfilters (`Amiga`).
    pub amiga_filter: AmigaFilterState,
    /// Zustand des modernen adaptiven Mehrband-Denoisers (`DeHiss`).
    pub dehiss_filter: AdaptiveDeHissState,
}

impl Default for DacMixMemory {
    fn default() -> Self {
        Self {
            mix_buffer: [StereoSample32::default(); MAX_MIX_BUF_SAMPLES + 1],
            sur_buffer: [StereoSample32::default(); MAX_MIX_BUF_SAMPLES + 1],
            surr_ring: [StereoSample32::default(); SURR_RING_FRAMES],
            vol_table: crate::engine::tables::DEFAULT_VOL_TABLE_512,
            peak_search: false,
            chn_scratch: [StereoSample32::default(); MAX_MIX_BUF_SAMPLES + 1],
            chn_declick: [ChannelDeClickState::default(); MAX_M4W_CHANNELS],
            declick_active: false,
            amiga_filter: AmigaFilterState::default(),
            dehiss_filter: AdaptiveDeHissState::default(),
        }
    }
}

impl DacMixMemory {
    /// Setzt den Per-Kanal-De-Click-Zustand zurück (z. B. bei `kanal_init` oder `seek_to_order_and_row`).
    pub fn reset_declick(&mut self) {
        self.chn_declick = [ChannelDeClickState::default(); MAX_M4W_CHANNELS];
        self.declick_active = false;
        self.amiga_filter = AmigaFilterState::default();
        self.dehiss_filter = AdaptiveDeHissState::default();
    }
}

/// Abstrahiert den linearen 32-Bit-Sample-Speicher (`FS:[ESI]` bzw. `SamplesPtr` in `dacmix.pas`).
///
/// Im Original-Assembler greift `dacmix.pas` über den 32-Bit-Selektor `FS` (`SamplesSel`) auf den
/// Sample-Speicher zu:
/// - Bei 8-Bit-Samples (`f16Bit == 0`) ist `ESI` (`chn.note`) ein direkter Byte-Index (`fs:[esi]`).
/// - Bei 16-Bit-Samples (`f16Bit != 0`) ist `ESI` (`chn.note`) ein 16-Bit-Sample-Index (`fs:[esi+esi]`,
///   d.h. Byte-Adresse `2 * ESI`, Little-Endian `i16`).
/// - Für die 4-Punkt Cubic-Spline-Interpolation greift der Assembler auf `esi - 1`, `esi`, `esi + 1`
///   und `esi + 2` zu.
#[inline(always)]
pub fn read_sample_u8(sample_mem: &[u8], esi: i32) -> u8 {
    if esi < 0 {
        return 0;
    }
    sample_mem.get(esi as usize).copied().unwrap_or(0)
}

/// Liest ein vorzeichenbehaftetes 8-Bit-Sample (`Shortint` / `i8`) an Byte-Adresse `ESI` (`fs:[esi]`).
#[inline(always)]
pub fn read_sample_i8(sample_mem: &[u8], esi: i32) -> i8 {
    read_sample_u8(sample_mem, esi) as i8
}

/// Liest ein vorzeichenbehaftetes 16-Bit-Sample (`Integer` / `i16`, Little-Endian) an Wort-Index `ESI`
/// (`fs:[esi+esi]` in `dacmix.pas`).
#[inline(always)]
pub fn read_sample_i16(sample_mem: &[u8], esi: i32) -> i16 {
    if esi < 0 {
        return 0;
    }
    let byte_idx = (esi as usize).wrapping_mul(2);
    if let Some(bytes) = sample_mem.get(byte_idx..byte_idx + 2) {
        i16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        0
    }
}

/// Schreibt ein 16-Bit-Sample (`i16`, Little-Endian) an Wort-Index `ESI` (`fs:[esi+esi]`) in einen Sample-Puffer.
#[inline]
pub fn write_sample_i16(sample_mem: &mut [u8], esi: usize, val: i16) {
    let byte_idx = esi * 2;
    if byte_idx + 2 <= sample_mem.len() {
        let b = val.to_le_bytes();
        sample_mem[byte_idx] = b[0];
        sample_mem[byte_idx + 1] = b[1];
    }
}

// ============================================================================
// 2. Einzel-Sample-Auswertung & Phasen-Vorlauf (dacmix.pas Zeilen 30–272)
// ============================================================================

/// Berechnet den aktuellen skalierten Sample-Wert eines Kanals ohne Fortschalten der Phase.
///
/// ### Entsprechende ASM-Prozedur
/// `FUNCTION GetNewSmpVal(Channel, Vol: Longint): Longint;` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 30–243),
/// inklusive der Sprungziele `@NoOverSamp08`, `@NoOverSamp16`, `@OverSamp08`, `@OverSamp16`,
/// `@Cubic08`, `@Cubic16`, `@fCubic08` und `@fCubic16`.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: Aktuelle ganzzahlige Sample-Position (`chn.note`)
/// - `EDX` (`DX`) -> `edx_rest`: 16-Bit gebrochener Phasenanteil (`chn.rest as u16 as i32`)
/// - `EBX` -> `ebx` / `bh_vol`: In 8-Bit-Routinen `BH = Vol` und `BL = SampleByte` (Index in `VolTable`);
///   in 16-Bit- und Cubic-Spline-Routinen temporäres Koeffizienten-Register (`Y0 - Y1` bzw. `b`)
/// - `EAX` -> `eax`: Akkumulator für den interpolierten Sample-Wert und den 32-Bit-Rückgabewert (`DX:AX`)
/// - `ECX` -> `ecx_c`: Temporäres Register in `@Cubic08` / `@Cubic16` (`c = Y1 - Ym1`)
/// - `DI` -> `di_jmp`: Sprungtabellen-Index `((chn.jmp_flags >> 3) & 0x0E)` (Wort-Offset in `@JmpTable`)
/// - `EBP` (`[ebp]`) -> `ebp_d`: Temporärer Zwischenspeicher (`oTemp`) für den Stützpunkt `d = Y0` in `@Cubic08` / `@Cubic16`
pub fn get_new_smp_val(
    chn: &Channel,
    vol: i32,
    vol_master: u16,
    vol_table: &[[i16; 256]; 65],
    sample_mem: &[u8],
) -> i32 {
    // cmp chn.Note, 0 / jz @Exit
    let mut esi: i32 = chn.note;
    if esi == 0 {
        return 0;
    }

    // @@0: cmp esi, chn.IEnd / jbe @@1 / sub esi, chn.Lln / jns @@0 / xor eax, eax / jmp @Exit
    let iend = chn.iend;
    let lln = chn.lln;
    while (esi as u32) > (iend as u32) {
        esi = esi.wrapping_sub(lln);
        if esi < 0 {
            return 0;
        }
    }

    let bh_vol = (vol as u8).min(64) as usize;
    let edx_rest: i32 = (chn.rest as u16) as i32; // movzx edx, word ptr chn.Rest
    let di_mode = (chn.jmp_flags >> 4) & 0x07;

    match di_mode {
        // --------------------------------------------------------------------
        // @NoOverSamp08 (dacmix.pas Zeilen 67–72):
        // mov bl, fs:[esi]
        // movsx eax, word ptr es:[ebx+ebx+VolTableOfs1]
        // shl eax, 8
        // --------------------------------------------------------------------
        0 => {
            let bl = read_sample_u8(sample_mem, esi) as usize;
            let eax = vol_table[bh_vol][bl] as i32;
            eax << 8
        }

        // --------------------------------------------------------------------
        // @NoOverSamp16 (dacmix.pas Zeilen 74–82):
        // movsx eax, word ptr fs:[esi+esi]
        // movsx edx, bh ; imul dx, VolMaster ; sar edx, 9 ; imul eax, edx
        // --------------------------------------------------------------------
        1 => {
            let eax = read_sample_i16(sample_mem, esi) as i32;
            let edx_scale = (((vol as i8 as i32) * (vol_master as i16 as i32)) as i32) >> 9;
            eax.wrapping_mul(edx_scale)
        }

        // --------------------------------------------------------------------
        // @OverSamp08 (dacmix.pas Zeilen 84–94):
        // mov bl, fs:[esi+1] ; movsx eax, VolTable[bh][bl]
        // mov bl, fs:[esi]   ; movsx ebx, VolTable[bh][bl]
        // sub eax, ebx ; imul eax, edx ; sar eax, 8 ; shl ebx, 8 ; add eax, ebx
        // --------------------------------------------------------------------
        2 => {
            let bl1 = read_sample_u8(sample_mem, esi.wrapping_add(1)) as usize;
            let mut eax = vol_table[bh_vol][bl1] as i32;
            let bl0 = read_sample_u8(sample_mem, esi) as usize;
            let ebx = vol_table[bh_vol][bl0] as i32;
            eax = eax.wrapping_sub(ebx);
            eax = eax.wrapping_mul(edx_rest);
            eax >>= 8;
            let ebx_shifted = ebx << 8;
            eax.wrapping_add(ebx_shifted)
        }

        // --------------------------------------------------------------------
        // @OverSamp16 (dacmix.pas Zeilen 96–109):
        // movsx ebx, word ptr fs:[esi+esi]
        // movsx eax, word ptr fs:[esi+esi+2]
        // sub eax, ebx ; sar eax, 1 ; imul eax, edx ; sar eax, 15 ; add eax, ebx
        // movsx edx, bh ; imul dx, VolMaster ; sar edx, 9 ; imul eax, edx
        // --------------------------------------------------------------------
        3 => {
            let ebx = read_sample_i16(sample_mem, esi) as i32;
            let mut eax = read_sample_i16(sample_mem, esi.wrapping_add(1)) as i32;
            eax = eax.wrapping_sub(ebx);
            eax >>= 1;
            eax = eax.wrapping_mul(edx_rest);
            eax >>= 15;
            eax = eax.wrapping_add(ebx);
            let edx_scale = (((vol as i8 as i32) * (vol_master as i16 as i32)) as i32) >> 9;
            eax.wrapping_mul(edx_scale)
        }

        // --------------------------------------------------------------------
        // @Cubic08 (dacmix.pas Zeilen 111–145):
        // 32-Bit Integer 4-Punkt Cubic-Spline-Interpolation für 8-Bit-Samples
        // Stützpunkte: Ym1 = fs:[esi-1], Y0 = fs:[esi], Y1 = fs:[esi+1], Y2 = fs:[esi+2]
        // --------------------------------------------------------------------
        4 => {
            let y0 = read_sample_i8(sample_mem, esi) as i32;
            let y1 = read_sample_i8(sample_mem, esi.wrapping_add(1)) as i32;
            let ym1 = read_sample_i8(sample_mem, esi.wrapping_sub(1)) as i32;
            let y2 = read_sample_i8(sample_mem, esi.wrapping_add(2)) as i32;

            let mut ebp_d = y0; // mov [ebp], ebx (d = Y0)
            let mut ebx = y0.wrapping_sub(y1); // sub ebx, ecx (Y0 - Y1)
            let ecx_c = y1.wrapping_sub(ym1); // sub ecx, eax (c = Y1 - Ym1)
            let mut eax = y2.wrapping_add(ebx.wrapping_mul(2)); // lea eax, [eax+2*ebx] (Y2 + 2*(Y0 - Y1))
            ebx = ebx.wrapping_sub(ym1); // sub ebx, edx (Y0 - Y1 - Ym1)
            eax = eax.wrapping_add(ebx); // add eax, ebx (a = 3*(Y0 - Y1) - Ym1 + Y2)
            ebx = ebx.wrapping_add(eax); // add ebx, eax
            let edx_x = edx_rest; // movzx edx, word ptr chn.Rest
            eax = eax.wrapping_mul(edx_x) >> 16; // imul eax, edx ; sar eax, 16
            ebx = ebx.wrapping_add(ebp_d); // add ebx, [ebp] (b = 2*Y0 - 2*Ym1 - Y1 + a)
            ebp_d <<= 8; // shl dword ptr [ebp], 8
            eax = eax.wrapping_sub(ebx); // sub eax, ebx
            eax = eax.wrapping_mul(edx_x) >> 16; // imul eax, edx ; sar eax, 16
            eax = eax.wrapping_add(ecx_c); // add eax, ecx
            eax = eax.wrapping_mul(edx_x) >> 9; // imul eax, edx ; sar eax, 9
            eax = eax.wrapping_add(ebp_d); // add eax, [ebp]

            let edx_scale = (((vol as i8 as i32) * (vol_master as i16 as i32)) as i32) >> 9;
            eax.wrapping_mul(edx_scale)
        }

        // --------------------------------------------------------------------
        // @Cubic16 (dacmix.pas Zeilen 146–184):
        // 32-Bit Integer 4-Punkt Cubic-Spline-Interpolation für 16-Bit-Samples
        // --------------------------------------------------------------------
        5 => {
            let y0 = read_sample_i16(sample_mem, esi) as i32;
            let y1 = read_sample_i16(sample_mem, esi.wrapping_add(1)) as i32;
            let ym1 = read_sample_i16(sample_mem, esi.wrapping_sub(1)) as i32;
            let y2 = read_sample_i16(sample_mem, esi.wrapping_add(2)) as i32;

            let ebp_d = y0; // mov [ebp], ebx
            let mut ebx = y0.wrapping_sub(y1); // sub ebx, ecx
            let ecx_c = y1.wrapping_sub(ym1); // sub ecx, eax
            let mut eax = y2.wrapping_add(ebx.wrapping_mul(2)); // lea eax, [eax+2*ebx]
            ebx = ebx.wrapping_sub(ym1); // sub ebx, edx
            eax = eax.wrapping_add(ebx); // add eax, ebx (a)
            ebx = ebx.wrapping_add(eax); // add ebx, eax
            let edx_x = edx_rest; // movzx edx, word ptr chn.Rest
            eax >>= 3; // sar eax, 3
            eax = eax.wrapping_mul(edx_x) >> 13; // imul eax, edx ; sar eax, 13
            ebx = ebx.wrapping_add(ebp_d); // add ebx, [ebp]
            eax = eax.wrapping_sub(ebx); // sub eax, ebx
            eax >>= 3; // sar eax, 3
            eax = eax.wrapping_mul(edx_x) >> 13; // imul eax, edx ; sar eax, 13
            eax = eax.wrapping_add(ecx_c); // add eax, ecx
            eax >>= 3; // sar eax, 3
            eax = eax.wrapping_mul(edx_x) >> 14; // imul eax, edx ; sar eax, 14
            eax = eax.wrapping_add(ebp_d); // add eax, [ebp]

            let edx_scale = (((vol as i8 as i32) * (vol_master as i16 as i32)) as i32) >> 9;
            eax.wrapping_mul(edx_scale)
        }

        // --------------------------------------------------------------------
        // @fCubic08 (dacmix.pas Zeilen 185–213):
        // x87 FPU 4-Punkt Cubic-Spline-Interpolation für 8-Bit-Samples
        // --------------------------------------------------------------------
        6 => {
            const ONE_BY_65536: f64 = 1.0 / 65536.0;
            const ONE_BY_4: f64 = 1.0 / 4.0;

            let ym1 = read_sample_i8(sample_mem, esi.wrapping_sub(1)) as i16 as f64;
            let y0 = read_sample_i8(sample_mem, esi) as i16 as f64;
            let y1 = read_sample_i8(sample_mem, esi.wrapping_add(1)) as i16 as f64;
            let y2 = read_sample_i8(sample_mem, esi.wrapping_add(2)) as i16 as f64;
            let x = (edx_rest as f64) * ONE_BY_65536;

            let y0_m_y1 = y0 - y1;
            let a = y0_m_y1 * 3.0 - ym1 + y2;
            let b = ym1 * -2.0 + y0 * 5.0 - y1 * 4.0 + y2;
            let c = y1 - ym1;
            let poly = ((a * x - b) * x + c) * x + y0 * 2.0;
            let vol_fpu = (((vol as i8 as i32) * (vol_master as i16 as i32)) as f64) * ONE_BY_4;
            (poly * vol_fpu).round_ties_even() as i32
        }

        // --------------------------------------------------------------------
        // @fCubic16 (dacmix.pas Zeilen 214–243):
        // x87 FPU 4-Punkt Cubic-Spline-Interpolation für 16-Bit-Samples
        // --------------------------------------------------------------------
        _ => {
            const ONE_BY_65536: f64 = 1.0 / 65536.0;
            const ONE_BY_1024: f64 = 1.0 / 1024.0;

            let ym1 = read_sample_i16(sample_mem, esi.wrapping_sub(1)) as f64;
            let y0 = read_sample_i16(sample_mem, esi) as f64;
            let y1 = read_sample_i16(sample_mem, esi.wrapping_add(1)) as f64;
            let y2 = read_sample_i16(sample_mem, esi.wrapping_add(2)) as f64;
            let x = (edx_rest as f64) * ONE_BY_65536;

            let y0_m_y1 = y0 - y1;
            let a = y0_m_y1 * 3.0 - ym1 + y2;
            let b = ym1 * -2.0 + y0 * 5.0 - y1 * 4.0 + y2;
            let c = y1 - ym1;
            let poly = ((a * x - b) * x + c) * x + y0 * 2.0;
            let vol_fpu = (((vol as i8 as i32) * (vol_master as i16 as i32)) as f64) * ONE_BY_1024;
            (poly * vol_fpu).round_ties_even() as i32
        }
    }
}

/// Berechnet die neuen Sample-Positionen aller aktiven Kanäle für einen Block von `num_samples`
/// ohne tatsächliches Audio-Mixing (wird beim schnellen Vorlauf/Seeking und Song-Scanning genutzt).
///
/// ### Entsprechende ASM-Prozedur
/// `PROCEDURE CalcSamplePositions; ASSEMBLER;` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 245–272).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `BX` -> `bx_ch`: Kanal-Schleifenzähler (`MI.num_tracks` abwärts bis `1`)
/// - `DI` -> `chn`: Zeiger auf den aktuellen Kanal (`Channels[bx_ch]`)
/// - `ESI` -> `esi`: Aktuelle Sample-Position (`chn.note`)
/// - `EAX` -> `eax_pitch`: Kombiniertes 16.16-Fixed-Point-Pitch-Wort (`dword ptr chn.Plo` = `(PHi << 16) | Plo`)
/// - `ECX` -> `ecx_num_samples`: Anzahl der zu überspringenden Samples (`num_samples as u32`)
/// - `EDX:EAX` -> `prod`: 64-Bit-Produkt `eax_pitch * ecx_num_samples`; `shrd eax, edx, 16` extrahiert den
///   ganzzahligen Sample-Fortschritt `(prod >> 16) as i32`.
pub fn calc_sample_positions(
    channels: &mut [Channel],
    num_tracks: u16,
    num_samples: u16,
) {
    let ecx_num_samples = num_samples as u32;
    let count = (num_tracks as usize).min(channels.len());

    for chn in channels.iter_mut().take(count) {
        let mut esi = chn.note;
        if esi != 0 {
            // mov eax, dword ptr es:[di+oPlo]
            // Da oPhi unmittelbar hinter oPlo (2 Bytes) liegt, liest dword ptr [oPlo]
            // die unteren 16 Bit von PHi im High-Word und Plo im Low-Word:
            let eax_pitch: u32 = ((chn.phi as u16 as u32) << 16) | (chn.plo as u32);
            // mul ecx ; shrd eax, edx, 16
            let prod: u64 = (eax_pitch as u64) * (ecx_num_samples as u64);
            let step: i32 = (prod >> 16) as i32;
            // add esi, eax
            esi = esi.wrapping_add(step);
            // cmp esi, es:[di+oIEnd] / jbe @@1
            if (esi as u32) > (chn.iend as u32) {
                // mov esi, es:[di+oIEnd] / sub esi, es:[di+oLln]
                esi = chn.iend.wrapping_sub(chn.lln);
                // jns @@1 / xor esi, esi
                if esi < 0 {
                    esi = 0;
                }
            }
            // @@1: mov es:[di+oNote], esi
            chn.note = esi;
        }
    }
}

// ============================================================================
// 3. Kanal-Abschluss & One-Shot-Endbehandlung (dacmix.pas Zeilen 2407–2442)
// ============================================================================

/// Behandelt das vorzeitige Ende eines One-Shot-Samples innerhalb eines Mix-Blocks
/// (`@WriteChnREnd`..`@WriteChnSEnd16` in `dacmix.pas` Zeilen 2407–2428).
///
/// Wenn `esi > iend` und `esi - lln < 0` während des Mischens eintritt, wiederholt der
/// Original-Assembler den zuletzt berechneten Sample-Wert `(ebx_left, eax_right)` für die
/// verbleibenden `cx_rem` Samples im Zielpuffer (um DC-Sprünge/Knacksen mitten im Block zu
/// verhindern) und setzt anschließend `esi = 0` (`chn.note = 0`).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `DI` -> `di_idx`: Aktueller Frame-Index im 32-Bit-Zielpuffer (`mix_buf`)
/// - `CX` -> `cx_rem`: Anzahl der noch zu schreibenden Stereo-Frames
/// - `EBX` -> `ebx_left`: Zuletzt berechneter linker (bzw. Mono/Right-) Sample-Wert
/// - `EAX` -> `eax_right`: Zuletzt berechneter rechter Sample-Wert
/// - `ESI` -> Rückgabewert `0` (`xor esi, esi` in Zeile 2427)
#[inline]
pub fn write_chn_end_tail(
    mix_buf: &mut [StereoSample32],
    mut di_idx: usize,
    mut cx_rem: usize,
    pan_mode: u16,
    ebx_val: i32,
    eax_val: i32,
) {
    let (ebx_left, eax_right) = match pan_mode & mixer_flags::F_PAN_MASK {
        // @WriteChnMiEnd / @WriteChnMiEnd16: mov eax, ebx
        mixer_flags::F_MONO => (ebx_val, ebx_val),
        // @WriteChnLEnd / @WriteChnLEnd16: xor eax, eax
        mixer_flags::F_LEFT => (ebx_val, 0),
        // @WriteChnREnd / @WriteChnREnd16: mov eax, ebx ; xor ebx, ebx
        mixer_flags::F_RIGHT => (0, ebx_val),
        // @WriteChnSEnd / @WriteChnSEnd16: ebx = left, eax = right
        _ => (ebx_val, eax_val),
    };

    while cx_rem > 0 {
        if let Some(frame) = mix_buf.get_mut(di_idx) {
            frame.left = frame.left.wrapping_add(ebx_left);
            frame.right = frame.right.wrapping_add(eax_right);
        }
        di_idx += 1;
        cx_rem -= 1;
    }
}

/// Führt den 16.16-Fixed-Point-Rückschritt am Ende eines vollständig gemischten Kanals aus
/// (`@WriteChn` in `dacmix.pas` Zeilen 2437–2442).
///
/// Da `ProcessSampleBlock` immer `BufSamples + 1` Frames mischt (Frame `BufSamples` dient als
/// Zielwert für das Click-Removal `@SlideOut`), wird die Sample-Phase am Kanalende um genau
/// einen Pitch-Schritt (`Plo` / `PHi`) zurückgestellt, damit der nächste Block nahtlos bei
/// diesem Sample fortfährt.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `DX` -> `dx_plo`: `chn.plo`
/// - `Rest` (Low-Word) -> `rest_lo`: `sub word ptr chn.Rest, dx`
/// - `ESI` -> `esi`: `sbb esi, chn.pHi`
#[inline]
pub fn write_chn_step_back(chn: &mut MixChannel, esi: i32) {
    let dx_plo = chn.plo;
    let rest_lo = chn.rest as u16;
    let (new_rest_lo, borrow) = rest_lo.overflowing_sub(dx_plo);
    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (new_rest_lo as u32)) as i32;
    let sbb = chn.phi.wrapping_add(if borrow { 1 } else { 0 });
    chn.note = esi.wrapping_sub(sbb);
}

// ============================================================================
// 4. 8-Bit & 16-Bit Channel-Mixer ohne Interpolation (dacmix.pas Zeilen 390–871)
// ============================================================================

/// Mischt einen 8-Bit-Kanal ohne Interpolation bei niedriger Schrittweite (`PHi == 0`,
/// `JmpFlags & 0x78 == 0x00`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@M08`, `@L08`, `@R08` und `@S08` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 492–657).
///
/// ### Überführung von Self-Modifying Code (SMC)
/// Im Original-Assembler patcht `@S08` (Zeilen 492–506) die Differenz `(lVol - rVol) << 9` als
/// 32-Bit-Displacement direkt in den `movsx eax, word ptr [ebx+ebx+12345678h]`-Befehl, während
/// `BH` mit `rVol` (bzw. `lVol` in `@M08`/`@L08`) geladen wird. Hier wird dies ohne Veränderung
/// der Sample-Mathematik über `vol_table[l_vol][bl]` und `vol_table[r_vol][bl]` und `match pan_mode`
/// abgebildet.
/// Innerhalb eines Eingabe-Samples (`add dx, bp` erzeugt noch keinen 16-Bit-Carry `CF == 0`)
/// verbleibt die innere Schleife (`@S08WriteSample`, `@M08WriteSample`, `@L08WriteSample`,
/// `@R08WriteSample`) exakt wie im Original im selben Sample-Wert `(ebx, eax)` ohne erneuten
/// Speicherzugriff.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 32-Bit Byte-Offset im Sample-Speicher (`chn.note`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` (`BP`) -> `bp_plo`: 16-Bit gebrochenes Pitch-Inkrement (`chn.plo`)
/// - `EBX` -> `ebx`: In `BH` Lautstärke-Index, in `BL` Sample-Byte (`fs:[esi]`), danach `ebx = VolTable[vol][bl] << 8`
/// - `EAX` -> `eax`: Bei `@S08` zweiter Kanalwert `VolTable[r_vol][bl] << 8`
pub fn mix_channel_08_nointerp_lo(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let bp_plo = chn.plo;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;
    let l_vol = (chn.l_vol.min(64)) as usize;
    let r_vol = (chn.r_vol.min(64)) as usize;

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    'get_sample: while cx > 0 {
        // cmp esi, chn.IEnd / jbe @@1 / @@0: sub esi, chn.Lln / js @WriteChn*End
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        // Sample-Byte lesen und über VolTable skalieren (`shl ebx, 8` / `shl eax, 8`)
        let bl = read_sample_u8(sample_mem, esi) as usize;
        match pan_mode {
            mixer_flags::F_MONO | mixer_flags::F_LEFT => {
                ebx = (vol_table[l_vol][bl] as i32) << 8;
            }
            mixer_flags::F_RIGHT => {
                ebx = (vol_table[r_vol][bl] as i32) << 8;
            }
            _ => {
                // @S08: eax = Left (lVol), ebx = Right (rVol), dann xchg eax, ebx -> ebx = Left, eax = Right
                ebx = (vol_table[l_vol][bl] as i32) << 8;
                eax = (vol_table[r_vol][bl] as i32) << 8;
            }
        }

        // Innere Schleife (@*08WriteSample): Solange add dx, bp keinen Carry erzeugt, selben Wert addieren
        loop {
            let (new_dx, carry) = dx_rest.overflowing_add(bp_plo);
            dx_rest = new_dx;

            if let Some(frame) = mix_buf.get_mut(di_idx) {
                match pan_mode {
                    mixer_flags::F_MONO => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    mixer_flags::F_LEFT => {
                        frame.left = frame.left.wrapping_add(ebx);
                    }
                    mixer_flags::F_RIGHT => {
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    _ => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(eax);
                    }
                }
            }
            di_idx += 1;
            cx -= 1;

            if carry {
                // jc @@2: inc esi ; dec cx ; jnz @*08GetSample
                esi = esi.wrapping_add(1);
                break;
            }
            if cx == 0 {
                break 'get_sample;
            }
        }
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 8-Bit-Kanal ohne Interpolation bei hoher Schrittweite (`PHi != 0` bzw. `fHiPitch`,
/// `JmpFlags & 0x78 == 0x08`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S081`, `@M081`, `@L081` und `@R081` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 390–490).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 32-Bit Byte-Offset im Sample-Speicher (`chn.note`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_phi`: 32-Bit ganzzahliger Pitch-Anteil (`chn.phi`), `plo` als 16-Bit-Inkrement
/// - `EBX` -> `ebx`: Linker (bzw. Mono/Right-) Sample-Wert (`VolTable[vol][bl] << 8`)
/// - `EAX` -> `eax`: Rechter Sample-Wert bei `@S081` (`VolTable[r_vol][bl] << 8`)
pub fn mix_channel_08_nointerp_hi(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let ebp_phi = chn.phi;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;
    let l_vol = (chn.l_vol.min(64)) as usize;
    let r_vol = (chn.r_vol.min(64)) as usize;

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let bl = read_sample_u8(sample_mem, esi) as usize;
        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(ebp_phi)
            .wrapping_add(if carry { 1 } else { 0 });

        if let Some(frame) = mix_buf.get_mut(di_idx) {
            match pan_mode {
                mixer_flags::F_MONO => {
                    ebx = (vol_table[l_vol][bl] as i32) << 8;
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(ebx);
                }
                mixer_flags::F_LEFT => {
                    ebx = (vol_table[l_vol][bl] as i32) << 8;
                    frame.left = frame.left.wrapping_add(ebx);
                }
                mixer_flags::F_RIGHT => {
                    ebx = (vol_table[r_vol][bl] as i32) << 8;
                    frame.right = frame.right.wrapping_add(ebx);
                }
                _ => {
                    ebx = (vol_table[l_vol][bl] as i32) << 8;
                    eax = (vol_table[r_vol][bl] as i32) << 8;
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(eax);
                }
            }
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 16-Bit-Kanal ohne Interpolation bei niedriger Schrittweite (`PHi == 0`,
/// `JmpFlags & 0x78 == 0x10`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S16`, `@M16`, `@L16` und `@R16` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 660–759).
///
/// ### Überführung von Self-Modifying Code (SMC)
/// Im Original-Assembler berechnen `@S16`..`@R16` vor der Schleife die 32-Bit-Skalierungsfaktoren
/// `movsx ebx, byte ptr lVol ; imul bx, VolMaster ; sar ebx, 9` (sowie für `rVol`) und patchen diese
/// als 32-Bit-Immediate in `imul ebx, eax, 12345678h`. Hier werden `l_scale` und `r_scale` exakt
/// nach derselben 16-Bit/32-Bit-Arithmetik berechnet.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 16-Bit-Sample-Index (`fs:[esi+esi]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` (`BP`) -> `bp_plo`: 16-Bit gebrochenes Pitch-Inkrement (`chn.plo`)
/// - `EAX` -> `eax`: Rohes 16-Bit-Sample (`movsx eax, word ptr fs:[esi+esi]`), bei `@S16` danach rechter Kanal
/// - `EBX` -> `ebx`: Skalierter linker (bzw. Mono/Right-) Kanalwert
pub fn mix_channel_16_nointerp_lo(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let bp_plo = chn.plo;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;

    // movsx ebx, byte ptr lVol ; imul bx, VolMaster ; sar ebx, 9
    // Hinweis: In 16-Bit-Register `imul bx, VolMaster` wird das 16-Bit-Produkt in BX abgelegt,
    // während die oberen 16 Bit von EBX (0x0000 bei positivem lVol <= 64) erhalten bleiben,
    // bevor `sar ebx, 9` ausgeführt wird. Wir bilden dies bit-exakt ab:
    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    'get_sample: while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let s0 = read_sample_i16(sample_mem, esi) as i32;
        match pan_mode {
            mixer_flags::F_MONO | mixer_flags::F_LEFT => {
                ebx = s0.wrapping_mul(l_scale);
            }
            mixer_flags::F_RIGHT => {
                ebx = s0.wrapping_mul(r_scale);
            }
            _ => {
                ebx = s0.wrapping_mul(l_scale);
                eax = s0.wrapping_mul(r_scale);
            }
        }

        loop {
            let (new_dx, carry) = dx_rest.overflowing_add(bp_plo);
            dx_rest = new_dx;

            if let Some(frame) = mix_buf.get_mut(di_idx) {
                match pan_mode {
                    mixer_flags::F_MONO => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    mixer_flags::F_LEFT => {
                        frame.left = frame.left.wrapping_add(ebx);
                    }
                    mixer_flags::F_RIGHT => {
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    _ => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(eax);
                    }
                }
            }
            di_idx += 1;
            cx -= 1;

            if carry {
                esi = esi.wrapping_add(1);
                break;
            }
            if cx == 0 {
                break 'get_sample;
            }
        }
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 16-Bit-Kanal ohne Interpolation bei hoher Schrittweite (`PHi != 0` bzw. `fHiPitch`,
/// `JmpFlags & 0x78 == 0x18`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S161`, `@M161`, `@L161` und `@R161` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 761–871).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 16-Bit-Sample-Index (`fs:[esi+esi]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_phi`: 32-Bit ganzzahliger Pitch-Anteil (`chn.phi`)
/// - `EBX` -> `ebx`: Skalierter linker (bzw. Mono/Right-) Sample-Wert
/// - `EAX` -> `eax`: Skalierter rechter Sample-Wert bei `@S161`
pub fn mix_channel_16_nointerp_hi(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let ebp_phi = chn.phi;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;

    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let s0 = read_sample_i16(sample_mem, esi) as i32;
        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(ebp_phi)
            .wrapping_add(if carry { 1 } else { 0 });

        if let Some(frame) = mix_buf.get_mut(di_idx) {
            match pan_mode {
                mixer_flags::F_MONO => {
                    ebx = s0.wrapping_mul(l_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(ebx);
                }
                mixer_flags::F_LEFT => {
                    ebx = s0.wrapping_mul(l_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                }
                mixer_flags::F_RIGHT => {
                    ebx = s0.wrapping_mul(r_scale);
                    frame.right = frame.right.wrapping_add(ebx);
                }
                _ => {
                    ebx = s0.wrapping_mul(l_scale);
                    eax = s0.wrapping_mul(r_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(eax);
                }
            }
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Berechnet den 16-Bit-Lautstärke-Skalierungsfaktor exakt wie die Assembler-Sequenz:
/// ```text
/// {movsx EBX, al}        DB 66h, 0Fh, 0BEh, 0D8h
/// {IMUL EBX, VolMaster}  DB 66h, 0Fh, 0AFh, 1Eh; DW oVolMaster
/// DB 66h; sar bx, 9      (-> sar ebx, 9)
/// ```
/// Wegen des 32-Bit-Operandenpräfixes `66h` im 16-Bit-Codesegment sind sowohl `IMUL EBX, dword ptr [oVolMaster]`
/// als auch `sar ebx, 9` echte 32-Bit-Operationen.
#[inline(always)]
pub const fn compute_16bit_vol_scale(vol: u8, vol_master: u16) -> i32 {
    (((vol as i8) as i32) * (vol_master as i32)) >> 9
}

// ============================================================================
// 5. Interpolated Dynamic Oversampling (Linear IDO, dacmix.pas Zeilen 872–1756)
// ============================================================================

/// Mischt einen 8-Bit-Kanal mit Interpolated Dynamic Oversampling (Linear IDO, `fOverSamp`)
/// bei niedriger Schrittweite (`Pitch <= 30000` und `PHi == 0`, `JmpFlags & 0x78 == 0x20`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S08O`, `@M08O`, `@L08O` und `@R08O` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 872–1128).
///
/// ### Funktionsweise des Low-Pitch-IDO-Algorithmus
/// Wenn ein Sample mehrfach hintereinander ausgegeben wird (`PHi == 0`), berechnet `@S08O`..`@R08O`
/// beim Eintritt in ein neues Quell-Sample `ESI` einmalig:
/// 1. die Steigung pro Ausgabeschritt: `incr = ((s1 - s0) * (Plo as i32)) >> 8` (`[oLIncr]` / `ECX`),
/// 2. den Startwert am aktuellen gebrochenen Phasenrest `DX`:
///    `val = (s0 << 8) + (((s1 - s0) * (dx as u16 as i32)) >> 8)`.
/// In der inneren Schleife (`@S08OWriteSample`..`@R08OWriteSample`) wird dann bei jedem Sample-Takt
/// ohne Carry (`add dx, bp` mit `CF == 0`) lediglich `val += incr` addiert!
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 32-Bit Byte-Offset im Sample-Speicher (`chn.note`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX` / `ECX`) -> `cx` (Schleifenzähler) und `ecx_r_incr` (rechtes Inkrement bzw. Mono/Left/Right-Inkrement)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_plo`: 32-Bit-erweitertes `Plo` (`movzx ebp, word ptr chn.Plo`)
/// - `EBX` -> `ebx`: Laufender interpolierter linker (bzw. Mono/Right-) Sample-Wert
/// - `EAX` -> `eax`: Laufender interpolierter rechter Sample-Wert bei `@S08O`
/// - `[oLIncr]` -> `l_incr`: Gespeichertes linkes Inkrement pro Ausgabetakt bei `@S08O`
pub fn mix_channel_08_oversamp_lo(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let bp_plo = chn.plo;
    let ebp_plo = bp_plo as i32;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;
    let l_vol = (chn.l_vol.min(64)) as usize;
    let r_vol = (chn.r_vol.min(64)) as usize;

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    if (esi as u32) > (iend as u32) {
        loop {
            esi = esi.wrapping_sub(lln);
            if esi < 0 {
                write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                chn.note = 0;
                return;
            }
            if (esi as u32) <= (iend as u32) {
                break;
            }
        }
    }

    // [oOldSmpL] / [oOldSmpR] vor der Schleife einmalig aus fs:[esi] initialisieren
    // (dacmix.pas Zeilen 809–819, 892–897, 976–981, 1033–1038):
    let bl_init = read_sample_u8(sample_mem, esi) as usize;
    let mut o_old_smp_l = vol_table[l_vol][bl_init] as i32;
    let mut o_old_smp_r = vol_table[r_vol][bl_init] as i32;

    'get_sample: while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let bl1 = read_sample_u8(sample_mem, esi.wrapping_add(1)) as usize;
        let edx_u16 = dx_rest as i32;

        let l_incr: i32;
        let ecx_r_incr: i32;

        match pan_mode {
            mixer_flags::F_MONO | mixer_flags::F_LEFT => {
                // @M08O / @L08O (dacmix.pas Zeilen 958–1068):
                // mov bl, fs:[esi+1] ; movsx eax, VolTable[lVol][bl1]
                // mov ebx, [oOldSmpL] ; mov [oOldSmpL], eax
                // sub eax, ebx ; mov ecx, eax ; imul ecx, ebp ; sar ecx, 8
                // imul eax, edx ; sar eax, 8 ; shl ebx, 8 ; add ebx, eax
                let s1 = vol_table[l_vol][bl1] as i32;
                let s0 = o_old_smp_l;
                o_old_smp_l = s1;
                let diff = s1.wrapping_sub(s0);
                ecx_r_incr = diff.wrapping_mul(ebp_plo) >> 8;
                l_incr = ecx_r_incr;
                let frac = diff.wrapping_mul(edx_u16) >> 8;
                ebx = (s0 << 8).wrapping_add(frac);
            }
            mixer_flags::F_RIGHT => {
                // @R08O (dacmix.pas Zeilen 1071–1128):
                let s1 = vol_table[r_vol][bl1] as i32;
                let s0 = o_old_smp_r;
                o_old_smp_r = s1;
                let diff = s1.wrapping_sub(s0);
                ecx_r_incr = diff.wrapping_mul(ebp_plo) >> 8;
                l_incr = ecx_r_incr;
                let frac = diff.wrapping_mul(edx_u16) >> 8;
                ebx = (s0 << 8).wrapping_add(frac);
            }
            _ => {
                // @S08O (dacmix.pas Zeilen 872–956):
                // Linker Kanal (lVol):
                let l_s1 = vol_table[l_vol][bl1] as i32;
                let l_s0 = o_old_smp_l;
                o_old_smp_l = l_s1;
                let l_diff = l_s1.wrapping_sub(l_s0);
                l_incr = l_diff.wrapping_mul(ebp_plo) >> 8;
                let l_frac = l_diff.wrapping_mul(edx_u16) >> 8;
                let o_l_smp = (l_s0 << 8).wrapping_add(l_frac);

                // Rechter Kanal (rVol):
                let r_s1 = vol_table[r_vol][bl1] as i32;
                let r_s0 = o_old_smp_r;
                o_old_smp_r = r_s1;
                let r_diff = r_s1.wrapping_sub(r_s0);
                ecx_r_incr = r_diff.wrapping_mul(ebp_plo) >> 8;
                let r_frac = r_diff.wrapping_mul(edx_u16) >> 8;
                eax = (r_s0 << 8).wrapping_add(r_frac);
                ebx = o_l_smp;
            }
        }

        // Innere Schleife (@*08OWriteSample):
        loop {
            if let Some(frame) = mix_buf.get_mut(di_idx) {
                match pan_mode {
                    mixer_flags::F_MONO => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    mixer_flags::F_LEFT => {
                        frame.left = frame.left.wrapping_add(ebx);
                    }
                    mixer_flags::F_RIGHT => {
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    _ => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(eax);
                    }
                }
            }
            di_idx += 1;
            cx -= 1;

            let (new_dx, carry) = dx_rest.overflowing_add(bp_plo);
            dx_rest = new_dx;

            if cx == 0 {
                if carry {
                    esi = esi.wrapping_add(1);
                }
                break 'get_sample;
            }
            if carry {
                esi = esi.wrapping_add(1);
                break;
            }

            match pan_mode {
                mixer_flags::F_MONO | mixer_flags::F_LEFT | mixer_flags::F_RIGHT => {
                    ebx = ebx.wrapping_add(ecx_r_incr);
                }
                _ => {
                    ebx = ebx.wrapping_add(l_incr);
                    eax = eax.wrapping_add(ecx_r_incr);
                }
            }
        }
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 8-Bit-Kanal mit Interpolated Dynamic Oversampling (Linear IDO, `fOverSamp`)
/// bei hoher Schrittweite (`Pitch > 30000` oder `PHi != 0`, `JmpFlags & 0x78 == 0x28`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S08O1`, `@M08O1`, `@L08O1` und `@R08O1` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 1129–1331).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 32-Bit Byte-Offset im Sample-Speicher (`chn.note`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX` / `ECX`) -> `cx` (gesichert in `[o_tmp]`) bzw. in `@S08O1` rechter Stützpunkt `s0_right`
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_phi`: 32-Bit ganzzahliger Pitch-Anteil (`chn.phi`)
/// - `EBX` -> `ebx`: Linker (bzw. Mono/Right-) interpolierter Sample-Wert
/// - `EAX` -> `eax`: Rechter interpolierter Sample-Wert bei `@S08O1`
pub fn mix_channel_08_oversamp_hi(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let ebp_phi = chn.phi;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;
    let l_vol = (chn.l_vol.min(64)) as usize;
    let r_vol = (chn.r_vol.min(64)) as usize;

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let bl0 = read_sample_u8(sample_mem, esi) as usize;
        let bl1 = read_sample_u8(sample_mem, esi.wrapping_add(1)) as usize;
        let edx_u16 = dx_rest as i32;

        match pan_mode {
            mixer_flags::F_MONO => {
                let s1 = vol_table[l_vol][bl1] as i32;
                let s0 = vol_table[l_vol][bl0] as i32;
                let frac = (s1.wrapping_sub(s0)).wrapping_mul(edx_u16) >> 8;
                ebx = (s0 << 8).wrapping_add(frac);
                if let Some(frame) = mix_buf.get_mut(di_idx) {
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(ebx);
                }
            }
            mixer_flags::F_LEFT => {
                let s1 = vol_table[l_vol][bl1] as i32;
                let s0 = vol_table[l_vol][bl0] as i32;
                let frac = (s1.wrapping_sub(s0)).wrapping_mul(edx_u16) >> 8;
                ebx = (s0 << 8).wrapping_add(frac);
                if let Some(frame) = mix_buf.get_mut(di_idx) {
                    frame.left = frame.left.wrapping_add(ebx);
                }
            }
            mixer_flags::F_RIGHT => {
                let s1 = vol_table[r_vol][bl1] as i32;
                let s0 = vol_table[r_vol][bl0] as i32;
                let frac = (s1.wrapping_sub(s0)).wrapping_mul(edx_u16) >> 8;
                ebx = (s0 << 8).wrapping_add(frac);
                if let Some(frame) = mix_buf.get_mut(di_idx) {
                    frame.right = frame.right.wrapping_add(ebx);
                }
            }
            _ => {
                // @S08O1 (dacmix.pas Zeilen 1129–1194):
                // Linker Kanal:
                let l_s1 = vol_table[l_vol][bl1] as i32;
                let l_s0 = vol_table[l_vol][bl0] as i32;
                let l_frac = (l_s1.wrapping_sub(l_s0)).wrapping_mul(edx_u16) >> 8;
                // Rechter Kanal:
                let r_s1 = vol_table[r_vol][bl1] as i32;
                let r_s0 = vol_table[r_vol][bl0] as i32;
                let r_frac = (r_s1.wrapping_sub(r_s0)).wrapping_mul(edx_u16) >> 8;

                ebx = (l_s0 << 8).wrapping_add(l_frac);
                eax = (r_s0 << 8).wrapping_add(r_frac);
                if let Some(frame) = mix_buf.get_mut(di_idx) {
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(eax);
                }
            }
        }

        // add dx, Plo ; adc esi, ebp
        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(ebp_phi)
            .wrapping_add(if carry { 1 } else { 0 });

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 16-Bit-Kanal mit Interpolated Dynamic Oversampling (Linear IDO, `fOverSamp`,
/// `JmpFlags & 0x70 == 0x30`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@S16O1`, `@M16O1`, `@L16O1` und `@R16O1` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 1332–1527).
/// Hinweis: In `@JmpTable` (Zeilen 320–329) zeigen sowohl die Low-Pitch-Einträge (`0x30..0x36`)
/// als auch die High-Pitch-Einträge (`0x38..0x3E`) auf `@M16o1..@S16o1`. Zusätzlich ist die
/// inkrementelle Low-Pitch-Variante `@S16O..@R16O` (Zeilen 1528–1756) als [`mix_channel_16_oversamp_lo`]
/// verfügbar.
///
/// ### Mathematische Formel (bit-identisch zu `@S16O1..@R16O1`)
/// ```text
/// movsx ebx, word ptr fs:[esi+esi]
/// movsx eax, word ptr fs:[esi+esi+2]
/// sub   eax, ebx
/// sar   eax, 1
/// imul  eax, edx        ; edx = movzx edx, word ptr Rest
/// sar   eax, 15
/// add   eax, ebx
/// imul  ebx, eax, l_scale
/// imul  eax, eax, r_scale
/// ```
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 16-Bit-Sample-Index (`fs:[esi+esi]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_phi`: 32-Bit ganzzahliger Pitch-Anteil (`chn.phi`)
/// - `EBX` -> `ebx`: Zuerst `s0`, danach skalierter linker (bzw. Mono/Right-) Sample-Wert
/// - `EAX` -> `eax`: Zuerst `s1`, dann interpoliertes Sample, danach skalierter rechter Sample-Wert
pub fn mix_channel_16_oversamp_hi(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let ebp_phi = chn.phi;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;

    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let s0 = read_sample_i16(sample_mem, esi) as i32;
        let s1 = read_sample_i16(sample_mem, esi.wrapping_add(1)) as i32;
        let edx_u16 = dx_rest as i32;

        // sub eax, ebx ; sar eax, 1 ; imul eax, edx ; sar eax, 15 ; add eax, ebx
        let mut interp = s1.wrapping_sub(s0);
        interp >>= 1;
        interp = interp.wrapping_mul(edx_u16);
        interp >>= 15;
        interp = interp.wrapping_add(s0);

        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(ebp_phi)
            .wrapping_add(if carry { 1 } else { 0 });

        if let Some(frame) = mix_buf.get_mut(di_idx) {
            match pan_mode {
                mixer_flags::F_MONO => {
                    ebx = interp.wrapping_mul(l_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(ebx);
                }
                mixer_flags::F_LEFT => {
                    ebx = interp.wrapping_mul(l_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                }
                mixer_flags::F_RIGHT => {
                    ebx = interp.wrapping_mul(r_scale);
                    frame.right = frame.right.wrapping_add(ebx);
                }
                _ => {
                    ebx = interp.wrapping_mul(l_scale);
                    eax = interp.wrapping_mul(r_scale);
                    frame.left = frame.left.wrapping_add(ebx);
                    frame.right = frame.right.wrapping_add(eax);
                }
            }
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Inkrementelle Low-Pitch-Variante des 16-Bit Linear-IDO-Mixers (`@S16O`, `@M16O`, `@L16O`, `@R16O`
/// in `legacy_src/PLAYER/dacmix.pas` Zeilen 1528–1756).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 16-Bit-Sample-Index (`fs:[esi+esi]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` (`CX` / `ECX`) -> `cx` (Schleifenzähler) und `ecx_r_incr` (rechtes bzw. Mono-Inkrement)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` -> `ebp_plo`: `Plo` als 32-Bit-Faktor (`movzx ebp, word ptr chn.Plo`)
/// - `EBX` -> `ebx`: Laufender linker (bzw. Mono/Right-) Sample-Wert
/// - `EAX` -> `eax`: Laufender rechter Sample-Wert bei `@S16O`
pub fn mix_channel_16_oversamp_lo(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let bp_plo = chn.plo;
    let ebp_plo = bp_plo as i32;
    let pan_mode = chn.jmp_flags & mixer_flags::F_PAN_MASK;

    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    'get_sample: while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    write_chn_end_tail(mix_buf, di_idx, cx, pan_mode, ebx, eax);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let s0 = read_sample_i16(sample_mem, esi) as i32;
        let s1 = read_sample_i16(sample_mem, esi.wrapping_add(1)) as i32;
        let edx_u16 = dx_rest as i32;

        let l_incr: i32;
        let ecx_r_incr: i32;

        match pan_mode {
            mixer_flags::F_MONO | mixer_flags::F_LEFT => {
                // @M16O / @L16O (dacmix.pas Zeilen 1599–1702):
                let ebp_s0 = s0.wrapping_mul(l_scale);
                let eax_s1 = s1.wrapping_mul(l_scale);
                let diff_half = eax_s1.wrapping_sub(ebp_s0) >> 1;
                ecx_r_incr = diff_half.wrapping_mul(ebp_plo) >> 15;
                l_incr = ecx_r_incr;
                let frac = diff_half.wrapping_mul(edx_u16) >> 15;
                ebx = ebp_s0.wrapping_add(frac);
            }
            mixer_flags::F_RIGHT => {
                // @R16O (dacmix.pas Zeilen 1705–1756):
                let ebp_s0 = s0.wrapping_mul(r_scale);
                let eax_s1 = s1.wrapping_mul(r_scale);
                let diff_half = eax_s1.wrapping_sub(ebp_s0) >> 1;
                ecx_r_incr = diff_half.wrapping_mul(ebp_plo) >> 15;
                l_incr = ecx_r_incr;
                let frac = diff_half.wrapping_mul(edx_u16) >> 15;
                ebx = ebp_s0.wrapping_add(frac);
            }
            _ => {
                // @S16O (dacmix.pas Zeilen 1528–1597):
                let l_s0 = s0.wrapping_mul(l_scale);
                let l_s1 = s1.wrapping_mul(l_scale);
                let l_diff_half = l_s1.wrapping_sub(l_s0) >> 1;
                l_incr = l_diff_half.wrapping_mul(ebp_plo) >> 15;
                let l_frac = l_diff_half.wrapping_mul(edx_u16) >> 15;
                let o_l_smp = l_s0.wrapping_add(l_frac);

                let r_s0 = s0.wrapping_mul(r_scale);
                let r_s1 = s1.wrapping_mul(r_scale);
                let r_diff_half = r_s1.wrapping_sub(r_s0) >> 1;
                ecx_r_incr = r_diff_half.wrapping_mul(ebp_plo) >> 15;
                let r_frac = r_diff_half.wrapping_mul(edx_u16) >> 15;
                eax = r_s0.wrapping_add(r_frac);
                ebx = o_l_smp;
            }
        }

        loop {
            if let Some(frame) = mix_buf.get_mut(di_idx) {
                match pan_mode {
                    mixer_flags::F_MONO => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    mixer_flags::F_LEFT => {
                        frame.left = frame.left.wrapping_add(ebx);
                    }
                    mixer_flags::F_RIGHT => {
                        frame.right = frame.right.wrapping_add(ebx);
                    }
                    _ => {
                        frame.left = frame.left.wrapping_add(ebx);
                        frame.right = frame.right.wrapping_add(eax);
                    }
                }
            }
            di_idx += 1;
            cx -= 1;

            let (new_dx, carry) = dx_rest.overflowing_add(bp_plo);
            dx_rest = new_dx;

            if cx == 0 {
                if carry {
                    esi = esi.wrapping_add(1);
                }
                break 'get_sample;
            }
            if carry {
                esi = esi.wrapping_add(1);
                break;
            }

            match pan_mode {
                mixer_flags::F_MONO | mixer_flags::F_LEFT | mixer_flags::F_RIGHT => {
                    ebx = ebx.wrapping_add(ecx_r_incr);
                }
                _ => {
                    ebx = ebx.wrapping_add(l_incr);
                    eax = eax.wrapping_add(ecx_r_incr);
                }
            }
        }
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

// ============================================================================
// 6. Cubic-Spline IDO Resampling (CPU 32-Bit & x87 FPU, dacmix.pas Zeilen 1758–2405)
// ============================================================================

/// Mischt einen 8-Bit-Kanal mit 32-Bit Integer Cubic-Spline-Interpolation (`fCPUCubic`,
/// `JmpFlags & 0x70 == 0x40`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@Cs08LoS` / `@Cs08HiS` (sowie die über SMC gepatchten Panning-Varianten für Mono, Left,
/// Right und Panned) in `legacy_src/PLAYER/dacmix.pas` (Zeilen 1758–1869).
///
/// ### Überführung von Self-Modifying Code (SMC)
/// Im Original-Assembler modifiziert der Code vor Schleifeneintritt die Befehle ab `@@s1`
/// (Zeilen 1766–1800) abhängig von `JmpFlags & fPanned`, um zwischen Stereo-Panned, Mono,
/// Left-Only und Right-Only umzuschalten, und patcht `l_scale` / `r_scale` als 32-Bit-Immediate.
/// Hier wird dies über `pan_mode` sauber verzweigt, während die innere 4-Punkt-Lagrange/Hermite-
/// Polynomauswertung (mit Rundungs-Bias `+(1 << 15)` vor `sar 16` und `+(1 << 8)` vor `sar 9`)
/// bit-identisch erhalten bleibt.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 32-Bit Byte-Offset im Sample-Speicher (`fs:[esi-1..esi+2]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `[o_tmp]` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` -> Temporär `ym1`, danach `edx_x` (`movzx edx, word ptr chn.Rest`)
/// - `EBP` (`[ebp]`) -> `ebp_d`: Temporärer Zwischenspeicher (`oTemp`) für Stützpunkt `d = Y0`
/// - `EBX` -> `ebx`: Koeffizienten-Akkumulator (`Y0 - Y1`, später `b`), am Ende linker Ausgabewert
/// - `ECX` -> `ecx_c`: Koeffizient `c = Y1 - Ym1`
/// - `EAX` -> `eax`: Polynom-Akkumulator (`(((a*x >> 16) - b)*x >> 16 + c)*x >> 9 + (d << 8)`),
///   am Ende rechter Ausgabewert
pub fn mix_channel_08_cpu_cubic(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let phi = chn.phi;

    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    // @Cs08LoS/@Cs08HiS springen bei OneShot-Ende mit EAX=l_sample, EBX=r_sample
                    // nach @WriteChnSEnd (add [di], ebx; add [di+4], eax)
                    write_chn_end_tail(mix_buf, di_idx, cx, mixer_flags::F_PANNED, eax, ebx);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        // 4 Stützpunkte laden: Ym1 = fs:[esi-1], Y0 = fs:[esi], Y1 = fs:[esi+1], Y2 = fs:[esi+2]
        let y0 = read_sample_i8(sample_mem, esi) as i32;
        let y1 = read_sample_i8(sample_mem, esi.wrapping_add(1)) as i32;
        let ym1 = read_sample_i8(sample_mem, esi.wrapping_sub(1)) as i32;
        let y2 = read_sample_i8(sample_mem, esi.wrapping_add(2)) as i32;

        let mut ebp_d = y0; // mov [ebp], ebx
        ebx = y0.wrapping_sub(y1); // sub ebx, ecx (Y0 - Y1)
        let ecx_c = y1.wrapping_sub(ym1); // sub ecx, eax (c = Y1 - Ym1)
        eax = y2.wrapping_add(ebx.wrapping_mul(2)); // lea eax, [eax+2*ebx]
        ebx = ebx.wrapping_sub(ym1); // sub ebx, edx
        eax = eax.wrapping_add(ebx); // add eax, ebx (a = 3*(Y0 - Y1) - Ym1 + Y2)
        ebx = ebx.wrapping_add(eax); // add ebx, eax
        let edx_x = dx_rest as i32; // movzx edx, word ptr chn.Rest
        eax = eax.wrapping_mul(edx_x).wrapping_add(1 << 15) >> 16; // imul eax, edx ; add eax, 1 shl 15 ; sar eax, 16
        ebx = ebx.wrapping_add(ebp_d); // add ebx, [ebp] (b)
        ebp_d <<= 8; // shl dword ptr [ebp], 8
        eax = eax.wrapping_sub(ebx); // sub eax, ebx
        eax = eax.wrapping_mul(edx_x).wrapping_add(1 << 15) >> 16; // imul eax, edx ; add eax, 1 shl 15 ; sar eax, 16
        eax = eax.wrapping_add(ecx_c); // add eax, ecx
        eax = eax.wrapping_mul(edx_x).wrapping_add(1 << 8) >> 9; // imul eax, edx ; add eax, 1 shl 8 ; sar eax, 9
        eax = eax.wrapping_add(ebp_d); // add eax, [ebp]

        // Phasen-Inkrement (dx += Plo; esi += PHi + CF)
        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(phi)
            .wrapping_add(if carry { 1 } else { 0 });

        // @@s1: Volume-Scaling und Addition in den Zielpuffer
        let interp = eax;
        ebx = interp.wrapping_mul(l_scale);
        eax = interp.wrapping_mul(r_scale);
        if let Some(frame) = mix_buf.get_mut(di_idx) {
            frame.left = frame.left.wrapping_add(ebx);
            frame.right = frame.right.wrapping_add(eax);
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 16-Bit-Kanal mit 32-Bit Integer Cubic-Spline-Interpolation (`fCPUCubic | f16Bit`,
/// `JmpFlags & 0x70 == 0x50`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@Cs16HiS` (sowie die Panning-Varianten für Mono, Left, Right und Panned) in
/// `legacy_src/PLAYER/dacmix.pas` (Zeilen 1871–1986).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi`: 16-Bit-Sample-Index (`fs:[esi+esi-2 .. esi+esi+4]`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `[o_tmp]` (`CX`) -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` -> Temporär `ym1`, danach `edx_x` (`movzx edx, word ptr chn.Rest`)
/// - `EBP` (`[ebp]`) -> `ebp_d`: Temporärer Zwischenspeicher (`oTemp`) für Stützpunkt `d = Y0`
/// - `EBX` -> `ebx`: Koeffizienten-Akkumulator (`Y0 - Y1`, später `b`), am Ende linker Ausgabewert
/// - `ECX` -> `ecx_c`: Koeffizient `c = Y1 - Ym1`
/// - `EAX` -> `eax`: Polynom-Akkumulator mit Zwischen-Shifts (`sar 3`, `sar 13`, `sar 3`, `sar 13`,
///   `sar 3`, `sar 14`), um 32-Bit-Überläufe bei 16-Bit-Samples zu verhindern; am Ende rechter Ausgabewert
pub fn mix_channel_16_cpu_cubic(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let phi = chn.phi;

    let l_scale = compute_16bit_vol_scale(chn.l_vol, vol_master);
    let r_scale = compute_16bit_vol_scale(chn.r_vol, vol_master);

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    // @Cs16HiS springt bei OneShot-Ende mit EAX=l_sample, EBX=r_sample
                    // nach @WriteChnSEnd16 (add [di], ebx; add [di+4], eax)
                    write_chn_end_tail(mix_buf, di_idx, cx, mixer_flags::F_PANNED, eax, ebx);
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let y0 = read_sample_i16(sample_mem, esi) as i32;
        let y1 = read_sample_i16(sample_mem, esi.wrapping_add(1)) as i32;
        let ym1 = read_sample_i16(sample_mem, esi.wrapping_sub(1)) as i32;
        let y2 = read_sample_i16(sample_mem, esi.wrapping_add(2)) as i32;

        let ebp_d = y0; // mov [ebp], ebx
        ebx = y0.wrapping_sub(y1); // sub ebx, ecx
        let ecx_c = y1.wrapping_sub(ym1); // sub ecx, eax
        eax = y2.wrapping_add(ebx.wrapping_mul(2)); // lea eax, [eax+2*ebx]
        ebx = ebx.wrapping_sub(ym1); // sub ebx, edx
        eax = eax.wrapping_add(ebx); // add eax, ebx (a)
        ebx = ebx.wrapping_add(eax); // add ebx, eax
        let edx_x = dx_rest as i32; // movzx edx, word ptr chn.Rest
        eax = eax.wrapping_add(4) >> 3; // add eax, 4 ; sar eax, 3
        eax = eax.wrapping_mul(edx_x).wrapping_add(4096) >> 13; // imul eax, edx ; add eax, 4096 ; sar eax, 13
        ebx = ebx.wrapping_add(ebp_d); // add ebx, [ebp]
        eax = eax.wrapping_sub(ebx); // sub eax, ebx
        eax = eax.wrapping_add(4) >> 3; // add eax, 4 ; sar eax, 3
        eax = eax.wrapping_mul(edx_x).wrapping_add(4096) >> 13; // imul eax, edx ; add eax, 4096 ; sar eax, 13
        eax = eax.wrapping_add(ecx_c); // add eax, ecx
        eax = eax.wrapping_add(4) >> 3; // add eax, 4 ; sar eax, 3
        eax = eax.wrapping_mul(edx_x).wrapping_add(8192) >> 14; // imul eax, edx ; add eax, 8192 ; sar eax, 14
        eax = eax.wrapping_add(ebp_d); // add eax, [ebp]

        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(phi)
            .wrapping_add(if carry { 1 } else { 0 });

        let interp = eax;
        ebx = interp.wrapping_mul(l_scale);
        eax = interp.wrapping_mul(r_scale);
        if let Some(frame) = mix_buf.get_mut(di_idx) {
            frame.left = frame.left.wrapping_add(ebx);
            frame.right = frame.right.wrapping_add(eax);
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Mischt einen 8-Bit- oder 16-Bit-Kanal mit x87 FPU Cubic-Spline-Interpolation (`fFPUCubic`,
/// `JmpFlags & 0x60 == 0x60`).
///
/// ### Entsprechende ASM-Prozeduren
/// `@fCs08LoS`, `@fCs08HiS`, `@fCs16LoS` und `@fCs16HiS` in `legacy_src/PLAYER/dacmix.pas`
/// (Zeilen 1988–2405).
///
/// ### Register-Mapping (x86 / x87 -> Rust)
/// - `ESI` -> `esi`: Sample-Index (`chn.note`)
/// - `EDI` (`DI`) -> `di_idx`: Aktueller Frame-Index im 32-Bit-Mischpuffer (`mix_buf`)
/// - `ECX` / `[o_tmp]` -> `cx`: Verbleibende Anzahl zu mischender Stereo-Frames (`buf_samples + 1`)
/// - `EDX` (`DX`) -> `dx_rest`: 16-Bit gebrochener Phasenakkumulator (`chn.rest as u16`)
/// - `EBP` (`BP`) -> `plo` / `phi`: Pitch-Schrittweite
/// - `ST(0)..ST(7)` -> `ym1, y0, y1, y2, a, b, c, d, x`: Gleitkomma-Register des x87-Stack
/// - `EBX` / `EAX` -> `ebx` (links) / `eax` (rechts) nach `fistp dword ptr [oTemp]` und Skalierung
pub fn mix_channel_fpu_cubic(
    chn: &mut MixChannel,
    mix_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_master: u16,
    total_frames: usize,
) {
    const ONE_BY_65536: f64 = 1.0 / 65536.0;
    const ONE_BY_4: f64 = 1.0 / 4.0;
    const ONE_BY_1024: f64 = 1.0 / 1024.0;

    let mut esi = chn.note;
    let iend = chn.iend;
    let lln = chn.lln;
    let mut dx_rest = chn.rest as u16;
    let plo = chn.plo;
    let phi = chn.phi;
    let is_16bit = (chn.jmp_flags & mixer_flags::F_16BIT) != 0;

    let scale_factor = if is_16bit { ONE_BY_1024 } else { ONE_BY_4 };
    let l_vol_fpu =
        (((chn.l_vol as i8 as i32) * (vol_master as i16 as i32)) as f64) * scale_factor;
    let r_vol_fpu =
        (((chn.r_vol as i8 as i32) * (vol_master as i16 as i32)) as f64) * scale_factor;

    let mut di_idx = 0usize;
    let mut cx = total_frames;
    let mut ebx: i32 = 0;
    let mut eax: i32 = 0;

    while cx > 0 {
        if (esi as u32) > (iend as u32) {
            loop {
                esi = esi.wrapping_sub(lln);
                if esi < 0 {
                    // @fCs08*/@fCs16* laden bei OneShot-Ende EAX=[o_d] (links) und EBX=[o_c] (rechts)
                    // und springen nach @WriteChnSEnd16 (add [di], ebx; add [di+4], eax)
                    write_chn_end_tail(mix_buf, di_idx, cx, mixer_flags::F_PANNED, eax, ebx);
                    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
                    chn.note = 0;
                    return;
                }
                if (esi as u32) <= (iend as u32) {
                    break;
                }
            }
        }

        let (ym1, y0, y1, y2) = if is_16bit {
            (
                read_sample_i16(sample_mem, esi.wrapping_sub(1)) as f64,
                read_sample_i16(sample_mem, esi) as f64,
                read_sample_i16(sample_mem, esi.wrapping_add(1)) as f64,
                read_sample_i16(sample_mem, esi.wrapping_add(2)) as f64,
            )
        } else {
            (
                read_sample_i8(sample_mem, esi.wrapping_sub(1)) as i16 as f64,
                read_sample_i8(sample_mem, esi) as i16 as f64,
                read_sample_i8(sample_mem, esi.wrapping_add(1)) as i16 as f64,
                read_sample_i8(sample_mem, esi.wrapping_add(2)) as i16 as f64,
            )
        };

        let y0_m_y1 = y0 - y1;
        let a = y0_m_y1 * 3.0 - ym1 + y2;
        let b = y0 * 5.0 - y1 * 4.0 - ym1 * 2.0 + y2;
        let c = y1 - ym1;
        let x = (dx_rest as f64) * ONE_BY_65536;

        // @fCs08LoS/@fCs08HiS/@fCs16LoS/@fCs16HiS: Y = ((a*x - b)*x + c)*x + 2*y0
        let poly = ((a * x - b) * x + c) * x + y0 * 2.0;

        let (new_dx, carry) = dx_rest.overflowing_add(plo);
        dx_rest = new_dx;
        esi = esi
            .wrapping_add(phi)
            .wrapping_add(if carry { 1 } else { 0 });

        ebx = (poly * l_vol_fpu).round_ties_even() as i32;
        eax = (poly * r_vol_fpu).round_ties_even() as i32;
        if let Some(frame) = mix_buf.get_mut(di_idx) {
            frame.left = frame.left.wrapping_add(ebx);
            frame.right = frame.right.wrapping_add(eax);
        }

        di_idx += 1;
        cx -= 1;
    }

    chn.rest = ((chn.rest as u32 & 0xFFFF_0000) | (dx_rest as u32)) as i32;
    write_chn_step_back(chn, esi);
}

/// Kanal-Sprungverteiler analog zu `@JmpTable` in `legacy_src/PLAYER/dacmix.pas` (Zeilen 288–359).
///
/// Wählt anhand von `chn.jmp_flags` (Bits `6..1`) die passende Mischroutine aus und mischt
/// `total_frames = buf_samples + 1` Stereo-Frames in den Zielpuffer (`mix_buffer` oder `sur_buffer`).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `BX` (`tMixChannel([oAsmChn]).JmpFlags`) -> `chn.jmp_flags`: Index in `@JmpTable`
/// - `ES:DI` (`word ptr tMixChannel([oAsmChn]).MixBuf`) -> `target_buf`: Ziel-Mischpuffer
/// - `CX` (`BufSamples + 1`) -> `total_frames`
pub fn dispatch_mix_channel(
    chn: &mut MixChannel,
    target_buf: &mut [StereoSample32],
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    vol_master: u16,
    total_frames: usize,
) {
    if chn.note == 0 || total_frames == 0 {
        return;
    }

    // Obere 4 Modus-Bits (Bits 6..3: Interpolation | 16Bit | HiPitch)
    match (chn.jmp_flags >> 3) & 0x0F {
        // 0x00..0x06: @M08, @L08, @R08, @S08 (8-Bit, kein Interp, Low-Pitch)
        0x00 => mix_channel_08_nointerp_lo(chn, target_buf, sample_mem, vol_table, total_frames),
        // 0x08..0x0E: @M081, @L081, @R081, @S081 (8-Bit, kein Interp, High-Pitch)
        0x01 => mix_channel_08_nointerp_hi(chn, target_buf, sample_mem, vol_table, total_frames),
        // 0x10..0x16: @M16, @L16, @R16, @S16 (16-Bit, kein Interp, Low-Pitch)
        0x02 => mix_channel_16_nointerp_lo(chn, target_buf, sample_mem, vol_master, total_frames),
        // 0x18..0x1E: @M161, @L161, @R161, @S161 (16-Bit, kein Interp, High-Pitch)
        0x03 => mix_channel_16_nointerp_hi(chn, target_buf, sample_mem, vol_master, total_frames),
        // 0x20..0x26: @M08o, @L08o, @R08o, @S08o (8-Bit, Linear IDO, Low-Pitch)
        0x04 => mix_channel_08_oversamp_lo(chn, target_buf, sample_mem, vol_table, total_frames),
        // 0x28..0x2E: @M08o1, @L08o1, @R08o1, @S08o1 (8-Bit, Linear IDO, High-Pitch)
        0x05 => mix_channel_08_oversamp_hi(chn, target_buf, sample_mem, vol_table, total_frames),
        // 0x30..0x3E: @M16o1, @L16o1, @R16o1, @S16o1 (16-Bit, Linear IDO, Low & High-Pitch in @JmpTable)
        0x06 | 0x07 => {
            mix_channel_16_oversamp_hi(chn, target_buf, sample_mem, vol_master, total_frames)
        }
        // 0x40..0x4E: @Cs08LoS / @Cs08HiS (8-Bit, CPU 32-Bit Cubic-Spline)
        0x08 | 0x09 => {
            mix_channel_08_cpu_cubic(chn, target_buf, sample_mem, vol_master, total_frames)
        }
        // 0x50..0x5E: @Cs16HiS (16-Bit, CPU 32-Bit Cubic-Spline)
        0x0A | 0x0B => {
            mix_channel_16_cpu_cubic(chn, target_buf, sample_mem, vol_master, total_frames)
        }
        // 0x60..0x7E: @fCs08LoS, @fCs08HiS, @fCs16LoS, @fCs16HiS (x87 FPU Cubic-Spline)
        _ => mix_channel_fpu_cubic(chn, target_buf, sample_mem, vol_master, total_frames),
    }
}

// ============================================================================
// 7. Click-Removal, Surround-Ringpuffer & Buffer-Clipping (dacmix.pas Zeilen 2457–2706)
// ============================================================================

/// Führt die lineare Click-Removal-Rampe (`@SlideOut` bzw. `@SurFadeLoop` in `dacmix.pas`
/// Zeilen 2457–2530) über einen 32-Bit-Stereo-Mischpuffer aus.
///
/// ### Funktionsweise im Original-Assembler
/// Beim Übergang zwischen zwei Mix-Blöcken (z. B. nach einem Instrumenten-, Lautstärke- oder
/// Sample-Wechsel) speichert `lFadeOut` / `rFadeOut` den Ziel-Pegel vom Ende des vorangegangenen
/// Blocks (`buf[buf_samples]`). Zu Beginn von `@SlideOut` wird die Sprungdifferenz zum neuen
/// Blockanfang (`buf[0]`) gebildet:
/// ```text
/// mov eax, lFadeOut ; sub eax, es:[di]
/// mov ebx, rFadeOut ; sub ebx, es:[di+4]
/// cdq ; idiv ecx ; mov esi, eax          ; esi = l_step = diff_l / num_samples
/// mov eax, ebx ; cdq ; idiv ecx          ; edx = r_step = diff_r / num_samples
/// ```
/// Anschließend wird diese Differenz über `cx = buf_samples` Frames linear abgebaut (`sub eax, esi`,
/// `sub ebx, edx`) und am Ende der neue Endwert `eax + buf[buf_samples].left` in `lFadeOut`
/// gesichert.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `EDI` (`DI`) -> `di_idx`: Laufender Index in `buf` (`0..buf_samples`)
/// - `ECX` (`CX`) -> `ecx_num_samples` (`num_samples as i32`) bzw. Schleifenzähler `buf_samples`
/// - `EAX` -> `eax_l`: Laufender linker Ausgleichswert (initial `l_fade_out - buf[0].left`)
/// - `EBX` -> `ebx_r`: Laufender rechter Ausgleichswert (initial `r_fade_out - buf[0].right`)
/// - `ESI` -> `esi_l_step`: Linkes Rampen-Dekrement pro Sample (`eax_l / num_samples`)
/// - `EDX` -> `edx_r_step`: Rechtes Rampen-Dekrement pro Sample (`ebx_r / num_samples`)
pub fn apply_click_removal_slide(
    buf: &mut [StereoSample32],
    buf_samples: usize,
    num_samples: u16,
    l_fade_out: &mut i32,
    r_fade_out: &mut i32,
) {
    if buf_samples == 0 || buf.len() <= buf_samples || num_samples == 0 {
        return;
    }

    let ecx_div = num_samples as i32;
    let mut eax_l = l_fade_out.wrapping_sub(buf[0].left);
    let mut ebx_r = r_fade_out.wrapping_sub(buf[0].right);

    // cdq ; idiv ecx (vorzeichenbehaftete 32-Bit-Integerdivision gegen 0 gerundet)
    let esi_l_step = eax_l.wrapping_div(ecx_div);
    let edx_r_step = ebx_r.wrapping_div(ecx_div);

    // eax stellt vor der Schleife lFadeOut - buf[0].left wieder her
    for frame in buf.iter_mut().take(buf_samples) {
        frame.left = frame.left.wrapping_add(eax_l);
        frame.right = frame.right.wrapping_add(ebx_r);
        eax_l = eax_l.wrapping_sub(esi_l_step);
        ebx_r = ebx_r.wrapping_sub(edx_r_step);
    }

    // Nach der Schleife zeigt DI auf Frame `buf_samples`:
    // add eax, es:[di] ; add ebx, es:[di+4] ; mov lFadeOut, eax ; mov rFadeOut, ebx
    *l_fade_out = eax_l.wrapping_add(buf[buf_samples].left);
    *r_fade_out = ebx_r.wrapping_add(buf[buf_samples].right);
}

/// Verarbeitet den ProLogic-Surround-Verzögerungs-Ringpuffer und mischt das verzögerte Signal
/// in den Hauptpuffer (`@SurrLoop` in `legacy_src/PLAYER/dacmix.pas` Zeilen 2531–2558).
///
/// ### Funktionsweise im Original-Assembler (`@SurrLoop`)
/// - `SI` (`BufStart`) und `BX` (`BufEnd`) sind Byte-Offsets (`0..8191`, Schrittweite 8 Bytes)
///   im 8192-Byte-Ringpuffer `SurrPtr` (`SURR_BUFFER_LEN = 8192`, Maske `0x1FFF`).
/// - Für jedes der `cx = buf_samples` Stereo-Frames:
///   1. Liest `eax = sur_buf[i].left` und schreibt es gekreuzt nach `surr_ring[bx].right` (`[bx+4]`).
///   2. Liest `eax = sur_buf[i].right` und schreibt es gekreuzt nach `surr_ring[bx].left` (`[bx]`).
///   3. Addiert das verzögerte Paar `surr_ring[si]` (`left`, `right`) auf `sur_buf[i]`.
///   4. Addiert die Summe aus `sur_buf[i]` auf den Haupt-Mischpuffer `mix_buf[i]`.
///   5. Inkrementiert `si = (si + 8) & 0x1FFF` und `bx = (bx + 8) & 0x1FFF`.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `DI` -> `i`: Frame-Index in `sur_buf` und `mix_buf` (`0..buf_samples`)
/// - `SI` -> `si_bytes`: Lese-Offset `buf_start` in Bytes (`(si_bytes / 8) & 0x3FF`)
/// - `BX` -> `bx_bytes`: Schreib-Offset `buf_end` in Bytes (`(bx_bytes / 8) & 0x3FF`)
/// - `CX` -> `buf_samples`: Anzahl der zu verarbeitenden Frames
/// - `EAX` -> Temporärer 32-Bit-Sample-Akkumulator
pub fn process_surround_buffer(
    mix_buf: &mut [StereoSample32],
    sur_buf: &mut [StereoSample32],
    surr_ring: &mut [StereoSample32; SURR_RING_FRAMES],
    buf_samples: usize,
    buf_start: &mut u16,
    buf_end: &mut u16,
) {
    let mut si_bytes = *buf_start;
    let mut bx_bytes = *buf_end;

    for i in 0..buf_samples {
        let si_idx = ((si_bytes & 0x1FFF) as usize) >> 3;
        let bx_idx = ((bx_bytes & 0x1FFF) as usize) >> 3;

        // mov eax, es:[di] (sur_buf.left) -> mov [bx+4], eax (surr_ring.right)
        let sur_l = sur_buf[i].left;
        surr_ring[bx_idx].right = sur_l;

        // mov eax, es:[di+4] (sur_buf.right) -> mov [bx], eax (surr_ring.left)
        let sur_r = sur_buf[i].right;
        surr_ring[bx_idx].left = sur_r;

        // mov eax, [si] ; add es:[di], eax ; mov eax, es:[di] ; add es:[di - diff], eax
        let delayed_l = surr_ring[si_idx].left;
        let sum_l = sur_l.wrapping_add(delayed_l);
        sur_buf[i].left = sum_l;
        mix_buf[i].left = mix_buf[i].left.wrapping_add(sum_l);

        // mov eax, [si+4] ; add es:[di+4], eax ; mov eax, es:[di+4] ; add es:[di - diff + 4], eax
        let delayed_r = surr_ring[si_idx].right;
        let sum_r = sur_r.wrapping_add(delayed_r);
        sur_buf[i].right = sum_r;
        mix_buf[i].right = mix_buf[i].right.wrapping_add(sum_r);

        // add si, 8 ; add bx, 8 ; and si, 1FFFh ; and bx, 1FFFh
        si_bytes = si_bytes.wrapping_add(8) & 0x1FFF;
        bx_bytes = bx_bytes.wrapping_add(8) & 0x1FFF;
    }

    *buf_start = si_bytes;
    *buf_end = bx_bytes;
}

/// Führt das finale Buffer-Clipping, Downshifting und Format-Packing aus (`@MixOut` in
/// `legacy_src/PLAYER/dacmix.pas` Zeilen 2560–2706).
///
/// ### Entsprechende ASM-Labels in `@MixOut`
/// 1. **Mit Clipping (`SoundSettings.PreAmp > $7F`, Bit 7 gesetzt)**:
///    - `@2Q16S` (Zeilen 2576–2601): 16-Bit Stereo mit hartem Sättigungs-Clipping auf `-32768..+32767`
///      (`sar eax, 8`, `cmp eax, edx (7FFFh)`, `cmp eax, ebx (-8000h)`).
///    - `@2Q16c` (Zeilen 2602–2611): 16-Bit Stereo Peak-Search (`PeakSearch != 0`), bestimmt
///      `MaxLevel = max(MaxLevel, abs(sample >> 8))` ohne PCM-Schreibzugriff.
///    - `@2Q16m` (Zeilen 2612–2637): 16-Bit Mono mit Clipping (`(left + right) >> 9`, geclippt auf `-32768..+32767`).
///    - `@2Q16mc` (Zeilen 2639–2648): 16-Bit Mono Peak-Search (`abs((left + right) >> 9)`).
///    - `@2Q8s` (Zeilen 2650–2668): 8-Bit Stereo mit Clipping (`sar eax, 16`, geclippt auf `-128..+127`,
///      danach `+ 0x80` für Unsigned 8-Bit PCM).
///    - `@2Q8m` (Zeilen 2670–2681): 8-Bit Mono mit Clipping (`(left + right) >> 17`, geclippt auf `-128..+127`,
///      danach `+ 0x80`).
/// 2. **Ohne Clipping (`SoundSettings.PreAmp <= $7F`, Bit 7 gelöscht)**:
///    - `@1Q16S` (Zeilen 2682–2690): 16-Bit Stereo unclipped (`(sample >> 8) as i16`).
///    - `@1Q16m` (Zeilen 2691–2696): 16-Bit Mono unclipped (`((left + right) >> 9) as i16`).
///    - `@1Q8s` (Zeilen 2697–2702): 8-Bit Stereo unclipped (`((sample >> 16) as u8).wrapping_add(0x80)`).
///    - `@1Q8m` (Zeilen 2703–2706): 8-Bit Mono unclipped (`(((left + right) >> 17) as u8).wrapping_add(0x80)`).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `DS:SI` -> `mix_buf`: 32-Bit-Quellpuffer (`MixBufferOfs`)
/// - `ES:DI` -> `out_16` / `out_8`: Finaler PCM-Ausgabepuffer (`SoundBuffer`)
/// - `CX` -> `buf_samples`: Anzahl der auszugebenden Samples (`BufSamples`, in `@2Q16S` verdoppelt für L+R)
/// - `EAX` -> `eax`: Aktueller 32-Bit-Sample-Wert aus dem Mischpuffer
/// - `EDX` -> `edx_max`: Obere Clipping-Grenze (`0x7FFF` bzw. `0x7F`) oder `MaxLevel` bei Peak-Search
/// - `EBX` -> `ebx_min`: Untere Clipping-Grenze (`-0x8000` bzw. `-0x80`)
pub fn mix_out_and_clip(
    mix_buf: &[StereoSample32],
    buf_samples: usize,
    bits: u8,
    stereo: u8,
    pre_amp: u8,
    peak_search: bool,
    max_level: &mut i32,
) -> MixOutputBuffer {
    let count = buf_samples.min(mix_buf.len());
    let clip_enabled = pre_amp > 0x7F;
    let is_stereo = stereo == 2;
    let is_16bit = bits == 16;

    if clip_enabled {
        if is_16bit {
            if is_stereo {
                if peak_search {
                    // @2Q16c (dacmix.pas Zeilen 2602–2611):
                    // Bestimmt den Spitzenpegel über cx = 2 * BufSamples Halbwörter
                    let mut edx_max = *max_level;
                    for frame in mix_buf.iter().take(count) {
                        for raw in [frame.left, frame.right] {
                            let mut eax = raw >> 8;
                            if eax < 0 {
                                eax = eax.wrapping_neg();
                            }
                            if eax > edx_max {
                                edx_max = eax;
                            }
                        }
                    }
                    *max_level = edx_max;
                    MixOutputBuffer::PeakSearchOnly
                } else {
                    // @2Q16S (dacmix.pas Zeilen 2582–2601):
                    let edx_max: i32 = 0x7FFF;
                    let ebx_min: i32 = -0x8000;
                    let mut out = Vec::with_capacity(count * 2);
                    for frame in mix_buf.iter().take(count) {
                        for raw in [frame.left, frame.right] {
                            let mut eax = raw >> 8;
                            if eax > edx_max {
                                eax = edx_max;
                            } else if eax < ebx_min {
                                eax = ebx_min;
                            }
                            out.push(eax as i16);
                        }
                    }
                    MixOutputBuffer::Pcm16(out)
                }
            } else if peak_search {
                // @2Q16mc (dacmix.pas Zeilen 2639–2648):
                let mut edx_max = *max_level;
                for frame in mix_buf.iter().take(count) {
                    let mut eax = frame.left.wrapping_add(frame.right) >> 9;
                    if eax < 0 {
                        eax = eax.wrapping_neg();
                    }
                    if eax > edx_max {
                        edx_max = eax;
                    }
                }
                *max_level = edx_max;
                MixOutputBuffer::PeakSearchOnly
            } else {
                // @2Q16m (dacmix.pas Zeilen 2623–2637):
                let edx_max: i32 = 0x7FFF;
                let ebx_min: i32 = -0x8000;
                let mut out = Vec::with_capacity(count);
                for frame in mix_buf.iter().take(count) {
                    let mut eax = frame.left.wrapping_add(frame.right) >> 9;
                    if eax > edx_max {
                        eax = edx_max;
                    } else if eax < ebx_min {
                        eax = ebx_min;
                    }
                    out.push(eax as i16);
                }
                MixOutputBuffer::Pcm16(out)
            }
        } else if is_stereo {
            // @2Q8s (dacmix.pas Zeilen 2650–2668):
            let edx_max: i32 = 0x7F;
            let ebx_min: i32 = -0x80;
            let mut out = Vec::with_capacity(count * 2);
            for frame in mix_buf.iter().take(count) {
                for raw in [frame.left, frame.right] {
                    let mut eax = raw >> 16;
                    if eax > edx_max {
                        eax = edx_max;
                    } else if eax < ebx_min {
                        eax = ebx_min;
                    }
                    out.push((eax as u8).wrapping_add(0x80));
                }
            }
            MixOutputBuffer::Pcm8(out)
        } else {
            // @2Q8m (dacmix.pas Zeilen 2670–2681):
            let edx_max: i32 = 0x7F;
            let ebx_min: i32 = -0x80;
            let mut out = Vec::with_capacity(count);
            for frame in mix_buf.iter().take(count) {
                let mut eax = frame.left.wrapping_add(frame.right) >> 17;
                if eax > edx_max {
                    eax = edx_max;
                } else if eax < ebx_min {
                    eax = ebx_min;
                }
                out.push((eax as u8).wrapping_add(0x80));
            }
            MixOutputBuffer::Pcm8(out)
        }
    } else {
        // Unclipped Modi (SoundSettings.PreAmp <= $7F)
        if is_16bit {
            if is_stereo {
                // @1Q16S (dacmix.pas Zeilen 2685–2690):
                let mut out = Vec::with_capacity(count * 2);
                for frame in mix_buf.iter().take(count) {
                    out.push((frame.left >> 8) as i16);
                    out.push((frame.right >> 8) as i16);
                }
                MixOutputBuffer::Pcm16(out)
            } else {
                // @1Q16m (dacmix.pas Zeilen 2691–2696):
                let mut out = Vec::with_capacity(count);
                for frame in mix_buf.iter().take(count) {
                    let eax = frame.left.wrapping_add(frame.right) >> 9;
                    out.push(eax as i16);
                }
                MixOutputBuffer::Pcm16(out)
            }
        } else if is_stereo {
            // @1Q8s (dacmix.pas Zeilen 2697–2702):
            let mut out = Vec::with_capacity(count * 2);
            for frame in mix_buf.iter().take(count) {
                out.push(((frame.left >> 16) as u8).wrapping_add(0x80));
                out.push(((frame.right >> 16) as u8).wrapping_add(0x80));
            }
            MixOutputBuffer::Pcm8(out)
        } else {
            // @1Q8m (dacmix.pas Zeilen 2703–2706):
            let mut out = Vec::with_capacity(count);
            for frame in mix_buf.iter().take(count) {
                let eax = frame.left.wrapping_add(frame.right) >> 17;
                out.push((eax as u8).wrapping_add(0x80));
            }
            MixOutputBuffer::Pcm8(out)
        }
    }
}

/// Extrahiert den [`MixChannel`]-Header (die ersten 38 Bytes von [`Channel`]) für den Assembler-Mixer
/// (`rep movsd` nach `oAsmChn` in `dacmix.pas` Zeilen 378–383).
#[inline]
pub fn channel_to_mix_channel(chn: &Channel) -> MixChannel {
    MixChannel {
        note: chn.note,
        iend: chn.iend,
        lln: chn.lln,
        lend: chn.lend,
        rest: chn.rest,
        plo: chn.plo,
        phi: chn.phi,
        mix_buf: chn.mix_buf,
        l_vol: chn.l_vol,
        r_vol: chn.r_vol,
        avg_vol: chn.avg_vol,
        jmp_flags: chn.jmp_flags,
        dummy: 0,
    }
}

/// Schreibt die 5 aktualisierten 32-Bit-Felder (`note`, `iend`, `lln`, `lend`, `rest`) aus `oAsmChn`
/// zurück in den Tracker-Kanal (`mov cx, 5 ; rep movsd` in `dacmix.pas` Zeilen 2445–2450).
#[inline]
pub fn write_back_mix_channel(chn: &mut Channel, asm_chn: &MixChannel) {
    chn.note = asm_chn.note;
    chn.iend = asm_chn.iend;
    chn.lln = asm_chn.lln;
    chn.lend = asm_chn.lend;
    chn.rest = asm_chn.rest;
}

/// Vollständige Block-Mixing-Prozedur der MOD4WIN-Engine (`ProcessSampleBlock` in
/// `legacy_src/PLAYER/dacmix.pas` Zeilen 274–2706).
///
/// ### Ablauf (1:1 zu `ProcessSampleBlock`)
/// 1. Wenn `am_repeating == false`:
///    - Löscht `mix_buffer` (`MixBufferOfs`) und – falls `use_surround` aktiv ist – `sur_buffer`
///      (`SurBufferOfs`) für `buf_samples + 1` Stereo-Frames (`rep stosd`, Zeilen 281–287).
///    - Iteriert über alle `mi.num_tracks` Kanäle (`@start`, Zeilen 365–389):
///      - Überspringt stumme Kanäle (`Note == 0` oder `AvgVol == 0`, Zeilen 367–374).
///      - Kopiert den Kanal-Header nach `asm_chn`, springt über `dispatch_mix_channel` (`@JmpTable`)
///        in die jeweilige Mischroutine für `buf_samples + 1` Frames und schreibt die 5 Longints
///        `note..rest` zurück (`@EndMix`, Zeilen 2445–2450).
///    - Führt die Click-Removal-Rampe `@SlideOut` auf `mix_buffer` aus (Zeilen 2457–2491).
///    - Falls `use_surround` aktiv ist (`@Surround`, Zeilen 2493–2558):
///      - Führt die Click-Removal-Rampe `@SurFadeLoop` auf `sur_buffer` aus.
///      - Verarbeitet den 8192-Byte-Surround-Ringpuffer (`@SurrLoop`) und addiert das Ergebnis
///        in `mix_buffer`.
/// 2. Führt `@MixOut` (`mix_out_and_clip`, Zeilen 2560–2706) aus, um den 32-Bit-Mischpuffer
///    optional zu clippen und nach 16-Bit/8-Bit Stereo/Mono PCM zu konvertieren.
pub fn process_sample_block(
    state: &mut EngineState,
    mem: &mut DacMixMemory,
    sample_mem: &[u8],
    am_repeating: bool,
) -> MixOutputBuffer {
    if am_repeating {
        return if mem.peak_search {
            MixOutputBuffer::PeakSearchOnly
        } else if state.sound_settings.bits <= 8 {
            MixOutputBuffer::Pcm8(Vec::new())
        } else {
            MixOutputBuffer::Pcm16(Vec::new())
        };
    }

    let total_tick_samples = (state.buf_samples as usize).min(MAX_MIX_BUF_SAMPLES);

    // Im Direct-to-Disk-Modus (DEV_HDD, BufferSize = 32768 Bytes = 8192 Stereo-16-Bit-Frames)
    // teilt dacmix.pas (@KERNEL / @DoLast, Zeilen 384–399) einen Tick exakt an der 32768-Byte-Puffergrenze
    // in zwei ProcessSampleBlock-Aufrufe (Part 1: BufSamples = Rest im Puffer, Part 2: SamplesRemain).
    // Während PeakSearch (search != 0, Zeilen 375–383) wird WaveEnd hingegen künstlich auf 4*NumSamples
    // gesetzt, sodass dort keine Teilung stattfindet.
    if !mem.peak_search && state.sound_settings.device_id == crate::engine::types::DEV_HDD {
        let block_align = ((state.sound_settings.bits.max(8) as usize / 8)
            * (state.sound_settings.stereo.max(1) as usize))
            .max(1);
        let hdd_buf_frames = 32768 / block_align;
        let mut rem_in_buf = if state.samples_remain == 0 {
            hdd_buf_frames
        } else {
            state.samples_remain as usize
        };

        if rem_in_buf < total_tick_samples {
            let part1_samples = rem_in_buf;
            let part2_samples = total_tick_samples - part1_samples;

            state.buf_samples = part1_samples as u16;
            let out1 =
                process_sample_subblock(state, mem, sample_mem, am_repeating, part1_samples, false);

            state.buf_samples = part2_samples as u16;
            let out2 =
                process_sample_subblock(state, mem, sample_mem, am_repeating, part2_samples, true);
            state.buf_samples = total_tick_samples as u16;

            rem_in_buf = hdd_buf_frames - part2_samples;
            state.samples_remain = if rem_in_buf == 0 {
                hdd_buf_frames as u16
            } else {
                rem_in_buf as u16
            };

            return match (out1, out2) {
                (MixOutputBuffer::Pcm16(mut v1), MixOutputBuffer::Pcm16(v2)) => {
                    v1.extend_from_slice(&v2);
                    MixOutputBuffer::Pcm16(v1)
                }
                (MixOutputBuffer::Pcm8(mut v1), MixOutputBuffer::Pcm8(v2)) => {
                    v1.extend_from_slice(&v2);
                    MixOutputBuffer::Pcm8(v1)
                }
                (o1, _) => o1,
            };
        } else {
            rem_in_buf -= total_tick_samples;
            state.samples_remain = if rem_in_buf == 0 {
                hdd_buf_frames as u16
            } else {
                rem_in_buf as u16
            };
        }
    }

    process_sample_subblock(
        state,
        mem,
        sample_mem,
        am_repeating,
        total_tick_samples,
        false,
    )
}

/// Liest ein einzelnes Sample (8-Bit oder 16-Bit) und normiert es auf den 16-Bit-Wertebereich
/// (`-32768..32767`) für die zerstörungsfreie De-Click-Endanalyse.
#[inline]
fn read_sample_norm_i32(sample_mem: &[u8], idx: i32, is_16bit: bool) -> i32 {
    if is_16bit {
        read_sample_i16(sample_mem, idx) as i32
    } else {
        (read_sample_i8(sample_mem, idx) as i32) << 8
    }
}

/// Analysiert das Ende eines Samples zerstörungsfrei (nur Lesezugriffe auf `sample_mem`), um:
/// 1. echte One-Shot-Samples sowie „Pseudo-One-Shot“-Samples mit stillem/flachem End-Loop
///    oder 2-Wort-Amiga-Stop-Loop zu erkennen (`is_effective_oneshot`), und
/// 2. das tatsächliche akustische Signalende `effective_iend` VOR etwaiger End-Stille
///    (Zero-Padding / gecleartes Sample-Ende), vor Amiga-Wort-Padding-/Ripper-Knacksern
///    und vor dem Cubic-Spline/Linear-IDO Guard-Byte-Lookahead zu bestimmen.
fn analyze_sample_tail(
    sample_mem: &[u8],
    i_offset: i32,
    start_esi: i32,
    iend: i32,
    lend: i32,
    lln: i32,
    is_16bit: bool,
) -> (bool, i32) {
    let sample_start = if i_offset > 0 && i_offset <= iend {
        i_offset
    } else {
        start_esi.min(iend).max(0)
    };

    let mut is_oneshot = lend == 0 || lln > iend + 1 || lln <= 0;
    let mut raw_end = iend;

    if !is_oneshot {
        let loop_start = (iend + 1).saturating_sub(lln).max(sample_start);
        let loop_len = iend.saturating_sub(loop_start) + 1;
        let pre_loop_len = loop_start.saturating_sub(sample_start);
        if loop_len <= 4 && pre_loop_len >= 64 {
            // Klassischer 2-Wort Amiga-End-Stop-Loop am Ende eines langen Samples
            is_oneshot = true;
            raw_end = loop_start;
        } else if loop_len > 0 && pre_loop_len >= 16 {
            let step = (loop_len / 128).max(1);
            let mut min_v = i32::MAX;
            let mut max_v = i32::MIN;
            let mut flat = true;
            let mut idx = loop_start;
            while idx <= iend {
                let v = read_sample_norm_i32(sample_mem, idx, is_16bit);
                if v < min_v {
                    min_v = v;
                }
                if v > max_v {
                    max_v = v;
                }
                if max_v.saturating_sub(min_v) > 512 {
                    flat = false;
                    break;
                }
                idx += step;
            }
            if flat {
                // Stiller bzw. reiner DC-End-Loop -> akustisch ein One-Shot bis `loop_start`
                is_oneshot = true;
                raw_end = loop_start;
            }
        }
    }

    if !is_oneshot {
        return (false, iend);
    }

    let sample_len = raw_end.saturating_sub(sample_start);
    if sample_len <= 12 {
        return (true, raw_end);
    }

    // 1. Letzte Rand-Samples (Amiga Word-Padding / Guard-Byte-Lookahead) abtrennen
    let boundary_trim = if sample_len > 32 { 4 } else { 2 };
    let mut true_end = raw_end.saturating_sub(boundary_trim);

    // 2. Isolierten End-Click-Blip (<= 8 Samples nahe `raw_end`, davor >= 8 Samples Stille) überspringen
    if sample_len > 48 {
        let mut blip_pos = None;
        for p in ((true_end - 8)..=true_end).rev() {
            if read_sample_norm_i32(sample_mem, p, is_16bit).abs() > 512 {
                blip_pos = Some(p);
                break;
            }
        }
        if let Some(bp) = blip_pos {
            let mut silent_before = true;
            for p in (bp - 16)..(bp - 8) {
                if read_sample_norm_i32(sample_mem, p, is_16bit).abs() > 512 {
                    silent_before = false;
                    break;
                }
            }
            if silent_before {
                true_end = bp - 8;
            }
        }
    }

    // 3. Rückwärts über End-Stille (Zero-Padding / gecleartes Sample-Ende) bis zum echten Signalende scannen
    let max_back = (sample_len / 2).min(4096);
    let min_pos = raw_end.saturating_sub(max_back).max(sample_start + 8);
    let mut p = true_end;
    while p > min_pos && read_sample_norm_i32(sample_mem, p, is_16bit).abs() <= 512 {
        p -= 1;
    }
    if p == min_pos && read_sample_norm_i32(sample_mem, p, is_16bit).abs() <= 512 {
        // Falls das gesamte Sample extrem leise ist, nur exakte Nullbytes überspringen
        p = true_end;
        while p > min_pos && read_sample_norm_i32(sample_mem, p, is_16bit) == 0 {
            p -= 1;
        }
    }
    true_end = p;

    // 4. Abrupten Ripper-/Truncate-Spike in den letzten 12 Samples vor `true_end` erkennen
    if true_end.saturating_sub(sample_start) > 48 {
        let mut body_max_step = 0i32;
        for i in (true_end - 32)..(true_end - 12) {
            let s0 = read_sample_norm_i32(sample_mem, i - 1, is_16bit);
            let s1 = read_sample_norm_i32(sample_mem, i, is_16bit);
            body_max_step = body_max_step.max((s1 - s0).abs());
        }
        let step_limit = body_max_step.saturating_mul(2).saturating_add(4096);
        for i in (true_end - 12)..=true_end {
            let s0 = read_sample_norm_i32(sample_mem, i - 1, is_16bit);
            let s1 = read_sample_norm_i32(sample_mem, i, is_16bit);
            if (s1 - s0).abs() > step_limit {
                true_end = i - 1;
                break;
            }
        }
    }

    // 5. Sicherheitsabstand vor `true_end` für Cubic-Spline/Linear-IDO Lookahead (+1/+2)
    let tail_trim = if true_end.saturating_sub(sample_start) > 32 {
        4
    } else {
        2
    };
    let effective_iend = true_end.saturating_sub(tail_trim).max(sample_start + 4);
    (true, effective_iend)
}

fn process_sample_subblock(
    state: &mut EngineState,
    mem: &mut DacMixMemory,
    sample_mem: &[u8],
    am_repeating: bool,
    buf_samples: usize,
    is_second_subblock: bool,
) -> MixOutputBuffer {
    let total_frames = buf_samples + 1;
    let use_declick = state.sound_settings.declick() && !mem.peak_search;

    if !am_repeating {
        if !use_declick && mem.declick_active {
            mem.declick_active = false;
        } else if use_declick && !mem.declick_active {
            mem.chn_declick = [ChannelDeClickState::default(); MAX_M4W_CHANNELS];
            mem.declick_active = true;
        }

        // 1. MixBuffer (und ggf. SurBuffer) mit 0 initialisieren (dacmix.pas Zeilen 281–287)
        for frame in mem.mix_buffer.iter_mut().take(total_frames) {
            *frame = StereoSample32::default();
        }
        if state.use_surround {
            for frame in mem.sur_buffer.iter_mut().take(total_frames) {
                *frame = StereoSample32::default();
            }
        }

        // 2. Alle aktiven Kanäle mischen (@start .. @EndMix, dacmix.pas Zeilen 365–2455)
        let num_tracks = (state.mi.num_tracks as usize).min(state.channels.len());
        let ramp_frames = (((state.sound_settings.samplerate.max(8000) as usize) * 64) / 44100)
            .clamp(16, 128);

        for ch_idx in 0..num_tracks {
            let chn = &mut state.channels[ch_idx];
            // OR esi, esi / jz @endmix
            let mut esi = chn.note;
            let pre_wrap_esi = esi;
            if esi == 0 {
                if use_declick {
                    let dc = &mut mem.chn_declick[ch_idx];
                    let n = ramp_frames.max(1);
                    if dc.prev_l != 0 || dc.prev_r != 0 {
                        let target_buf = if dc.was_surround && state.use_surround {
                            &mut mem.sur_buffer[..total_frames]
                        } else {
                            &mut mem.mix_buffer[..total_frames]
                        };
                        let limit = n.min(total_frames);
                        for k in 0..limit {
                            let w = (n - k) as i64;
                            let dl = ((dc.prev_l as i64 * w) / (n as i64)) as i32;
                            let dr = ((dc.prev_r as i64 * w) / (n as i64)) as i32;
                            target_buf[k].left = target_buf[k].left.wrapping_add(dl);
                            target_buf[k].right = target_buf[k].right.wrapping_add(dr);
                        }
                    }
                    if buf_samples < n && (dc.prev_l != 0 || dc.prev_r != 0) {
                        let w_end = (n - buf_samples) as i64;
                        dc.prev_l = ((dc.prev_l as i64 * w_end) / (n as i64)) as i32;
                        dc.prev_r = ((dc.prev_r as i64 * w_end) / (n as i64)) as i32;
                    } else {
                        dc.prev_l = 0;
                        dc.prev_r = 0;
                    }
                    dc.was_active = false;
                    dc.expected_next_esi = 0;
                }
                continue;
            }
            // @@0: cmp esi, IEnd / jbe @@1 / sub esi, LLn / jns @@0 / xor esi, esi / jmp @WriteChnEnd
            while (esi as u32) > (chn.iend as u32) {
                esi = esi.wrapping_sub(chn.lln);
                if esi < 0 {
                    chn.note = 0;
                    esi = 0;
                    break;
                }
            }
            if esi == 0 {
                if use_declick {
                    let dc = &mut mem.chn_declick[ch_idx];
                    let n = ramp_frames.max(1);
                    if dc.prev_l != 0 || dc.prev_r != 0 {
                        let target_buf = if dc.was_surround && state.use_surround {
                            &mut mem.sur_buffer[..total_frames]
                        } else {
                            &mut mem.mix_buffer[..total_frames]
                        };
                        let limit = n.min(total_frames);
                        for k in 0..limit {
                            let w = (n - k) as i64;
                            let dl = ((dc.prev_l as i64 * w) / (n as i64)) as i32;
                            let dr = ((dc.prev_r as i64 * w) / (n as i64)) as i32;
                            target_buf[k].left = target_buf[k].left.wrapping_add(dl);
                            target_buf[k].right = target_buf[k].right.wrapping_add(dr);
                        }
                    }
                    if buf_samples < n && (dc.prev_l != 0 || dc.prev_r != 0) {
                        let w_end = (n - buf_samples) as i64;
                        dc.prev_l = ((dc.prev_l as i64 * w_end) / (n as i64)) as i32;
                        dc.prev_r = ((dc.prev_r as i64 * w_end) / (n as i64)) as i32;
                    } else {
                        dc.prev_l = 0;
                        dc.prev_r = 0;
                    }
                    dc.was_active = false;
                    dc.expected_next_esi = 0;
                }
                continue;
            }
            chn.note = esi;

            // @@1: cmp AvgVol, 0 / jnz @NoVol0
            if chn.avg_vol == 0 {
                // @VOL0 (dacmix.pas Zeilen 460–482):
                // mov ebx, cx (total_frames = BufSamples + 1)
                // mov eax, dword ptr [oAsmChn].Plo ; mul ebx ; shrd eax, edx, 16 ; add esi, eax
                let eax_pitch: u32 = ((chn.phi as u16 as u32) << 16) | (chn.plo as u32);
                let prod: u64 = (eax_pitch as u64) * (total_frames as u64);
                let step: i32 = (prod >> 16) as i32;
                esi = esi.wrapping_add(step);
                while (esi as u32) > (chn.iend as u32) {
                    esi = esi.wrapping_sub(chn.lln);
                    if esi < 0 {
                        esi = 0;
                        break;
                    }
                }
                chn.note = esi;
                if use_declick {
                    let dc = &mut mem.chn_declick[ch_idx];
                    let n = ramp_frames.max(1);
                    if dc.prev_l != 0 || dc.prev_r != 0 {
                        let target_buf = if dc.was_surround && state.use_surround {
                            &mut mem.sur_buffer[..total_frames]
                        } else {
                            &mut mem.mix_buffer[..total_frames]
                        };
                        let limit = n.min(total_frames);
                        for k in 0..limit {
                            let w = (n - k) as i64;
                            let dl = ((dc.prev_l as i64 * w) / (n as i64)) as i32;
                            let dr = ((dc.prev_r as i64 * w) / (n as i64)) as i32;
                            target_buf[k].left = target_buf[k].left.wrapping_add(dl);
                            target_buf[k].right = target_buf[k].right.wrapping_add(dr);
                        }
                    }
                    if buf_samples < n && (dc.prev_l != 0 || dc.prev_r != 0) {
                        let w_end = (n - buf_samples) as i64;
                        dc.prev_l = ((dc.prev_l as i64 * w_end) / (n as i64)) as i32;
                        dc.prev_r = ((dc.prev_r as i64 * w_end) / (n as i64)) as i32;
                    } else {
                        dc.prev_l = 0;
                        dc.prev_r = 0;
                    }
                    dc.was_active = false;
                    dc.expected_next_esi = esi;
                }
                continue;
            }

            let mut asm_chn = channel_to_mix_channel(chn);
            let target_is_surround =
                state.use_surround && (asm_chn.mix_buf as u16 == SUR_BUFFER_OFS);

            if !use_declick {
                if target_is_surround {
                    dispatch_mix_channel(
                        &mut asm_chn,
                        &mut mem.sur_buffer[..total_frames],
                        sample_mem,
                        &mem.vol_table,
                        state.vol_master,
                        total_frames,
                    );
                } else {
                    dispatch_mix_channel(
                        &mut asm_chn,
                        &mut mem.mix_buffer[..total_frames],
                        sample_mem,
                        &mem.vol_table,
                        state.vol_master,
                        total_frames,
                    );
                }
                write_back_mix_channel(chn, &asm_chn);
            } else {
                // Per-Channel De-Click (`CL+DC`):
                for frame in mem.chn_scratch.iter_mut().take(total_frames) {
                    *frame = StereoSample32::default();
                }
                let start_esi = asm_chn.note;
                let start_rest = asm_chn.rest as u16;
                let i_offset = chn.i_offset;
                let iend = asm_chn.iend;
                let lln = asm_chn.lln;
                let lend = asm_chn.lend;
                let plo = asm_chn.plo;
                let phi = asm_chn.phi;
                let is_16bit = (asm_chn.jmp_flags & mixer_flags::F_16BIT) != 0;

                dispatch_mix_channel(
                    &mut asm_chn,
                    &mut mem.chn_scratch[..total_frames],
                    sample_mem,
                    &mem.vol_table,
                    state.vol_master,
                    total_frames,
                );
                write_back_mix_channel(chn, &asm_chn);

                let dc = &mut mem.chn_declick[ch_idx];
                let e_inst_triggered =
                    !is_second_subblock && (chn.event_chg & event_flags::E_INST) != 0;
                let sample_pointer_jumped =
                    !dc.was_active || pre_wrap_esi != dc.expected_next_esi || e_inst_triggered;

                let pitch_fp = (((phi as u16 as u64) << 16) | (plo as u64)).max(1);
                let pitch_fp_i64 = pitch_fp as i64;
                let is_real_oneshot = lend == 0 || lln > iend || lln <= 0;
                let (is_oneshot, effective_iend) = analyze_sample_tail(
                    sample_mem,
                    i_offset,
                    start_esi,
                    iend,
                    lend,
                    lln,
                    is_16bit,
                );
                let is_looping = !is_oneshot && lln > 0;

                // 1a. One-Shot & Pseudo-One-Shot End-De-Clicking:
                //     Blendet das tatsächliche Signal bereits vor `effective_iend` sanft ab und
                //     schneidet End-Garbage, Zero-Padding-Klippen sowie Guard-Byte-Drops ab.
                if is_oneshot {
                    let end_fp = ((effective_iend as i64) << 16) | 0xFFFF;
                    let pre_n = ramp_frames.max(8) as i64;
                    let denom_pre = pre_n * 2;
                    let mut sim_esi = start_esi;
                    let mut sim_dx = start_rest;
                    let mut end_k: Option<usize> = None;

                    for k in 0..total_frames {
                        if (sim_esi as u32) > (effective_iend as u32) {
                            end_k = Some(k);
                            break;
                        }
                        let pos_fp = ((sim_esi as i64) << 16) | (sim_dx as i64);
                        let rem_frames = ((end_fp - pos_fp).max(0) / pitch_fp_i64) + 1;
                        if rem_frames < pre_n {
                            let w = pre_n + rem_frames;
                            mem.chn_scratch[k].left =
                                ((mem.chn_scratch[k].left as i64 * w) / denom_pre) as i32;
                            mem.chn_scratch[k].right =
                                ((mem.chn_scratch[k].right as i64 * w) / denom_pre) as i32;
                        }

                        let (next_dx, carry) = sim_dx.overflowing_add(plo);
                        sim_dx = next_dx;
                        sim_esi = sim_esi
                            .wrapping_add(phi)
                            .wrapping_add(if carry { 1 } else { 0 });
                    }

                    if let Some(ek) = end_k {
                        let anchor = if ek > 0 {
                            mem.chn_scratch[ek - 1]
                        } else {
                            StereoSample32 {
                                left: dc.prev_l,
                                right: dc.prev_r,
                            }
                        };
                        let n = ramp_frames.max(1);
                        for k in ek..total_frames {
                            let step = if ek > 0 { k - ek + 1 } else { k };
                            if step < n {
                                let w = (n - step) as i64;
                                mem.chn_scratch[k].left =
                                    ((anchor.left as i64 * w) / (n as i64)) as i32;
                                mem.chn_scratch[k].right =
                                    ((anchor.right as i64 * w) / (n as i64)) as i32;
                            } else {
                                mem.chn_scratch[k] = StereoSample32::default();
                            }
                        }
                        if is_real_oneshot {
                            chn.note = 0;
                        }
                    }
                } else if is_looping && lln >= 8 {
                    // 1b. Loop-Seam Guard-Byte Hold:
                    //     Verhindert, dass Linear-IDO und Cubic-Spline auf den letzten 1–2 Samples
                    //     vor `iend` bereits innerhalb eines einzigen Quell-Samples in die Guard-Bytes
                    //     (`sample[loop_start]`) abtauchen, bevor die Loop-Rampe in Schritt 3 greift.
                    let mut seam_guard_start = iend.saturating_sub(if lln >= 16 { 2 } else { 1 });
                    if lln >= 48 {
                        let s_m4 = read_sample_norm_i32(sample_mem, iend - 4, is_16bit);
                        let s_m3 = read_sample_norm_i32(sample_mem, iend - 3, is_16bit);
                        let s_m2 = read_sample_norm_i32(sample_mem, iend - 2, is_16bit);
                        if (s_m2 - s_m3).abs() > 8192 || (s_m3 - s_m4).abs() > 8192 {
                            seam_guard_start = iend - 4;
                        }
                    }
                    let mut sim_esi = start_esi;
                    let mut sim_dx = start_rest;
                    for k in 0..total_frames {
                        while (sim_esi as u32) > (iend as u32) {
                            sim_esi = sim_esi.wrapping_sub(lln);
                            if sim_esi < 0 {
                                break;
                            }
                        }
                        if sim_esi >= 0 && (sim_esi as u32) > (seam_guard_start as u32) {
                            mem.chn_scratch[k] = if k > 0 {
                                mem.chn_scratch[k - 1]
                            } else {
                                StereoSample32 {
                                    left: dc.prev_l,
                                    right: dc.prev_r,
                                }
                            };
                        }
                        let (next_dx, carry) = sim_dx.overflowing_add(plo);
                        sim_dx = next_dx;
                        sim_esi = sim_esi
                            .wrapping_add(phi)
                            .wrapping_add(if carry { 1 } else { 0 });
                    }
                }

                // 2. Note-Trigger / Retrigger / Sample-Offset Crossfade:
                let mut ramp_l: i32 = 0;
                let mut ramp_r: i32 = 0;
                let mut rem: usize = 0;

                if sample_pointer_jumped {
                    let fade_len = ramp_frames.min(total_frames).max(1);
                    let old_l = dc.prev_l as i64;
                    let old_r = dc.prev_r as i64;
                    let n64 = fade_len as i64;
                    for k in 0..fade_len {
                        let w_new = k as i64;
                        let w_old = n64 - w_new;
                        let new_l = mem.chn_scratch[k].left as i64;
                        let new_r = mem.chn_scratch[k].right as i64;
                        mem.chn_scratch[k].left = ((old_l * w_old + new_l * w_new) / n64) as i32;
                        mem.chn_scratch[k].right = ((old_r * w_old + new_r * w_new) / n64) as i32;
                    }
                } else {
                    // Kontinuierliche Note: sanfte Rampe bei Lautstärke-/Panning-Sprung
                    // oder nahtlose Fortsetzung einer aus dem Vorblock laufenden Rampe
                    ramp_l = dc.prev_l.wrapping_sub(mem.chn_scratch[0].left);
                    ramp_r = dc.prev_r.wrapping_sub(mem.chn_scratch[0].right);
                    if ramp_l != 0 || ramp_r != 0 {
                        rem = ramp_frames;
                    }
                }

                // 3. Mid-Tick Loop-Seam De-Clicking + Rampen-Anwendung:
                let loop_out_frames = if is_looping {
                    (((lln as u64) << 16) / pitch_fp) as usize
                } else {
                    0
                };
                let loop_ramp_frames = if loop_out_frames >= 16 {
                    ramp_frames.min(loop_out_frames / 2).max(4)
                } else {
                    0
                };

                let mut sim_esi = start_esi;
                let mut sim_dx = start_rest;
                for k in 0..total_frames {
                    if is_looping && (sim_esi as u32) > (iend as u32) {
                        while (sim_esi as u32) > (iend as u32) {
                            sim_esi = sim_esi.wrapping_sub(lln);
                            if sim_esi < 0 {
                                break;
                            }
                        }
                        if k > 0 && loop_ramp_frames > 0 && sim_esi >= 0 {
                            // `chn_scratch[k - 1]` enthält bereits den geglätteten Vorgängerwert
                            let prev_out_l = mem.chn_scratch[k - 1].left;
                            let prev_out_r = mem.chn_scratch[k - 1].right;
                            ramp_l = prev_out_l.wrapping_sub(mem.chn_scratch[k].left);
                            ramp_r = prev_out_r.wrapping_sub(mem.chn_scratch[k].right);
                            rem = loop_ramp_frames;
                        }
                    }

                    if rem > 0 && (ramp_l != 0 || ramp_r != 0) {
                        mem.chn_scratch[k].left = mem.chn_scratch[k].left.wrapping_add(ramp_l);
                        mem.chn_scratch[k].right = mem.chn_scratch[k].right.wrapping_add(ramp_r);
                        let step_l = ramp_l / (rem as i32);
                        let step_r = ramp_r / (rem as i32);
                        ramp_l = ramp_l.wrapping_sub(step_l);
                        ramp_r = ramp_r.wrapping_sub(step_r);
                        rem -= 1;
                    }

                    let (next_dx, carry) = sim_dx.overflowing_add(plo);
                    sim_dx = next_dx;
                    sim_esi = sim_esi
                        .wrapping_add(phi)
                        .wrapping_add(if carry { 1 } else { 0 });
                }

                // Kanal-Puffer in den Zielpuffer (`mix_buffer` oder `sur_buffer`) akkumulieren
                let target_buf = if target_is_surround {
                    &mut mem.sur_buffer[..total_frames]
                } else {
                    &mut mem.mix_buffer[..total_frames]
                };
                for k in 0..total_frames {
                    target_buf[k].left = target_buf[k].left.wrapping_add(mem.chn_scratch[k].left);
                    target_buf[k].right =
                        target_buf[k].right.wrapping_add(mem.chn_scratch[k].right);
                }

                // Lookahead-Frame `buf_samples` als Referenz für den nächsten Subblock sichern
                if chn.note == 0 {
                    dc.prev_l = mem.chn_scratch[buf_samples].left;
                    dc.prev_r = mem.chn_scratch[buf_samples].right;
                    dc.was_active = false;
                    dc.expected_next_esi = 0;
                    dc.was_surround = target_is_surround;
                } else {
                    dc.prev_l = mem.chn_scratch[buf_samples].left;
                    dc.prev_r = mem.chn_scratch[buf_samples].right;
                    dc.was_active = true;
                    dc.expected_next_esi = chn.note;
                    dc.was_surround = target_is_surround;
                }
            }
        }

        // 3. Click-Removal (@SlideOut, dacmix.pas Zeilen 2457–2491)
        if !use_declick {
            apply_click_removal_slide(
                &mut mem.mix_buffer,
                buf_samples,
                state.num_samples,
                &mut state.l_fade_out,
                &mut state.r_fade_out,
            );
        } else if buf_samples < mem.mix_buffer.len() {
            state.l_fade_out = mem.mix_buffer[buf_samples].left;
            state.r_fade_out = mem.mix_buffer[buf_samples].right;
        }

        // 4. Surround-Verarbeitung (@Surround: @SurFadeLoop + @SurrLoop, dacmix.pas Zeilen 2493–2558)
        if state.use_surround {
            if !use_declick {
                apply_click_removal_slide(
                    &mut mem.sur_buffer,
                    buf_samples,
                    state.num_samples,
                    &mut state.l_sur_fade_out,
                    &mut state.r_sur_fade_out,
                );
            } else if buf_samples < mem.sur_buffer.len() {
                state.l_sur_fade_out = mem.sur_buffer[buf_samples].left;
                state.r_sur_fade_out = mem.sur_buffer[buf_samples].right;
            }
            process_surround_buffer(
                &mut mem.mix_buffer,
                &mut mem.sur_buffer,
                &mut mem.surr_ring,
                buf_samples,
                &mut state.buf_start,
                &mut state.buf_end,
            );
        }

        // 4b. Optionale Post-Filter: Moderner adaptiver Mehrband-Denoiser (`DeHiss`) & Amiga-Paula-Filter (`Amiga`)
        apply_post_filters(state, mem, buf_samples);
    }

    // 5. Finales Buffer-Clipping und PCM-Format-Ausgabe (@MixOut, dacmix.pas Zeilen 2560–2706)
    mix_out_and_clip(
        &mem.mix_buffer,
        buf_samples,
        state.sound_settings.bits,
        state.sound_settings.stereo,
        state.sound_settings.pre_amp,
        mem.peak_search,
        &mut state.max_level,
    )
}

/// Wendet – sofern aktiviert und nicht im `PeakSearch`-Vorlauf – den modernen adaptiven
/// 5-Band-Wiener-/Spektral-Denoiser (`DeHiss`) und/oder den authentischen Amiga-Paula-Tiefpassfilter (`Amiga`)
/// auf den 32-Bit-Summenpuffer `mem.mix_buffer[0..buf_samples]` an.
pub fn apply_post_filters(
    state: &EngineState,
    mem: &mut DacMixMemory,
    buf_samples: usize,
) {
    if mem.peak_search || buf_samples == 0 {
        return;
    }
    let use_dehiss = state.sound_settings.dehiss();
    let use_amiga = state.sound_settings.amiga_filter();

    if !use_dehiss && mem.dehiss_filter.active {
        mem.dehiss_filter = AdaptiveDeHissState::default();
    }
    if !use_amiga && mem.amiga_filter.active {
        mem.amiga_filter = AmigaFilterState::default();
    }
    if !use_dehiss && !use_amiga {
        return;
    }

    let sr = (state.sound_settings.samplerate.max(8000)) as f64;
    let nyq_limit = sr * 0.45;
    let limit = buf_samples.min(mem.mix_buffer.len());

    // 1. Moderner adaptiver 5-Band-Wiener-/Spektral-Denoiser (`DeHiss`)
    if use_dehiss {
        let dh = &mut mem.dehiss_filter;
        dh.active = true;

        // Prüfe, ob aktuell 8-Bit-Kanäle aktiv sind (höheres Quantisierungs-Grundrauschen)
        let num_tracks = (state.mi.num_tracks as usize).min(state.channels.len());
        let mut has_8bit_active = false;
        for ch in &state.channels[..num_tracks] {
            if ch.note != 0 && ch.avg_vol > 0 && (ch.jmp_flags & mixer_flags::F_16BIT) == 0 {
                has_8bit_active = true;
                break;
            }
        }

        // Zweifach kaskadierte phasenkompensierte TPT-SVF-Hauptweiche bei 1200 Hz
        const F0_HZ: f64 = 1200.0;
        const SVF_K: f64 = 0.65;
        let f0_clamped = F0_HZ.min(nyq_limit);
        let g0 = (std::f64::consts::PI * f0_clamped / sr).tan();
        let svf_a1 = 1.0 / (1.0 + g0 * (g0 + SVF_K));
        let svf_a2 = g0 * svf_a1;
        let svf_a3 = g0 * svf_a2;

        const SUB_XOVER_HZ: [f64; 3] = [2200.0, 4400.0, 8800.0];
        let mut g_sub = [0.0f64; 3];
        for i in 0..3 {
            let f_clamped = SUB_XOVER_HZ[i].min(nyq_limit);
            let wd = (std::f64::consts::PI * f_clamped / sr).tan();
            g_sub[i] = wd / (1.0 + wd);
        }

        // Zeitkonstanten für schnelle/langsame Hüllkurven, MCRA-Noise-Tracker und Gain-Ballistik
        let att_fast = 1.0 - (-1.0 / (0.0005 * sr)).exp();
        let rel_fast = 1.0 - (-1.0 / (0.0120 * sr)).exp();
        let att_slow = 1.0 - (-1.0 / (0.0120 * sr)).exp();
        let rel_slow = 1.0 - (-1.0 / (0.0600 * sr)).exp();
        let noise_down = 1.0 - (-1.0 / (0.0080 * sr)).exp();
        let noise_up = 1.0 - (-1.0 / (0.3800 * sr)).exp();
        let gain_att = 1.0 - (-1.0 / (0.0005 * sr)).exp();
        let gain_rel = 1.0 - (-1.0 / (0.0100 * sr)).exp();
        let trans_decay = (-1.0 / (0.0060 * sr)).exp();

        let quant_scale = if has_8bit_active { 1.0 } else { 0.45 };
        const QUANT_RATIOS: [f64; 4] = [0.028, 0.045, 0.065, 0.085];
        const ABS_FLOORS: [f64; 4] = [800.0, 750.0, 650.0, 550.0];
        const OVER_SUB: [f64; 4] = [1.35, 1.65, 1.95, 2.25];
        const G_MIN: [f64; 4] = [0.05, 0.04, 0.03, 0.02];

        for frame in &mut mem.mix_buffer[..limit] {
            let xl = frame.left as f64;
            let xr = frame.right as f64;

            // Doppelte phasenkompensierte TPT-SVF-Hochpasskaskade bei 1150 Hz:
            // u1 = H_HP,comp1(H_HP,comp0(x)), b0 = x - u1 -> b0 + u1 == x exakt!
            let (_, ua_l) = dh.xover_l[0].process(xl, svf_a1, svf_a2, svf_a3, SVF_K, 0.0);
            let (_, u1_l) = dh.xover_l[1].process(ua_l, svf_a1, svf_a2, svf_a3, SVF_K, 0.0);
            let b0_l = xl - u1_l;

            let (_, ua_r) = dh.xover_r[0].process(xr, svf_a1, svf_a2, svf_a3, SVF_K, 0.0);
            let (_, u1_r) = dh.xover_r[1].process(ua_r, svf_a1, svf_a2, svf_a3, SVF_K, 0.0);
            let b0_r = xr - u1_r;

            // Komplementäre TPT-Aufspaltung von u1 in 4 Hochfrequenz-Subbänder (1200..2200, 2200..4400, 4400..8800, > 8800 Hz)
            let gs0 = g_sub[0];
            let v0l = (u1_l - dh.xover_l[2].ic1_a) * gs0;
            let band0_l = v0l + dh.xover_l[2].ic1_a;
            dh.xover_l[2].ic1_a = band0_l + v0l;
            let u2_l = u1_l - band0_l;

            let v0r = (u1_r - dh.xover_r[2].ic1_a) * gs0;
            let band0_r = v0r + dh.xover_r[2].ic1_a;
            dh.xover_r[2].ic1_a = band0_r + v0r;
            let u2_r = u1_r - band0_r;

            let gs1 = g_sub[1];
            let v1l = (u2_l - dh.xover_l[2].ic2_a) * gs1;
            let band1_l = v1l + dh.xover_l[2].ic2_a;
            dh.xover_l[2].ic2_a = band1_l + v1l;
            let u3_l = u2_l - band1_l;

            let v1r = (u2_r - dh.xover_r[2].ic2_a) * gs1;
            let band1_r = v1r + dh.xover_r[2].ic2_a;
            dh.xover_r[2].ic2_a = band1_r + v1r;
            let u3_r = u2_r - band1_r;

            let gs2 = g_sub[2];
            let v2l = (u3_l - dh.xover_l[3].ic1_a) * gs2;
            let band2_l = v2l + dh.xover_l[3].ic1_a;
            dh.xover_l[3].ic1_a = band2_l + v2l;
            let band3_l = u3_l - band2_l;

            let v2r = (u3_r - dh.xover_r[3].ic1_a) * gs2;
            let band2_r = v2r + dh.xover_r[3].ic1_a;
            dh.xover_r[3].ic1_a = band2_r + v2r;
            let band3_r = u3_r - band2_r;

            let bands_l = [band0_l, band1_l, band2_l, band3_l];
            let bands_r = [band0_r, band1_r, band2_r, band3_r];

            // Stereo-gekoppelte Tiefton-Hüllkurve (< 850 Hz)
            let e0 = b0_l.abs().max(b0_r.abs());
            let c0 = if e0 > dh.env_low { att_fast } else { rel_fast };
            dh.env_low += c0 * (e0 - dh.env_low);

            // Hüllkurven & Spectral-Flux für die 4 oberen Bänder aktualisieren
            let mut hf_flux_triggered = false;
            for b in 0..4 {
                let eb = bands_l[b].abs().max(bands_r[b].abs());
                let cf = if eb > dh.env_fast[b] { att_fast } else { rel_fast };
                dh.env_fast[b] += cf * (eb - dh.env_fast[b]);

                let cs = if eb > dh.env_slow[b] { att_slow } else { rel_slow };
                dh.env_slow[b] += cs * (eb - dh.env_slow[b]);

                // Transienten-Detektor in den Präsenz-/Brillanz-Bändern (Hi-Hats, Snares, Cymbals)
                if b >= 1
                    && dh.env_fast[b] > 4000.0
                    && dh.env_fast[b] > dh.env_slow[b] * 2.5 + dh.noise_floor[b] * 1.8
                    && (dh.env_fast[2] + dh.env_fast[3]) > dh.env_low * 0.28
                {
                    hf_flux_triggered = true;
                }

                // Minimum-Statistics / MCRA stationärer Rausch-Tracker
                if dh.env_fast[b] < dh.noise_floor[b] {
                    dh.noise_floor[b] += noise_down * (dh.env_fast[b] - dh.noise_floor[b]);
                } else if !hf_flux_triggered {
                    dh.noise_floor[b] += noise_up * (dh.env_fast[b] - dh.noise_floor[b]);
                }
                dh.noise_floor[b] = dh.noise_floor[b].max(ABS_FLOORS[b]);
            }

            if hf_flux_triggered {
                dh.transient_hold = 1.0;
            } else {
                dh.transient_hold *= trans_decay;
            }

            // Signal-abhängiges 8-Bit-Quantisierungsrauschen + Wiener-Filterung pro Band
            let ref_low_mid = dh.env_low + 0.45 * dh.env_fast[0];
            let mut out_l = b0_l;
            let mut out_r = b0_r;

            for b in 0..4 {
                let n_quant = ref_low_mid * QUANT_RATIOS[b] * quant_scale;
                let n_est = dh.noise_floor[b].max(n_quant).max(ABS_FLOORS[b]);
                let snr = dh.env_fast[b] / (n_est + 1.0);
                let xi = (snr - OVER_SUB[b]).max(0.0);
                let wiener = (xi * xi) / (xi * xi + 1.0);
                let g_min = G_MIN[b];
                let target_g = g_min + (1.0 - g_min) * wiener.max(dh.transient_hold);
                let cg = if target_g > dh.smooth_gain[b] {
                    gain_att
                } else {
                    gain_rel
                };
                dh.smooth_gain[b] += cg * (target_g - dh.smooth_gain[b]);

                out_l += bands_l[b] * dh.smooth_gain[b];
                out_r += bands_r[b] * dh.smooth_gain[b];
            }

            frame.left = out_l.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
            frame.right = out_r.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        }
    }

    // 2. Authentischer Amiga-Paula-Tiefpassfilter (A500 4.42 kHz 1-Pol RC + ~3.275 kHz 2-Pol Sallen-Key LED-Filter)
    if use_amiga {
        let af = &mut mem.amiga_filter;
        af.active = true;

        // Stufe 1: Statischer Amiga-RC-Rekonstruktionsfilter (fc = 4421 Hz, 6 dB/Okt)
        let fc_rc = 4421.0f64.min(nyq_limit);
        let g_rc_tan = (std::f64::consts::PI * fc_rc / sr).tan();
        let g_rc = g_rc_tan / (1.0 + g_rc_tan);

        // Stufe 2: Schaltbarer Amiga "Power-LED" Sallen-Key Tiefpass (fc = 3275 Hz, Q = 0.76, 12 dB/Okt)
        let fc_led = 3275.0f64.min(nyq_limit);
        let g = (std::f64::consts::PI * fc_led / sr).tan();
        let k = 1.0 / 0.76; // Dämpfung k = 1/Q (leicht warme Sallen-Key-Charakteristik)
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        for frame in &mut mem.mix_buffer[..limit] {
            let xl = frame.left as f64;
            let xr = frame.right as f64;

            // 1-Pol RC Tiefpass
            let vl_rc = (xl - af.rc_l) * g_rc;
            let yl_rc = vl_rc + af.rc_l;
            af.rc_l = yl_rc + vl_rc;

            let vr_rc = (xr - af.rc_r) * g_rc;
            let yr_rc = vr_rc + af.rc_r;
            af.rc_r = yr_rc + vr_rc;

            // 2-Pol Sallen-Key / ZDF-SVF Tiefpass
            let v3_l = yl_rc - af.svf_ic2_l;
            let v1_l = a1 * af.svf_ic1_l + a2 * v3_l;
            let v2_l = af.svf_ic2_l + a2 * af.svf_ic1_l + a3 * v3_l;
            af.svf_ic1_l = 2.0 * v1_l - af.svf_ic1_l;
            af.svf_ic2_l = 2.0 * v2_l - af.svf_ic2_l;

            let v3_r = yr_rc - af.svf_ic2_r;
            let v1_r = a1 * af.svf_ic1_r + a2 * v3_r;
            let v2_r = af.svf_ic2_r + a2 * af.svf_ic1_r + a3 * v3_r;
            af.svf_ic1_r = 2.0 * v1_r - af.svf_ic1_r;
            af.svf_ic2_r = 2.0 * v2_r - af.svf_ic2_r;

            frame.left = v2_l.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
            frame.right = v2_r.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        }
    }
}

// ============================================================================
// 8. 16-fach entrollte (REPT 16) IT-Referenz-Mixer (M12BIT.ASM & M32BITI.ASM)
// ============================================================================

/// Panning-/Routing-Modus für die 16-fach entrollten (`REPT 16`) Assembler-Mixer
/// aus `legacy_src/IT/M12BIT.ASM` und `legacy_src/IT/M32BITI.ASM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItMixRouting {
    /// Mono-Ausgabe oder reiner Einzelkanal (`Mix*Single`: subtrahiert `AX`/`EAX` auf `[SI]`).
    Single,
    /// ProLogic-Surround (`Mix*Surround`: subtrahiert auf Links `[SI]`, addiert gegenphasig auf Rechts `[SI+2]`/`[SI+4]`).
    Surround,
    /// Mitten-Stereo (`Mix*Central`: subtrahiert identischen Wert auf Links und Rechts).
    Central,
    /// Freies Stereo-Panning (`Mix*Panned`: getrennte linke und rechte Lautstärke).
    Panned,
}

/// Berechnet ein einzelnes linear interpoliertes 8-Bit-Sample nach dem `Get32Bit8Waveform`-Makro
/// in `legacy_src/IT/M32BITI.ASM` (Zeilen 49–56).
///
/// ### Entsprechendes ASM-Makro (`M32BITI.ASM`)
/// ```text
/// MACRO Get32Bit8Waveform
///   MovSX EAX, Byte Ptr [EBX+EDX+1]
///   MovSX EDX, Byte Ptr [EBX+EDX]
///   Sub   EAX, EDX
///   IMul  EAX, ECX
///   SAR   EAX, 8
///   SHL   EDX, 8
///   Add   EAX, EDX
/// ENDM
/// ```
///
/// ### Register-Mapping (x86 -> Rust)
/// - `EBX` -> `sample_mem` / `ebx_base`: Basisadresse des Sample-Speichers
/// - `EDX` -> `edx_pos`: Ganzzahliger Sample-Index (`[EBX+EDX]`), danach `s0` (`s0 << 8`)
/// - `ECX` (`CX`) -> `ecx_frac`: 16-Bit gebrochener Phasenanteil (`0..65535`)
/// - `EAX` -> Rückgabewert `((s1 - s0) * ecx_frac >> 8) + (s0 << 8)` (16-Bit-Äquivalentbereich `-32768..+32512`)
#[inline(always)]
pub fn get_32bit_8_waveform(sample_mem: &[u8], edx_pos: i32, ecx_frac: u16) -> i32 {
    let mut eax = read_sample_i8(sample_mem, edx_pos.wrapping_add(1)) as i32;
    let mut edx = read_sample_i8(sample_mem, edx_pos) as i32;
    eax = eax.wrapping_sub(edx);
    eax = eax.wrapping_mul(ecx_frac as i32);
    eax >>= 8;
    edx <<= 8;
    eax.wrapping_add(edx)
}

/// Berechnet ein einzelnes linear interpoliertes 16-Bit-Sample nach dem `Get32Bit16Waveform`-Makro
/// in `legacy_src/IT/M32BITI.ASM` (Zeilen 58–65).
///
/// ### Entsprechendes ASM-Makro (`M32BITI.ASM`)
/// ```text
/// MACRO Get32Bit16Waveform
///   MovSX EAX, Word Ptr [EBX+EDX+2]
///   MovSX EDX, Word Ptr [EBX+EDX]
///   Sub   EAX, EDX
///   SAR   EAX, 1
///   IMul  EAX, ECX
///   SAR   EAX, 15
///   Add   EAX, EDX
/// ENDM
/// ```
///
/// ### Register-Mapping (x86 -> Rust)
/// - `EBX` -> `sample_mem`: Basisadresse des Sample-Speichers
/// - `EDX` -> `edx_pos`: 16-Bit-Sample-Index (`[EBX + 2*EDX]`), danach `s0`
/// - `ECX` (`CX`) -> `ecx_frac`: 16-Bit gebrochener Phasenanteil (`0..65535`)
/// - `EAX` -> Rückgabewert `(((s1 - s0) >> 1) * ecx_frac >> 15) + s0`
#[inline(always)]
pub fn get_32bit_16_waveform(sample_mem: &[u8], edx_pos: i32, ecx_frac: u16) -> i32 {
    let mut eax = read_sample_i16(sample_mem, edx_pos.wrapping_add(1)) as i32;
    let edx = read_sample_i16(sample_mem, edx_pos) as i32;
    eax = eax.wrapping_sub(edx);
    eax >>= 1;
    eax = eax.wrapping_mul(ecx_frac as i32);
    eax >>= 15;
    eax.wrapping_add(edx)
}

/// Übersetzung der 16-fach entrollten (`REPT 16`) 32-Bit Linear-IDO Mixing-Routinen aus
/// `legacy_src/IT/M32BITI.ASM` (`Mix32BitI8Single`, `Mix32BitI16Single`, `Mix32BitI8Surround`,
/// `Mix32BitI16Surround`, `Mix32BitI8Central`, `Mix32BitI16Central`, `Mix32BitI8Panned`,
/// `Mix32BitI16Panned`, Zeilen 70–664).
///
/// ### Überführung von Self-Modifying Code (SMC) und `REPT 16`-Loop-Unrolling
/// Im Original-Assembler (`M32BITI.ASM`) werden:
/// 1. die 16-Bit-Lautstärkefaktoren (`LeftVolume` / `RightVolume`) per SMC in die 16 entrollten
///    `IMul EAX, EAX, 1234h`-Befehle gepatcht (`PreMix32BitI*`),
/// 2. der Einsprungpunkt innerhalb des `REPT 16`-Blocks über ` MixCount & 15 ` berechnet und
/// 3. in jedem Schritt `Get32Bit8Waveform` bzw. `Get32Bit16Waveform` ausgewertet, mit der Lautstärke
///    multipliziert, vom 32-Bit-Mischpuffer subtrahiert (`Sub [SI], EAX`, bei Surround rechts
///    `Add [SI+4], EAX`) und die Phase über `Add CX, BP ; AdC ESI, EBP_hi` weitergeschaltet.
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi_pos`: Aktuelle ganzzahlige Sample-Position (wird aktualisiert zurückgegeben)
/// - `ECX` (`CX`) -> `cx_frac`: 16-Bit gebrochener Phasenanteil (`0..65535`, wird aktualisiert)
/// - `BP` / `EBP` (High-Word) -> `step_frac` (`u16`) / `step_int` (`i32`): 16.16-Fixed-Point-Pitch-Inkrement
/// - `EBX` -> `sample_mem`: Sample-Speicher
/// - `EDX` -> Temporärer Sample-Index (`MOV EDX, ESI`) und Stützpunkt `s0`
/// - `EAX` -> Interpolierter und skalierter Sample-Wert
/// - `DS:SI` -> `mix_buf`: 32-Bit-Mischpuffer
pub fn mix_it_32bit_ido(
    sample_mem: &[u8],
    is_16bit: bool,
    routing: ItMixRouting,
    esi_pos: &mut i32,
    cx_frac: &mut u16,
    step_int: i32,
    step_frac: u16,
    left_vol: i16,
    right_vol: i16,
    mix_buf: &mut [StereoSample32],
    num_frames: usize,
) {
    let l_vol = left_vol as i32;
    let r_vol = right_vol as i32;
    let count = num_frames.min(mix_buf.len());

    for frame in mix_buf.iter_mut().take(count) {
        let wave = if is_16bit {
            get_32bit_16_waveform(sample_mem, *esi_pos, *cx_frac)
        } else {
            get_32bit_8_waveform(sample_mem, *esi_pos, *cx_frac)
        };

        match routing {
            ItMixRouting::Single => {
                // Sub [SI], EAX
                let eax = wave.wrapping_mul(l_vol);
                frame.left = frame.left.wrapping_sub(eax);
            }
            ItMixRouting::Surround => {
                // Sub [SI], EAX ; Add [SI+4], EAX
                let eax = wave.wrapping_mul(l_vol);
                frame.left = frame.left.wrapping_sub(eax);
                frame.right = frame.right.wrapping_add(eax);
            }
            ItMixRouting::Central => {
                // Sub [SI], EAX ; Sub [SI+4], EAX
                let eax = wave.wrapping_mul(l_vol);
                frame.left = frame.left.wrapping_sub(eax);
                frame.right = frame.right.wrapping_sub(eax);
            }
            ItMixRouting::Panned => {
                // Mov EDX, EAX ; IMul EAX, LeftVol ; IMul EDX, RightVol ; Sub [SI], EAX ; Sub [SI+4], EDX
                let eax = wave.wrapping_mul(l_vol);
                let edx = wave.wrapping_mul(r_vol);
                frame.left = frame.left.wrapping_sub(eax);
                frame.right = frame.right.wrapping_sub(edx);
            }
        }

        // Add CX, BP ; AdC ESI, step_int
        let (new_cx, carry) = cx_frac.overflowing_add(step_frac);
        *cx_frac = new_cx;
        *esi_pos = esi_pos
            .wrapping_add(step_int)
            .wrapping_add(if carry { 1 } else { 0 });
    }
}

/// Übersetzung der Volume-Ramping-Routinen `Mix32Bit8Ramp` und `Mix32Bit16Ramp` aus
/// `legacy_src/IT/M32BITI.ASM` (Zeilen 666–814).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi_pos`: Ganzzahlige Sample-Position
/// - `ECX` (`CX`) -> `cx_frac`: 16-Bit gebrochene Sample-Phase
/// - `EBP` -> `cur_left_vol_16_16` / `cur_right_vol_16_16`: 16.16-Fixed-Point-Lautstärke-Akkumulator
///   (wird pro Sample um `left_vol_ramp_step` / `right_vol_ramp_step` erhöht; der Multiplikator ist
///   `cur_vol >> 16` bzw. `EBP >> 16` nach `ROR EBP, 16`)
/// - `EBX` -> `sample_mem`: Sample-Speicher
/// - `EAX` / `EDX` -> Interpolierter und mit der aktuellen Rampen-Lautstärke skalierter Sample-Wert
#[allow(clippy::too_many_arguments)]
pub fn mix_it_32bit_ido_ramp(
    sample_mem: &[u8],
    is_16bit: bool,
    esi_pos: &mut i32,
    cx_frac: &mut u16,
    step_int: i32,
    step_frac: u16,
    cur_left_vol_16_16: &mut i32,
    cur_right_vol_16_16: &mut i32,
    left_vol_ramp_step: i32,
    right_vol_ramp_step: i32,
    mix_buf: &mut [StereoSample32],
    num_frames: usize,
) {
    let count = num_frames.min(mix_buf.len());
    for frame in mix_buf.iter_mut().take(count) {
        let wave = if is_16bit {
            get_32bit_16_waveform(sample_mem, *esi_pos, *cx_frac)
        } else {
            get_32bit_8_waveform(sample_mem, *esi_pos, *cx_frac)
        };

        let l_vol = (*cur_left_vol_16_16 >> 16) as i16 as i32;
        let r_vol = (*cur_right_vol_16_16 >> 16) as i16 as i32;

        let eax = wave.wrapping_mul(l_vol);
        let edx = wave.wrapping_mul(r_vol);
        frame.left = frame.left.wrapping_sub(eax);
        frame.right = frame.right.wrapping_sub(edx);

        *cur_left_vol_16_16 = cur_left_vol_16_16.wrapping_add(left_vol_ramp_step);
        *cur_right_vol_16_16 = cur_right_vol_16_16.wrapping_add(right_vol_ramp_step);

        let (new_cx, carry) = cx_frac.overflowing_add(step_frac);
        *cx_frac = new_cx;
        *esi_pos = esi_pos
            .wrapping_add(step_int)
            .wrapping_add(if carry { 1 } else { 0 });
    }
}

/// Übersetzung der 16-fach entrollten (`REPT 16`) 12-Bit Volume-Table Mixing-Routinen aus
/// `legacy_src/IT/M12BIT.ASM` (`Mix12Bit8Single`, `Mix12Bit16Single`, `Mix12Bit8Surround`,
/// `Mix12Bit16Surround`, `Mix12Bit8Central`, `Mix12Bit16Central`, `Mix12Bit8Panned`,
/// `Mix12Bit16Panned`, Zeilen 48–813).
///
/// ### Funktionsweise im Original-Assembler (`M12BIT.ASM`)
/// - Bei 8-Bit-Samples wird das Sample-Byte `DL = [EBX+ESI]` um 1 Bit nach links geschoben
///   (`Add DL, DL`, wobei das Carry-Bit über `AdC DH, VolumeTable_Hi` das obere/untere 128-Einträge-Segment
///   der 256-Wort-Lautstärketabelle auswählt), was exakt dem Vorzeichenlosen Byte-Index `raw_byte`
///   in der 256-Einträge-Lautstärketabelle `vol_table[vol][raw_byte]` entspricht.
/// - Bei 16-Bit-Samples werden die oberen 8 Bit (`Mov DL, [EBX+ESI*2+1]`) genutzt (`shr 8`).
///
/// ### Register-Mapping (x86 -> Rust)
/// - `ESI` -> `esi_pos`: Ganzzahlige Sample-Position
/// - `ECX` (`CX`) -> `cx_frac`: 16-Bit gebrochene Sample-Phase
/// - `BP` / `CH` -> `step_frac` / `step_int`: 16.16-Fixed-Point-Pitch-Inkrement
/// - `EBX` -> `sample_mem`: Sample-Speicher
/// - `EDX` (`DX`) -> Index in `vol_table` (`DH = vol`, `DL = sample_u8`)
/// - `AX` -> Aus `ES:[EDX]` gelesener 16-Bit-Tabellenwert
/// - `DS:SI` -> `mix_buf_16`: 16-Bit-Stereo-Akkumulationspuffer (`[Left: i16, Right: i16]`)
#[allow(clippy::too_many_arguments)]
pub fn mix_it_12bit_voltable(
    sample_mem: &[u8],
    vol_table: &[[i16; 256]; 65],
    is_16bit: bool,
    routing: ItMixRouting,
    esi_pos: &mut i32,
    cx_frac: &mut u16,
    step_int: i32,
    step_frac: u16,
    left_vol: u8,
    right_vol: u8,
    mix_buf_16: &mut [(i16, i16)],
    num_frames: usize,
) {
    let l_vol = (left_vol.min(64)) as usize;
    let r_vol = (right_vol.min(64)) as usize;
    let count = num_frames.min(mix_buf_16.len());

    for (left_acc, right_acc) in mix_buf_16.iter_mut().take(count) {
        let dl_byte: usize = if is_16bit {
            // Mov DL, [EBX+ESI*2+1] (High-Byte des 16-Bit-Samples)
            ((read_sample_i16(sample_mem, *esi_pos) as u16) >> 8) as usize
        } else {
            read_sample_u8(sample_mem, *esi_pos) as usize
        };

        let ax_l = vol_table[l_vol][dl_byte];

        match routing {
            ItMixRouting::Single => {
                *left_acc = left_acc.wrapping_sub(ax_l);
            }
            ItMixRouting::Surround => {
                *left_acc = left_acc.wrapping_sub(ax_l);
                *right_acc = right_acc.wrapping_add(ax_l);
            }
            ItMixRouting::Central => {
                *left_acc = left_acc.wrapping_sub(ax_l);
                *right_acc = right_acc.wrapping_sub(ax_l);
            }
            ItMixRouting::Panned => {
                let ax_r = vol_table[r_vol][dl_byte];
                *left_acc = left_acc.wrapping_sub(ax_l);
                *right_acc = right_acc.wrapping_sub(ax_r);
            }
        }

        let (new_cx, carry) = cx_frac.overflowing_add(step_frac);
        *cx_frac = new_cx;
        *esi_pos = esi_pos
            .wrapping_add(step_int)
            .wrapping_add(if carry { 1 } else { 0 });
    }
}

// ============================================================================
// 9. Unit-Tests (inkl. Sinus-Sample-Kanal mit ungeradem Pitch-Step durch IDO)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tables::{DEFAULT_VOL_TABLE_512, SIN_TABLE};
    use crate::engine::types::{mixer_flags, MIX_BUFFER_OFS};

    /// Erzeugt einen Sample-Puffer mit einer 256-Sample-Sinusperiode aus `SIN_TABLE`
    /// ab Sample-Offset `base_offset` (inklusive Guard-Bytes für Interpolation und Loop).
    fn build_8bit_sine_sample_memory(base_offset: usize) -> Vec<u8> {
        let mut mem = vec![0u8; base_offset + 512];
        // Guard-Sample bei base_offset - 1 für Cubic-Spline (entspricht SIN_TABLE[255])
        if base_offset > 0 {
            mem[base_offset - 1] = SIN_TABLE[255] as u8;
        }
        // 2 volle Perioden kopieren, damit an den Loop-Rändern (esi+1, esi+2) nahtlose Sinuswerte liegen
        for i in 0..512 {
            mem[base_offset + i] = SIN_TABLE[i & 0xFF] as u8;
        }
        mem
    }

    #[test]
    fn test_sine_channel_odd_pitch_step_ido_resampling() {
        // --------------------------------------------------------------------
        // Unit-Test gemäß Vorgabe:
        // Schickt einen einzelnen Sinus-Sample-Kanal (`SIN_TABLE`) mit ungeradem
        // 16.16-Pitch-Step durch das IDO-Resampling (`@S08O` / `@S08O1`) und
        // verifiziert die Bit-Identität der Fixed-Point-Arithmetik.
        // --------------------------------------------------------------------
        let base_ofs: i32 = 16;
        let sample_mem = build_8bit_sine_sample_memory(base_ofs as usize);
        let vol_table = &DEFAULT_VOL_TABLE_512;

        // Ungerader gebrochener Pitch-Step:
        // PHi = 0, Plo = 0x3333 (13107 -> ungerade, ~0.200003 Samples pro Ausgabetakt)
        // Start-Phase Rest = 0x0001 (ungerade)
        let odd_plo: u16 = 0x3333;
        let initial_rest: u16 = 0x0001;
        let l_vol: u8 = 64;
        let r_vol: u8 = 32;

        let mut chn_lo = MixChannel {
            note: base_ofs,
            iend: base_ofs + 255,
            lln: 256,
            lend: base_ofs + 255,
            rest: initial_rest as i32,
            plo: odd_plo,
            phi: 0,
            mix_buf: MIX_BUFFER_OFS as i32,
            l_vol,
            r_vol,
            avg_vol: 48,
            jmp_flags: mixer_flags::F_OVERSAMP | mixer_flags::F_PANNED,
            dummy: 0,
        };

        let total_frames = 64usize;
        let mut mix_buf_lo = vec![StereoSample32::default(); total_frames];
        mix_channel_08_oversamp_lo(
            &mut chn_lo,
            &mut mix_buf_lo,
            &sample_mem,
            vol_table,
            total_frames,
        );

        // Prüfe Frame 0 und Frame 1 bit-exakt gegen die manuelle ASM-Formel aus `@S08O`:
        // Am Start: esi = 16 (SIN_TABLE[0] = 0), esi+1 = 17 (SIN_TABLE[1] = -3)
        // VolTable[64][0] = 0, VolTable[64][253 (-3)] = -3 * 64 = -192
        let s0_l = vol_table[l_vol as usize][SIN_TABLE[0] as u8 as usize] as i32; // 0
        let s1_l = vol_table[l_vol as usize][SIN_TABLE[1] as u8 as usize] as i32; // -192
        let l_diff = s1_l - s0_l; // -192
        let l_incr = (l_diff * (odd_plo as i32)) >> 8; // (-192 * 13107) >> 8 = -9831
        let l_start = (s0_l << 8) + ((l_diff * (initial_rest as i32)) >> 8); // (0) + ((-192 * 1) >> 8) = -1

        let s0_r = vol_table[r_vol as usize][SIN_TABLE[0] as u8 as usize] as i32; // 0
        let s1_r = vol_table[r_vol as usize][SIN_TABLE[1] as u8 as usize] as i32; // -96
        let r_diff = s1_r - s0_r; // -96
        let r_incr = (r_diff * (odd_plo as i32)) >> 8; // (-96 * 13107) >> 8 = -4916
        let r_start = (s0_r << 8) + ((r_diff * (initial_rest as i32)) >> 8); // -1

        assert_eq!(mix_buf_lo[0].left, l_start);
        assert_eq!(mix_buf_lo[0].right, r_start);
        assert_eq!(mix_buf_lo[1].left, l_start + l_incr);
        assert_eq!(mix_buf_lo[1].right, r_start + r_incr);

        // Prüfe, dass nach 64 Frames (inkl. `@WriteChn` Rückschritt um 1 Schritt = 63 effektive Schritte)
        // die 16.16-Fixed-Point-Phase exakt `initial_rest + 63 * odd_plo` beträgt:
        let expected_total_phase: u32 = (initial_rest as u32) + 63 * (odd_plo as u32);
        let expected_rest_u16 = (expected_total_phase & 0xFFFF) as u16;
        let expected_note = base_ofs + ((expected_total_phase >> 16) as i32);
        let actual_rest_u16 = chn_lo.rest as u16;
        let actual_note = chn_lo.note;
        assert_eq!(actual_rest_u16, expected_rest_u16);
        assert_eq!(actual_note, expected_note);

        // Teste nun auch einen ungeraden High-Pitch-Step (PHi = 1, Plo = 0x5555) durch `@S08O1`
        // und vergleiche jedes Sample mit `get_new_smp_val`!
        let odd_hi_plo: u16 = 0x5555;
        let odd_hi_phi: i32 = 1;
        let mut chn_hi = MixChannel {
            note: base_ofs,
            iend: base_ofs + 255,
            lln: 256,
            lend: base_ofs + 255,
            rest: 0x1357,
            plo: odd_hi_plo,
            phi: odd_hi_phi,
            mix_buf: MIX_BUFFER_OFS as i32,
            l_vol: 64,
            r_vol: 48,
            avg_vol: 56,
            jmp_flags: mixer_flags::F_OVERSAMP | mixer_flags::F_HI_PITCH | mixer_flags::F_PANNED,
            dummy: 0,
        };

        // Referenzberechnung über Einzel-Sample-Funktion `get_new_smp_val`:
        let mut probe_chn = Channel::default();
        probe_chn.note = chn_hi.note;
        probe_chn.iend = chn_hi.iend;
        probe_chn.lln = chn_hi.lln;
        probe_chn.rest = chn_hi.rest;
        probe_chn.jmp_flags = chn_hi.jmp_flags;
        let expected_f0_l = get_new_smp_val(&probe_chn, 64, 512, vol_table, &sample_mem);
        let expected_f0_r = get_new_smp_val(&probe_chn, 48, 512, vol_table, &sample_mem);

        let hi_frames = 256usize;
        let mut mix_buf_hi = vec![StereoSample32::default(); hi_frames];
        mix_channel_08_oversamp_hi(
            &mut chn_hi,
            &mut mix_buf_hi,
            &sample_mem,
            vol_table,
            hi_frames,
        );

        assert_eq!(mix_buf_hi[0].left, expected_f0_l);
        assert_eq!(mix_buf_hi[0].right, expected_f0_r);

        // Verifiziere, dass der Sinusverlauf sowohl positive als auch negative Halbwelle durchläuft
        let has_positive = mix_buf_hi.iter().any(|s| s.left > 100_000);
        let has_negative = mix_buf_hi.iter().any(|s| s.left < -100_000);
        assert!(has_positive && has_negative);
    }

    #[test]
    fn test_16bit_ido_and_cubic_spline_modes() {
        // Erzeuge 16-Bit-Sinus-Sample im Speicher (SIN_TABLE[i] * 256)
        let base_ofs: i32 = 8;
        let mut sample_mem = vec![0u8; ((base_ofs as usize) + 512) * 2];
        write_sample_i16(
            &mut sample_mem,
            (base_ofs - 1) as usize,
            (SIN_TABLE[255] as i16) * 256,
        );
        for i in 0..512 {
            write_sample_i16(
                &mut sample_mem,
                (base_ofs as usize) + i,
                (SIN_TABLE[i & 0xFF] as i16) * 256,
            );
        }

        let total_frames = 32usize;
        let vol_master = 512u16; // scale = (64 * 512) >> 9 = 64

        for interp_flag in [
            mixer_flags::F_OVERSAMP,
            mixer_flags::F_CPU_CUBIC,
            mixer_flags::F_FPU_CUBIC,
        ] {
            let mut chn = MixChannel {
                note: base_ofs,
                iend: base_ofs + 255,
                lln: 256,
                lend: base_ofs + 255,
                rest: 0x4321,
                plo: 0x7777, // ungerader Pitch-Step
                phi: 1,
                mix_buf: MIX_BUFFER_OFS as i32,
                l_vol: 64,
                r_vol: 64,
                avg_vol: 64,
                jmp_flags: interp_flag | mixer_flags::F_16BIT | mixer_flags::F_MONO,
                dummy: 0,
            };

            let mut buf = vec![StereoSample32::default(); total_frames];
            dispatch_mix_channel(
                &mut chn,
                &mut buf,
                &sample_mem,
                &DEFAULT_VOL_TABLE_512,
                vol_master,
                total_frames,
            );

            // Mono: Left und Right müssen in jedem Frame exakt übereinstimmen
            for frame in &buf {
                assert_eq!(frame.left, frame.right);
            }
            assert!(buf.iter().any(|f| f.left != 0));
        }
    }

    #[test]
    fn test_click_removal_surround_and_clipping() {
        let mut state = EngineState::default();
        let mut mem = DacMixMemory::default();

        let base_ofs: i32 = 16;
        let sample_mem = build_8bit_sine_sample_memory(base_ofs as usize);

        state.mi.num_tracks = 1;
        state.buf_samples = 32;
        state.num_samples = 32;
        state.vol_master = 512;
        state.use_surround = true;
        state.buf_start = 0;
        state.buf_end = 16 * 8; // 16 Frames Delay
        state.sound_settings.bits = 16;
        state.sound_settings.stereo = 2;
        state.sound_settings.pre_amp = 0x80 | 20; // Clipping aktiv

        state.channels[0].note = base_ofs;
        state.channels[0].iend = base_ofs + 255;
        state.channels[0].lln = 256;
        state.channels[0].lend = base_ofs + 255;
        state.channels[0].rest = 0x0001;
        state.channels[0].plo = 0x5111;
        state.channels[0].phi = 1;
        state.channels[0].mix_buf = SUR_BUFFER_OFS as i32;
        state.channels[0].l_vol = 64;
        state.channels[0].r_vol = 64;
        state.channels[0].avg_vol = 64;
        state.channels[0].jmp_flags = mixer_flags::F_OVERSAMP | mixer_flags::F_PANNED;

        let out = process_sample_block(&mut state, &mut mem, &sample_mem, false);
        match out {
            MixOutputBuffer::Pcm16(pcm) => {
                assert_eq!(pcm.len(), 64); // 32 Stereo-Frames = 64 i16-Werte
                assert!(pcm.iter().any(|&v| v != 0));
            }
            _ => panic!("Expected Pcm16 output"),
        }

        // Prüfe hartes Sättigungs-Clipping in `@2Q16S`:
        mem.mix_buffer[0] = StereoSample32 {
            left: 100_000 << 8,  // > +32767
            right: -100_000 << 8, // < -32768
        };
        let clipped = mix_out_and_clip(
            &mem.mix_buffer,
            1,
            16,
            2,
            0x80 | 20,
            false,
            &mut state.max_level,
        );
        if let MixOutputBuffer::Pcm16(v) = clipped {
            assert_eq!(v[0], 32767);
            assert_eq!(v[1], -32768);
        } else {
            panic!("Expected Pcm16");
        }
    }

    #[test]
    fn test_it_m32biti_and_m12bit_reference_mixers() {
        let base_ofs: i32 = 4;
        let sample_mem = build_8bit_sine_sample_memory(base_ofs as usize);

        let mut esi_pos = base_ofs;
        let mut cx_frac = 0x1111u16;
        let mut buf32 = vec![StereoSample32::default(); 20];

        mix_it_32bit_ido(
            &sample_mem,
            false,
            ItMixRouting::Surround,
            &mut esi_pos,
            &mut cx_frac,
            0,
            0x3333,
            64,
            64,
            &mut buf32,
            20,
        );

        // Bei Surround in M32BITI.ASM gilt: Left = -val, Right = +val
        for frame in &buf32 {
            assert_eq!(frame.left, -frame.right);
        }
        assert!(buf32.iter().any(|f| f.left != 0));
    }

    #[test]
    fn test_per_channel_declick_loop_wrap_and_oneshot_end() {
        let base_ofs: i32 = 16;
        // Baue ein Sample mit hartem Loop-Sprung: erste Hälfte -100, zweite Hälfte +100
        let mut sample_mem = vec![0u8; 512];
        for i in 0..64 {
            sample_mem[(base_ofs as usize) + i] = (-100i8) as u8;
        }
        for i in 64..128 {
            sample_mem[(base_ofs as usize) + i] = (100i8) as u8;
        }
        // Guard-Bytes am Loop-Ende kopieren Loop-Start (-100)
        for g in 0..4 {
            sample_mem[(base_ofs as usize) + 128 + g] = (-100i8) as u8;
        }

        let run_loop_test = |declick: bool| -> i32 {
            let mut state = EngineState::default();
            let mut mem = DacMixMemory::default();
            state.mi.num_tracks = 1;
            state.buf_samples = 200;
            state.num_samples = 200;
            state.vol_master = 512;
            state.sound_settings.samplerate = 44100;
            state.sound_settings.bits = 16;
            state.sound_settings.stereo = 2;
            state.sound_settings.pre_amp = 0x80 | 20;
            state.sound_settings.set_declick(declick);

            // Starte kurz vor dem Loop-Ende (bei Index 110 von 127), damit der Wrap mitten im Tick passiert
            state.channels[0].note = base_ofs + 110;
            state.channels[0].iend = base_ofs + 127;
            state.channels[0].lln = 128;
            state.channels[0].lend = base_ofs + 127;
            state.channels[0].rest = 0;
            state.channels[0].plo = 0x4000; // Pitch 0.25 -> ~68 Output-Frames bis zum Wrap
            state.channels[0].phi = 0;
            state.channels[0].l_vol = 64;
            state.channels[0].r_vol = 64;
            state.channels[0].avg_vol = 64;
            state.channels[0].jmp_flags = mixer_flags::F_PANNED;

            let MixOutputBuffer::Pcm16(pcm) =
                process_sample_block(&mut state, &mut mem, &sample_mem, false)
            else {
                panic!("Expected Pcm16");
            };
            // Messe den maximalen Sprung zwischen zwei aufeinanderfolgenden Samples ab Frame 40 (um den Loop-Wrap bei Frame ~68)
            let mut max_step = 0i32;
            for f in 40..190 {
                let s0 = pcm[f * 2] as i32;
                let s1 = pcm[(f + 1) * 2] as i32;
                max_step = max_step.max((s1 - s0).abs());
            }
            max_step
        };

        let step_without_dc = run_loop_test(false);
        let step_with_dc = run_loop_test(true);
        assert!(
            step_with_dc < step_without_dc / 4,
            "De-Click muss den Loop-Wrap-Sprung deutlich glätten (ohne DC: {}, mit DC: {})",
            step_without_dc,
            step_with_dc
        );

        // Teste Mid-Tick One-Shot-Ende mit 2-Byte-Amiga-End-Artefakt (+127 an den letzten 2 Bytes)
        let mut oneshot_mem = vec![0u8; 256];
        for i in 0..40 {
            oneshot_mem[(base_ofs as usize) + i] = (80i8) as u8;
        }
        oneshot_mem[(base_ofs as usize) + 39] = (127i8) as u8;
        oneshot_mem[(base_ofs as usize) + 40] = (-128i8) as u8;

        let mut state = EngineState::default();
        let mut mem = DacMixMemory::default();
        state.mi.num_tracks = 1;
        state.buf_samples = 160;
        state.num_samples = 160;
        state.vol_master = 512;
        state.sound_settings.samplerate = 44100;
        state.sound_settings.bits = 16;
        state.sound_settings.stereo = 2;
        state.sound_settings.pre_amp = 0x80 | 20;
        state.sound_settings.set_declick(true);

        state.channels[0].i_offset = base_ofs;
        state.channels[0].note = base_ofs + 20;
        state.channels[0].iend = base_ofs + 40;
        state.channels[0].lln = base_ofs + 41; // One-Shot (lln > iend)
        state.channels[0].lend = 0;
        state.channels[0].rest = 0;
        state.channels[0].plo = 0;
        state.channels[0].phi = 1;
        state.channels[0].l_vol = 64;
        state.channels[0].r_vol = 64;
        state.channels[0].avg_vol = 64;
        state.channels[0].jmp_flags = mixer_flags::F_PANNED;

        let MixOutputBuffer::Pcm16(pcm) =
            process_sample_block(&mut state, &mut mem, &oneshot_mem, false)
        else {
            panic!("Expected Pcm16");
        };
        let final_note = state.channels[0].note;
        assert_eq!(final_note, 0);
        // Am Ende des Blocks muss das One-Shot-Sample sanft auf 0 ausgeblendet sein (kein DC-Halten!)
        assert_eq!(pcm[159 * 2], 0);

        // Teste schlechtes Sample mit abruptem Abbruch (+90 -> 0) vor 32 Null-Bytes (Zero-Padding)
        // plus Ripper-Spike (-128/+127) am Pufferende bei aktivem Cubic-Spline!
        let mut bad_tail_mem = vec![0u8; 512];
        for i in 0..80 {
            bad_tail_mem[(base_ofs as usize) + i] = (90i8) as u8;
        }
        // Ab Index 80..110: Stille (0), dann am Ende 110..112 ein Ripper-Click (-128, +127)
        bad_tail_mem[(base_ofs as usize) + 110] = (-128i8) as u8;
        bad_tail_mem[(base_ofs as usize) + 111] = (127i8) as u8;

        let (is_os, eff_end) = analyze_sample_tail(
            &bad_tail_mem,
            base_ofs,
            base_ofs,
            base_ofs + 111,
            0,
            base_ofs + 200,
            false,
        );
        assert!(is_os);
        // `effective_iend` muss VOR der Klippe bei base_ofs + 80 liegen!
        assert!(
            eff_end < base_ofs + 80,
            "effective_iend ({}) muss vor der Zero-Padding-Klippe ({}) liegen",
            eff_end,
            base_ofs + 80
        );
    }

    #[test]
    fn test_amiga_filter_and_adaptive_dehiss_denoiser() {
        let sr: i32 = 44100;
        let frames = 4410usize; // 100 ms

        // 1. Bypass-Test: Wenn weder Amiga noch DeHiss aktiv ist, bleibt mix_buffer bit-identisch.
        let mut state = EngineState::default();
        let mut mem = DacMixMemory::default();
        state.sound_settings.samplerate = sr;
        state.sound_settings.set_amiga_filter(false);
        state.sound_settings.set_dehiss(false);

        for i in 0..256 {
            mem.mix_buffer[i] = StereoSample32 {
                left: (i as i32 * 137) - 10000,
                right: (i as i32 * -91) + 5000,
            };
        }
        let orig_buf = mem.mix_buffer[..256].to_vec();
        apply_post_filters(&state, &mut mem, 256);
        assert_eq!(&mem.mix_buffer[..256], &orig_buf[..]);

        // 2. Amiga-Filter-Test: 250 Hz passiert nahezu ungedämpft, 7000 Hz wird stark bedämpft.
        let measure_amiga_rms = |freq_hz: f32| -> f32 {
            let mut st = EngineState::default();
            let mut m = DacMixMemory::default();
            st.sound_settings.samplerate = sr;
            st.sound_settings.set_amiga_filter(true);
            st.sound_settings.set_dehiss(false);

            let amp = 100_000.0f32;
            let mut sum_sq = 0.0f64;
            let mut count = 0usize;
            let chunk = 512usize;
            for block in 0..8 {
                for i in 0..chunk {
                    let t = ((block * chunk + i) as f32) / (sr as f32);
                    let v = (2.0 * std::f32::consts::PI * freq_hz * t).sin() * amp;
                    m.mix_buffer[i] = StereoSample32 {
                        left: v as i32,
                        right: v as i32,
                    };
                }
                apply_post_filters(&st, &mut m, chunk);
                if block >= 2 {
                    for i in 0..chunk {
                        let y = m.mix_buffer[i].left as f64;
                        sum_sq += y * y;
                        count += 1;
                    }
                }
            }
            (sum_sq / (count as f64)).sqrt() as f32
        };

        let rms_250 = measure_amiga_rms(250.0);
        let rms_7000 = measure_amiga_rms(7000.0);
        assert!(
            rms_250 > 60_000.0,
            "Amiga-Filter muss 250 Hz nahezu ungedämpft durchlassen (rms_250 = {})",
            rms_250
        );
        assert!(
            rms_7000 < rms_250 * 0.22,
            "Amiga-Filter muss 7000 Hz stark dämpfen (rms_250 = {}, rms_7000 = {})",
            rms_250,
            rms_7000
        );

        // 3. DeHiss Adaptive 5-Band Wiener Denoiser Test:
        // Ein kräftiger 220-Hz-Bass/Pad-Ton (Amplitude 120_000) überlagert mit 8-Bit-Quantisierungsrauschen
        // bei 6200 Hz (Amplitude 1_200). DeHiss muss den 6200-Hz-Rauschanteil um > 12 dB absenken,
        // während der 220-Hz-Grundton erhalten bleibt.
        let mut st_dh = EngineState::default();
        let mut m_dh = DacMixMemory::default();
        st_dh.sound_settings.samplerate = sr;
        st_dh.sound_settings.set_amiga_filter(false);
        st_dh.sound_settings.set_dehiss(true);

        let chunk = 512usize;
        let mut hf_energy_in = 0.0f64;
        let mut hf_energy_out = 0.0f64;
        let mut lf_energy_in = 0.0f64;
        let mut lf_energy_out = 0.0f64;

        for block in 0..(frames / chunk) {
            let mut pure_lf = vec![0.0f32; chunk];
            let mut pure_hf = vec![0.0f32; chunk];
            for i in 0..chunk {
                let sample_idx = block * chunk + i;
                let t = (sample_idx as f32) / (sr as f32);
                let lf = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 120_000.0;
                let hf = (2.0 * std::f32::consts::PI * 6200.0 * t).sin() * 1_200.0;
                pure_lf[i] = lf;
                pure_hf[i] = hf;
                let tot = (lf + hf).round() as i32;
                m_dh.mix_buffer[i] = StereoSample32 {
                    left: tot,
                    right: tot,
                };
            }
            apply_post_filters(&st_dh, &mut m_dh, chunk);
            if block >= 2 {
                for i in 0..chunk {
                    let out = m_dh.mix_buffer[i].left as f32;
                    let resid_hf = (out - pure_lf[i]) as f64;
                    hf_energy_in += (pure_hf[i] as f64).powi(2);
                    hf_energy_out += resid_hf.powi(2);
                    lf_energy_in += (pure_lf[i] as f64).powi(2);
                    lf_energy_out += (out as f64).powi(2);
                }
            }
        }

        let hf_rms_ratio = (hf_energy_out / hf_energy_in).sqrt();
        let lf_rms_ratio = (lf_energy_out / lf_energy_in).sqrt();
        assert!(
            hf_rms_ratio < 0.25,
            "DeHiss muss hochfrequentes Quantisierungsrauschen stark dämpfen (hf_rms_ratio = {:.3})",
            hf_rms_ratio
        );
        assert!(
            (lf_rms_ratio - 1.0).abs() < 0.05,
            "DeHiss darf den 220-Hz-Grundton nicht verfälschen (lf_rms_ratio = {:.3})",
            lf_rms_ratio
        );

        // 4. Transienten-Schutz (Hi-Hat / Snare Burst):
        // Ein plötzlicher starker Hochfrequenz-Impuls muss `transient_hold` auslösen und klar passieren.
        for i in 0..chunk {
            let t = (i as f32) / (sr as f32);
            let burst = (2.0 * std::f32::consts::PI * 6500.0 * t).sin() * 90_000.0;
            m_dh.mix_buffer[i] = StereoSample32 {
                left: burst as i32,
                right: burst as i32,
            };
        }
        apply_post_filters(&st_dh, &mut m_dh, chunk);
        let max_burst_out = m_dh.mix_buffer[..chunk]
            .iter()
            .map(|s| s.left.abs())
            .max()
            .unwrap_or(0);
        assert!(
            max_burst_out > 75_000,
            "DeHiss Transienten-Schutz muss scharfe Hi-Hat-Bursts ungedämpft durchlassen (max_burst_out = {})",
            max_burst_out
        );
    }
}
