//! Exakte Rust-Äquivalente der zentralen Datenstrukturen der MOD4WIN 2.40 Beta Audio-Engine.
//!
//! Quelle:
//! - `legacy_src/PLAYER/playdefs.pas`
//! - `legacy_src/PLAYER/dacmix.pas`
//! - `legacy_src/PLAYER/play.inc`
//! - `legacy_src/SOURCE/modc.pas`
//!
//! Alle Register- und Feldbreiten entsprechen 1:1 den 16/32-Bit Borland-Pascal-7.0-Typen:
//! - `Byte` / `Char` / `Boolean` -> `u8` / `bool`
//! - `Shortint` -> `i8`
//! - `Word` / `WordBool` -> `u16`
//! - `Integer` -> `i16`
//! - `Longint` -> `i32`

// ============================================================================
// 1. Globale Engine-, Puffer- und Speicherkonstanten (playdefs.pas, modc.pas)
// ============================================================================

/// Maximale Anzahl der Tracker-Kanäle (`MaxM4WChannels = 32` in `modc.pas`).
pub const MAX_M4W_CHANNELS: usize = 32;

/// Maximale Anzahl der Patterns im Arrangement (`MaxPatterns = 256` in `playdefs.pas`).
pub const MAX_PATTERNS: usize = 256;

/// Maximale Zeilenanzahl pro Pattern (`MaxPattLen = 256` in `playdefs.pas`).
pub const MAX_PATT_LEN: usize = 256;

/// Maximale Anzahl von Samples pro Mix-Block (`MaxMixBufSamples = 2000` in `playdefs.pas`).
pub const MAX_MIX_BUF_SAMPLES: usize = 2000;

/// Standardwert für eine leere Lautstärkespalte (`DefChVol = $FF` in `playdefs.pas`).
pub const DEF_CH_VOL: u8 = 0xFF;

/// Höchster Index im `tPlayTimes`-Synchronisations-Ringpuffer (`MaxPlayBuffer = 127`).
pub const MAX_PLAY_BUFFER: usize = 127;

/// Maximale Anzahl von WaveOut-Puffern (`MaxBuffers = 99`).
pub const MAX_BUFFERS: usize = 99;

/// Länge des Surround-Ringpuffers in Bytes (`SurrBufferLen = 8192` in `playdefs.pas`).
pub const SURR_BUFFER_LEN: usize = 8192;

/// Surround-Verzögerungszeit in Millisekunden (`SurrLen = 15` in `playdefs.pas`).
pub const SURR_LEN: u16 = 15;

// --- Offsets innerhalb des 16-Bit/32-Bit-Mischspeichers (PlayMemSel in playdefs.pas) ---

/// Offset des temporären Kanal-Speichers (`ChannelOfs = 14208` = `0x3780`).
pub const CHANNEL_OFS: u16 = 14208;

/// Offset des 32-Bit-Stereo-Hauptmischpuffers (`MixBufferOfs = 33280` in `playdefs.pas` Zeile 757).
pub const MIX_BUFFER_OFS: u16 = 33280;

/// Offset des 32-Bit-Surround-Mischpuffers (`SurBufferOfs = MixBufferOfs + (MaxMixBufSamples + 1) * 8 = 49288` in `playdefs.pas` Zeile 758).
pub const SUR_BUFFER_OFS: u16 = 49288;

/// Primärer Offset der 8-Bit-Lautstärke-Lookup-Tabelle (`VolTableOfs1 = 16000` = `0x3E80`).
pub const VOL_TABLE_OFS1: u16 = 16000;

/// Sekundärer Offset der 8-Bit-Lautstärke-Lookup-Tabelle (`VolTableOfs2 = 48000` = `0xBB80`).
pub const VOL_TABLE_OFS2: u16 = 48000;

// --- Sound-Device-IDs (modc.pas / playdefs.pas) ---

pub const DEV_MAPPER: i16 = -1;
pub const DEV_DAC: i16 = 0;
pub const DEV_HDD: i16 = -2;
pub const DEV_OPL4: i16 = -3;
pub const DEV_GF1: i16 = -4;
pub const DEV_EWS: i16 = -5;
pub const DEV_DS: i16 = 256;

// --- Timer-Modi (playdefs.pas) ---

pub const TMODE_MM_TASK: u8 = 0;
pub const TMODE_WM_TIMER: u8 = 1;
pub const TMODE_MM_TIMER: u8 = 2;
/// Bit 4 (`0x10`) in `SoundSettings::t_mode` kodiert den authentischen Amiga-Paula-Tiefpassfilter
/// (4.42 kHz RC + ~3.275 kHz 12 dB/Okt Sallen-Key LED-Filter).
pub const TMODE_AMIGA_FILTER_FLAG: u8 = 0x10;
/// Bit 5 (`0x20`) in `SoundSettings::t_mode` kodiert den modernen adaptiven Mehrband-Denoiser (`DeHiss`).
pub const TMODE_DEHISS_FLAG: u8 = 0x20;
/// Bit 6 (`0x40`) in `SoundSettings::t_mode` kodiert den optionalen Kanal-De-Click-Filter (`DeClick`).
pub const TMODE_DECLICK_FLAG: u8 = 0x40;
/// Bit 7 (`0x80`) in `SoundSettings::t_mode` kodiert den optionalen Clean-Mode
/// (`true` = 1995er Stack-/Speicher-Bugs bereinigt, `false` = 100% bit-identische Full-Emulation).
pub const TMODE_CLEAN_FLAG: u8 = 0x80;
/// Maske für alle Zusatzflags in `SoundSettings::t_mode`.
pub const TMODE_FLAGS_MASK: u8 =
    TMODE_CLEAN_FLAG | TMODE_DECLICK_FLAG | TMODE_DEHISS_FLAG | TMODE_AMIGA_FILTER_FLAG;

// --- Fehler-Codes (playdefs.pas / modc.pas) ---

pub const ID_NO_MEM: u16 = 1;
pub const ID_FILE_NOT_FOUND: u16 = 2;
pub const ID_NO_DEVICE: u16 = 3;
pub const ID_SYNC_DEV: u16 = 4;
pub const ID_NO_MOD_FILE: u16 = 5;
pub const ID_FILE_CORRUPTED: u16 = 6;
pub const ID_NIX_FILE: u16 = 9;
pub const ID_INV_MOD_FILE: u16 = 10;
pub const ID_FILE_UNSUPPORT: u16 = 11;

// ============================================================================
// 2. Globale Mixer-Flags, Sample-Flags und Effekt-IDs (dacmix.pas, playdefs.pas)
// ============================================================================

/// Bitflags für `MixChannel::jmp_flags` bzw. `Channel::jmp_flags` (`dacmix.pas` Zeilen 9–17).
///
/// Steuert direkt den Sprungverteiler im Assembler-Mixer (`dacmix.pas`: `ProcessSampleBlock`),
/// ob 8/16-Bit, Mono/Links/Rechts/Panned, High-Pitch (`PHi > 0` bzw. `Pitch > 30000`) sowie
/// ohne Interpolation, lineare Interpolation (`fOverSamp`), CPU-Cubic-Spline oder FPU-Cubic-Spline
/// gemischt wird.
pub mod mixer_flags {
    /// Mittiges Panning (`lVol == rVol`, `fMono = $00`).
    pub const F_MONO: u16 = 0x0000;
    /// Nur linker Kanal aktiv (`rVol == 0`, `fLeft = $02`).
    pub const F_LEFT: u16 = 0x0002;
    /// Nur rechter Kanal aktiv (`lVol == 0`, `fRight = $04`).
    pub const F_RIGHT: u16 = 0x0004;
    /// Freies Stereo-Panning (`lVol != rVol` und beide `!= 0`, `fPanned = $06`).
    pub const F_PANNED: u16 = 0x0006;
    /// Maske für die Panning-Routing-Bits (`0x0006`).
    pub const F_PAN_MASK: u16 = 0x0006;
    /// Hohe Schrittweite (`Pitch > 30000` bzw. `PHi > 0`, `fHiPitch = $08`).
    pub const F_HI_PITCH: u16 = 0x0008;
    /// 16-Bit-Sampledaten statt 8-Bit (`f16Bit = $10`).
    pub const F_16BIT: u16 = 0x0010;
    /// Lineares 2x-Oversampling / Interpolation (`fOverSamp = $20`).
    pub const F_OVERSAMP: u16 = 0x0020;
    /// 32-Bit Integer Cubic-Spline-Interpolation (`fCPUCubic = $40`).
    pub const F_CPU_CUBIC: u16 = 0x0040;
    /// x87 FPU Cubic-Spline-Interpolation (`fFPUCubic = $60`).
    pub const F_FPU_CUBIC: u16 = 0x0060;
    /// Maske für die Interpolations-Modi (`0x0060`).
    pub const F_INTERP_MASK: u16 = 0x0060;
}

