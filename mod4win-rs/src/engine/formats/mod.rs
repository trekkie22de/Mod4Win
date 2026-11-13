//! Gemeinsame Helfer, Sample-Konvertierung (`MemWrite` / `LoadInstrument`), Format-Erkennung (`GetInfo`)
//! und Haupt-Dispatcher `load_module` / `load_module_with_ext` für alle Tracker-Formate.
//!
//! Untermodule:
//! - [`protracker`]: `.MOD`, `.WOW`, `.NST`
//! - [`xm`]: FastTracker II `.XM`
//! - [`s3m`]: ScreamTracker 3 `.S3M`
//! - [`mtm_669_stm`]: MultiTracker `.MTM`, Composer 669 `.669`, ScreamTracker 2 `.STM`
//! - [`far_okt`]: Farandole `.FAR` & Amiga Oktalyzer `.OKT`

pub mod far_okt;
pub mod it;
pub mod mtm_669_stm;
pub mod protracker;
pub mod s3m;
pub mod xm;

pub use far_okt::{load_far, load_okt};
pub use it::{load_it, unwrap_umx, ItModuleData};
pub use mtm_669_stm::{load_669, load_mtm, load_stm};
pub use protracker::{load_mod, load_wow};
pub use s3m::load_s3m;
pub use xm::load_xm;

use crate::engine::types::{
    effect_ids, EngineState, Ft2RuntimeInstrument, Ft2RuntimeSample, ModType, Note, DEF_CH_VOL,
    ID_INV_MOD_FILE, ID_NO_MOD_FILE, MAX_M4W_CHANNELS,
};

use protracker::is_wow_candidate;

// ============================================================================
// 1. Geladene Modul-Repräsentation (Header, Patterns, Sample-RAM, FT2-Daten)
// ============================================================================

/// Maximale Anzahl von FT2-Instrumenten (`MaxFT2Insts = 255` in `playdefs.pas`).
pub const MAX_FT2_INSTS: usize = 255;

/// Maximale Anzahl von FT2-Samples (`MaxFT2Samples = 1024` in `playdefs.pas`).
pub const MAX_FT2_SAMPLES: usize = 1024;

/// Vollständig im Speicher entpacktes Tracker-Modul inklusive initialisiertem [`EngineState`],
/// entpackten Pattern-Spuren (`Note`), vorbereitetem Sample-Speicher (`SamplesPtr` / `PlayMem`)
/// sowie FastTracker-II-Laufzeit-Instrumenten und -Samples.
#[derive(Clone)]
pub struct LoadedModule {
    /// Initialisierter globaler Engine-Zustand (`MI`, `DefPanning`, `PArrangement`, `PtrnBreaks`, Tempi).
    pub state: EngineState,
    /// Entpackte Patterns: `patterns[pat_idx][row_idx][chn_idx]` (`row_idx` in `0..MAX_PATT_LEN`, `chn_idx` in `0..MAX_M4W_CHANNELS`).
    pub patterns: Vec<Vec<[Note; MAX_M4W_CHANNELS]>>,
    /// Linearer Sample-Speicher (`PlayMem` / `FS:[ESI]` in `dacmix.pas`), inklusive Cubic-Spline- und
    /// Oversampling-Guard-Bytes sowie entrollten Ping-Pong- (BiDi) und Kurz-Loops (`< 1024` Bytes).
    pub sample_mem: Vec<u8>,
    /// FastTracker-II-Instrumente (1-basiert indiziert `1..=128`, Index `0` ist Dummy).
    pub ft2_instruments: Vec<Ft2RuntimeInstrument>,
    /// FastTracker-II-Samples (1-basiert indiziert `1..=MAX_FT2_SAMPLES`, Index `0` ist Dummy).
    pub ft2_samples: Vec<Ft2RuntimeSample>,
    /// Gekapselte Impulse-Tracker-Moduldaten (`.IT`, `None` für alle klassischen Formate).
    pub it_module: Option<Box<ItModuleData>>,
}

impl Default for LoadedModule {
    fn default() -> Self {
        Self {
            state: EngineState::default(),
            patterns: Vec::new(),
            // PlayMemOfs startet in GetPlayMem (playdefs.pas Zeile 1397) bei SurrBufferLen + 65536 (8192 + 65536 = 73728),
            // wobei in PlayMemSel[0..33280] die VolumeTable liegt (SetVolTable, play.inc Zeilen 2208–2240).
            sample_mem: vec![0u8; 8192 + 65536],
            ft2_instruments: vec![Ft2RuntimeInstrument::default(); MAX_FT2_INSTS + 1],
            ft2_samples: vec![Ft2RuntimeSample::default(); MAX_FT2_SAMPLES + 1],
            it_module: None,
        }
    }
}

// ============================================================================
// 2. Hilfsfunktionen zum Lesen von Primitivtypen & Strings (loaders.pas)
// ============================================================================

