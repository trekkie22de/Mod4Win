//! Impulse Tracker (`.IT`, Format-Version 1.00 bis 2.14/2.15+) Loader inkl. IT214/IT215
//! Bitstream-Sample-Dekompression sowie Unreal Music Container (`.UMX`, Unreal Engine 1 Package)
//! Unpacker.
//!
//! Referenzen:
//! - Jeffrey Lim: `ITTECH.TXT` (Impulse Tracker 2.14 Format Specification)
//! - Olav Sørensen (`8bitbubsy`): `it2play` (IT2.14/2.15 Decompressor, Loader & Engine)

use super::{
    bitset_include, read_le_i16, read_le_u16, read_le_u32, sconvert_and_reduce,
    LoadedModule,
};
use crate::engine::types::{
    effect_ids, sample_flags, EngineState, Instrument, ModType, Note,
    ID_FILE_CORRUPTED, ID_NO_MOD_FILE, MAX_M4W_CHANNELS, MAX_PATTERNS,
};

// ============================================================================
// 1. Unreal Engine 1 Music Container (.UMX) Unpacker
// ============================================================================

/// Unreal Engine 1 Package Magic (`0x9E2A83C1` in Little-Endian: `[0xC1, 0x83, 0x2A, 0x9E]`).
pub const UMX_PACKAGE_MAGIC: u32 = 0x9E2A_83C1;

/// Liest einen Unreal Engine 1 `FCompactIndex` (variabel langen, vorzeichenbehafteten 32-Bit-Integer)
/// ab `*pos` und rückt `*pos` entsprechend weiter.
pub fn read_ue1_compact_index(data: &[u8], pos: &mut usize) -> Option<i32> {
    let b0 = *data.get(*pos)?;
    *pos += 1;

    let neg = (b0 & 0x80) != 0;
    let mut more = (b0 & 0x40) != 0;
    let mut val: i32 = (b0 & 0x3F) as i32;

    let shifts = [6u32, 13, 20, 27];
    for (idx, &shift) in shifts.iter().enumerate() {
        if !more {
            break;
        }
        let b = *data.get(*pos)?;
        *pos += 1;
        if idx < 3 {
            val |= ((b & 0x7F) as i32) << shift;
            more = (b & 0x80) != 0;
        } else {
            val |= (b as i32) << shift;
            more = false;
        }
    }

    Some(if neg { -val } else { val })
}

/// Prüft, ob bei `slice` ein bekannter Tracker-Header (`.it`, `.xm`, `.s3m`, `.mod`, `.mtm`, `.far`, `.okt`, `.stm`, `.669`) beginnt.
fn is_known_inner_tracker_header(slice: &[u8]) -> bool {
    if slice.len() >= 4 && &slice[0..4] == b"IMPM" {
        return true;
    }
    if slice.len() >= 38 && &slice[0..17] == b"Extended Module: " && slice[37] == 0x1A {
        return true;
    }
    if slice.len() >= 48 && &slice[44..48] == b"SCRM" {
        return true;
    }
    if slice.len() >= 1084 {
        let sig = &slice[1080..1084];
        if matches!(
            sig,
            b"M.K." | b"M!K!" | b"M&K!" | b"M.K!" | b"N.T." | b"OCTA" | b"CD81" | b"FLT4" | b"FLT8"
        ) || (&sig[1..4] == b"CHN" && sig[0].is_ascii_digit())
            || (&sig[2..4] == b"CH" && sig[0].is_ascii_digit() && sig[1].is_ascii_digit())
        {
            return true;
        }
    }
    if slice.len() >= 66 && &slice[0..3] == b"MTM" {
        return true;
    }
    if slice.len() >= 98 && &slice[0..4] == b"FAR\xFE" {
        return true;
    }
    if slice.len() >= 16 && &slice[0..8] == b"OKTASONG" {
        return true;
    }
    if slice.len() >= 30 && matches!(&slice[20..28], b"!Scream!" | b"BMOD2STM" | b"SWavePro") {
        return true;
    }
    false
}

/// Entpackt einen Unreal Engine 1 `.umx`-Container und gibt den eingebetteten Tracker-Daten-Slice
/// (`.it`, `.s3m`, `.xm`, `.mod` usw.) zurück.
///
/// Falls `data` kein `.umx`-Container ist, wird `None` zurückgegeben.
pub fn unwrap_umx(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 36 || read_le_u32(data, 0)? != UMX_PACKAGE_MAGIC {
        return None;
    }

    let pkg_version = read_le_u16(data, 4)?;
    let name_count = read_le_u32(data, 12)? as usize;
    let name_offset = read_le_u32(data, 16)? as usize;
    let export_count = read_le_u32(data, 20)? as usize;
    let export_offset = read_le_u32(data, 24)? as usize;
    let import_count = read_le_u32(data, 28)? as usize;
    let import_offset = read_le_u32(data, 32)? as usize;

    // 1. Name-Table einlesen
    let mut names: Vec<String> = Vec::with_capacity(name_count.min(4096));
    let mut npos = name_offset;
    for _ in 0..name_count.min(16384) {
        if npos >= data.len() {
            break;
        }
        let name_str = if pkg_version >= 64 {
            let slen = read_ue1_compact_index(data, &mut npos)?.max(0) as usize;
            if npos + slen > data.len() {
                break;
            }
            let raw = &data[npos..npos + slen];
            npos += slen;
            let trimmed = raw.split(|&b| b == 0).next().unwrap_or(raw);
            String::from_utf8_lossy(trimmed).into_owned()
        } else {
            let start = npos;
            while npos < data.len() && data[npos] != 0 {
                npos += 1;
            }
            let s = String::from_utf8_lossy(&data[start..npos]).into_owned();
            if npos < data.len() {
                npos += 1; // Nullbyte überspringen
            }
            s
        };
        if npos + 4 > data.len() {
            break;
        }
        npos += 4; // ObjectFlags (u32)
        names.push(name_str);
    }

    // 2. Import-Table einlesen (um Class-Referenzen < 0 aufzulösen)
    let mut import_obj_names: Vec<usize> = Vec::with_capacity(import_count.min(4096));
    let mut ipos = import_offset;
    for _ in 0..import_count.min(16384) {
        if ipos >= data.len() {
            break;
        }
        let _class_pkg = read_ue1_compact_index(data, &mut ipos)?;
        let _class_name = read_ue1_compact_index(data, &mut ipos)?;
        if pkg_version >= 60 {
            if ipos + 4 > data.len() {
                break;
            }
            ipos += 4; // Package (i32)
        } else {
            let _pkg = read_ue1_compact_index(data, &mut ipos)?;
        }
        let obj_name = read_ue1_compact_index(data, &mut ipos)?.max(0) as usize;
        import_obj_names.push(obj_name);
    }

    // 3. Export-Table durchsuchen nach `Music`-Objekten (bzw. dem größten Export-Chunk)
    let mut epos = export_offset;
    let mut best_serial: Option<(usize, usize, bool)> = None;

    for _ in 0..export_count.min(16384) {
        if epos >= data.len() {
            break;
        }
        let class_index = read_ue1_compact_index(data, &mut epos)?;
        let _super_index = read_ue1_compact_index(data, &mut epos)?;
        if pkg_version >= 60 {
            if epos + 4 > data.len() {
                break;
            }
            epos += 4; // Group (i32)
        }
        let _obj_name = read_ue1_compact_index(data, &mut epos)?;
        if epos + 4 > data.len() {
            break;
        }
        epos += 4; // ObjectFlags (u32)
        let serial_size = read_ue1_compact_index(data, &mut epos)?;
        if serial_size <= 0 {
            continue;
        }
        let serial_offset = read_ue1_compact_index(data, &mut epos)?;
        if serial_offset <= 0 {
            continue;
        }

        let s_ofs = serial_offset as usize;
        let s_len = serial_size as usize;
        if s_ofs >= data.len() || s_len < 32 {
            continue;
        }
        let clamped_len = s_len.min(data.len() - s_ofs);

        let is_music = if class_index < 0 {
            let imp_idx = (-class_index - 1) as usize;
            import_obj_names
                .get(imp_idx)
                .and_then(|&nidx| names.get(nidx))
                .map(|s| s.eq_ignore_ascii_case("music"))
                .unwrap_or(false)
        } else {
            false
        };

        match best_serial {
            None => best_serial = Some((s_ofs, clamped_len, is_music)),
            Some((_, prev_len, prev_music)) => {
                if (is_music && !prev_music) || (is_music == prev_music && clamped_len > prev_len) {
                    best_serial = Some((s_ofs, clamped_len, is_music));
                }
            }
        }
    }

    // 4. Innerhalb des gefundenen Export-Objekts den UMusic-Chunk-Header überspringen
    if let Some((s_ofs, s_len, _)) = best_serial {
        let export_slice = &data[s_ofs..s_ofs + s_len];

        // 4a. Versuche zuerst das exakte UObject-Property + UMusic-Header-Parsing:
        // UObject-Properties enden mit dem Name-Index für "None".
        if let Some(parsed_slice) = try_parse_umusic_payload(export_slice, pkg_version, &names) {
            if is_known_inner_tracker_header(parsed_slice) {
                return Some(parsed_slice);
            }
        }

        // 4b. Scan-Fallback über die ersten 512 Bytes des Export-Objekts nach bekannten Signaturen
        let scan_limit = export_slice.len().min(512);
        for delta in 0..scan_limit {
            let sub = &export_slice[delta..];
            if is_known_inner_tracker_header(sub) {
                return Some(sub);
            }
        }
    }

    // 5. Ultimativer Fallback: Scanne die ersten 4096 Bytes der Datei nach "IMPM" / "Extended Module: " / "SCRM"
    let global_scan = data.len().min(4096);
    for ofs in 36..global_scan {
        let sub = &data[ofs..];
        if is_known_inner_tracker_header(sub) {
            return Some(sub);
        }
    }

    None
}