/// Sample-Format- und Loop-Flags für `MemWrite` und `Instrument::bits` (`playdefs.pas` Zeilen 889–902).
pub mod sample_flags {
    /// 8-Bit-Sample (`s8Bit = 8`).
    pub const S_8BIT: u8 = 0x08;
    /// 16-Bit-Sample (`s16Bit = 16`).
    pub const S_16BIT: u8 = 0x10;
    /// Maske für die Bitbreite (`Bits AND $1F`).
    pub const S_BITS_MASK: u8 = 0x1F;
    /// Vorzeichenbehaftete PCM-Daten (`sSigned = 0`).
    pub const S_SIGNED: u8 = 0x00;
    /// Vorzeichenlose PCM-Daten (`sUnsigned = 32`).
    pub const S_UNSIGNED: u8 = 0x20;
    /// Delta-kodierte Sample-Daten (`sDelta = 64`, z. B. FT2/XM).
    pub const S_DELTA: u8 = 0x40;
    /// Bidirektionaler (Ping-Pong) Loop (`sBidiLoop = 128`, wird beim Laden entpackt).
    pub const S_BIDI_LOOP: u8 = 0x80;
}

/// Bitflags für `Channel::event_chg` und `Channel::back_e_chg`
/// (`SET OF (EInst, EPitch, EVol, EReset)` in `playdefs.pas` Zeile 196).
pub mod event_flags {
    /// Instrument wurde neu getriggert (`EInst = 0`).
    pub const E_INST: u8 = 1 << 0;
    /// Tonhöhe / Periode hat sich geändert (`EPitch = 1`).
    pub const E_PITCH: u8 = 1 << 1;
    /// Lautstärke oder Panning hat sich geändert (`EVol = 2`).
    pub const E_VOL: u8 = 1 << 2;
    /// Kanal wurde gestoppt / zurückgesetzt (`EReset = 3`).
    pub const E_RESET: u8 = 1 << 3;
}

/// Interne Effekt-Konstanten der Audio-Engine (`playdefs.pas` Zeilen 904–971).
pub mod effect_ids {
    pub const ARPEGGIO: u8 = 0x00;
    pub const PORTAMENTO_UP: u8 = 0x01;
    pub const PORTAMENTO_DOWN: u8 = 0x02;
    pub const TONE_PORTAMENTO: u8 = 0x03;
    pub const VIBRATO: u8 = 0x04;
    pub const PORT_VOLSLIDE: u8 = 0x05;
    pub const VIBRA_VOLSLID: u8 = 0x06;
    pub const TREMOLO: u8 = 0x07;
    pub const PANNING: u8 = 0x08;
    pub const PLAY_OFFSET: u8 = 0x09;
    pub const VOLUME_SLIDE: u8 = 0x0A;
    pub const POSITION_JUMP: u8 = 0x0B;
    pub const SET_VOLUME: u8 = 0x0C;
    pub const PATTERN_BREAK: u8 = 0x0D;
    pub const ENHANCED: u8 = 0x0E;
    pub const SET_SPEED: u8 = 0x0F;
    // Extended Effects (E0x..EFx -> $10..$1F)
    pub const SET_FILTER: u8 = 0x10;
    pub const F_SLD_UP: u8 = 0x11;
    pub const F_SLD_DWN: u8 = 0x12;
    pub const GLISSANDO_CTRL: u8 = 0x13;
    pub const VIBRATO_WAVE: u8 = 0x14;
    pub const FINE_TUNE: u8 = 0x15;
    pub const PATTRN_LOOP: u8 = 0x16;
    pub const TREMOLO_WAVE: u8 = 0x17;
    pub const E_PANNING: u8 = 0x18;
    pub const RETRIG_NOTE: u8 = 0x19;
    pub const FINE_VOL_UP: u8 = 0x1A;
    pub const FINE_VOL_DWN: u8 = 0x1B;
    pub const NOTE_CUT: u8 = 0x1C;
    pub const NOTE_DELAY: u8 = 0x1D;
    pub const PATTERN_DELAY: u8 = 0x1E;
    pub const INV_LOOP: u8 = 0x1F;
    // Oktalyzer Effects ($20..$26)
    pub const OKT_ARPG1: u8 = 0x20;
    pub const OKT_ARPG2: u8 = 0x21;
    pub const OKT_SLD_UP: u8 = 0x22;
    pub const OKT_SLD_DWN: u8 = 0x23;
    pub const OKT_SLD1_UP: u8 = 0x24;
    pub const OKT_SLD1_DWN: u8 = 0x25;
    pub const OKT_OLD_VOL: u8 = 0x26;
    // FAR Effects ($27..$33)
    pub const FAR_SPECIAL: u8 = 0x27;
    pub const FAR_SLD_UP: u8 = 0x28;
    pub const FAR_SLD_DWN: u8 = 0x29;
    pub const FAR_PORTAMENTO: u8 = 0x2A;
    pub const FAR_RETRIG: u8 = 0x2B;
    pub const VIBRATO_DEPTH: u8 = 0x2C;
    pub const FAR_VIBRATO: u8 = 0x2D;
    pub const FAR_VOL_UP: u8 = 0x2E;
    pub const FAR_VOL_DWN: u8 = 0x2F;
    pub const VIBRATO_SUST: u8 = 0x30;
    pub const VOLUME_PORT: u8 = 0x31;
    pub const FINE_TEMPO_DWN: u8 = 0x32;
    pub const FINE_TEMPO_UP: u8 = 0x33;
    pub const FINE_FAR_TEMPO: u8 = 0x33;
    // STM / S3M Effects ($34..$3B)
    pub const TREMOR: u8 = 0x34;
    pub const XTRA_F_SLD_UP: u8 = 0x35;
    pub const XTRA_F_SLD_DWN: u8 = 0x36;
    pub const RETRIG_VOL_SLD: u8 = 0x37;
    pub const STEREO_CONTROL: u8 = 0x38;
    pub const SET_GLOB_VOL: u8 = 0x39;
    pub const S3M_BPM: u8 = 0x3A;
    pub const FINE_VIBRATO: u8 = 0x3B;
    // UNIS / 669 Effects ($3C..$3F)
    pub const FREQU_ADJUST: u8 = 0x3C;
    pub const FINE_PAN_LEFT: u8 = 0x3D;
    pub const FINE_PAN_RIGHT: u8 = 0x3E;
    pub const SLOT_RETRIGGER: u8 = 0x3F;
    // FT2 / XM Effects ($40..$43)
    pub const GLOB_VOL_SLD: u8 = 0x40;
    pub const SET_ENVELOPE_POS: u8 = 0x41;
    pub const PAN_SLD: u8 = 0x42;
    pub const KEY_OFF: u8 = 0x43;
    // Kompatibilitäts-Konstanten
    pub const S3M_SPEED: u8 = 0x0F;
    pub const MULTI_RETRIG: u8 = 0x37;
    pub const FINE_PORT_UP: u8 = 0x11;
    pub const FINE_PORT_DOWN: u8 = 0x12;
    /// Kein aktiver Effekt (`NoEffekt = $FF`).
    pub const NO_EFFEKT: u8 = 0xFF;
}

// ============================================================================
// 3. Gemeinsame Modul- und Konfigurations-Strukturen (modc.pas, playdefs.pas)
// ============================================================================

/// Unterstützte Tracker-Modulformate (`TModType` in `modc.pas` Zeile 16).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModType {
    #[default]
    NoiseTr = 0,
    StarTr = 1,
    ProTr = 2,
    Ftrk = 3,
    WOW = 4,
    MTM = 5,
    Oktalyz = 6,
    F669 = 7,
    STM = 8,
    FFar = 9,
    ULT = 10,
    S3M = 11,
    UNIS = 12,
    DMF = 13,
    FT2 = 14,
    IT = 15,
}

/// Laufzeit-Instrumenten-Deskriptor (`TInstrument` in `modc.pas` Zeilen 530–540).
///
/// Speichert Metadaten sowie die linearen Speicher-Offsets im Sample-Puffer (`SamplesPtr`).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Instrument {
    /// Länge des Pascal-Strings `Name: STRING[40]`.
    pub name_len: u8,
    /// Zeichen des Instrumentennamens (`STRING[40]`).
    pub name: [u8; 40],
    /// Start-Offset des Samples im Sample-Speicher (`Longint`). `0` = leer.
    pub offset: i32,
    /// End-Offset des Samples im Sample-Speicher (`Longint`, in Samples bzw. Elementen).
    pub iend: i32,
    /// Loop-Länge in Samples (`Longint`; im Loader wird `LoopStart` als Loop-Länge abgelegt).
    pub loop_start: i32,
    /// Loop-Ende im Sample-Speicher (`Longint`; `0` = kein Loop).
    pub loop_end: i32,
    /// Standard-Lautstärke `0..64` (`Byte`).
    pub volume: u8,
    /// Finetune-Wert (`Byte`; `0..15` bei MOD, signed Byte bei FT2).
    pub fine_tune: u8,
    /// Bitbreite und Flags (`Byte`; `s8Bit`, `s16Bit`, `sUnsigned`, `sDelta`, `sBidiLoop`).
    pub bits: u8,
    /// C-2 / C-4 Tuning-Frequenz in Hz (`Word`, z. B. `8363`).
    pub tuning: u16,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name_len: 0,
            name: [0; 40],
            offset: 0,
            iend: 0,
            loop_start: 0,
            loop_end: 0,
            volume: 0,
            fine_tune: 0,
            bits: 0,
            tuning: 0,
        }
    }
}