/// Liest ein Big-Endian `u16` (entspricht `Swap(w)` auf Little-Endian x86 in `loaders.pas`).
#[inline(always)]
pub fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// Liest ein Big-Endian `u32` (entspricht `SwapLong(l)` in `loaders.pas` Zeile 469).
#[inline(always)]
pub fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Liest ein Little-Endian `u16`.
#[inline(always)]
pub fn read_le_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Liest ein Little-Endian `i16`.
#[inline(always)]
pub fn read_le_i16(data: &[u8], offset: usize) -> Option<i16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(i16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Liest ein Little-Endian `u32`.
#[inline(always)]
pub fn read_le_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Liest ein Little-Endian `i32`.
#[inline(always)]
pub fn read_le_i32(data: &[u8], offset: usize) -> Option<i32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Konvertiert ein festes ASCII-Char-Array ohne Nullterminierung in ein Pascal-String-Array
/// (`sconvert` und optional `ReduceString` in `loaders.pas` Zeilen 274–318).
pub fn sconvert_and_reduce(src: &[u8], max_len: usize, reduce: bool, out_buf: &mut [u8; 40]) -> u8 {
    *out_buf = [0u8; 40];
    let limit = max_len.min(src.len()).min(40);
    let mut temp = [0u8; 40];
    let mut len = 0usize;

    for &b in src.iter().take(limit) {
        if b == 0 {
            break;
        }
        temp[len] = if b < b' ' { b' ' } else { b };
        len += 1;
    }

    let mut slice = &temp[..len];
    if reduce {
        while !slice.is_empty() && slice[slice.len() - 1] == b' ' {
            slice = &slice[..slice.len() - 1];
        }
        while !slice.is_empty() && slice[0] == b' ' {
            slice = &slice[1..];
        }
    }

    out_buf[..slice.len()].copy_from_slice(slice);
    slice.len() as u8
}

/// Setzt ein Bit in einem 256-Bit-Pascal-`SET OF 0..255` (`[u8; 32]`, entspricht `Include(Set, Elem)`).
#[inline]
pub fn bitset_include(set: &mut [u8; 32], elem: u8) {
    set[(elem >> 3) as usize] |= 1 << (elem & 7);
}

/// Prüft, ob ein Bit in einem 256-Bit-Pascal-`SET OF 0..255` (`[u8; 32]`) gesetzt ist (`Elem IN Set`).
#[inline]
pub fn bitset_contains(set: &[u8; 32], elem: u8) -> bool {
    (set[(elem >> 3) as usize] & (1 << (elem & 7))) != 0
}

// ============================================================================
// 3. Sample-Konvertierung & Guard-Byte-Aufbereitung (MemWrite & LoadInstrument)
// ============================================================================

/// Konvertiert rohe Sample-Daten in vorzeichenbehaftete Little-Endian PCM-Daten
/// (`MemWrite` 1. Stufe in `legacy_src/PLAYER/playdefs.pas` Zeilen 1001–1269).
pub fn convert_sample_stage1(raw: &[u8], ins_type: u8) -> Vec<u8> {
    let mut out = raw.to_vec();
    match ins_type {
        0 | 2 => {}
        1 => {
            for b in &mut out {
                *b ^= 0x80;
            }
        }
        3 => {
            for chunk in out.chunks_exact_mut(2) {
                chunk[1] ^= 0x80;
            }
        }
        4 => {
            for b in &mut out {
                *b <<= 1;
            }
        }
        7 => {
            let mut acc: u8 = 0;
            for b in &mut out {
                acc = acc.wrapping_add(*b);
                *b = acc;
            }
        }
        8 => {
            let mut acc: u16 = 0;
            for chunk in out.chunks_exact_mut(2) {
                let delta = u16::from_le_bytes([chunk[0], chunk[1]]);
                acc = acc.wrapping_add(delta);
                let le = acc.to_le_bytes();
                chunk[0] = le[0];
                chunk[1] = le[1];
            }
        }
        _ => {}
    }
    out
}

/// Erzeugt eine rückwärts gelesene Kopie eines Loop-Segments für Ping-Pong-Loops (`BiDi`).
fn reverse_loop_segment(src: &[u8], is_16bit: bool) -> Vec<u8> {
    if is_16bit {
        let mut out = Vec::with_capacity(src.len());
        let word_count = src.len() / 2;
        for i in (0..word_count).rev() {
            out.push(src[i * 2]);
            out.push(src[i * 2 + 1]);
        }
        out
    } else {
        let mut out = src.to_vec();
        out.reverse();
        out
    }
}

/// Lädt und präpariert ein einzelnes Sample bit-identisch zu `LoadInstrument` in
/// `legacy_src/PLAYER/loaders.pas` (Zeilen 337–465).
#[allow(clippy::too_many_arguments)]
pub fn load_instrument_into_sample_mem(
    sample_mem: &mut Vec<u8>,
    raw_bytes: &[u8],
    offset: &mut i32,
    iend: &mut i32,
    loop_start: &mut i32,
    loop_end: &mut i32,
    volume: &mut u8,
    bits: u8,
    ins_type: u8,
    bidi: bool,
) {
    let mut dummy_top = usize::MAX;
    let mut dummy_limit = usize::MAX;
    load_instrument_into_sample_mem_with_top(
        sample_mem,
        raw_bytes,
        offset,
        iend,
        loop_start,
        loop_end,
        volume,
        bits,
        ins_type,
        bidi,
        &mut dummy_top,
        &mut dummy_limit,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn load_instrument_into_sample_mem_with_top(
    sample_mem: &mut Vec<u8>,
    raw_bytes: &[u8],
    offset: &mut i32,
    iend: &mut i32,
    loop_start: &mut i32,
    loop_end: &mut i32,
    volume: &mut u8,
    bits: u8,
    ins_type: u8,
    mut bidi: bool,
    play_mem_top: &mut usize,
    effective_limit: &mut usize,
) {
    const MAX_CLASSIC_SAMPLE_MEM: usize = 256 * 1024 * 1024;
    let actual_len = (raw_bytes.len() as i32).min(*iend);
    *iend = actual_len;
    if *iend <= 0
        || sample_mem
            .len()
            .saturating_add((*iend as usize).saturating_mul(2))
            .saturating_add(2048)
            > MAX_CLASSIC_SAMPLE_MEM
    {
        *offset = 0;
        *iend = 0;
        *loop_start = 0;
        *loop_end = 0;
        return;
    }

    if *volume > 64 {
        *volume = 64;
    }
    if *loop_end > *iend {
        *loop_end = *iend;
    }
    if *loop_start > *iend {
        *loop_start = 0;
    }
    if *loop_start < 0 || *loop_end < 0 || (*loop_end).saturating_sub(*loop_start) < 3 {
        *loop_start = 0;
        *loop_end = 0;
    }

    // BIT-EXACTNESS INVARIANT (99.97% PLAYER32.DLL Parity — DO NOT "FIX"):
    // Ungerade Byte-Grenzen (`*iend`, `*loop_start`, `*loop_end`) bei 16-Bit-Samples werden hier
    // bewusst NICHT mit `& !1` gerade gerundet (Carbon-Review MITTEL-LOADER-2). `loaders.pas`
    // übernimmt diese Byte-Längen 1:1 ungerundet in das `< 1024`-Kurzloop-Unrolling; eine Rundung
    // verschiebt die Loop-Phase historischer `.XM`/`.MTM`/`.FAR`-Module gegenüber `PLAYER32.DLL`.
    let is_16bit = (bits & 0x1F) == 16;
    let mut pcm = convert_sample_stage1(&raw_bytes[..(*iend as usize)], ins_type);

    // Zu kurze Loops (< 1024 Bytes) entrollen (loaders.pas Zeilen 389–407)
    if *loop_end > 0 && *loop_end < 1024 {
        let ls = (*loop_start as usize).min(pcm.len());
        let mut le = (*loop_end as usize).min(pcm.len());

        if bidi && le > ls {
            let i_len = le - ls;
            let rev = reverse_loop_segment(&pcm[ls..le], is_16bit);
            if pcm.len() < le + i_len {
                pcm.resize(le + i_len, 0);
            }
            let write_start = le + i_len - rev.len();
            pcm[write_start..le + i_len].copy_from_slice(&rev);
            le += i_len;
            *iend += i_len as i32;
            *loop_end = le as i32;
            bidi = false;
        }

        if le > ls {
            while le < 1024 {
                let i_len = le - ls;
                let seg = pcm[ls..le].to_vec();
                if pcm.len() < le + i_len {
                    pcm.resize(le + i_len, 0);
                }
                pcm[le..le + i_len].copy_from_slice(&seg);
                le += i_len;
                *loop_end = le as i32;
                *iend += i_len as i32;
            }
        }
        if pcm.len() < (*iend as usize) {
            pcm.resize(*iend as usize, 0);
        }
    }

    // PlayMemOfs := (PlayMemOfs DIV 4 + 1) * 4 (loaders.pas Zeile 416)
    let aligned_ofs = ((sample_mem.len() / 4) + 1) * 4;
    sample_mem.resize(aligned_ofs, 0);

    // FitPlayMem(PlayMemOfs + IEnd + 16) (loaders.pas Zeile 417)
    let need1 = aligned_ofs + (*iend as usize) + 16;
    if need1 >= *play_mem_top {
        let was_initial = (*play_mem_top & 0x3FF) == 0 && *effective_limit == *play_mem_top;
        let old_page = (*play_mem_top - 1) >> 12;
        let new_page = (need1 - 1) >> 12;
        *play_mem_top = (need1 + 31) & !31;
        if new_page > old_page || was_initial || *play_mem_top <= 0x100000 {
            *effective_limit = *play_mem_top;
        }
    }

    // 2 Null-Bytes für Cubic-Spline voranstellen (loaders.pas Zeilen 423–424)
    sample_mem.extend_from_slice(&[0u8, 0u8]);

    let smp_offset = sample_mem.len() as i32;
    *offset = smp_offset;

    // Sample-Daten schreiben (loaders.pas Zeilen 426–428)
    let valid_len = (*iend as usize).min(pcm.len());
    sample_mem.extend_from_slice(&pcm[..valid_len]);
    let mut play_mem_ofs = (smp_offset + *iend) as usize;
    if sample_mem.len() < play_mem_ofs {
        sample_mem.resize(play_mem_ofs, 0);
    }
    if sample_mem.len() > *effective_limit {
        sample_mem[*effective_limit..].fill(0);
    }

    // BiDi-Loop entrollen, falls noch aktiv (loaders.pas Zeilen 429–441)
    if bidi && *loop_end > *loop_start {
        let ls = (smp_offset + *loop_start) as usize;
        let le = (smp_offset + *loop_end) as usize;
        if le <= sample_mem.len() && ls < le {
            let loop_len = le - ls;
            let need2 = play_mem_ofs + loop_len + 16;
            if need2 >= *play_mem_top {
                let was_initial = (*play_mem_top & 0x3FF) == 0 && *effective_limit == *play_mem_top;
                let old_page = (*play_mem_top - 1) >> 12;
                let new_page = (need2 - 1) >> 12;
                *play_mem_top = (need2 + 31) & !31;
                if new_page > old_page || was_initial || *play_mem_top <= 0x100000 {
                    *effective_limit = *play_mem_top;
                }
            }
            let rev = reverse_loop_segment(&sample_mem[ls..le], is_16bit);
            play_mem_ofs += loop_len;
            let needed = play_mem_ofs.max(le + loop_len);
            if sample_mem.len() < needed {
                sample_mem.resize(needed, 0);
            }
            let write_start = le + loop_len - rev.len();
            sample_mem[write_start..le + loop_len].copy_from_slice(&rev);
            if sample_mem.len() > *effective_limit {
                sample_mem[*effective_limit..].fill(0);
            }
            *iend += loop_len as i32;
            *loop_end += loop_len as i32;
        }
    }

    // 4 Guard-Bytes für Oversampling/Cubic-Spline schreiben (loaders.pas Zeilen 443–446)
    if *loop_end == 0 {
        let needed = play_mem_ofs + 4;
        if sample_mem.len() < needed {
            sample_mem.resize(needed, 0);
        }
        sample_mem[play_mem_ofs..play_mem_ofs + 4].fill(0);
    } else {
        let ls = (smp_offset + *loop_start) as usize;
        let le = (smp_offset + *loop_end) as usize;
        let mut guard = [0u8; 4];
        for (idx, g) in guard.iter_mut().enumerate() {
            if let Some(&b) = sample_mem.get(ls + idx) {
                *g = b;
            }
        }
        let needed = (play_mem_ofs + 4).max(le + 4);
        if sample_mem.len() < needed {
            sample_mem.resize(needed, 0);
        }
        sample_mem[le..le + 4].copy_from_slice(&guard);
        if sample_mem.len() > *effective_limit {
            sample_mem[*effective_limit..].fill(0);
        }
    }

    // Offsets anpassen (loaders.pas Zeilen 448–461)
    *iend += *offset;
    if *loop_end > 0 {
        *loop_start += *offset;
        *loop_end += *offset;
        *iend = *loop_end;
    }

    if is_16bit {
        *offset >>= 1;
        *loop_start >>= 1;
        *loop_end >>= 1;
        *iend >>= 1;
    }
}

/// Führt die Effekt-Normalisierung und Flag-Aktualisierung aus `TrackWrite`
/// (`legacy_src/PLAYER/loaders.pas` Zeilen 98–150) für eine geladene [`Note`] durch.
pub fn normalize_note_for_track_write(note: &mut Note, state: &mut EngineState) {
    // ProTracker Enhanced Effects ($0E -> $10 + (Operands SHR 4))
    if note.effekt == 0x0E {
        note.effekt = 0x10 + (note.operands >> 4);
        note.operands &= 0x0F;
    }

    if note.effekt != effect_ids::NO_EFFEKT {
        let is_s3m_or_stm = matches!(state.mi.file_type, ModType::STM | ModType::S3M);
        // Wichtig: In loaders.pas (Zeilen 112–129, {$DEFINE CompressTracks}) wird `note.effekt`
        // VOR der CASE-Verzweigung unverändert in den gepackten Track-Puffer (`bBuf`) geschrieben!
        // Die Umwandlung nach FSldUp/XtraFSldUp/FSldDwn/XtraFSldDwn/FineVolUp/FineVolDwn dient
        // dort nur für `Include(MI.EffectsUsedSet, Effekt)`, während die Laufzeit-Konvertierung
        // (inkl. `PrtOps`/`VSldOps`-Memory und `Operands AND $0F`) erst in `ProcessNewNote`
        // (`play.inc` Zeilen 773–815) stattfindet.
        let mut used_eff = note.effekt;
        match note.effekt {
            effect_ids::PORTAMENTO_UP if is_s3m_or_stm => {
                if note.operands >= 0xF0 {
                    used_eff = effect_ids::F_SLD_UP;
                } else if note.operands >= 0xE0 {
                    used_eff = effect_ids::XTRA_F_SLD_UP;
                }
            }
            effect_ids::PORTAMENTO_DOWN if is_s3m_or_stm => {
                if note.operands >= 0xF0 {
                    used_eff = effect_ids::F_SLD_DWN;
                } else if note.operands >= 0xE0 {
                    used_eff = effect_ids::XTRA_F_SLD_DWN;
                }
            }
            effect_ids::VOLUME_SLIDE if is_s3m_or_stm => {
                if (note.operands & 0x0F) == 0x0F && (note.operands & 0xF0) != 0 {
                    used_eff = effect_ids::FINE_VOL_UP;
                } else if (note.operands & 0xF0) == 0xF0 && (note.operands & 0x0F) != 0 {
                    used_eff = effect_ids::FINE_VOL_DWN;
                }
            }
            effect_ids::STEREO_CONTROL if is_s3m_or_stm => {
                state.scontrol = true;
            }
            effect_ids::PANNING | effect_ids::E_PANNING => {
                state.mi.panning_off = false;
            }
            _ => {}
        }
        bitset_include(&mut state.mi.effects_used_set, used_eff);
    }

    if note.chn_vol != DEF_CH_VOL {
        if state.mi.file_type == ModType::FT2 {
            match note.chn_vol {
                0x10..=0x50 => bitset_include(&mut state.mi.effects_used_set, effect_ids::SET_VOLUME),
                0x60..=0x7F => bitset_include(&mut state.mi.effects_used_set, effect_ids::VOLUME_SLIDE),
                0x80..=0x8F => bitset_include(&mut state.mi.effects_used_set, effect_ids::FINE_VOL_DWN),
                0x90..=0x9F => bitset_include(&mut state.mi.effects_used_set, effect_ids::FINE_VOL_UP),
                0xB0..=0xBF => bitset_include(&mut state.mi.effects_used_set, effect_ids::VIBRATO),
                0xC0..=0xCF => bitset_include(&mut state.mi.effects_used_set, effect_ids::E_PANNING),
                0xD0..=0xEF => bitset_include(&mut state.mi.effects_used_set, effect_ids::PAN_SLD),
                0xF0..=0xFF => bitset_include(&mut state.mi.effects_used_set, effect_ids::TONE_PORTAMENTO),
                _ => {}
            }
        } else {
            bitset_include(&mut state.mi.effects_used_set, effect_ids::SET_VOLUME);
        }
    }
}

// ============================================================================
// 4. Format-Erkennung & Haupt-Dispatcher (GetInfo in loaders.pas Zeilen 610–660)
// ============================================================================

/// Erkennt das Modulformat anhand der Binär-Signaturen im `&[u8]`-Slice.
pub fn detect_module_type(data: &[u8]) -> Option<ModType> {
    detect_module_type_with_ext(data, None)
}

/// Erkennt das Modulformat anhand der Binär-Signaturen im `&[u8]`-Slice sowie optional der Dateiendung.
pub fn detect_module_type_with_ext(data: &[u8], ext: Option<&str>) -> Option<ModType> {
    let data = unwrap_umx(data).unwrap_or(data);
    let ext_clean = ext.map(|e| e.trim_start_matches('.').to_ascii_uppercase());

    // Explizite .WOW-Endung wie in GetInfo (loaders.pas Zeile 649)
    if matches!(ext_clean.as_deref(), Some("WOW") | Some("WO_"))
        && data.len() >= 1084
        && &data[1080..1084] == b"M.K."
    {
        return Some(ModType::WOW);
    }

    // 0. Impulse Tracker (.IT): Offset 0..4 == "IMPM"
    if data.len() >= 192 && &data[0..4] == b"IMPM" {
        return Some(ModType::IT);
    }

    // 1. FastTracker II (.XM): Offset 0..17 == "Extended Module: "
    if data.len() >= 60 && &data[0..17] == b"Extended Module: " && data[37] == 0x1A {
        return Some(ModType::FT2);
    }

    // 2. ScreamTracker 3 (.S3M): Offset 44..48 == "SCRM"
    if data.len() >= 96 && &data[44..48] == b"SCRM" {
        return Some(ModType::S3M);
    }

    // 3. Amiga Oktalyzer (.OKT): Offset 0..8 == "OKTASONG"
    if data.len() >= 16 && &data[0..8] == b"OKTASONG" {
        return Some(ModType::Oktalyz);
    }

    // 4. Farandole Composer (.FAR): Offset 0..4 == "FAR\xFE" und Offset 44..47 == "\r\n\x1A"
    if data.len() >= 98 && &data[0..4] == b"FAR\xFE" && &data[44..47] == b"\r\n\x1A" {
        return Some(ModType::FFar);
    }

    // 5. MultiTracker (.MTM): Offset 0..3 == "MTM", Attribute == 0, NTracks in 1..=32
    if data.len() >= 66
        && &data[0..3] == b"MTM"
        && data[31] == 0
        && (1..=32).contains(&data[33])
        && data[34..66].iter().all(|&p| p <= 0x0F)
    {
        return Some(ModType::MTM);
    }

    // 6. ScreamTracker 2 (.STM): Offset 20..28 == "!Scream!" / "BMOD2STM" / "SWavePro", Offset 28 == 0x1A
    if data.len() >= 1168
        && matches!(&data[20..28], b"!Scream!" | b"BMOD2STM" | b"SWavePro")
        && data[28] == 0x1A
    {
        return Some(ModType::STM);
    }

    // 7. UltraTracker (.ULT): Offset 0..12 == "MAS_UTrack_V"
    if data.len() >= 49 && &data[0..12] == b"MAS_UTrack_V" {
        return Some(ModType::ULT);
    }

    // 8. X-Tracker (.DMF): Offset 0..4 == "DDMF"
    if data.len() >= 66 && &data[0..4] == b"DDMF" {
        return Some(ModType::DMF);
    }

    // 9. 31-Instrumente Amiga/PC .MOD oder .WOW: Signatur bei Offset 1080..1084
    if data.len() >= 1084 {
        let sig = &data[1080..1084];
        if sig == b"M.K." && is_wow_candidate(data) {
            return Some(ModType::WOW);
        }
        if matches!(
            sig,
            b"M.K." | b"M!K!" | b"M&K!" | b"M.K!" | b"N.T." | b"OCTA" | b"CD81"
        ) {
            return Some(ModType::ProTr);
        }
        if matches!(sig, b"FLT4" | b"FLT8" | b"EX04" | b"EX08") {
            return Some(ModType::StarTr);
        }
        if &sig[1..4] == b"CHN" && sig[0].is_ascii_digit() {
            return Some(ModType::Ftrk);
        }
        if &sig[2..4] == b"CH" && sig[0].is_ascii_digit() && sig[1].is_ascii_digit() {
            return Some(ModType::Ftrk);
        }
    }

    // 10. Composer 669 / UNIS 669 (.669): Offset 0..2 == 0x6669 ("if") oder 0x4E4A ("JN")
    if data.len() >= 497 {
        let sign = read_le_u16(data, 0).unwrap_or(0);
        let nos = data[110];
        let nop = data[111];
        if matches!(sign, 0x6669 | 0x4E4A) && (1..=64).contains(&nos) && (1..=128).contains(&nop) {
            return if sign == 0x4E4A {
                Some(ModType::UNIS)
            } else {
                Some(ModType::F669)
            };
        }
    }

    // 11. 15-Instrumente SoundTracker / NoiseTracker .MOD (Headergröße 600 Bytes)
    if data.len() >= 600 {
        let arrange_size = data[470];
        if (1..=128).contains(&arrange_size) {
            return Some(ModType::NoiseTr);
        }
    }

    None
}

/// Lädt ein unterstütztes Tracker-Modul anhand der Binär-Signatur aus einem `&[u8]`-Slice.
pub fn load_module(data: &[u8]) -> Result<LoadedModule, u16> {
    load_module_with_ext(data, None)
}

/// Lädt ein unterstütztes Tracker-Modul unter Berücksichtigung einer optionalen Dateiendung.
pub fn load_module_with_ext(data: &[u8], ext: Option<&str>) -> Result<LoadedModule, u16> {
    let data = unwrap_umx(data).unwrap_or(data);
    match detect_module_type_with_ext(data, ext) {
        Some(ModType::IT) => load_it(data),
        Some(ModType::FT2) => load_xm(data),
        Some(ModType::S3M) => load_s3m(data),
        Some(ModType::MTM) => load_mtm(data),
        Some(ModType::F669 | ModType::UNIS) => load_669(data),
        Some(ModType::STM) => load_stm(data),
        Some(ModType::FFar) => load_far(data),
        Some(ModType::Oktalyz) => load_okt(data),
        Some(ModType::WOW) => load_wow(data),
        Some(ModType::ProTr | ModType::StarTr | ModType::Ftrk | ModType::NoiseTr) => load_mod(data),
        Some(ModType::ULT | ModType::DMF) => Err(ID_INV_MOD_FILE),
        None => Err(ID_NO_MOD_FILE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_mtm_669_stm_far_okt_and_wow() {
        // 1. MultiTracker (.MTM) Test
        {
            let mut mtm = vec![0u8; 66];
            mtm[0..3].copy_from_slice(b"MTM");
            mtm[3] = 0x10;
            mtm[4..12].copy_from_slice(b"Test MTM");
            mtm[24..26].copy_from_slice(&1u16.to_le_bytes());
            mtm[26] = 0;
            mtm[27] = 0;
            mtm[28..30].copy_from_slice(&0u16.to_le_bytes());
            mtm[30] = 1;
            mtm[31] = 0;
            mtm[32] = 64;
            mtm[33] = 4;
            for i in 0..32 {
                mtm[34 + i] = if i % 2 == 0 { 3 } else { 12 };
            }
            let mut inst = [0u8; 37];
            inst[0..6].copy_from_slice(b"Lead 1");
            inst[22..26].copy_from_slice(&32i32.to_le_bytes());
            inst[26..30].copy_from_slice(&0i32.to_le_bytes());
            inst[30..34].copy_from_slice(&0i32.to_le_bytes());
            inst[34] = 0;
            inst[35] = 64;
            inst[36] = 0;
            mtm.extend_from_slice(&inst);
            mtm.extend_from_slice(&[0u8; 128]);
            let mut trk = [0u8; 192];
            trk[0] = 12 << 2;
            trk[1] = (1 << 4) | 0x0F;
            trk[2] = 0x04;
            mtm.extend_from_slice(&trk);
            let mut seq = [0u8; 64];
            seq[0..2].copy_from_slice(&1u16.to_le_bytes());
            mtm.extend_from_slice(&seq);
            mtm.extend_from_slice(&[0x80u8; 32]);

            assert_eq!(detect_module_type(&mtm), Some(ModType::MTM));
            let loaded = load_module(&mtm).expect("MTM load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            let num_instruments = loaded.state.mi.num_instruments;
            assert_eq!(loaded.state.mi.file_type, ModType::MTM);
            assert_eq!(num_tracks, 4);
            assert_eq!(num_instruments, 1);
            assert_eq!(loaded.patterns[0][0][0].instrument, 1);
            assert_eq!(loaded.patterns[0][0][0].effekt, effect_ids::SET_SPEED);
            assert_eq!(loaded.patterns[0][0][0].operands, 4);
        }

        // 2. Composer 669 / UNIS 669 (.669) Test
        {
            let mut f669 = vec![0u8; 497];
            f669[0..2].copy_from_slice(&0x6669u16.to_le_bytes());
            f669[2..10].copy_from_slice(b"669 Song");
            f669[110] = 1;
            f669[111] = 1;
            f669[112] = 0;
            f669[113] = 0;
            f669[114..241].fill(0xFF);
            f669[241] = 4;
            f669[369] = 31;
            let mut inst = [0u8; 25];
            inst[0..5].copy_from_slice(b"Synth");
            inst[13..17].copy_from_slice(&16i32.to_le_bytes());
            inst[17..21].copy_from_slice(&0i32.to_le_bytes());
            inst[21..25].copy_from_slice(&32i32.to_le_bytes());
            f669.extend_from_slice(&inst);
            let mut pat = vec![0xFFu8; 1536];
            pat[0] = 12 << 2;
            pat[1] = 0x0F;
            pat[2] = 0x53;
            f669.extend_from_slice(&pat);
            f669.extend_from_slice(&[0x80u8; 16]);

            assert_eq!(detect_module_type(&f669), Some(ModType::F669));
            let loaded = load_module(&f669).expect("669 load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            assert_eq!(loaded.state.mi.file_type, ModType::F669);
            assert_eq!(num_tracks, 8);
            assert_eq!(loaded.state.init_speed, 4);
            assert_eq!(loaded.state.ptrn_breaks[0], 31);
            assert_eq!(loaded.patterns[0][0][0].instrument, 1);
            assert_eq!(loaded.patterns[0][0][0].effekt, effect_ids::SET_SPEED);
            assert_eq!(loaded.patterns[0][0][0].operands, 3);
        }

        // 3. ScreamTracker 2 (.STM) Test
        {
            let mut stm = vec![0u8; 1168];
            stm[0..8].copy_from_slice(b"STM Test");
            stm[20..28].copy_from_slice(b"!Scream!");
            stm[28] = 0x1A;
            stm[29] = 2;
            stm[32] = 0x60;
            stm[33] = 1;
            stm[34] = 64;
            stm[48 + 14..48 + 16].copy_from_slice(&137u16.to_le_bytes());
            stm[48 + 16..48 + 18].copy_from_slice(&16u16.to_le_bytes());
            stm[48 + 20..48 + 22].copy_from_slice(&0xFFFFu16.to_le_bytes());
            stm[48 + 22] = 64;
            stm[48 + 24..48 + 26].copy_from_slice(&8363u16.to_le_bytes());
            stm[1040] = 0;
            stm[1041..1168].fill(99);
            let mut pat = vec![0xFFu8; 1024];
            pat[0] = 2 << 4;
            pat[1] = 1 << 3;
            pat[2] = 1;
            pat[3] = 0x50;
            stm.extend_from_slice(&pat);
            stm.extend_from_slice(&[0u8; 16]);

            assert_eq!(detect_module_type(&stm), Some(ModType::STM));
            let loaded = load_module(&stm).expect("STM load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            assert_eq!(loaded.state.mi.file_type, ModType::STM);
            assert_eq!(num_tracks, 4);
            assert_eq!(loaded.state.init_speed, 6);
            assert_eq!(loaded.patterns[0][0][0].instrument, 1);
            assert_eq!(loaded.patterns[0][0][0].effekt, effect_ids::SET_SPEED);
            assert_eq!(loaded.patterns[0][0][0].operands, 5);
        }

        // 4. Farandole Composer (.FAR) Test
        {
            let mut far = vec![0u8; 98];
            far[0..4].copy_from_slice(b"FAR\xFE");
            far[4..12].copy_from_slice(b"FAR Song");
            far[44..47].copy_from_slice(b"\r\n\x1A");
            let header_len: u16 = 98 + 771;
            far[47..49].copy_from_slice(&header_len.to_le_bytes());
            far[49] = 0x10;
            far[50..66].fill(1);
            far[75] = 4;
            far[76..92].fill(8);
            far[96..98].copy_from_slice(&0u16.to_le_bytes());

            let mut hdr2 = vec![0u8; 771];
            hdr2[0] = 0;
            hdr2[256] = 1;
            hdr2[257] = 1;
            hdr2[258] = 0;
            let pat0_len: u16 = 2 + 4 * 64;
            hdr2[259..261].copy_from_slice(&pat0_len.to_le_bytes());
            far.extend_from_slice(&hdr2);

            let mut pat0 = vec![0u8; pat0_len as usize];
            pat0[0] = 2;
            pat0[2] = 13;
            pat0[3] = 0;
            pat0[4] = 16;
            pat0[5] = 0xD2;
            far.extend_from_slice(&pat0);

            let mut smap = [0u8; 8];
            smap[0] = 1;
            far.extend_from_slice(&smap);

            let mut finst = vec![0u8; 48];
            finst[0..8].copy_from_slice(b"FAR Smp1");
            finst[32..36].copy_from_slice(&16i32.to_le_bytes());
            finst[37] = 64;
            far.extend_from_slice(&finst);
            far.extend_from_slice(&[0u8; 16]);

            assert_eq!(detect_module_type(&far), Some(ModType::FFar));
            let loaded = load_module(&far).expect("FAR load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            assert_eq!(loaded.state.mi.file_type, ModType::FFar);
            assert_eq!(num_tracks, 16);
            assert_eq!(loaded.state.ptrn_breaks[0], 3);
            assert_eq!(loaded.patterns[0][0][0].instrument, 1);
            assert_eq!(loaded.patterns[0][0][0].effekt, effect_ids::FINE_TEMPO_DWN);
            assert_eq!(loaded.patterns[0][0][0].operands, 2);
        }

        // 5. Amiga Oktalyzer (.OKT) Test
        {
            let mut okt = Vec::new();
            okt.extend_from_slice(b"OKTASONG");
            okt.extend_from_slice(b"CMOD");
            okt.extend_from_slice(&8u32.to_be_bytes());
            okt.extend_from_slice(&[0u8; 8]);
            okt.extend_from_slice(b"SAMP");
            okt.extend_from_slice(&32u32.to_be_bytes());
            let mut oinst = [0u8; 32];
            oinst[0..7].copy_from_slice(b"OktSmp1");
            oinst[20..24].copy_from_slice(&16u32.to_be_bytes());
            oinst[29] = 64;
            oinst[31] = 1;
            okt.extend_from_slice(&oinst);
            okt.extend_from_slice(b"SPEE");
            okt.extend_from_slice(&2u32.to_be_bytes());
            okt.extend_from_slice(&[0, 5]);
            okt.extend_from_slice(b"SLEN");
            okt.extend_from_slice(&2u32.to_be_bytes());
            okt.extend_from_slice(&0u16.to_be_bytes());
            okt.extend_from_slice(b"PLEN");
            okt.extend_from_slice(&2u32.to_be_bytes());
            okt.extend_from_slice(&1u16.to_be_bytes());
            okt.extend_from_slice(b"PATT");
            okt.extend_from_slice(&1u32.to_be_bytes());
            okt.push(0);
            okt.extend_from_slice(b"PBOD");
            okt.extend_from_slice(&66u32.to_be_bytes());
            okt.extend_from_slice(&4u16.to_be_bytes());
            let mut pbod = vec![0u8; 64];
            pbod[0] = 13;
            pbod[1] = 0;
            pbod[2] = 0x1C;
            pbod[3] = 4;
            okt.extend_from_slice(&pbod);
            okt.extend_from_slice(b"SBOD");
            okt.extend_from_slice(&16u32.to_be_bytes());
            okt.extend_from_slice(&[0u8; 16]);

            assert_eq!(detect_module_type(&okt), Some(ModType::Oktalyz));
            let loaded = load_module(&okt).expect("OKT load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            assert_eq!(loaded.state.mi.file_type, ModType::Oktalyz);
            assert_eq!(num_tracks, 4);
            assert_eq!(loaded.state.init_speed, 5);
            assert_eq!(loaded.state.ptrn_breaks[0], 3);
            assert_eq!(loaded.patterns[0][0][0].instrument, 1);
            assert_eq!(loaded.patterns[0][0][0].effekt, effect_ids::SET_SPEED);
            assert_eq!(loaded.patterns[0][0][0].operands, 4);
        }

        // 6. Mod's Grave (.WOW) 8-Kanal Test
        {
            let mut wow = vec![0u8; 1084 + 2048];
            wow[0..8].copy_from_slice(b"WOW Test");
            wow[950] = 1;
            wow[952] = 0;
            wow[1080..1084].copy_from_slice(b"M.K.");
            assert_eq!(detect_module_type(&wow), Some(ModType::WOW));
            let loaded = load_module(&wow).expect("WOW load failed");
            let num_tracks = loaded.state.mi.num_tracks;
            assert_eq!(loaded.state.mi.file_type, ModType::WOW);
            assert_eq!(num_tracks, 8);
        }
    }
}