fn try_parse_umusic_payload<'a>(
    export_slice: &'a [u8],
    pkg_version: u16,
    names: &[String],
) -> Option<&'a [u8]> {
    let mut p = 0usize;
    // Überspringe UObject-Properties bis "None"
    for _ in 0..64 {
        let name_idx = read_ue1_compact_index(export_slice, &mut p)?;
        if name_idx < 0 || (name_idx as usize) >= names.len() {
            return None;
        }
        if names[name_idx as usize].eq_ignore_ascii_case("none") {
            break;
        }
        // Property-Info-Byte lesen
        let info = *export_slice.get(p)?;
        p += 1;
        let prop_type = info & 0x0F;
        let size_type = (info >> 4) & 0x07;
        let is_array = (info & 0x80) != 0;

        if prop_type == 10 {
            // StructProperty -> StructName (CompactIndex)
            let _struct_name = read_ue1_compact_index(export_slice, &mut p)?;
        }
        let prop_size: usize = match size_type {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 12,
            4 => 16,
            5 => {
                let b = *export_slice.get(p)? as usize;
                p += 1;
                b
            }
            6 => {
                let w = read_le_u16(export_slice, p)? as usize;
                p += 2;
                w
            }
            7 => {
                let d = read_le_u32(export_slice, p)? as usize;
                p += 4;
                d
            }
            _ => return None,
        };
        if prop_type == 3 {
            // BoolProperty hat keine Extra-Datenbytes (Wert steckt in is_array)
            continue;
        }
        if is_array {
            let b = *export_slice.get(p)?;
            p += if (b & 0x80) == 0 {
                1
            } else if (b & 0xC0) == 0x80 {
                2
            } else {
                4
            };
        }
        p = p.checked_add(prop_size)?;
        if p > export_slice.len() {
            return None;
        }
    }

    // UMusic::Serialize Header:
    // 1. Format-Name (FCompactIndex)
    let _fmt_name = read_ue1_compact_index(export_slice, &mut p)?;
    // 2. Bei pkg_version > 61: LazyLoader End-Skip-Offset (u32)
    if pkg_version > 61 {
        if p + 4 > export_slice.len() {
            return None;
        }
        p += 4;
    }
    // 3. ChunkSize (FCompactIndex)
    let chunk_size = read_ue1_compact_index(export_slice, &mut p)?;
    if chunk_size <= 0 {
        return None;
    }
    let c_len = (chunk_size as usize).min(export_slice.len().saturating_sub(p));
    if c_len < 16 {
        return None;
    }
    Some(&export_slice[p..p + c_len])
}

// ============================================================================
// 2. Impulse Tracker (.IT) Datenstrukturen
// ============================================================================

/// Impulse Tracker Header-Flags (`IMPM` Offset `0x2C`).
pub mod it_flags {
    pub const STEREO: u16 = 0x0001;
    pub const VOL0_OPTIMIZATIONS: u16 = 0x0002;
    pub const USE_INSTRUMENTS: u16 = 0x0004;
    pub const LINEAR_SLIDES: u16 = 0x0008;
    pub const OLD_EFFECTS: u16 = 0x0010;
    pub const LINK_EFFECT_G: u16 = 0x0020;
    pub const USE_MIDI_PITCH: u16 = 0x0040;
    pub const EMBEDDED_MIDI_CONFIG: u16 = 0x0080;
}

/// Impulse Tracker Sample-Flags (`IMPS` Offset `0x12`).
pub mod it_smp_flags {
    pub const HAS_DATA: u8 = 0x01;
    pub const IS_16BIT: u8 = 0x02;
    pub const STEREO: u8 = 0x04;
    pub const COMPRESSED: u8 = 0x08;
    pub const LOOP: u8 = 0x10;
    pub const SUS_LOOP: u8 = 0x20;
    pub const PINGPONG_LOOP: u8 = 0x40;
    pub const PINGPONG_SUS_LOOP: u8 = 0x80;
}

/// Impulse Tracker Hüllkurven-Flags (`IMPI` Envelope Offset `+0`).
pub mod it_env_flags {
    pub const ON: u8 = 0x01;
    pub const LOOP: u8 = 0x02;
    pub const SUS_LOOP: u8 = 0x04;
    pub const CARRY: u8 = 0x08;
    pub const FILTER: u8 = 0x80;
}

/// Einzelner Knotenpunkt einer IT-Hüllkurve (`25` Punkte max).
#[derive(Clone, Copy, Debug, Default)]
pub struct ItEnvNode {
    pub tick: i16,
    pub val: i8,
}

/// Hüllkurve eines IT-Instruments (Volume, Panning oder Pitch/Filter).
#[derive(Clone, Copy, Debug)]
pub struct ItEnvelope {
    pub flags: u8,
    pub num_nodes: u8,
    pub loop_begin: u8,
    pub loop_end: u8,
    pub sus_loop_begin: u8,
    pub sus_loop_end: u8,
    pub nodes: [ItEnvNode; 25],
}

impl Default for ItEnvelope {
    fn default() -> Self {
        Self {
            flags: 0,
            num_nodes: 0,
            loop_begin: 0,
            loop_end: 0,
            sus_loop_begin: 0,
            sus_loop_end: 0,
            nodes: [ItEnvNode::default(); 25],
        }
    }
}

/// Impulse Tracker Instrument (`IMPI`).
#[derive(Clone, Debug)]
pub struct ItInstrument {
    pub name: String,
    pub dos_filename: [u8; 12],
    /// New Note Action: `0` = Cut, `1` = Continue, `2` = Note Off, `3` = Note Fade.
    pub nna: u8,
    /// Duplicate Check Type: `0` = Off, `1` = Note, `2` = Sample, `3` = Instrument.
    pub dct: u8,
    /// Duplicate Check Action: `0` = Cut, `1` = Note Off, `2` = Note Fade.
    pub dca: u8,
    /// Fadeout-Dekrement pro Tick (`0..=128`, wird von `1024` abgezogen).
    pub fadeout: u16,
    /// Pitch-Pan Separation (`-32..=32`).
    pub pps: i8,
    /// Pitch-Pan Center (`0..=119`, `60` = C-5).
    pub ppc: u8,
    /// Global Volume (`0..=128`).
    pub gbv: u8,
    /// Default Pan (`0..=64`, Bit 7 `0x80` = deaktiviert!).
    pub dfp: u8,
    /// Random Volume Variation (`0..=100`).
    pub rv: u8,
    /// Random Panning Variation (`0..=64`).
    pub rp: u8,
    /// Initial Filter Cutoff (`Bit 7` = aktiv, `0..=127`).
    pub ifc: u8,
    /// Initial Filter Resonance (`Bit 7` = aktiv, `0..=127`).
    pub ifr: u8,
    pub midi_chn: u8,
    pub midi_prog: u8,
    pub midi_bank: u16,
    /// Note-Sample-Keyboard-Mapping für Noten `0..120`: `(target_note_0_119, sample_1based)`.
    pub note_sample_table: [(u8, u8); 120],
    pub vol_env: ItEnvelope,
    pub pan_env: ItEnvelope,
    pub pitch_env: ItEnvelope,
}

impl Default for ItInstrument {
    fn default() -> Self {
        let mut nst = [(0u8, 0u8); 120];
        for (i, entry) in nst.iter_mut().enumerate() {
            *entry = (i as u8, 0);
        }
        Self {
            name: String::new(),
            dos_filename: [0; 12],
            nna: 0,
            dct: 0,
            dca: 0,
            fadeout: 0,
            pps: 0,
            ppc: 60,
            gbv: 128,
            dfp: 0x80 | 32,
            rv: 0,
            rp: 0,
            ifc: 0,
            ifr: 0,
            midi_chn: 0,
            midi_prog: 0,
            midi_bank: 0,
            note_sample_table: nst,
            vol_env: ItEnvelope::default(),
            pan_env: ItEnvelope::default(),
            pitch_env: ItEnvelope::default(),
        }
    }
}