/// Globale Modul-Informationsstruktur (`TModulInfo` / `MI` in `modc.pas` Zeilen 547–590).
///
/// Wird zwischen `PLAYER32.DLL` und der UI (`MOD4WIN.EXE`) geteilt.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ModulInfo {
    /// Erkanntes Modulformat (`TModType`).
    pub file_type: ModType,
    /// Dateigröße in Kilobytes (`Word`).
    pub file_siz: u16,
    /// Länge des Modulnamens (`STRING[40]`).
    pub mod_name_len: u8,
    /// Modulname (`STRING[40]`).
    pub mod_name: [u8; 40],
    /// Anzahl der genutzten Spuren/Kanäle (`1..32`, `Word`).
    pub num_tracks: u16,
    /// Anzahl unterschiedlicher Spuren (`Word`).
    pub diff_tracks: u16,
    /// Anzahl der Instrumente (`Word`).
    pub num_instruments: u16,
    /// Array aller 256 Instrumenten-Slots (`ARRAY[1..256] OF TInstrument`).
    pub instruments: [Instrument; 256],
    /// Anzahl leerer/defekter Patterns (`Word`).
    pub dummy_patterns: u16,
    /// Überzählige Bytes am Dateiende (`Longint`).
    pub extra_bytes: i32,
    /// Fehlende Bytes am Dateiende (`Longint`).
    pub missing_bytes: i32,
    /// Bitset der im Song verwendeten Effekte (`EffectSet = SET OF 0..255`, 32 Bytes).
    pub effects_used_set: [u8; 32],
    /// Gibt an, ob das Format kein eigenes Panning nutzt (`Boolean`).
    pub panning_off: bool,
    /// Belegter Sample-Speicher in Bytes (`Longint`).
    pub song_mem: i32,
    /// Pattern-Position, ab der zurückgesprungen wird (`Integer`).
    pub loop_from: i16,
    /// Ziel-Pattern-Position des Song-Loops (`Integer`).
    pub loop_to: i16,
    /// Gibt an, ob die Wiedergabe aktuell läuft (`Boolean`).
    pub playing: bool,
    /// Aktuelle CPU-Auslastung in Promille * 100 (`Longint`).
    pub cpu_usage: i32,
    /// Gesamtlaufzeit des Songs in Millisekunden (`Longint`).
    pub total_time: i32,
    /// Aktuelle Wiedergabezeit in Millisekunden (`Longint`).
    pub curr_time: i32,
    /// Aktuell vorgemischte Pufferlänge in Millisekunden (`Longint`).
    pub buf_time_len: i32,
    /// Aktuelle Position in der Pattern-Order-Tabelle (`Word`, 1-basiert für UI).
    pub curr_pttrn: u16,
    /// Gesamtanzahl der Positionen in der Order-Tabelle (`Word`).
    pub num_patterns: u16,
    /// Anzahl unterschiedlicher physischer Patterns (`Word`).
    pub diff_patterns: u16,
    /// Aktuelle Zeile im Pattern (`Word`, 1-basiert für UI).
    pub curr_line: u16,
    /// Aktuelle Effekte+Operanden je Kanal (`EffectState = ARRAY[1..32] OF Word`).
    pub curr_effects: [u16; MAX_M4W_CHANNELS],
    /// Aktuelle Instrumentennummer je Kanal (`tByteState = ARRAY[1..32] OF Byte`).
    pub curr_instr: [u8; MAX_M4W_CHANNELS],
    /// Aktuelle Lautstärke je Kanal (`tByteState = ARRAY[1..32] OF Byte`).
    pub curr_vols: [u8; MAX_M4W_CHANNELS],
    /// Aktuelle Notennummer je Kanal (`tByteState = ARRAY[1..32] OF Byte`).
    pub curr_notes: [u8; MAX_M4W_CHANNELS],
    /// Aktuelle Tick-Speed (`Byte`).
    pub curr_spd: u8,
    /// Aktuelles Tempo in BPM (`Byte`).
    pub curr_bpm: u8,
    /// Länge des Song-Kommentars (`Word`).
    pub comment_len: u16,
}

impl Default for ModulInfo {
    fn default() -> Self {
        Self {
            file_type: ModType::NoiseTr,
            file_siz: 0,
            mod_name_len: 0,
            mod_name: [0; 40],
            num_tracks: 0,
            diff_tracks: 0,
            num_instruments: 0,
            instruments: [Instrument::default(); 256],
            dummy_patterns: 0,
            extra_bytes: 0,
            missing_bytes: 0,
            effects_used_set: [0; 32],
            panning_off: false,
            song_mem: 0,
            loop_from: 0,
            loop_to: 0,
            playing: false,
            cpu_usage: 0,
            total_time: 0,
            curr_time: 0,
            buf_time_len: 0,
            curr_pttrn: 0,
            num_patterns: 0,
            diff_patterns: 0,
            curr_line: 0,
            curr_effects: [0; MAX_M4W_CHANNELS],
            curr_instr: [0; MAX_M4W_CHANNELS],
            curr_vols: [0; MAX_M4W_CHANNELS],
            curr_notes: [0; MAX_M4W_CHANNELS],
            curr_spd: 6,
            curr_bpm: 125,
            comment_len: 0,
        }
    }
}

impl ModulInfo {
    /// Gibt `true` zurück, wenn das Modul mindestens ein echtes 16-Bit-Sample enthält (`S_16BIT = 0x10`).
    #[inline]
    pub fn has_16bit_samples(&self) -> bool {
        self.instruments
            .iter()
            .any(|inst| inst.iend > inst.offset && (inst.bits & sample_flags::S_16BIT) != 0)
    }

    /// Gibt `true` zurück, wenn es sich um ein `.XM`- (`FT2`) oder `.IT`-Modul mit echten 16-Bit-Samples handelt,
    /// bei dem der `De-Hiss`-Filter standardmäßig automatisch deaktiviert wird.
    #[inline]
    pub fn is_16bit_xm_or_it(&self) -> bool {
        matches!(self.file_type, ModType::FT2 | ModType::IT) && self.has_16bit_samples()
    }
}

/// Audio- und Mixer-Konfiguration (`TSettings` in `modc.pas` Zeilen 599–613).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SoundSettings {
    /// Ausgabegerät (`Integer`: `-1` = Wave Mapper, `0..` = WaveOut, `-2` = HDD).
    pub device_id: i16,
    /// Länge des Gerätenamens (`STRING[40]`).
    pub name_len: u8,
    /// Gerätename (`STRING[40]`).
    pub name: [u8; 40],
    /// Abtastrate in Hz (`Longint`, `11000..48000`, Standard `44100`).
    pub samplerate: i32,
    /// Kanalanzahl (`Byte`: `1` = Mono, `2` = Stereo).
    pub stereo: u8,
    /// Auflösung (`Byte`: `8` oder `16` Bit).
    pub bits: u8,
    /// Anzahl der WaveOut-Puffer (`Byte`: `2..99`).
    pub n_buffers: u8,
    /// ProLogic-Surround-Effekt aktiv (`Boolean`).
    pub surround: bool,
    /// BPM-Temposteuerung ignorieren / nur CIA-Speed nutzen (`Boolean`).
    pub no_bpm: bool,
    /// Timer-Modus (`Byte`: `0` = MMTask, `1` = WM_Timer, `2` = MMTimer).
    pub t_mode: u8,
    /// Master-Vorverstärkung & Clipping-Flag (`Byte`: Bit 7 = Clipping aktiv, Bits 6..0 = `20 + dB`).
    pub pre_amp: u8,
    /// Interpolations-Modus (`Byte`: `0` = aus, `1` = Linear/Oversamp, `2` = CPU Cubic, `3` = FPU Cubic).
    pub oversamp: u8,
    /// Weiches Stereo-Panning aktiv (`Boolean`; wenn `false`, hartes L/M/R-Snapping).
    pub panning: bool,
    /// Rückgabewert für Onboard-RAM bei Hardware-Soundkarten (`Longint`).
    pub mem_size: i32,
}

impl Default for SoundSettings {
    /// Entspricht `defDACsetting` aus `modc.pas` Zeile 616.
    fn default() -> Self {
        let label = b"Wave Mapper";
        let mut name = [0u8; 40];
        name[..label.len()].copy_from_slice(label);
        Self {
            device_id: DEV_MAPPER,
            name_len: label.len() as u8,
            name,
            samplerate: 44100,
            stereo: 2,
            bits: 16,
            n_buffers: 24,
            surround: true,
            no_bpm: false,
            t_mode: TMODE_MM_TASK,
            pre_amp: 148, // 0x80 (Clipping) | 20 (+0 dB)
            oversamp: 1,
            panning: true,
            mem_size: 0,
        }
    }
}

impl SoundSettings {
    /// Gibt zurück, ob der Clean-Mode aktiv ist (`true` = 1995er Stack-/Speicher-Bugs bereinigt,
    /// `false` = 100% bit-identische Full-Emulation).
    #[inline]
    pub fn clean_mode(&self) -> bool {
        (self.t_mode & TMODE_CLEAN_FLAG) != 0
    }

    /// Schaltet zwischen Clean-Mode (`true`) und Full-Emulation (`false`) um,
    /// ohne den visuellen Timer-Modus (`0..=2`) oder das De-Click-Flag zu verändern.
    #[inline]
    pub fn set_clean_mode(&mut self, clean: bool) {
        if clean {
            self.t_mode |= TMODE_CLEAN_FLAG;
        } else {
            self.t_mode &= !TMODE_CLEAN_FLAG;
        }
    }

    /// Gibt zurück, ob der zusätzliche Kanal-De-Click-Filter (`DeClick`) aktiv ist.
    #[inline]
    pub fn declick(&self) -> bool {
        (self.t_mode & TMODE_DECLICK_FLAG) != 0
    }

    /// Schaltet den zusätzlichen Kanal-De-Click-Filter (`DeClick`) ein (`true`) oder aus (`false`),
    /// ohne die übrigen Flags in `t_mode` zu verändern.
    #[inline]
    pub fn set_declick(&mut self, declick: bool) {
        if declick {
            self.t_mode |= TMODE_DECLICK_FLAG;
        } else {
            self.t_mode &= !TMODE_DECLICK_FLAG;
        }
    }

    /// Gibt zurück, ob der authentische Amiga-Paula-Tiefpassfilter (`Amiga`) aktiv ist.
    #[inline]
    pub fn amiga_filter(&self) -> bool {
        (self.t_mode & TMODE_AMIGA_FILTER_FLAG) != 0
    }

    /// Schaltet den authentischen Amiga-Paula-Tiefpassfilter (`Amiga`) ein (`true`) oder aus (`false`).
    #[inline]
    pub fn set_amiga_filter(&mut self, enabled: bool) {
        if enabled {
            self.t_mode |= TMODE_AMIGA_FILTER_FLAG;
        } else {
            self.t_mode &= !TMODE_AMIGA_FILTER_FLAG;
        }
    }

    /// Gibt zurück, ob der moderne adaptive Mehrband-Denoiser (`DeHiss`) aktiv ist.
    #[inline]
    pub fn dehiss(&self) -> bool {
        (self.t_mode & TMODE_DEHISS_FLAG) != 0
    }

    /// Schaltet den modernen adaptiven Mehrband-Denoiser (`DeHiss`) ein (`true`) oder aus (`false`).
    #[inline]
    pub fn set_dehiss(&mut self, enabled: bool) {
        if enabled {
            self.t_mode |= TMODE_DEHISS_FLAG;
        } else {
            self.t_mode &= !TMODE_DEHISS_FLAG;
        }
    }

    /// Liefert den visuellen Hauptfenster-Timer-Modus (`0 = MMTask`, `1 = WM_Timer`, `2 = MMTimer`).
    #[inline]
    pub fn timer_mode(&self) -> u8 {
        (self.t_mode & !TMODE_FLAGS_MASK).min(2)
    }

    /// Setzt den visuellen Hauptfenster-Timer-Modus (`0..=2`), ohne `Clean-Mode` oder `De-Click` zu verändern.
    #[inline]
    pub fn set_timer_mode(&mut self, mode: u8) {
        let flags = self.t_mode & TMODE_FLAGS_MASK;
        self.t_mode = (mode.min(2) & !TMODE_FLAGS_MASK) | flags;
    }
}

// ============================================================================
// 4. Pattern-Note, Event- und Kanal-/Voice-Strukturen (playdefs.pas)
// ============================================================================

/// Entpackte interne Pattern-Zelle (`TNote` in `playdefs.pas` Zeilen 32–38, exakt 6 Bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Instrumentennummer (`Byte`, `0` = kein Instrument).
    pub instrument: u8,
    /// Amiga-Periode (`Ton * 8`) bzw. Notenindex bei FT2 (`Word`, `0` = keine Note, `0xFFFF` = KeyOff).
    pub ton: u16,
    /// Interner Effekt-Befehl (`Byte`, `0xFF` = `NoEffekt`).
    pub effekt: u8,
    /// Effekt-Parameter (`Byte`).
    pub operands: u8,
    /// Lautstärkespalten-Wert (`Byte`, `0..64`, `0xFF` = `DefChVol` / leer).
    pub chn_vol: u8,
}

impl Default for Note {
    fn default() -> Self {
        Self {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::NO_EFFEKT,
            operands: 0,
            chn_vol: DEF_CH_VOL,
        }
    }
}

/// Ein einzelner Eintrag im 128-Elemente-Ringpuffer zur UI-Synchronisation
/// (`tPlayTime` in `playdefs.pas` Zeilen 213–224, exakt 176 Bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PlayTimeEntry {
    pub time: i32,
    pub byte_pos: i32,
    pub pattrn: u16,
    pub effects: [u16; MAX_M4W_CHANNELS],
    pub volumes: [u8; MAX_M4W_CHANNELS],
    pub notes: [u8; MAX_M4W_CHANNELS],
    pub play_instr: [u8; MAX_M4W_CHANNELS],
    pub bpm: u16,
    pub spd: u16,
    pub play_line: u16,
}

/// Hardware-/Mixer-Event-Struktur eines Kanals (`TMODEvent` in `playdefs.pas` Zeilen 88–94, 14 Bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModEvent {
    /// Aktive Sample-/Instrumentennummer (`Word`).
    pub inst: u16,
    /// Sample-Startoffset (`Longint`).
    pub offset: i32,
    /// Berechnete 16.16-Fixed-Point-Schrittweite (`Longint`).
    pub pitch: i32,
    /// Linke Gesamtlautstärke (`Word`, `0..65535`).
    pub l_volume: u16,
    /// Rechte Gesamtlautstärke (`Word`, `0..65535`).
    pub r_volume: u16,
}

/// Kompakte Assembler-Mixer-Kanalstruktur (`tMixChannel` in `playdefs.pas` Zeilen 96–111, 39 Bytes).
///
/// Bildet exakt die ersten 35 Bytes von [`Channel`] (`tChannel`) plus 4 Bytes `dummy` ab.
/// Beim Seeking/Scanning werden diese ersten 35 Bytes über `SaveChannels` / `RestoreChannels`
/// gesichert, und während `ProcessSampleBlock` in `dacmix.pas` wird `tMixChannel` nach `oAsmChn`
/// kopiert und anschließend (`note..rest`, 5 `i32`-Werte = 20 Bytes) zurückgeschrieben.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MixChannel {
    /// Aktueller Sample-Offset im Sample-Speicher (`Longint`; `0` = Kanal stumm).
    pub note: i32,
    /// Effektives Sample-/Loop-Ende (`Longint`).
    pub iend: i32,
    /// Loop-Länge in Samples (`Longint`; bei One-Shot-Samples `iend + 65536`).
    pub lln: i32,
    /// Loop-Ende (`Longint`; `0` bei One-Shot-Samples).
    pub lend: i32,
    /// 16-Bit gebrochener Phasenanteil (Sub-Sample-Position im unteren `u16`-Wort, `Longint`).
    pub rest: i32,
    /// Unteres 16-Bit-Wort der Phasen-Schrittweite (`Word`, Nachkommaanteil von `Pitch`).
    pub plo: u16,
    /// Oberes 32-Bit-Wort der Phasen-Schrittweite (`Longint`, Ganzzahlanteil von `Pitch`).
    pub phi: i32,
    /// Ziel-Mischpuffer-Offset (`Longint`: `MixBufferOfs` oder `SurBufferOfs`).
    pub mix_buf: i32,
    /// Linke Kanallautstärke `0..64` (`Byte`).
    pub l_vol: u8,
    /// Rechte Kanallautstärke `0..64` (`Byte`).
    pub r_vol: u8,
    /// Mittlere Kanallautstärke `0..64` (`Byte`).
    pub avg_vol: u8,
    /// Bitflags für den Assembler-Mixer (`Word`, siehe [`mixer_flags`]).
    pub jmp_flags: u16,
    /// Padding-Feld aus `tMixChannel` (`Longint`).
    pub dummy: i32,
}