/// Impulse Tracker Sample (`IMPS`).
#[derive(Clone, Debug)]
pub struct ItSample {
    pub name: String,
    pub dos_filename: [u8; 12],
    /// Global Volume (`0..=64`).
    pub gvl: u8,
    /// Sample Flags (`it_smp_flags::*`).
    pub flags: u8,
    /// Default Volume (`0..=64`).
    pub vol: u8,
    /// Convert Flags (`Bit 0` = Signed, `Bit 2` = IT215 Delta).
    pub cvt: u8,
    /// Default Panning (`0..=64`, Bit 7 `0x80` = aktiv!).
    pub dfp: u8,
    /// Länge in Samples/Frames.
    pub length: u32,
    pub loop_begin: u32,
    pub loop_end: u32,
    pub c5_speed: u32,
    pub sus_loop_begin: u32,
    pub sus_loop_end: u32,
    /// Auto-Vibrato Speed / Rate (`0..=64`).
    pub vis: u8,
    /// Auto-Vibrato Depth (`0..=32`).
    pub vid: u8,
    /// Auto-Vibrato Sweep / Attack Rate (`0..=255`).
    pub vir: u8,
    /// Auto-Vibrato Waveform (`0`=Sine, `1`=RampDown, `2`=Square, `3`=Random).
    pub vit: u8,
    /// Normalisierte 16-Bit Signed PCM-Daten (Mono / linker Kanal).
    pub data_16: Vec<i16>,
    /// Optionaler rechter Stereo-Kanal für Stereo-Samples (`flags & STEREO`).
    pub data_16_right: Vec<i16>,
}

impl Default for ItSample {
    fn default() -> Self {
        Self {
            name: String::new(),
            dos_filename: [0; 12],
            gvl: 64,
            flags: 0,
            vol: 64,
            cvt: 1,
            dfp: 32,
            length: 0,
            loop_begin: 0,
            loop_end: 0,
            c5_speed: 8363,
            sus_loop_begin: 0,
            sus_loop_end: 0,
            vis: 0,
            vid: 0,
            vir: 0,
            vit: 0,
            data_16: Vec::new(),
            data_16_right: Vec::new(),
        }
    }
}

/// Einzelne Notenzelle in einem entpackten 64-Kanal-IT-Pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItNoteCell {
    /// `0` = leer, `1..=120` = Note `C-0..B-9` (`raw_note + 1`),
    /// `253` = Note Fade (`~~~`), `254` = Note Cut (`^^^`), `255` = Note Off (`===`).
    pub note: u8,
    /// `0` = leer, `1..=255` = Instrument/Sample.
    pub instrument: u8,
    /// `255` = leer, `0..=212` = IT Volume/Panning-Spalte.
    pub vol_cmd: u8,
    /// `0` = leer, `1..=26` = Effekt `'A'..='Z'`.
    pub command: u8,
    /// Effekt-Parameter `0x00..=0xFF`.
    pub param: u8,
}

impl Default for ItNoteCell {
    fn default() -> Self {
        Self {
            note: 0,
            instrument: 0,
            vol_cmd: 0xFF,
            command: 0,
            param: 0,
        }
    }
}

/// Entpacktes Impulse-Tracker-Pattern (`1..=200` Zeilen × 64 Kanäle).
#[derive(Clone, Debug)]
pub struct ItPattern {
    pub num_rows: u16,
    pub rows: Vec<[ItNoteCell; 64]>,
}

impl Default for ItPattern {
    fn default() -> Self {
        Self {
            num_rows: 64,
            rows: vec![[ItNoteCell::default(); 64]; 64],
        }
    }
}

/// MIDI-Macro-Konfiguration (`SF0..SFF` parametrisierte Macros für `Z00..Z7F` und
/// `Z80..ZFF` feste Macros, standardmäßig auf IT2.14 Filter Cutoff & Resonance initialisiert).
#[derive(Clone, Debug)]
pub struct ItMidiConfig {
    /// 16 parametrisierte Macros `SF0..SFF` (je 32 Bytes ASCII).
    pub sfx: [[u8; 32]; 16],
    /// 128 feste Macros `Z80..ZFF` (je 32 Bytes ASCII).
    pub zxx: [[u8; 32]; 128],
}

impl Default for ItMidiConfig {
    fn default() -> Self {
        let mut cfg = Self {
            sfx: [[0u8; 32]; 16],
            zxx: [[0u8; 32]; 128],
        };
        // Standard-IT2.14-Belegung:
        // SF0 = "F0F000z" (Set Filter Cutoff = z)
        cfg.sfx[0][..7].copy_from_slice(b"F0F000z");
        // Z80..Z8F = "F0F001xx" (Set Filter Resonance = (i * 8) in 0..=120)
        for i in 0..16usize {
            let s = format!("F0F001{:02X}", i * 8);
            cfg.zxx[i][..8].copy_from_slice(s.as_bytes());
        }
        cfg
    }
}

/// Vollständige Impulse-Tracker-Moduldaten (`IMPM`), gekapselt auf [`LoadedModule`].
#[derive(Clone, Debug)]
pub struct ItModuleData {
    pub song_name: String,
    pub philigt: u16,
    pub cwtv: u16,
    pub cmwt: u16,
    pub flags: u16,
    pub special: u16,
    pub global_vol: u8,
    pub mix_vol: u8,
    pub initial_speed: u8,
    pub initial_tempo: u8,
    pub pan_sep: u8,
    pub pwd: u8,
    pub chn_pan: [u8; 64],
    pub chn_vol: [u8; 64],
    pub orders: Vec<u8>,
    /// 1-basiert indizierte Instrumente (`instruments[0]` ist Dummy).
    pub instruments: Vec<ItInstrument>,
    /// 1-basiert indizierte Samples (`samples[0]` ist Dummy).
    pub samples: Vec<ItSample>,
    pub patterns: Vec<ItPattern>,
    pub midi_config: ItMidiConfig,
    pub song_message: String,
    pub num_active_host_channels: u16,
}

// ============================================================================
// 3. IT214 / IT215 Bitstream-Sample-Dekompression
// ============================================================================

struct ItBitReader<'a> {
    block: &'a [u8],
    byte_pos: usize,
    bit_buf: u32,
    bit_num: u8,
}