/// Vollständiger Zustand eines Tracker-Kanals / einer Voice (`tChannel` in `playdefs.pas` Zeilen 114–201).
///
/// Die ersten 12 Felder (`note` bis `jmp_flags`, 35 Bytes) sind bit-identisch mit [`MixChannel`],
/// sodass der 32-Bit-Assembler-Mixer (`dacmix.pas`) direkt über Kanal-Offsets darauf zugreifen kann.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Channel {
    // --- Mixer-Header (identisch mit tMixChannel ohne Dummy, 35 Bytes) ---
    /// Aktuelle Sample-Position (`Longint`; `0` = Kanal inaktiv).
    pub note: i32,
    /// Sample- bzw. Loop-Endposition (`Longint`).
    pub iend: i32,
    /// Loop-Zurücksprungdistanz (`Longint`).
    pub lln: i32,
    /// Loop-Endposition (`Longint`, `0` = kein Loop).
    pub lend: i32,
    /// Gebrochener 16-Bit-Phasenakkumulator im Low-Word (`Longint`).
    pub rest: i32,
    /// Gebrochener Anteil (16 Bit) des Pitch-Inkrements (`Word`).
    pub plo: u16,
    /// Ganzzahliger Anteil des Pitch-Inkrements (`Longint`; direkt hinter `plo`, sodass `[di]` als 32-Bit `Plo:Phi` beschrieben werden kann!).
    pub phi: i32,
    /// Offset des Ziel-Mischpuffers (`Longint`: `MixBufferOfs` oder `SurBufferOfs`).
    pub mix_buf: i32,
    /// Linke Lautstärke `0..64` (`Byte`).
    pub l_vol: u8,
    /// Rechte Lautstärke `0..64` (`Byte`).
    pub r_vol: u8,
    /// Durchschnittliche Lautstärke `0..64` (`Byte`).
    pub avg_vol: u8,
    /// Mixer-Routing- und Interpolations-Flags (`Word`, siehe [`mixer_flags`]).
    pub jmp_flags: u16,

    // --- Tracker-Voice-State (playdefs.pas ab Zeile 132) ---
    /// Aktuelle Kanal-Lautstärke `0..64` (`Byte`).
    pub vol: u8,
    /// Aktuelle Panning-Position `0..255` (`Byte`: `0` = links, `128` = Mitte, `255` = rechts).
    pub pan_val: u8,
    /// Aktuelle Basis-Periode des Kanals (`Word`).
    pub tonh: u16,
    /// Aktuelle effekt-modulierte Periode für den Mixer (`Word`, z. B. nach Vibrato/Arpeggio).
    pub new_tonh: u16,
    /// Überschriebener Finetune-Wert durch Effekt `E5x` (`Byte`).
    pub new_fine_tune: u8,
    /// Berechneter Notenindex `1..145` für die UI-Anzeige (`Byte`).
    pub note_val: u8,
    /// Aktiver Effekt-Befehl (`Byte`).
    pub effkt: u8,
    /// Aktiver Effekt-Operand (`Byte`).
    pub opnd: u8,
    /// Aktuell geladenes Instrument (`Byte`).
    pub inst: u8,
    /// Als nächstes vorgewähltes Instrument (`Byte`).
    pub nxt_inst: u8,
    /// Ziel-Periode für Tone-Portamento (`Word`).
    pub tpton: u16,
    /// Gerasterte Periode bei aktivem Glissando (`Word`).
    pub tp_gl_ton: u16,
    /// Basis-Lautstärke vor Tremolo / Volume-Portamento (`Byte`).
    pub volsld: u8,
    /// Wellenform-Tabellenindex für Vibrato / Tremolo (`Byte`).
    pub vtini: u8,
    /// Tick-Zähler für Arpeggio (`0..2`) und Tremor (`Byte`).
    pub acnt: u8,
    /// Tatsächlicher Sample-Slot (bei FT2 `SampleSt + SampleNos[Note]`, sonst `NxtInst`, `Word`).
    pub smp_num: u16,
    /// C-2/C-4-Tuning des aktuellen Samples in Hz (`Longint`).
    pub i_tune: i32,
    /// Finetune des aktuellen Samples (`Byte`).
    pub i_fin_tun: u8,
    /// Start-Offset des aktuellen Samples im Sample-Speicher (`Longint`).
    pub i_offset: i32,

    // --- Effekt-Gedächtnis (Effect Memory Bytes, playdefs.pas Zeilen 153–181) ---
    /// Letzter Operand für Vibrato (`Byte`).
    pub v_ops: u8,
    /// Letzter Operand für Tremolo (`Byte`).
    pub t_ops: u8,
    /// Gespeicherte Lautstärke für Tremor (`Byte`).
    pub t_vol: u8,
    /// Ziel-Lautstärke für Volume-Portamento (`Byte`).
    pub vol_prt: u8,
    /// Tick-Zähler für Volume-Portamento (`Byte`).
    pub vp_vblanc: u8,
    /// Tick-Zähler für FAR-Tone-Portamento (`Byte`).
    pub fp_vblanc: u8,
    /// Start-Periode für FAR-Tone-Portamento (`Word`).
    pub fp_tonh: u16,
    /// Glissando-Steuerung (`E3x`) aktiv (`Boolean`).
    pub glissando_on: bool,
    /// Tabellen-Kennung / Offset für Vibrato-Wellenform (`Word`: `0`=Sin, `1`=RampDn, `2`=Sqr).
    pub v_table: u16,
    /// Tabellen-Kennung / Offset für Tremolo-Wellenform (`Word`: `0`=Sin, `1`=RampDn, `2`=Sqr).
    pub t_table: u16,
    /// Verbleibende Durchläufe bei Pattern-Loop `E6x` (`Byte`).
    pub pattern_loops: u8,
    /// Startzeile des Pattern-Loops `E60` (`Byte`).
    pub pattern_loopst: u8,
    /// S3M Stereo-Control-Status (`Byte`, `0` = inaktiv, `1..8` = Modus + 1).
    pub s_cont: u8,
    /// Gespeicherte Lautstärke für Oktalyzer-Effekt (`Byte`).
    pub old_volume: u8,
    /// Letzter Operand für Sample-Offset `9xx` (`Byte`).
    pub ofs_ops: u8,
    /// Letzter Operand für Arpeggio `0xy` (`Byte`).
    pub arp_ops: u8,
    /// Letzter Operand für Volume-Slide `Axy` / `Dxy` (`Byte`).
    pub vsld_ops: u8,
    /// Letzter Operand für Fine-Volume-Slide Up `EAx` (`Byte`).
    pub f_vu_ops: u8,
    /// Letzter Operand für Fine-Volume-Slide Down `EBx` (`Byte`).
    pub f_vd_ops: u8,
    /// Letzter Operand für Pattern-Delay `EEx` (`Byte`).
    pub p_del_ops: u8,
    /// Letzter Operand für Retrigger `E9x` / `Qxy` (`Byte`).
    pub retr_ops: u8,
    /// Letzter Operand für Oktalyzer Arpeggio 1 (`Byte`).
    pub arp1_ops: u8,
    /// Letzter Operand für Oktalyzer Arpeggio 2 (`Byte`).
    pub arp2_ops: u8,
    /// Letzter Operand für Oktalyzer Slide Up (`Byte`).
    pub okt_su_ops: u8,
    /// Letzter Operand für Oktalyzer Slide Up Once (`Byte`).
    pub okt_su1_ops: u8,
    /// Letzter Operand für Oktalyzer Slide Down (`Byte`).
    pub okt_sd_ops: u8,
    /// Letzter Operand für Oktalyzer Slide Down Once (`Byte`).
    pub okt_sd1_ops: u8,
    /// Letzter Operand für Tremor `Ixy` (`Byte`).
    pub tremor_ops: u8,
    /// Letzter Operand für Portamento Up/Down (`Byte`).
    pub prt_ops: u8,
    /// Letzter Operand für Tone-Portamento `3xx` (`Byte`).
    pub prt_n_ops: u8,
    /// Letzter Operand für Global-Volume-Slide (`Byte`).
    pub gv_sld_ops: u8,
    /// Letzter Operand für Panning-Slide (`Byte`).
    pub p_sld_ops: u8,
    /// Lautstärkespalten-Gedächtnis für FT2 (`Byte`).
    pub old_chn_vol: u8,
    /// FT2 VolColumn Volume-Slide Up Gedächtnis (`Byte`).
    pub v_sld_u_ops: u8,
    /// FT2 VolColumn Volume-Slide Down Gedächtnis (`Byte`).
    pub v_sld_d_ops: u8,
    /// FT2 VolColumn Fine-Volume-Slide Up Gedächtnis (`Byte`).
    pub f_vsld_u_ops: u8,
    /// FT2 VolColumn Fine-Volume-Slide Down Gedächtnis (`Byte`).
    pub f_vsld_d_ops: u8,
    /// FT2 VolColumn Panning-Slide Left Gedächtnis (`Byte`).
    pub p_sld_l_ops: u8,
    /// FT2 VolColumn Panning-Slide Right Gedächtnis (`Byte`).
    pub p_sld_r_ops: u8,

    // --- FastTracker-II (XM) Hüllkurven & Auto-Vibrato (playdefs.pas Zeilen 182–194) ---
    /// Aktueller Stützpunkt-Index der Lautstärke-Hüllkurve (`Byte`, `0` = inaktiv).
    pub env_vol_pos: u8,
    /// Aktueller Tick auf der Lautstärke-Hüllkurve (`Word`).
    pub env_vol_tck: u16,
    /// Aktueller interpolierter Lautstärke-Hüllkurvenwert `0..64` (`Byte`).
    pub env_vol: u8,
    /// Aktueller Stützpunkt-Index der Panning-Hüllkurve (`Byte`, `0` = inaktiv).
    pub env_pan_pos: u8,
    /// Aktueller Tick auf der Panning-Hüllkurve (`Word`).
    pub env_pan_tck: u16,
    /// Aktueller interpolierter Panning-Hüllkurvenwert `0..64` (`Byte`).
    pub env_pan: u8,
    /// Tick-Zähler für Auto-Vibrato-Sweep (`Byte`).
    pub vib_tck: u8,
    /// Wellenform-Auswahl für Instrumenten-Auto-Vibrato (`Word`).
    pub vib_tbl: u16,
    /// Phasen-Index für Instrumenten-Auto-Vibrato (`Byte`).
    pub vib_ptr: u8,
    /// Aktuelle Fadeout-Lautstärke `0..65536` (`Longint`).
    pub fade_vol: i32,
    /// Gibt an, ob KeyOff ausgelöst wurde und Fadeout läuft (`Boolean`).
    pub do_fade_out: bool,

    // --- Event-Change-Sets & Hardware-Mirror (playdefs.pas Zeilen 195–201) ---
    /// Gesicherte Event-Change-Flags (`Byte`, Bitset aus [`event_flags`]).
    pub back_e_chg: u8,
    /// Aktuelle Event-Change-Flags (`Byte`, Bitset aus [`event_flags`]).
    pub event_chg: u8,
    /// Zugeordneter Hardware-Wiedergabekanal (`Shortint`, `-1` bei DAC).
    pub play_channel: i8,
    /// Gesicherter Hardware-Wiedergabekanal (`Shortint`).
    pub back_channel: i8,
    /// Aktuelles Kanal-Event (`TMODEvent`).
    pub mod_event: ModEvent,
    /// Gesichertes Kanal-Event (`TMODEvent`).
    pub back_event: ModEvent,
}

// ============================================================================
// 5. Globale Engine-Zustandsvariablen (playdefs.pas Zeilen 680–885, dacmix.pas)
// ============================================================================

/// Bündelt sämtliche globalen Status- und Mixer-Variablen aus `playdefs.pas`, `play.inc` und `dacmix.pas`.
#[derive(Clone)]
pub struct EngineState {
    pub mi: ModulInfo,
    pub sound_settings: SoundSettings,
    pub channels: [Channel; MAX_M4W_CHANNELS],
    pub back_channels: [MixChannel; MAX_M4W_CHANNELS],

    // --- Globale Tempo-, Lautstärke- und Tuning-Register ---
    pub speed: u8,
    pub init_speed: u8,
    pub bpm_speed: u16,
    pub init_bpm_speed: u16,
    pub glob_vol: u8,
    pub init_glob_vol: u8,
    pub master_vol: u8,
    pub vol_master: u16,
    pub def_mod_tuning: u16,
    pub note_stretch: f64,
    pub use_linear_tune: bool,
    pub scontrol: bool,
    pub use_surround: bool,
    pub force_bpm: bool,
    pub use_patt_brk_ops: bool,
    pub fast_slides: bool,
    pub pm_is_working: bool,
    pub player_repeats: bool,
    pub repeat_song: bool,
    pub global_fade_out: bool,
    pub global_fade_out_time: i32,
    pub break_to: u8,
    pub time_step_rest: u16,

    // --- Format-spezifische globale Register (FAR / Pattern-Delay) ---
    pub far_vib_depth: u8,
    pub far_tempo: u8,
    pub fine_far_tempo: i8,
    pub patterndelay: u8,
    pub delay_pattern: bool,

    // --- Sequencer-Position ---
    pub curr_pattern: u16,
    pub curr_note: u16,
    pub curr_vblanc: u16,
    pub play_time: i32,
    pub loop_to_time: i32,

    // --- Mixer- und Surround-Ringpuffer-Zustand (dacmix.pas) ---
    pub samples_remain: u16,
    pub buf_samples: u16,
    pub num_samples: u16,
    pub time_step: i32,
    pub mix_time: u16,
    pub buf_start: u16,
    pub buf_end: u16,
    /// Click-Removal-Fadeout-Differenzen für Hauptpuffer (`dacmix.pas` Zeilen 2464–2491).
    pub l_fade_out: i32,
    pub r_fade_out: i32,
    /// Click-Removal-Fadeout-Differenzen für Surroundpuffer (`dacmix.pas` Zeilen 2503–2530).
    pub l_sur_fade_out: i32,
    pub r_sur_fade_out: i32,
    /// Maximaler Pegel während Peak-Suche (`MaxLevel` in `dacmix.pas`).
    pub max_level: i32,
    /// Zuletzt auf dem Stack bei `[bp-2]` (`SP_loop - 32`) geschriebener Byte-Wert für
    /// uninitialisierte `VAR Value: Byte` in `ProcessEnvelope` (`play.inc` Zeilen 78–107).
    pub env_stack_val: u8,
    /// Akkumulierte Stereo-Frames im aktuellen 8192-Frame `DEV_HDD` `WaveHeader`-Puffer (`BufferSize = 32768`).
    pub hdd_buf_frames: u32,

    // --- Kanal-Konfigurationstabellen ---
    /// Standard-Panning pro Kanal (`DefPanning: ARRAY[1..MaxM4WChannels] OF Byte`).
    pub def_panning: [u8; MAX_M4W_CHANNELS],
    /// Oktalyzer-Kanalpaare (`RelatedChn: ARRAY[1..MaxM4WChannels] OF Byte`).
    pub related_chn: [u8; MAX_M4W_CHANNELS],
    /// Pattern-Order-Tabelle (`PArrangement: ARRAY[0..MaxPatterns-1] OF Byte`).
    pub p_arrangement: [u8; MAX_PATTERNS],
    /// Zeilenende pro Pattern (`PtrnBreaks: ARRAY[0..MaxPatterns-1] OF Byte`).
    pub ptrn_breaks: [u8; MAX_PATTERNS],
    /// Initiales Tempo pro Pattern für 669 (`PtrnTempi: ARRAY[0..MaxPatterns-1] OF Byte`).
    pub ptrn_tempi: [u8; MAX_PATTERNS],
    /// Startzeitpunkt jedes Patterns in ms (`PatternTimes: ARRAY[0..MaxPatterns-1] OF Longint`).
    pub pattern_times: [i32; MAX_PATTERNS],
    /// Gespielte Zeilen je Pattern zur Loop-Erkennung (`RowPlayed: ARRAY[0..255] OF SET OF 0..255`).
    pub row_played: [[u8; 32]; MAX_PATTERNS],
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            mi: ModulInfo::default(),
            sound_settings: SoundSettings::default(),
            channels: [Channel::default(); MAX_M4W_CHANNELS],
            back_channels: [MixChannel::default(); MAX_M4W_CHANNELS],
            speed: 6,
            init_speed: 6,
            bpm_speed: 125,
            init_bpm_speed: 125,
            glob_vol: 64,
            init_glob_vol: 64,
            master_vol: 64,
            vol_master: 0,
            def_mod_tuning: 8363,
            note_stretch: 65536.0 * 8.0 * 428.0 * 8363.0,
            use_linear_tune: false,
            scontrol: false,
            use_surround: true,
            force_bpm: false,
            use_patt_brk_ops: true,
            fast_slides: false,
            pm_is_working: false,
            player_repeats: false,
            repeat_song: true,
            global_fade_out: false,
            global_fade_out_time: 0,
            break_to: 0,
            time_step_rest: 0,
            far_vib_depth: 4,
            far_tempo: 4,
            fine_far_tempo: 0,
            patterndelay: 0,
            delay_pattern: false,
            curr_pattern: 0,
            curr_note: 0,
            curr_vblanc: 0,
            play_time: 0,
            loop_to_time: 0,
            samples_remain: 0,
            buf_samples: 0,
            num_samples: 0,
            time_step: 0,
            mix_time: 0,
            buf_start: 0,
            buf_end: 0,
            l_fade_out: 0,
            r_fade_out: 0,
            l_sur_fade_out: 0,
            r_sur_fade_out: 0,
            max_level: 0,
            env_stack_val: 0x8F,
            hdd_buf_frames: 0,
            def_panning: [128; MAX_M4W_CHANNELS],
            related_chn: [0; MAX_M4W_CHANNELS],
            p_arrangement: [0; MAX_PATTERNS],
            ptrn_breaks: [63; MAX_PATTERNS],
            ptrn_tempi: [0; MAX_PATTERNS],
            pattern_times: [0; MAX_PATTERNS],
            row_played: [[0; 32]; MAX_PATTERNS],
        }
    }
}

// ============================================================================
// 6. Exakte Binär-Header-Strukturen der Tracker-Formate (playdefs.pas 231–707)
// ============================================================================

// --- 6.1 ProTracker / NoiseTracker / StarTrekker / FastTracker / WOW (MOD) ---