impl<'a> ItBitReader<'a> {
    fn new(block: &'a [u8]) -> Self {
        Self {
            block,
            byte_pos: 0,
            bit_buf: 0,
            bit_num: 0,
        }
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> u32 {
        if n == 0 || n > 24 {
            return 0;
        }
        while self.bit_num < n {
            let b = if self.byte_pos < self.block.len() {
                let v = self.block[self.byte_pos];
                self.byte_pos += 1;
                v as u32
            } else {
                0
            };
            self.bit_buf |= b << self.bit_num;
            self.bit_num += 8;
        }
        let mask = (1u32 << n) - 1;
        let val = self.bit_buf & mask;
        self.bit_buf >>= n;
        self.bit_num -= n;
        val
    }
}

/// Dekomprimiert einen Kanal eines IT214- oder IT215-komprimierten 8-Bit-Samples (`num_samples` Samples)
/// ab `*file_pos` in `data` und gibt die vorzeichenbehafteten 8-Bit-Samples als `Vec<i8>` zurück.
pub fn decompress_it_sample_8bit(
    data: &[u8],
    file_pos: &mut usize,
    num_samples: usize,
    is_it215: bool,
) -> Vec<i8> {
    let mut out = Vec::with_capacity(num_samples);
    let mut remaining = num_samples;

    while remaining > 0 {
        let chunk_samples = remaining.min(0x8000);
        let Some(block_len) = read_le_u16(data, *file_pos) else {
            out.resize(num_samples, 0);
            break;
        };
        *file_pos += 2;
        let block_end = (*file_pos + block_len as usize).min(data.len());
        let block_slice = &data[*file_pos..block_end];
        *file_pos = block_end;

        let mut br = ItBitReader::new(block_slice);
        let mut width: u8 = 9;
        let mut d1: i8 = 0;
        let mut d2: i8 = 0;

        let mut i = 0usize;
        let mut guard = 0usize;
        let max_guard = chunk_samples * 8 + 64;

        while i < chunk_samples {
            guard += 1;
            if guard > max_guard {
                break;
            }
            if width == 0 || width > 9 {
                width = 9;
            }
            let v = br.read_bits(width);
            let delta: i8;

            if width < 7 {
                if v == (1u32 << (width - 1)) {
                    let mut new_w = (br.read_bits(3) as u8) + 1;
                    if new_w >= width {
                        new_w += 1;
                    }
                    width = new_w;
                    continue;
                }
                let shift = 8 - width;
                delta = ((v as i8) << shift) >> shift;
            } else if width <= 8 {
                let border_lo = (1u32 << (width - 1)) - 4;
                let border_hi = border_lo + 7;
                if v >= border_lo && v <= border_hi {
                    let mut new_w = ((v - border_lo) as u8) + 1;
                    if new_w >= width {
                        new_w += 1;
                    }
                    width = new_w;
                    continue;
                }
                let shift = 8 - width;
                delta = ((v as i8) << shift) >> shift;
            } else {
                // width == 9
                if (v & 0x100) != 0 {
                    width = ((v & 0xFF) as u8) + 1;
                    continue;
                }
                delta = (v & 0xFF) as i8;
            }

            d1 = d1.wrapping_add(delta);
            d2 = d2.wrapping_add(d1);
            out.push(if is_it215 { d2 } else { d1 });
            i += 1;
        }

        while i < chunk_samples {
            out.push(0);
            i += 1;
        }
        remaining -= chunk_samples;
    }

    out
}

/// Dekomprimiert einen Kanal eines IT214- oder IT215-komprimierten 16-Bit-Samples (`num_samples` Samples,
/// bis zu `0x4000` Samples = `0x8000` Bytes pro Block) ab `*file_pos` in `data`.
pub fn decompress_it_sample_16bit(
    data: &[u8],
    file_pos: &mut usize,
    num_samples: usize,
    is_it215: bool,
) -> Vec<i16> {
    let mut out = Vec::with_capacity(num_samples);
    let mut remaining = num_samples;

    while remaining > 0 {
        let chunk_samples = remaining.min(0x4000);
        let Some(block_len) = read_le_u16(data, *file_pos) else {
            out.resize(num_samples, 0);
            break;
        };
        *file_pos += 2;
        let block_end = (*file_pos + block_len as usize).min(data.len());
        let block_slice = &data[*file_pos..block_end];
        *file_pos = block_end;

        let mut br = ItBitReader::new(block_slice);
        let mut width: u8 = 17;
        let mut d1: i16 = 0;
        let mut d2: i16 = 0;

        let mut i = 0usize;
        let mut guard = 0usize;
        let max_guard = chunk_samples * 8 + 64;

        while i < chunk_samples {
            guard += 1;
            if guard > max_guard {
                break;
            }
            if width == 0 || width > 17 {
                width = 17;
            }
            let v = br.read_bits(width);
            let delta: i16;

            if width < 7 {
                if v == (1u32 << (width - 1)) {
                    let mut new_w = (br.read_bits(4) as u8) + 1;
                    if new_w >= width {
                        new_w += 1;
                    }
                    width = new_w;
                    continue;
                }
                let shift = 16 - width;
                delta = ((v as i16) << shift) >> shift;
            } else if width <= 16 {
                let border_lo = (1u32 << (width - 1)) - 8;
                let border_hi = border_lo + 15;
                if v >= border_lo && v <= border_hi {
                    let mut new_w = ((v - border_lo) as u8) + 1;
                    if new_w >= width {
                        new_w += 1;
                    }
                    width = new_w;
                    continue;
                }
                let shift = 16 - width;
                delta = ((v as i16) << shift) >> shift;
            } else {
                // width == 17
                if (v & 0x10000) != 0 {
                    width = ((v & 0xFF) as u8) + 1;
                    continue;
                }
                delta = (v & 0xFFFF) as i16;
            }

            d1 = d1.wrapping_add(delta);
            d2 = d2.wrapping_add(d1);
            out.push(if is_it215 { d2 } else { d1 });
            i += 1;
        }

        while i < chunk_samples {
            out.push(0);
            i += 1;
        }
        remaining -= chunk_samples;
    }

    out
}

/// Entpackt ein ModPlug 4-Bit ADPCM komprimiertes IT-Sample (`cvt == 0xFF`):
/// 16-Byte `i8` Delta-Lookup-Tabelle gefolgt von `(num_samples + 1) / 2` gepackten Nibble-Bytes.
pub fn decompress_it_sample_adpcm4(
    data: &[u8],
    file_pos: &mut usize,
    num_samples: usize,
) -> Vec<i16> {
    let mut out = Vec::with_capacity(num_samples);
    if *file_pos + 16 > data.len() {
        out.resize(num_samples, 0);
        return out;
    }
    let mut table = [0i8; 16];
    for i in 0..16 {
        table[i] = data[*file_pos + i] as i8;
    }
    *file_pos += 16;

    let mut delta: i8 = 0;
    let packed_bytes = (num_samples + 1) / 2;
    let avail_bytes = data.len().saturating_sub(*file_pos).min(packed_bytes);
    for i in 0..avail_bytes {
        let b = data[*file_pos + i];
        let nib0 = (b & 0x0F) as usize;
        delta = delta.wrapping_add(table[nib0]);
        out.push((delta as i16) * 256);
        if out.len() < num_samples {
            let nib1 = ((b >> 4) & 0x0F) as usize;
            delta = delta.wrapping_add(table[nib1]);
            out.push((delta as i16) * 256);
        }
    }
    *file_pos = (*file_pos + packed_bytes).min(data.len());
    out.resize(num_samples, 0);
    out
}

// ============================================================================
// 4. Impulse Tracker (.IT) Haupt-Loader
// ============================================================================

fn parse_ascii_trimmed(bytes: &[u8]) -> String {
    let mut buf = Vec::with_capacity(bytes.len());
    for &b in bytes {
        if b == 0 {
            break;
        }
        buf.push(if b < b' ' { b' ' } else { b });
    }
    String::from_utf8_lossy(&buf).trim().to_string()
}

fn parse_it_envelope(raw: &[u8]) -> ItEnvelope {
    if raw.len() < 81 {
        return ItEnvelope::default();
    }
    let flags = raw[0];
    let num_nodes = raw[1].min(25);
    let loop_begin = raw[2].min(num_nodes.saturating_sub(1));
    let loop_end = raw[3].min(num_nodes.saturating_sub(1));
    let sus_loop_begin = raw[4].min(num_nodes.saturating_sub(1));
    let sus_loop_end = raw[5].min(num_nodes.saturating_sub(1));
    let mut nodes = [ItEnvNode::default(); 25];
    for i in 0..25 {
        let base = 6 + i * 3;
        let val = raw[base] as i8;
        let tick = read_le_i16(raw, base + 1).unwrap_or(0);
        nodes[i] = ItEnvNode { tick, val };
    }
    ItEnvelope {
        flags,
        num_nodes,
        loop_begin,
        loop_end,
        sus_loop_begin,
        sus_loop_end,
        nodes,
    }
}

/// Bildet einen IT-Effekt (`command` in `1..=26` für `'A'..='Z'` + `param`) auf die interne
/// MOD4WIN-Effekt-ID (`effect_ids::*`) ab, damit das Effekte-Fenster (`ana.pas`) alle genutzten
/// Effekt-Spalten anzeigt und während der Wiedergabe animiert.
pub fn map_it_command_to_m4w_effect(command: u8, param: u8) -> Option<u8> {
    match command {
        1 => Some(effect_ids::SET_SPEED),         // Axx: Set Speed
        2 => Some(effect_ids::POSITION_JUMP),     // Bxx: Jump to Order
        3 => Some(effect_ids::PATTERN_BREAK),     // Cxx: Break to Row
        4 => {
            // Dxy: Volume Slide / Fine Volume Slide
            let hi = param >> 4;
            let lo = param & 0x0F;
            if lo == 0x0F && hi != 0 {
                Some(effect_ids::FINE_VOL_UP)
            } else if hi == 0x0F && lo != 0 {
                Some(effect_ids::FINE_VOL_DWN)
            } else {
                Some(effect_ids::VOLUME_SLIDE)
            }
        }
        5 => match param >> 4 {
            // Exx: Portamento Down
            0x0F => Some(effect_ids::F_SLD_DWN),
            0x0E => Some(effect_ids::XTRA_F_SLD_DWN),
            _ => Some(effect_ids::PORTAMENTO_DOWN),
        },
        6 => match param >> 4 {
            // Fxx: Portamento Up
            0x0F => Some(effect_ids::F_SLD_UP),
            0x0E => Some(effect_ids::XTRA_F_SLD_UP),
            _ => Some(effect_ids::PORTAMENTO_UP),
        },
        7 => Some(effect_ids::TONE_PORTAMENTO),   // Gxx: Tone Portamento
        8 => Some(effect_ids::VIBRATO),           // Hxy: Vibrato
        9 => Some(effect_ids::TREMOR),            // Ixy: Tremor
        10 => Some(effect_ids::ARPEGGIO),         // Jxy: Arpeggio
        11 => Some(effect_ids::VIBRA_VOLSLID),    // Kxy: Vibrato + Volume Slide
        12 => Some(effect_ids::PORT_VOLSLIDE),    // Lxy: Tone Porta + Volume Slide
        13 => Some(effect_ids::SET_VOLUME),       // Mxx: Set Channel Volume
        14 => Some(effect_ids::VOLUME_SLIDE),     // Nxy: Channel Volume Slide
        15 => Some(effect_ids::PLAY_OFFSET),      // Oxx: Set Sample Offset
        16 => Some(effect_ids::PAN_SLD),          // Pxy: Panning Slide
        17 => Some(effect_ids::RETRIG_VOL_SLD),   // Qxy: Retrig Note
        18 => Some(effect_ids::TREMOLO),          // Rxy: Tremolo
        19 => match param >> 4 {
            // Sxy: Special Subcommands
            0x01 => Some(effect_ids::GLISSANDO_CTRL),
            0x02 => Some(effect_ids::FINE_TUNE),
            0x03 => Some(effect_ids::VIBRATO_WAVE),
            0x04 => Some(effect_ids::TREMOLO_WAVE),
            0x06 | 0x0E => Some(effect_ids::PATTERN_DELAY),
            0x07 => Some(effect_ids::SET_ENVELOPE_POS),
            0x08 => Some(effect_ids::E_PANNING),
            0x09 => Some(effect_ids::STEREO_CONTROL),
            0x0A => Some(effect_ids::PLAY_OFFSET),
            0x0B => Some(effect_ids::PATTRN_LOOP),
            0x0C => Some(effect_ids::NOTE_CUT),
            0x0D => Some(effect_ids::NOTE_DELAY),
            _ => None,
        },
        20 => Some(effect_ids::S3M_BPM),          // Txx: Set Tempo / Tempo Slide
        21 => Some(effect_ids::FINE_VIBRATO),     // Uxy: Fine Vibrato
        22 => Some(effect_ids::SET_GLOB_VOL),     // Vxx: Set Global Volume
        23 => Some(effect_ids::GLOB_VOL_SLD),     // Wxy: Global Volume Slide
        24 => Some(effect_ids::PANNING),          // Xxx: Set Panning
        25 => Some(effect_ids::PAN_SLD),          // Yxy: Panbrello
        26 => Some(effect_ids::SET_FILTER),       // Zxx: Filter Cutoff / Resonance / MIDI Macro
        _ => None,
    }
}

/// Bildet einen IT-Volume-Column-Befehl (`0..=212`) auf eine `effect_ids::*`-Spalte ab.
pub fn map_it_vol_cmd_to_m4w_effect(vol_cmd: u8) -> Option<u8> {
    match vol_cmd {
        0..=64 => Some(effect_ids::SET_VOLUME),
        65..=74 => Some(effect_ids::FINE_VOL_UP),
        75..=84 => Some(effect_ids::FINE_VOL_DWN),
        85..=94 | 95..=104 => Some(effect_ids::VOLUME_SLIDE),
        105..=114 => Some(effect_ids::PORTAMENTO_DOWN),
        115..=124 => Some(effect_ids::PORTAMENTO_UP),
        128..=192 => Some(effect_ids::PANNING),
        193..=202 => Some(effect_ids::TONE_PORTAMENTO),
        203..=212 => Some(effect_ids::VIBRATO),
        _ => None,
    }
}

/// Lädt ein Impulse Tracker (`.IT`) Modul aus einem `&[u8]`-Slice.
pub fn load_it(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 192 || &data[0..4] != b"IMPM" {
        return Err(ID_NO_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state: &mut EngineState = &mut module.state;

    state.mi.file_type = ModType::IT;
    state.mi.file_siz = (data.len() / 1024).min(u16::MAX as usize) as u16;
    state.mi.mod_name_len =
        sconvert_and_reduce(&data[4..30], 26, true, &mut state.mi.mod_name);
    let song_name = parse_ascii_trimmed(&data[4..30]);

    let philigt = read_le_u16(data, 0x1E).unwrap_or(0x1004);
    let ord_num = read_le_u16(data, 0x20).ok_or(ID_FILE_CORRUPTED)?;
    let ins_num = read_le_u16(data, 0x22).ok_or(ID_FILE_CORRUPTED)?;
    let smp_num = read_le_u16(data, 0x24).ok_or(ID_FILE_CORRUPTED)?;
    let pat_num = read_le_u16(data, 0x26).ok_or(ID_FILE_CORRUPTED)?;
    let cwtv = read_le_u16(data, 0x28).unwrap_or(0x0214);
    let cmwt = read_le_u16(data, 0x2A).unwrap_or(0x0214);
    let flags = read_le_u16(data, 0x2C).unwrap_or(0);
    let special = read_le_u16(data, 0x2E).unwrap_or(0);

    let global_vol = data[0x30].min(128);
    let mix_vol = data[0x31].min(128);
    let initial_speed = data[0x32].max(1);
    let initial_tempo = data[0x33].max(32);
    let pan_sep = data[0x34].min(128);
    let pwd = data[0x35];
    let msg_len = read_le_u16(data, 0x36).unwrap_or(0) as usize;
    let msg_ofs = read_le_u32(data, 0x38).unwrap_or(0) as usize;

    let mut chn_pan = [32u8; 64];
    chn_pan.copy_from_slice(&data[0x40..0x80]);
    let mut chn_vol = [64u8; 64];
    chn_vol.copy_from_slice(&data[0x80..0xC0]);

    let mut pos = 0xC0usize;
    if pos + (ord_num as usize) > data.len() {
        return Err(ID_FILE_CORRUPTED);
    }
    let orders = data[pos..pos + (ord_num as usize)].to_vec();
    pos += ord_num as usize;

    // Para-Pointer-Tabellen für Instrumente, Samples und Patterns
    let mut ins_offsets = Vec::with_capacity(ins_num as usize);
    for _ in 0..ins_num {
        let ofs = read_le_u32(data, pos).ok_or(ID_FILE_CORRUPTED)?;
        ins_offsets.push(ofs as usize);
        pos += 4;
    }
    let mut smp_offsets = Vec::with_capacity(smp_num as usize);
    for _ in 0..smp_num {
        let ofs = read_le_u32(data, pos).ok_or(ID_FILE_CORRUPTED)?;
        smp_offsets.push(ofs as usize);
        pos += 4;
    }
    let mut pat_offsets = Vec::with_capacity(pat_num as usize);
    for _ in 0..pat_num {
        let ofs = read_le_u32(data, pos).ok_or(ID_FILE_CORRUPTED)?;
        pat_offsets.push(ofs as usize);
        pos += 4;
    }

    // Eingebettete Edit-History (`special & 0x02`) und MIDI-Konfiguration (`special & 0x08` oder `flags & 0x80`)
    let mut midi_config = ItMidiConfig::default();
    if (special & 0x02) != 0 && pos + 2 <= data.len() {
        let hist_count = read_le_u16(data, pos).unwrap_or(0) as usize;
        pos = (pos + 2 + hist_count * 8).min(data.len());
    }
    if ((special & 0x08) != 0 || (flags & it_flags::EMBEDDED_MIDI_CONFIG) != 0)
        && pos + 4896 <= data.len()
    {
        // +288: 16 * 32 Bytes SF0..SFF
        for i in 0..16 {
            let base = pos + 288 + i * 32;
            midi_config.sfx[i].copy_from_slice(&data[base..base + 32]);
        }
        // +800: 128 * 32 Bytes Z80..ZFF
        for i in 0..128 {
            let base = pos + 800 + i * 32;
            midi_config.zxx[i].copy_from_slice(&data[base..base + 32]);
        }
    }

    let song_message = if (special & 0x01) != 0 && msg_len > 0 && msg_ofs < data.len() {
        let end = (msg_ofs + msg_len).min(data.len());
        String::from_utf8_lossy(&data[msg_ofs..end]).into_owned()
    } else {
        String::new()
    };

    // 1. Instrumente (`IMPI`) laden (1-basiert)
    let mut it_instruments = Vec::with_capacity((ins_num as usize) + 1);
    it_instruments.push(ItInstrument::default()); // Index 0 = Dummy
    for &iofs in &ins_offsets {
        if iofs == 0 || iofs + 0x40 > data.len() || &data[iofs..iofs + 4] != b"IMPI" {
            it_instruments.push(ItInstrument::default());
            continue;
        }
        let mut inst = ItInstrument::default();
        inst.dos_filename.copy_from_slice(&data[iofs + 4..iofs + 16]);
        inst.name = parse_ascii_trimmed(&data[iofs + 0x20..iofs + 0x3A]);

        if cmwt < 0x0200 {
            // Alter Instrument-Header (IT 1.xx, 554 Bytes)
            let flg = data[iofs + 0x11];
            let vls = data[iofs + 0x12];
            let vle = data[iofs + 0x13];
            let sls = data[iofs + 0x14];
            let sle = data[iofs + 0x15];
            let old_fade = read_le_u16(data, iofs + 0x18).unwrap_or(0);
            inst.fadeout = (old_fade.saturating_mul(2)).min(1024);
            inst.nna = data[iofs + 0x1A].min(3);
            let dnc = data[iofs + 0x1B];
            if dnc != 0 {
                inst.dct = 1; // Note
                inst.dca = 0; // Cut
            }
            if iofs + 0x130 <= data.len() {
                for n in 0..120 {
                    let b = iofs + 0x40 + n * 2;
                    inst.note_sample_table[n] = (data[b].min(119), data[b + 1]);
                }
            }
            if iofs + 0x22A <= data.len() {
                inst.vol_env.flags = flg;
                inst.vol_env.loop_begin = vls;
                inst.vol_env.loop_end = vle;
                inst.vol_env.sus_loop_begin = sls;
                inst.vol_env.sus_loop_end = sle;
                let mut count = 0u8;
                for p in 0..25 {
                    let b = iofs + 0x1F8 + p * 2;
                    let tick = data[b];
                    let val = data[b + 1];
                    if tick == 0xFF {
                        break;
                    }
                    inst.vol_env.nodes[p] = ItEnvNode {
                        tick: tick as i16,
                        val: val.min(64) as i8,
                    };
                    count += 1;
                }
                inst.vol_env.num_nodes = count;
            }
        } else {
            // Neuer Instrument-Header (IT 2.00+, 554 Bytes)
            inst.nna = data[iofs + 0x11].min(3);
            inst.dct = data[iofs + 0x12].min(3);
            inst.dca = data[iofs + 0x13].min(2);
            inst.fadeout = read_le_u16(data, iofs + 0x14).unwrap_or(0).min(1024);
            inst.pps = (data[iofs + 0x16] as i8).clamp(-32, 32);
            inst.ppc = data[iofs + 0x17].min(119);
            inst.gbv = data[iofs + 0x18].min(128);
            inst.dfp = data[iofs + 0x19];
            inst.rv = data[iofs + 0x1A].min(100);
            inst.rp = data[iofs + 0x1B].min(64);
            inst.ifc = data[iofs + 0x3A];
            inst.ifr = data[iofs + 0x3B];
            inst.midi_chn = data[iofs + 0x3C];
            inst.midi_prog = data[iofs + 0x3D];
            inst.midi_bank = read_le_u16(data, iofs + 0x3E).unwrap_or(0);

            if iofs + 0x130 <= data.len() {
                for n in 0..120 {
                    let b = iofs + 0x40 + n * 2;
                    inst.note_sample_table[n] = (data[b].min(119), data[b + 1]);
                }
            }
            if iofs + 0x226 <= data.len() {
                inst.vol_env = parse_it_envelope(&data[iofs + 0x130..iofs + 0x182]);
                inst.pan_env = parse_it_envelope(&data[iofs + 0x182..iofs + 0x1D4]);
                inst.pitch_env = parse_it_envelope(&data[iofs + 0x1D4..iofs + 0x226]);
            }
        }
        it_instruments.push(inst);
    }

    // 2. Samples (`IMPS`) laden (1-basiert, inkl. IT214/IT215 Dekompression)
    let mut it_samples = Vec::with_capacity((smp_num as usize) + 1);
    it_samples.push(ItSample::default()); // Index 0 = Dummy
    let mut total_sample_bytes: i32 = 0;

    for &sofs in &smp_offsets {
        if sofs == 0 || sofs + 80 > data.len() || &data[sofs..sofs + 4] != b"IMPS" {
            it_samples.push(ItSample::default());
            continue;
        }
        let mut smp = ItSample::default();
        smp.dos_filename.copy_from_slice(&data[sofs + 4..sofs + 16]);
        smp.gvl = data[sofs + 0x11].min(64);
        smp.flags = data[sofs + 0x12];
        smp.vol = data[sofs + 0x13].min(64);
        smp.name = parse_ascii_trimmed(&data[sofs + 0x14..sofs + 0x2E]);
        smp.cvt = data[sofs + 0x2E];
        smp.dfp = data[sofs + 0x2F];
        smp.length = read_le_u32(data, sofs + 0x30).unwrap_or(0);
        smp.loop_begin = read_le_u32(data, sofs + 0x34).unwrap_or(0);
        smp.loop_end = read_le_u32(data, sofs + 0x38).unwrap_or(0);
        smp.c5_speed = read_le_u32(data, sofs + 0x3C).unwrap_or(8363).max(1);
        smp.sus_loop_begin = read_le_u32(data, sofs + 0x40).unwrap_or(0);
        smp.sus_loop_end = read_le_u32(data, sofs + 0x44).unwrap_or(0);
        let sample_ptr = read_le_u32(data, sofs + 0x48).unwrap_or(0) as usize;
        smp.vis = data[sofs + 0x4C].min(64);
        smp.vid = data[sofs + 0x4D].min(32);
        smp.vir = data[sofs + 0x4E];
        smp.vit = data[sofs + 0x4F].min(3);

        // Loop-Grenzen absichern
        if smp.loop_end > smp.length {
            smp.loop_end = smp.length;
        }
        if smp.loop_begin >= smp.loop_end {
            smp.flags &= !it_smp_flags::LOOP;
        }
        if smp.sus_loop_end > smp.length {
            smp.sus_loop_end = smp.length;
        }
        if smp.sus_loop_begin >= smp.sus_loop_end {
            smp.flags &= !it_smp_flags::SUS_LOOP;
        }

        let has_data = (smp.flags & it_smp_flags::HAS_DATA) != 0
            && smp.length > 0
            && sample_ptr > 0
            && sample_ptr < data.len();

        if has_data {
            let len = (smp.length as usize).min(16 * 1024 * 1024);
            smp.length = len as u32;
            let is_16bit = (smp.flags & it_smp_flags::IS_16BIT) != 0;
            let is_stereo = (smp.flags & it_smp_flags::STEREO) != 0;
            let is_compressed = (smp.flags & it_smp_flags::COMPRESSED) != 0;
            let is_signed = (smp.cvt & 0x01) != 0;
            let is_it215 = (smp.cvt & 0x04) != 0;

            if is_compressed {
                let mut fpos = sample_ptr;
                if is_16bit {
                    let mut left = decompress_it_sample_16bit(data, &mut fpos, len, is_it215);
                    if !is_signed {
                        for v in &mut left {
                            *v = (*v as u16 ^ 0x8000) as i16;
                        }
                    }
                    smp.data_16 = left;
                    if is_stereo && fpos < data.len() {
                        let mut right = decompress_it_sample_16bit(data, &mut fpos, len, is_it215);
                        if !is_signed {
                            for v in &mut right {
                                *v = (*v as u16 ^ 0x8000) as i16;
                            }
                        }
                        smp.data_16_right = right;
                    }
                } else {
                    let left8 = decompress_it_sample_8bit(data, &mut fpos, len, is_it215);
                    smp.data_16 = left8
                        .into_iter()
                        .map(|b| {
                            let s8 = if is_signed {
                                b
                            } else {
                                ((b as u8) ^ 0x80) as i8
                            };
                            (s8 as i16) * 256
                        })
                        .collect();
                    if is_stereo && fpos < data.len() {
                        let right8 = decompress_it_sample_8bit(data, &mut fpos, len, is_it215);
                        smp.data_16_right = right8
                            .into_iter()
                            .map(|b| {
                                let s8 = if is_signed {
                                    b
                                } else {
                                    ((b as u8) ^ 0x80) as i8
                                };
                                (s8 as i16) * 256
                            })
                            .collect();
                    }
                }
            } else if smp.cvt == 0xFF {
                // ModPlug 4-Bit ADPCM Sample-Kompression (16-Byte Delta-Tabelle + 4-Bit Nibbles)
                let mut fpos = sample_ptr;
                smp.data_16 = decompress_it_sample_adpcm4(data, &mut fpos, len);
                if is_stereo && fpos < data.len() {
                    smp.data_16_right = decompress_it_sample_adpcm4(data, &mut fpos, len);
                }
            } else if is_16bit {
                let avail_samples = (data.len() - sample_ptr) / 2;
                let read_len = len.min(avail_samples);
                let mut left = Vec::with_capacity(len);
                for i in 0..read_len {
                    let raw = read_le_u16(data, sample_ptr + i * 2).unwrap_or(0);
                    let s16 = if is_signed {
                        raw as i16
                    } else {
                        (raw ^ 0x8000) as i16
                    };
                    left.push(s16);
                }
                left.resize(len, 0);
                smp.data_16 = left;

                if is_stereo {
                    let right_ptr = sample_ptr + len * 2;
                    if right_ptr < data.len() {
                        let avail_r = (data.len() - right_ptr) / 2;
                        let r_len = len.min(avail_r);
                        let mut right = Vec::with_capacity(len);
                        for i in 0..r_len {
                            let raw = read_le_u16(data, right_ptr + i * 2).unwrap_or(0);
                            let s16 = if is_signed {
                                raw as i16
                            } else {
                                (raw ^ 0x8000) as i16
                            };
                            right.push(s16);
                        }
                        right.resize(len, 0);
                        smp.data_16_right = right;
                    }
                }
            } else {
                let avail_samples = data.len() - sample_ptr;
                let read_len = len.min(avail_samples);
                let mut left = Vec::with_capacity(len);
                for i in 0..read_len {
                    let raw = data[sample_ptr + i];
                    let s8 = if is_signed {
                        raw as i8
                    } else {
                        (raw ^ 0x80) as i8
                    };
                    left.push((s8 as i16) * 256);
                }
                left.resize(len, 0);
                smp.data_16 = left;

                if is_stereo {
                    let right_ptr = sample_ptr + len;
                    if right_ptr < data.len() {
                        let avail_r = data.len() - right_ptr;
                        let r_len = len.min(avail_r);
                        let mut right = Vec::with_capacity(len);
                        for i in 0..r_len {
                            let raw = data[right_ptr + i];
                            let s8 = if is_signed {
                                raw as i8
                            } else {
                                (raw ^ 0x80) as i8
                            };
                            right.push((s8 as i16) * 256);
                        }
                        right.resize(len, 0);
                        smp.data_16_right = right;
                    }
                }
            }

            total_sample_bytes = total_sample_bytes
                .saturating_add((len as i32) * if is_16bit { 2 } else { 1 });
        }

        it_samples.push(smp);
    }

    // 3. Gepackte IT-Patterns (`pat_offsets`) entpacken
    let mut it_patterns = Vec::with_capacity(pat_num.max(1) as usize);
    let mut max_used_chn: usize = 4;

    for &pofs in &pat_offsets {
        if pofs == 0 || pofs + 8 > data.len() {
            it_patterns.push(ItPattern::default());
            continue;
        }
        let packed_len = read_le_u16(data, pofs).unwrap_or(0) as usize;
        let num_rows = read_le_u16(data, pofs + 2).unwrap_or(64).clamp(1, 1024);
        let mut rows = vec![[ItNoteCell::default(); 64]; num_rows as usize];

        let stream_start = pofs + 8;
        let stream_end = (stream_start + packed_len).min(data.len());
        let stream = &data[stream_start..stream_end];
        let mut spos = 0usize;

        let mut last_mask = [0u8; 64];
        let mut last_note = [0u8; 64];
        let mut last_inst = [0u8; 64];
        let mut last_vol = [0xFFu8; 64];
        let mut last_cmd = [0u8; 64];
        let mut last_param = [0u8; 64];

        for row in rows.iter_mut() {
            while spos < stream.len() {
                let chn_var = stream[spos];
                spos += 1;
                if chn_var == 0 {
                    break; // Ende der Zeile
                }
                let chn = ((chn_var - 1) & 63) as usize;
                if chn + 1 > max_used_chn {
                    max_used_chn = chn + 1;
                }
                if (chn_var & 0x80) != 0 {
                    if spos >= stream.len() {
                        break;
                    }
                    last_mask[chn] = stream[spos];
                    spos += 1;
                }
                let mask = last_mask[chn];
                let cell = &mut row[chn];

                if (mask & 0x01) != 0 {
                    if spos >= stream.len() {
                        break;
                    }
                    let raw_n = stream[spos];
                    spos += 1;
                    let n = if raw_n < 120 {
                        raw_n + 1
                    } else if raw_n == 254 {
                        254
                    } else if raw_n == 255 {
                        255
                    } else {
                        253
                    };
                    last_note[chn] = n;
                    cell.note = n;
                }
                if (mask & 0x02) != 0 {
                    if spos >= stream.len() {
                        break;
                    }
                    last_inst[chn] = stream[spos];
                    spos += 1;
                    cell.instrument = last_inst[chn];
                }
                if (mask & 0x04) != 0 {
                    if spos >= stream.len() {
                        break;
                    }
                    last_vol[chn] = stream[spos];
                    spos += 1;
                    cell.vol_cmd = last_vol[chn];
                }
                if (mask & 0x08) != 0 {
                    if spos + 1 >= stream.len() {
                        break;
                    }
                    last_cmd[chn] = stream[spos];
                    last_param[chn] = stream[spos + 1];
                    spos += 2;
                    cell.command = last_cmd[chn];
                    cell.param = last_param[chn];
                }
                if (mask & 0x10) != 0 {
                    cell.note = last_note[chn];
                }
                if (mask & 0x20) != 0 {
                    cell.instrument = last_inst[chn];
                }
                if (mask & 0x40) != 0 {
                    cell.vol_cmd = last_vol[chn];
                }
                if (mask & 0x80) != 0 {
                    cell.command = last_cmd[chn];
                    cell.param = last_param[chn];
                }

                // Registriere genutzte Effekte für das MOD4WIN Effekte-Fenster (`ana.pas`)
                if cell.vol_cmd != 0xFF {
                    if let Some(eff_id) = map_it_vol_cmd_to_m4w_effect(cell.vol_cmd) {
                        bitset_include(&mut state.mi.effects_used_set, eff_id);
                    }
                }
                if cell.command != 0 {
                    if let Some(eff_id) = map_it_command_to_m4w_effect(cell.command, cell.param) {
                        bitset_include(&mut state.mi.effects_used_set, eff_id);
                    }
                }
            }
        }

        it_patterns.push(ItPattern { num_rows, rows });
    }

    if it_patterns.is_empty() {
        it_patterns.push(ItPattern::default());
    }

    // 4. Befülle `state.mi` Metadaten für Module-Info (`bm10`), Instrument-Info (`bm11`) und Effekte (`bm12`)
    let use_instruments = (flags & it_flags::USE_INSTRUMENTS) != 0 && ins_num > 0;
    let display_tracks = max_used_chn.clamp(1, 64) as u16;
    state.mi.num_tracks = display_tracks.min(MAX_M4W_CHANNELS as u16);
    state.mi.diff_tracks = display_tracks;
    state.mi.song_mem = total_sample_bytes;

    // Orders in `state.p_arrangement` übernehmen (für UI Pattern/Orders-Zähler)
    let mut valid_orders = 0usize;
    for &ord in orders.iter().take(MAX_PATTERNS) {
        if ord == 0xFF {
            state.p_arrangement[valid_orders] = 0xFF;
            break;
        }
        state.p_arrangement[valid_orders] = ord;
        valid_orders += 1;
    }
    state.mi.num_patterns = valid_orders.max(1) as u16;
    state.mi.diff_patterns = pat_num.max(1);
    state.init_speed = initial_speed;
    state.init_bpm_speed = initial_tempo as u16;
    state.init_glob_vol = (global_vol / 2).min(64);
    state.glob_vol = state.init_glob_vol;

    for ch in 0..MAX_M4W_CHANNELS {
        let p = chn_pan[ch] & 0x7F;
        state.def_panning[ch] = if p <= 64 { p } else { 32 };
    }

    // Instrumenten-/Sample-Liste für `bm10` (Module Info) und `bm11` (Instrument Info)
    if use_instruments {
        let count = (ins_num as usize).min(255);
        state.mi.num_instruments = count as u16;
        for i in 1..=count {
            let it_ins = &it_instruments[i];
            let mut m4w_ins = Instrument::default();
            m4w_ins.name_len = sconvert_and_reduce(
                it_ins.name.as_bytes(),
                26,
                true,
                &mut m4w_ins.name,
            );
            // Finde repräsentatives Sample (z. B. C-5 = Note 60) für Länge/Loop/Tuning im Info-Dialog
            let rep_smp_idx = it_ins
                .note_sample_table
                .get(60)
                .map(|&(_, s)| s as usize)
                .filter(|&s| s > 0 && s < it_samples.len())
                .or_else(|| {
                    it_ins
                        .note_sample_table
                        .iter()
                        .map(|&(_, s)| s as usize)
                        .find(|&s| s > 0 && s < it_samples.len())
                });
            if let Some(sidx) = rep_smp_idx {
                let smp = &it_samples[sidx];
                m4w_ins.offset = 0;
                m4w_ins.iend = smp.length as i32;
                if (smp.flags & it_smp_flags::LOOP) != 0 && smp.loop_end > smp.loop_begin {
                    m4w_ins.loop_end = smp.loop_end as i32;
                    m4w_ins.loop_start = (smp.loop_end - smp.loop_begin) as i32;
                }
                m4w_ins.volume = smp.vol;
                m4w_ins.bits = if (smp.flags & it_smp_flags::IS_16BIT) != 0 {
                    sample_flags::S_16BIT
                } else {
                    sample_flags::S_8BIT
                };
                m4w_ins.tuning = smp.c5_speed.min(u16::MAX as u32) as u16;
            } else {
                m4w_ins.volume = (it_ins.gbv / 2).min(64);
                m4w_ins.tuning = 8363;
            }
            state.mi.instruments[i] = m4w_ins;
        }
    } else {
        let count = (smp_num as usize).min(255);
        state.mi.num_instruments = count as u16;
        for i in 1..=count {
            let smp = &it_samples[i];
            let mut m4w_ins = Instrument::default();
            m4w_ins.name_len =
                sconvert_and_reduce(smp.name.as_bytes(), 26, true, &mut m4w_ins.name);
            m4w_ins.offset = 0;
            m4w_ins.iend = smp.length as i32;
            if (smp.flags & it_smp_flags::LOOP) != 0 && smp.loop_end > smp.loop_begin {
                m4w_ins.loop_end = smp.loop_end as i32;
                m4w_ins.loop_start = (smp.loop_end - smp.loop_begin) as i32;
            }
            m4w_ins.volume = smp.vol;
            m4w_ins.bits = if (smp.flags & it_smp_flags::IS_16BIT) != 0 {
                sample_flags::S_16BIT
            } else {
                sample_flags::S_8BIT
            };
            m4w_ins.tuning = smp.c5_speed.min(u16::MAX as u32) as u16;
            state.mi.instruments[i] = m4w_ins;
        }
    }

    // Erzeuge auch 1 Dummy-Pattern in `module.patterns` für Rückwärtskompatibilität
    module.patterns = vec![vec![[Note::default(); MAX_M4W_CHANNELS]; 64]];

    module.it_module = Some(Box::new(ItModuleData {
        song_name,
        philigt,
        cwtv,
        cmwt,
        flags,
        special,
        global_vol,
        mix_vol,
        initial_speed,
        initial_tempo,
        pan_sep,
        pwd,
        chn_pan,
        chn_vol,
        orders,
        instruments: it_instruments,
        samples: it_samples,
        patterns: it_patterns,
        midi_config,
        song_message,
        num_active_host_channels: display_tracks,
    }));

    Ok(module)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Baut ein minimales synthetisches `.IT`-Modul (2.14) mit 1 Instrument, 1 Sample und 1 Pattern.
    pub(crate) fn build_minimal_it_bytes() -> Vec<u8> {
        let mut buf = vec![0u8; 2048];
        buf[0..4].copy_from_slice(b"IMPM");
        buf[4..20].copy_from_slice(b"TEST IT MODULE\0\0");
        buf[0x1E..0x20].copy_from_slice(&0x0410u16.to_le_bytes()); // philigt
        buf[0x20..0x22].copy_from_slice(&2u16.to_le_bytes()); // ord_num = 2 (Order 0 = Pat 0, Order 1 = 0xFF)
        buf[0x22..0x24].copy_from_slice(&1u16.to_le_bytes()); // ins_num = 1
        buf[0x24..0x26].copy_from_slice(&1u16.to_le_bytes()); // smp_num = 1
        buf[0x26..0x28].copy_from_slice(&1u16.to_le_bytes()); // pat_num = 1
        buf[0x28..0x2A].copy_from_slice(&0x0214u16.to_le_bytes()); // cwtv = 2.14
        buf[0x2A..0x2C].copy_from_slice(&0x0214u16.to_le_bytes()); // cmwt = 2.14
        let flags = it_flags::STEREO | it_flags::USE_INSTRUMENTS | it_flags::LINEAR_SLIDES;
        buf[0x2C..0x2E].copy_from_slice(&flags.to_le_bytes());
        buf[0x30] = 128; // global_vol
        buf[0x31] = 48; // mix_vol
        buf[0x32] = 6; // initial_speed
        buf[0x33] = 125; // initial_tempo
        buf[0x34] = 128; // pan_sep
        for ch in 0..64 {
            buf[0x40 + ch] = 32; // center pan
            buf[0x80 + ch] = 64; // full channel vol
        }
        // Orders at 0xC0: [0, 0xFF]
        buf[0xC0] = 0;
        buf[0xC1] = 0xFF;
        // Offsets table at 0xC2:
        // ins[0] = 0x100, smp[0] = 0x350, pat[0] = 0x400
        buf[0xC2..0xC6].copy_from_slice(&0x100u32.to_le_bytes());
        buf[0xC6..0xCA].copy_from_slice(&0x350u32.to_le_bytes());
        buf[0xCA..0xCE].copy_from_slice(&0x400u32.to_le_bytes());

        // Instrument 1 at 0x100 (IMPI, 554 bytes)
        let ins_off = 0x100usize;
        buf[ins_off..ins_off + 4].copy_from_slice(b"IMPI");
        buf[ins_off + 0x11] = 1; // NNA = Continue
        buf[ins_off + 0x12] = 1; // DCT = Note
        buf[ins_off + 0x13] = 2; // DCA = NoteFade
        buf[ins_off + 0x14..ins_off + 0x16].copy_from_slice(&64u16.to_le_bytes()); // fadeout
        buf[ins_off + 0x18] = 128; // gbv
        buf[ins_off + 0x19] = 32; // dfp (pan enabled)
        buf[ins_off + 0x20..ins_off + 0x28].copy_from_slice(b"LEAD INS");
        buf[ins_off + 0x3A] = 0x80 | 96; // ifc (cutoff enabled = 96)
        buf[ins_off + 0x3B] = 0x80 | 24; // ifr (resonance enabled = 24)
        // Note-Sample keyboard table at ins_off + 0x40 (120 pairs)
        for n in 0..120usize {
            buf[ins_off + 0x40 + n * 2] = n as u8;
            buf[ins_off + 0x40 + n * 2 + 1] = 1; // Sample 1
        }
        // Volume envelope at ins_off + 0x130: enabled, 2 nodes (0->64, 16->32)
        let venv = ins_off + 0x130;
        buf[venv] = it_env_flags::ON;
        buf[venv + 1] = 2; // num_nodes = 2
        buf[venv + 6] = 64;
        buf[venv + 7..venv + 9].copy_from_slice(&0u16.to_le_bytes());
        buf[venv + 9] = 32;
        buf[venv + 10..venv + 12].copy_from_slice(&16u16.to_le_bytes());

        // Sample 1 at 0x350 (IMPS, 80 bytes)
        let smp_off = 0x350usize;
        buf[smp_off..smp_off + 4].copy_from_slice(b"IMPS");
        buf[smp_off + 0x11] = 64; // gvl
        buf[smp_off + 0x12] = it_smp_flags::HAS_DATA | it_smp_flags::LOOP;
        buf[smp_off + 0x13] = 64; // vol
        buf[smp_off + 0x14..smp_off + 0x1C].copy_from_slice(b"LEAD SMP");
        buf[smp_off + 0x2E] = 0x01; // cvt = signed PCM
        buf[smp_off + 0x30..smp_off + 0x34].copy_from_slice(&32u32.to_le_bytes()); // length = 32
        buf[smp_off + 0x34..smp_off + 0x38].copy_from_slice(&0u32.to_le_bytes()); // loop_begin = 0
        buf[smp_off + 0x38..smp_off + 0x3C].copy_from_slice(&32u32.to_le_bytes()); // loop_end = 32
        buf[smp_off + 0x3C..smp_off + 0x40].copy_from_slice(&8363u32.to_le_bytes()); // C5 speed
        buf[smp_off + 0x48..smp_off + 0x4C].copy_from_slice(&0x500u32.to_le_bytes()); // sample_pointer = 0x500
        for i in 0..32usize {
            buf[0x500 + i] = if i < 16 { 64 } else { 192 }; // Signed square wave (+64 / -64)
        }

        // Pattern 0 at 0x400: 4 rows
        // Row 0: Ch 0 -> Note C-5 (60), Ins 1, Vol 64, Cmd Z60 (Filter Cutoff)
        // Row 1: Ch 0 -> Note E-5 (64), Ins 1 (triggers NNA Continue on previous voice!)
        let pat_data: [u8; 16] = [
            0x81, // ch 1 + read new mask
            0x0F, // mask = note | ins | vol | cmd
            60,   // C-5
            1,    // Ins 1
            64,   // Vol 64
            26,   // Cmd 'Z' (26)
            0x60, // Param 0x60
            0x00, // End of row 0
            0x81, // ch 1 + read new mask
            0x03, // mask = note | ins
            64,   // E-5
            1,    // Ins 1
            0x00, // End of row 1
            0x00, // End of row 2
            0x00, // End of row 3
            0x00,
        ];
        let pat_off = 0x400usize;
        buf[pat_off..pat_off + 2].copy_from_slice(&(pat_data.len() as u16).to_le_bytes());
        buf[pat_off + 2..pat_off + 4].copy_from_slice(&4u16.to_le_bytes()); // 4 rows
        buf[pat_off + 8..pat_off + 8 + pat_data.len()].copy_from_slice(&pat_data);

        buf
    }

    #[test]
    fn test_load_it_and_umx_container_unwrap() {
        let it_bytes = build_minimal_it_bytes();
        let loaded = load_it(&it_bytes).expect("Minimal .IT muss fehlerfrei laden");
        assert_eq!(loaded.state.mi.file_type, ModType::IT);
        let it_mod = loaded.it_module.as_ref().expect("it_module muss gesetzt sein");
        assert_eq!(it_mod.song_name, "TEST IT MODULE");
        assert_eq!(it_mod.instruments.len(), 2); // 1-basiert
        assert_eq!(it_mod.samples.len(), 2);
        assert_eq!(it_mod.patterns.len(), 1);
        assert_eq!(it_mod.patterns[0].num_rows, 4);

        // Verpacke das .IT-Modul in einen synthetischen Unreal Engine 1 .UMX-Container (Magic 0x9E2A83C1)
        let mut umx = vec![0u8; 64];
        umx[0..4].copy_from_slice(&UMX_PACKAGE_MAGIC.to_le_bytes());
        umx[4..6].copy_from_slice(&61u16.to_le_bytes()); // UE1 package version 61
        umx.extend_from_slice(&it_bytes);

        let unwrapped = unwrap_umx(&umx).expect("unwrap_umx muss eingebetteten Stream finden");
        assert_eq!(&unwrapped[0..4], b"IMPM", "unwrap_umx muss den eingebetteten IMPM-Stream finden");

        let via_loader =
            crate::engine::formats::load_module_with_ext(&umx, Some("umx")).expect("UMX laden");
        assert_eq!(via_loader.state.mi.file_type, ModType::IT);
    }

    #[test]
    fn test_real_it_corpus_if_present() {
        let dir = std::path::Path::new("F:/VS Code/M4W-Diag/ModBatchModland/Mega10K/Impulsetracker");
        if !dir.is_dir() {
            return;
        }
        fn collect_some_it_files(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>, max: usize) {
            if out.len() >= max {
                return;
            }
            if let Ok(entries) = std::fs::read_dir(d) {
                for entry in entries.flatten() {
                    if out.len() >= max {
                        break;
                    }
                    let p = entry.path();
                    if p.is_dir() {
                        collect_some_it_files(&p, out, max);
                    } else if p
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("it"))
                        .unwrap_or(false)
                    {
                        out.push(p);
                    }
                }
            }
        }
        let mut it_files = Vec::new();
        collect_some_it_files(dir, &mut it_files, 15);
        for p in &it_files {
            let bytes = std::fs::read(p).expect("read .it file");
            let m = load_it(&bytes).unwrap_or_else(|e| {
                panic!("Fehler beim Laden von {:?}: {}", p, e)
            });
            assert_eq!(m.state.mi.file_type, ModType::IT);
            assert!(m.it_module.is_some());
        }
    }
}