/// Gepackte 4-Byte-Notenzelle einer MOD-Datei (`TFileNote` in `playdefs.pas` Zeile 232).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ModFileNote(pub [u8; 4]);

/// 30-Byte Sample-Header einer MOD-Datei (`TFileInstrument` in `playdefs.pas` Zeilen 235–242).
///
/// Hinweis: `slength`, `lstart` und `llength` sind in der Datei Big-Endian-Wörter (in Wort-Einheiten = Byte/2).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ModFileInstrument {
    pub sname: [u8; 22],
    pub slength: u16,
    pub finetune: u8,
    pub volume: u8,
    pub lstart: u16,
    pub llength: u16,
}

/// 1084-Byte MOD-Datei-Header mit 31 Instrumenten (`TMODHeader` in `playdefs.pas` Zeilen 247–254).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ModHeader31 {
    pub mname: [u8; 20],
    pub instruments: [ModFileInstrument; 31],
    pub mlength: u8,
    pub looppos: u8,
    pub patterntable: [u8; 128],
    pub sign: [u8; 4],
}

/// 600-Byte SoundTracker/NoiseTracker MOD-Header mit 15 Instrumenten (`T15MODHeader` in `playdefs.pas` Zeilen 257–263).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ModHeader15 {
    pub mname: [u8; 20],
    pub instruments: [ModFileInstrument; 15],
    pub mlength: u8,
    pub looppos: u8,
    pub patterntable: [u8; 128],
}

// --- 6.2 MultiTracker (MTM) ---

/// 3-Byte-Notenzelle eines MTM-Tracks (`TMTMFileNote` in `playdefs.pas` Zeile 267).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MtmFileNote(pub [u8; 3]);

/// 66-Byte MTM-Datei-Header (`TMTMHeader` in `playdefs.pas` Zeilen 273–287).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MtmHeader {
    pub sign: [u8; 3],
    pub version: u8,
    pub mname: [u8; 20],
    pub numtracks: u16,
    pub lastpattern: u8,
    pub lastorder: u8,
    pub commentlen: u16,
    pub numsamples: u8,
    pub attribute: u8,
    pub beat_sp_track: u8,
    pub numchannels: u8,
    pub panpos: [u8; 32],
}

/// 37-Byte MTM-Sample-Header (`tMTMInstrument` in `playdefs.pas` Zeilen 290–298).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MtmInstrument {
    pub sname: [u8; 22],
    pub slength: i32,
    pub lstart: i32,
    pub lend: i32,
    pub finetune: u8,
    pub volume: u8,
    pub attribute: u8,
}

// --- 6.3 ScreamTracker 2 (STM) ---

/// 32-Byte STM-Sample-Header (`stminstrument` in `playdefs.pas` Zeilen 307–320).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct StmInstrument {
    pub name: [u8; 12],
    pub id: u8,
    pub idisk: u8,
    pub rsvd1: u16,
    pub length: u16,
    pub loopbeg: u16,
    pub loopend: u16,
    pub volume: u8,
    pub rsvd2: u8,
    pub c2spd: u16,
    pub rsvd3: i32,
    pub paralen: u16,
}

/// 1168-Byte STM-Datei-Header (`Tstmheader` in `playdefs.pas` Zeilen 323–337).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct StmHeader {
    pub name: [u8; 20],
    pub trackername: [u8; 8],
    pub id: u8,
    pub ftyp: u8,
    pub ver_major: u8,
    pub ver_minor: u8,
    pub inittempo: u8,
    pub numpatterns: u8,
    pub gvol: u8,
    pub reserved: [u8; 13],
    pub instruments: [StmInstrument; 31],
    pub patorder: [u8; 128],
}

// --- 6.4 Composer 669 / UNIS 669 (669) ---

/// 497-Byte 669-Datei-Header (`T669Header` in `playdefs.pas` Zeilen 343–353).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Mod669Header {
    pub sign: [u8; 2],
    pub message: [u8; 108],
    pub nos: u8,
    pub nop: u8,
    pub looporder: u8,
    pub order: [u8; 0x80],
    pub tempolist: [u8; 0x80],
    pub breaks: [u8; 0x80],
}

/// 25-Byte 669-Sample-Header (`T669Instrument` in `playdefs.pas` Zeilen 356–361).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mod669Instrument {
    pub filename: [u8; 13],
    pub length: i32,
    pub loopbeg: i32,
    pub loopend: i32,
}

/// 3-Byte 669-Notenzelle (`t669Note` in `playdefs.pas` Zeile 364).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mod669Note(pub [u8; 3]);

// --- 6.5 Farandole Composer (FAR) ---

/// 98-Byte primärer FAR-Datei-Header (`TFarHeader` in `playdefs.pas` Zeilen 373–386).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FarHeader {
    pub far_id: u32,
    pub songname: [u8; 40],
    pub eof_marker: [u8; 3],
    pub headerlen: u16,
    pub version: u8,
    pub onoff: [u8; 16],
    pub edit1: [u8; 9],
    pub defspeed: u8,
    pub panning: [u8; 16],
    pub edit2: [u8; 4],
    pub stlen: u16,
}

/// 771-Byte sekundärer FAR-Order-Header (`TFarHeader2` in `playdefs.pas` Zeilen 389–395).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FarHeader2 {
    pub orders: [u8; 256],
    pub numpatts: u8,
    pub songlen: u8,
    pub restartpos: u8,
    pub pattsiz: [u16; 256],
}

/// 4-Byte FAR-Notenzelle (`TFarNote` in `playdefs.pas` Zeilen 398–401).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FarNote {
    pub note: u8,
    pub inst: u8,
    pub vol: u8,
    pub eff: u8,
}

/// 48-Byte FAR-Sample-Header (`TFarInstrument` in `playdefs.pas` Zeilen 411–420).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FarInstrument {
    pub name: [u8; 32],
    pub length: i32,
    pub finetune: u8,
    pub volume: u8,
    pub repstart: i32,
    pub repend: i32,
    pub stype: u8,
    pub loopmode: u8,
}

// --- 6.6 ScreamTracker 3 (S3M) ---

/// 96-Byte S3M-Datei-Header (`TS3MHeader` in `playdefs.pas` Zeilen 426–449).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct S3mHeader {
    pub mname: [u8; 28],
    pub kennung: u8,
    pub typ: u8,
    pub dummy1: u16,
    pub ordnum: u16,
    pub insnum: u16,
    pub patnum: u16,
    pub flags: u16,
    pub cwtv: u16,
    pub ffv: u16,
    pub scrm: [u8; 4],
    pub gvol: u8,
    pub ispd: u8,
    pub itempo: u8,
    pub mvol: u8,
    pub uc: u8,
    pub dp: u8,
    pub dummy2: [u8; 8],
    pub special: u16,
    pub chanset: [u8; 32],
}

/// 80-Byte S3M-Sample-Header (`TS3MSample` in `playdefs.pas` Zeilen 465–485).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct S3mSample {
    pub typ: u8,
    pub dosname: [u8; 12],
    pub memseg_hi: u8,
    pub memseg: u16,
    pub length: i32,
    pub loopbeg: i32,
    pub loopend: i32,
    pub vol: u8,
    pub dsk: u8,
    pub pack: u8,
    pub flags: u8,
    pub c2spd: i32,
    pub dummy: [u8; 4],
    pub guspos: u16,
    pub int512: u16,
    pub lastused: i32,
    pub name: [u8; 28],
    pub scri: [u8; 4],
}

// --- 6.7 UltraTracker (ULT) ---

/// 49-Byte ULT-Datei-Header (`TULTHeader` in `playdefs.pas` Zeilen 539–544).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UltHeader {
    pub id: [u8; 15],
    pub mname: [u8; 32],
    pub reserved: u8,
    pub nos: u8,
}

/// 66-Byte ULT-Sample-Header für Version >= 1.4 (`TSampleStruc` in `playdefs.pas` Zeilen 547–558).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UltSampleStruc {
    pub sname: [u8; 32],
    pub dosname: [u8; 12],
    pub loopstart: i32,
    pub loopend: i32,
    pub sizestart: i32,
    pub sizeend: i32,
    pub volume: u8,
    pub bidiloop: u8,
    pub c2spd: u16,
    pub finetune: i16,
}

/// 64-Byte ULT-Sample-Header für Version < 1.4 (`TSampleStruc2` in `playdefs.pas` Zeilen 561–571).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UltSampleStruc2 {
    pub sname: [u8; 32],
    pub dosname: [u8; 12],
    pub loopstart: i32,
    pub loopend: i32,
    pub sizestart: i32,
    pub sizeend: i32,
    pub volume: u8,
    pub bidiloop: u8,
    pub finetune: i16,
}

/// 258-Byte ULT-Order-Header (`TULTHeader2` in `playdefs.pas` Zeilen 574–578).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UltHeader2 {
    pub ptable: [u8; 256],
    pub noc: u8,
    pub nop: u8,
}

// --- 6.8 X-Tracker (DMF) ---

/// 66-Byte DMF-Datei-Header (`TDMFHeader` in `playdefs.pas` Zeilen 591–601).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DmfHeader {
    pub id: u32,
    pub version: u8,
    pub tracker: [u8; 8],
    pub mname: [u8; 30],
    pub composer: [u8; 20],
    pub date: [u8; 3],
}

/// 13-Byte DMF-CMWT-Chunk-Header (`TDMFCompMessage` in `playdefs.pas` Zeilen 604–609).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DmfCompMessage {
    pub sig: u32,
    pub blocksize: i32,
    pub lines: u8,
    pub width: u8,
    pub dummy: [u8; 3],
}

// --- 6.9 FastTracker II (FT2 / XM) ---

/// 336-Byte FT2/XM-Datei-Header (`TFT2Header` in `playdefs.pas` Zeilen 612–629).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ft2Header {
    pub id: [u8; 17],
    pub mname: [u8; 20],
    pub id_1a: u8,
    pub tname: [u8; 20],
    pub version: u16,
    pub headersize: i32,
    pub songlen: u16,
    pub restartpos: u16,
    pub numchannels: u16,
    pub numpatterns: u16,
    pub numinstr: u16,
    pub flags: u16,
    pub deftempo: u16,
    pub defbpm: u16,
    pub ptable: [u8; 256],
}

/// 9-Byte FT2/XM-Pattern-Header (`TFT2Pattern` in `playdefs.pas` Zeilen 632–637).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft2PatternHeader {
    pub patternheaderlen: i32,
    pub packtype: u8,
    pub numrows: u16,
    pub datasize: u16,
}

/// 29-Byte Basis-Instrumenten-Header in FT2/XM (`TFT2Instrument` in `playdefs.pas` Zeilen 640–645).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft2InstrumentHeader {
    pub instsize: i32,
    pub iname: [u8; 22],
    pub itype: u8,
    pub numsamples: u16,
}

/// Einzelner Stützpunkt einer FT2-Hüllkurve (`RECORD Tick, Value: Word END` in `playdefs.pas` Zeile 647).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnvPoint {
    pub tick: u16,
    pub value: u16,
}

/// 12 Hüllkurven-Stützpunkte (`tEnvPts = ARRAY[1..12] OF RECORD Tick, Value: Word END`).
pub type EnvPoints = [EnvPoint; 12];

/// 214-Byte erweiterter FT2/XM-Instrumenten-Header (`TFT2Instrument2` in `playdefs.pas` Zeilen 649–672).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ft2InstrumentHeader2 {
    pub sampleheadersize: i32,
    pub samplenos: [u8; 96],
    pub volenvpts: EnvPoints,
    pub panenvpts: EnvPoints,
    pub numvolpts: u8,
    pub numpanpts: u8,
    pub volsustain: u8,
    pub volloopst: u8,
    pub volloopend: u8,
    pub pansustain: u8,
    pub panloopst: u8,
    pub panloopend: u8,
    pub voltype: u8,
    pub pantype: u8,
    pub vibrtype: u8,
    pub vibrsweep: u8,
    pub vibrdepth: u8,
    pub vibrrate: u8,
    pub volfadeout: u16,
    pub reserved: u16,
}

impl Default for Ft2InstrumentHeader2 {
    fn default() -> Self {
        Self {
            sampleheadersize: 0,
            samplenos: [0; 96],
            volenvpts: [EnvPoint::default(); 12],
            panenvpts: [EnvPoint::default(); 12],
            numvolpts: 0,
            numpanpts: 0,
            volsustain: 0,
            volloopst: 0,
            volloopend: 0,
            pansustain: 0,
            panloopst: 0,
            panloopend: 0,
            voltype: 0,
            pantype: 0,
            vibrtype: 0,
            vibrsweep: 0,
            vibrdepth: 0,
            vibrrate: 0,
            volfadeout: 0,
            reserved: 0,
        }
    }
}

/// 40-Byte FT2/XM-Sample-Header (`TFT2Sample` in `playdefs.pas` Zeilen 675–686).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft2SampleHeader {
    pub samplelen: i32,
    pub loopstart: i32,
    pub loopend: i32,
    pub volume: u8,
    pub finetune: i8,
    pub sampletype: u8,
    pub panning: u8,
    pub relnote: i8,
    pub reserved: u8,
    pub sname: [u8; 22],
}

/// Laufzeit-Instrumenteneintrag für FT2 (`tFT2Instruments`-Element in `playdefs.pas` Zeilen 691–696).
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Ft2RuntimeInstrument {
    pub sample_st: u16,
    pub num_samples: u8,
    pub ext: Ft2InstrumentHeader2,
}

/// Laufzeit-Sampleeintrag für FT2 (`tFT2Samples`-Element in `playdefs.pas` Zeilen 701–705).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft2RuntimeSample {
    pub samp_offset: i32,
    pub hdr: Ft2SampleHeader,
}

// --- 6.10 Oktalyzer (OKT) ---

/// 32-Byte Oktalyzer-Sample-Header (`TOktInstrument` in `playdefs.pas` Zeilen 790–798).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OktInstrument {
    pub sname: [u8; 20],
    pub slength: i32,
    pub lstart: u16,
    pub llength: u16,
    pub pad1: u8,
    pub volume: u8,
    pub pad2: u16,
}

/// 4-Byte Oktalyzer-Notenzelle (`TOktNote` in `playdefs.pas` Zeilen 801–804).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OktNote {
    pub note: u8,
    pub inst: u8,
    pub effekt: u8,
    pub operands: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn test_binary_struct_sizes() {
        // Zentrale Engine-Typen (exakt wie in Borland Pascal 7.0 16-bit)
        assert_eq!(size_of::<Note>(), 6);
        assert_eq!(size_of::<PlayTimeEntry>(), 176);
        assert_eq!(size_of::<ModEvent>(), 14);
        assert_eq!(size_of::<MixChannel>(), 39);
        assert_eq!(size_of::<Instrument>(), 62);
        assert_eq!(size_of::<SoundSettings>(), 60);

        // Prüfe, dass die ersten 35 Bytes von Channel exakt den Feldern von MixChannel entsprechen
        assert_eq!(offset_of!(Channel, note), 0);
        assert_eq!(offset_of!(Channel, iend), 4);
        assert_eq!(offset_of!(Channel, lln), 8);
        assert_eq!(offset_of!(Channel, lend), 12);
        assert_eq!(offset_of!(Channel, rest), 16);
        assert_eq!(offset_of!(Channel, plo), 20);
        assert_eq!(offset_of!(Channel, phi), 22);
        assert_eq!(offset_of!(Channel, mix_buf), 26);
        assert_eq!(offset_of!(Channel, l_vol), 30);
        assert_eq!(offset_of!(Channel, r_vol), 31);
        assert_eq!(offset_of!(Channel, avg_vol), 32);
        assert_eq!(offset_of!(Channel, jmp_flags), 33);
        assert_eq!(offset_of!(Channel, vol), 35);

        // Datei-Header-Größen
        assert_eq!(size_of::<ModFileNote>(), 4);
        assert_eq!(size_of::<ModFileInstrument>(), 30);
        assert_eq!(size_of::<ModHeader31>(), 1084);
        assert_eq!(size_of::<ModHeader15>(), 600);
        assert_eq!(size_of::<MtmFileNote>(), 3);
        assert_eq!(size_of::<MtmHeader>(), 66);
        assert_eq!(size_of::<MtmInstrument>(), 37);
        assert_eq!(size_of::<StmInstrument>(), 32);
        assert_eq!(size_of::<StmHeader>(), 1168);
        assert_eq!(size_of::<Mod669Header>(), 497);
        assert_eq!(size_of::<Mod669Instrument>(), 25);
        assert_eq!(size_of::<Mod669Note>(), 3);
        assert_eq!(size_of::<FarHeader>(), 98);
        assert_eq!(size_of::<FarHeader2>(), 771);
        assert_eq!(size_of::<FarNote>(), 4);
        assert_eq!(size_of::<FarInstrument>(), 48);
        assert_eq!(size_of::<S3mHeader>(), 96);
        assert_eq!(size_of::<S3mSample>(), 80);
        assert_eq!(size_of::<UltHeader>(), 49);
        assert_eq!(size_of::<UltSampleStruc>(), 66);
        assert_eq!(size_of::<UltSampleStruc2>(), 64);
        assert_eq!(size_of::<UltHeader2>(), 258);
        assert_eq!(size_of::<DmfHeader>(), 66);
        assert_eq!(size_of::<DmfCompMessage>(), 13);
        assert_eq!(size_of::<Ft2Header>(), 336);
        assert_eq!(size_of::<Ft2PatternHeader>(), 9);
        assert_eq!(size_of::<Ft2InstrumentHeader>(), 29);
        assert_eq!(size_of::<EnvPoint>(), 4);
        assert_eq!(size_of::<Ft2InstrumentHeader2>(), 214);
        assert_eq!(size_of::<Ft2SampleHeader>(), 40);
        assert_eq!(size_of::<OktInstrument>(), 32);
        assert_eq!(size_of::<OktNote>(), 4);
    }
}
