//! Klassischer `.MOD` / `.WOW` / `.NST` Loader (ProTracker, FastTracker, StarTrekker, NoiseTracker, Mod's Grave).
//!
//! Portiert aus `legacy_src/PLAYER/loaders.pas` (`GetInfo` Zeilen 1266–1365 & `MOD_GetFileData` Zeilen 1621–1717).

use crate::engine::tables::DEF_MOD_TUNING_NTSC;
use crate::engine::types::{
    effect_ids, ModType, Note, DEF_CH_VOL, ID_NO_MOD_FILE, MAX_M4W_CHANNELS, MAX_PATTERNS,
};

use super::{
    bitset_contains, bitset_include, load_instrument_into_sample_mem_with_arena,
    normalize_note_for_track_write, read_be_u16, sconvert_and_reduce, LoadedModule, PlayMemArena,
};

/// Prüft anhand der Dateigröße und Instrumenten-Header, ob eine `"M.K."`-Datei ein 8-Kanal
/// Mod's-Grave-`.WOW`-Modul ist (`loaders.pas` Zeilen 649 & 1266–1289).
pub(crate) fn is_wow_candidate(data: &[u8]) -> bool {
    if data.len() < 1084 || &data[1080..1084] != b"M.K." {
        return false;
    }
    let arrange_size = data[950] as usize;
    if arrange_size == 0 || arrange_size > 128 {
        return false;
    }
    let mut instr_bytes: usize = 0;
    for i in 0..31 {
        let base = 20 + i * 30;
        let slen = read_be_u16(data, base + 22).unwrap_or(0) as usize;
        let finetune = data[base + 24];
        let vol = data[base + 25];
        if finetune != 0 || vol > 64 {
            return false;
        }
        instr_bytes += slen * 2;
    }
    let mut max_pat: usize = 0;
    for i in 0..128 {
        let p = data[952 + i] as usize;
        if p < 128 && p > max_pat {
            max_pat = p;
        }
    }
    let num_pats = max_pat + 1;
    let expected_wow_len = 1084 + num_pats * 2048 + instr_bytes;
    data.len() == expected_wow_len
}

/// Analysiert die 4-Byte-Signatur an Offset `1080..1084` einer `.MOD`-Datei
/// (`loaders.pas` Zeilen 1272–1291).
///
/// Gibt `(ModType, num_tracks, is_31_instruments)` zurück.
pub fn parse_mod_signature(sig: &[u8; 4]) -> (ModType, u16, bool) {
    match sig {
        b"M.K." | b"M!K!" | b"M&K!" | b"M.K!" | b"N.T." => (ModType::ProTr, 4, true),
        b"OCTA" | b"CD81" => (ModType::ProTr, 8, true),
        b"FLT4" | b"EX04" => (ModType::StarTr, 4, true),
        b"FLT8" | b"EX08" => (ModType::StarTr, 8, true),
        _ => {
            if &sig[1..4] == b"CHN" && sig[0].is_ascii_digit() {
                return (ModType::Ftrk, (sig[0] - b'0') as u16, true);
            }
            if &sig[2..4] == b"CH" && sig[0].is_ascii_digit() && sig[1].is_ascii_digit() {
                let ch = ((sig[0] - b'0') as u16) * 10 + ((sig[1] - b'0') as u16);
                return (ModType::Ftrk, ch, true);
            }
            // Keine bekannte 31-Sample-Signatur -> 15-Sample NoiseTracker / SoundTracker MOD
            (ModType::NoiseTr, 4, false)
        }
    }
}

/// Lädt eine klassische `.MOD`-Datei (31-Instrumente ProTracker/StarTrekker/FastTracker oder
/// 15-Instrumente SoundTracker/NoiseTracker) aus einem `&[u8]`-Slice.
pub fn load_mod(data: &[u8]) -> Result<LoadedModule, u16> {
    load_mod_or_wow(data, false)
}

/// Lädt eine 8-Kanal Mod's-Grave-`.WOW`-Datei (`MI.FileType = ModType::WOW`, `MI.NumTracks = 8`)
/// exakt wie in `legacy_src/PLAYER/loaders.pas` (Zeilen 1266–1365 & 1621–1717).
pub fn load_wow(data: &[u8]) -> Result<LoadedModule, u16> {
    load_mod_or_wow(data, true)
}

fn load_mod_or_wow(data: &[u8], force_wow: bool) -> Result<LoadedModule, u16> {
    if data.len() < 600 {
        return Err(ID_NO_MOD_FILE);
    }

    let (mut file_type, num_tracks, is_31_inst) = if force_wow {
        if data.len() < 1084 {
            return Err(ID_NO_MOD_FILE);
        }
        (ModType::WOW, 8, true)
    } else if data.len() >= 1084 {
        let mut sig = [0u8; 4];
        sig.copy_from_slice(&data[1080..1084]);
        parse_mod_signature(&sig)
    } else {
        (ModType::NoiseTr, 4, false)
    };

    if !(4..=32).contains(&num_tracks) {
        return Err(ID_NO_MOD_FILE);
    }

    let num_inst_slots = if is_31_inst { 31 } else { 15 };
    let arrange_offset = if is_31_inst { 950 } else { 470 };
    let header_end = if is_31_inst { 1084 } else { 600 };

    if data.len() < header_end {
        return Err(ID_NO_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = file_type;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.mi.num_tracks = num_tracks;

    // Modulname (Offset 0..20)
    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[0..20], 20, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    // Arrangement & Songlänge (Offset 950 bzw. 470)
    let arrange_size = data[arrange_offset];
    if arrange_size == 0 || arrange_size > 128 {
        return Err(ID_NO_MOD_FILE);
    }
    state.mi.num_patterns = arrange_size as u16;
    state.init_bpm_speed = 125;
    state.init_speed = 6;
    state.speed = 6;
    state.bpm_speed = 125;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    state.p_arrangement[0..128].copy_from_slice(&data[arrange_offset + 2..arrange_offset + 130]);

    // Höchste Patternnummer (LastPattern) und DiffPatterns bestimmen (loaders.pas Zeilen 1311–1328)
    let mut last_pattern: usize = 0;
    state.mi.diff_patterns = 0;
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    for i in 0..(arrange_size as usize) {
        let pat = state.p_arrangement[i] as usize;
        if pat > last_pattern {
            last_pattern = pat;
        }
        if !bitset_contains(&state.row_played[pat], 1) {
            bitset_include(&mut state.row_played[pat], 1);
            state.mi.diff_patterns += 1;
        }
    }
    // DummyPatterns bestimmen (loaders.pas Zeilen 1323–1326)
    let mut dummy_patterns: usize = 0;
    for i in (arrange_size as usize)..128 {
        let pat = state.p_arrangement[i] as usize;
        if pat > dummy_patterns {
            dummy_patterns = pat;
        }
    }
    if dummy_patterns > last_pattern {
        dummy_patterns -= last_pattern;
    } else {
        dummy_patterns = 0;
    }
    state.mi.dummy_patterns = dummy_patterns as u16;

    if last_pattern > 127 {
        return Err(ID_NO_MOD_FILE);
    }

    // Instrumenten-Header (30 Bytes je Slot ab Offset 20, Big-Endian Wortlängen!)
    let mut instr_mem_size: usize = 0;
    state.mi.num_instruments = 0;
    for i in 0..num_inst_slots {
        let base = 20 + i * 30;
        let samplen_words = read_be_u16(data, base + 22).unwrap_or(0);
        let samptune = data[base + 24];
        let sampvol = data[base + 25];
        let sampreps_words = read_be_u16(data, base + 26).unwrap_or(0);
        let samprepl_words = read_be_u16(data, base + 28).unwrap_or(0);

        let inst = &mut state.mi.instruments[i];
        let mut inst_name = [0u8; 40];
        inst.name_len = sconvert_and_reduce(&data[base..base + 22], 22, false, &mut inst_name);
        inst.name = inst_name;

        if samplen_words > 0 {
            let iend_bytes = (samplen_words as i32) << 1;
            let loop_start_bytes = (sampreps_words as i32) << 1;
            let loop_end_bytes = loop_start_bytes + ((samprepl_words as i32) << 1);

            inst.offset = 0;
            inst.iend = iend_bytes;
            inst.loop_start = loop_start_bytes;
            inst.loop_end = loop_end_bytes;
            inst.volume = sampvol;
            inst.fine_tune = samptune & 0x0F;
            inst.bits = 8;
            inst.tuning = DEF_MOD_TUNING_NTSC;

            state.mi.num_instruments += 1;
            instr_mem_size += iend_bytes as usize;
        }
    }

    // Standard-Amiga-Panning (LRRL: FOR i:=MI.NumTracks DOWNTO 1 DO DefPanning[i]:=255*((i MOD 4)DIV 2))
    for i in 1..=(num_tracks as usize) {
        state.def_panning[i - 1] = (255 * ((i % 4) / 2)) as u8;
    }
    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [63; MAX_PATTERNS];

    // Pattern-Daten dekodieren (MOD_GetFileData, loaders.pas Zeilen 1621–1674)
    let pat_byte_size = 64 * 4 * (num_tracks as usize);
    let mut cursor = header_end;
    let num_phys_patterns = last_pattern + 1;
    module.patterns = Vec::with_capacity(num_phys_patterns);

    for pat_idx in 0..num_phys_patterns {
        if cursor + pat_byte_size > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let pat_slice = &data[cursor..cursor + pat_byte_size];
        cursor += pat_byte_size;

        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        let is_played = bitset_contains(&state.row_played[pat_idx], 1);

        if is_played {
            for row in 0..64 {
                for ch in 0..(num_tracks as usize) {
                    // Bei StarTrekker 8-Kanal (FLT8/EX08) liegen zuerst 64 Zeilen für Kanal 1..4,
                    // danach 64 Zeilen für Kanal 5..8 (loaders.pas Zeilen 1663–1669).
                    let cell_index = if file_type == ModType::StarTr && num_tracks == 8 {
                        if ch < 4 {
                            row * 4 + ch
                        } else {
                            256 + row * 4 + (ch - 4)
                        }
                    } else {
                        row * (num_tracks as usize) + ch
                    };

                    let cell_ofs = cell_index * 4;
                    let byte1 = pat_slice[cell_ofs];
                    let byte2 = pat_slice[cell_ofs + 1];
                    let byte3 = pat_slice[cell_ofs + 2];
                    let byte4 = pat_slice[cell_ofs + 3];

                    let instrument = (byte1 & 0xF0) | (byte3 >> 4);
                    // Big-Endian 12-Bit Amiga-Periode, skaliert mit Faktor 8 (SHL 3):
                    let ton = ((((byte1 & 0x0F) as u16) << 8) | (byte2 as u16)) << 3;
                    let mut effekt = byte3 & 0x0F;
                    let mut operands = byte4;

                    if operands == 0 {
                        match effekt {
                            0x00 | 0x01 | 0x02 | 0x0A => effekt = effect_ids::NO_EFFEKT,
                            0x05 | 0x06 => effekt -= 2,
                            _ => {}
                        }
                    }
                    if effekt == 0x0E && matches!(operands, 0x10 | 0x20 | 0xA0 | 0xB0) {
                        effekt = effect_ids::NO_EFFEKT;
                        operands = 0;
                    }
                    if file_type == ModType::ProTr
                        && ton != 0
                        && (ton > (856 << 3) || ton < (113 << 3))
                    {
                        file_type = ModType::Ftrk;
                        state.mi.file_type = ModType::Ftrk;
                    }

                    let mut note = Note {
                        instrument,
                        ton,
                        effekt,
                        operands,
                        chn_vol: DEF_CH_VOL,
                    };
                    normalize_note_for_track_write(&mut note, state);
                    rows[row][ch] = note;
                }
            }
        }
        module.patterns.push(rows);
    }

    // Eventuelle unreferenzierte Dummy-Patterns vor den Sample-Daten überspringen (loaders.pas Zeilen 1677–1699)
    let uni_long = (data.len() as i64) - (instr_mem_size as i64);
    let mut reached = false;
    let mut last_a_result: usize = 0;
    let mut stopped_i: usize = 0;
    for i in 1..=(state.mi.dummy_patterns as usize) {
        stopped_i = i;
        let a_result = data.len().saturating_sub(cursor).min(pat_byte_size);
        last_a_result = a_result;
        cursor += a_result;
        if a_result != pat_byte_size {
            reached = true;
            break;
        }
        let pat_buf = &data[cursor - pat_byte_size..cursor];
        for j in 0..64 {
            for k in 0..(num_tracks as usize) {
                let byte1 = pat_buf[(j * (num_tracks as usize) + k) * 4];
                if (byte1 >> 4) > 1 || (byte1 & 0x0F) > 6 {
                    reached = true;
                }
            }
        }
        if reached && (cursor as i64) <= uni_long {
            reached = false;
        }
        if reached {
            break;
        }
    }
    if reached {
        cursor = cursor.saturating_sub(last_a_result);
        state.mi.dummy_patterns = stopped_i.saturating_sub(1) as u16;
    }

    // Sample-Daten in den Sample-Speicher laden (loaders.pas Zeilen 1705–1707)
    let mut play_mem_arena = PlayMemArena::from_file_len(data.len());
    for i in 0..num_inst_slots {
        let inst = &mut state.mi.instruments[i];
        let mut iend = inst.iend;
        if iend > 0 {
            let avail = data.len().saturating_sub(cursor);
            let read_len = (iend as usize).min(avail);
            let raw_slice = &data[cursor..cursor + read_len];
            cursor += read_len;

            let mut offset = inst.offset;
            let mut loop_start = inst.loop_start;
            let mut loop_end = inst.loop_end;
            let mut volume = inst.volume;
            let bits = inst.bits;

            load_instrument_into_sample_mem_with_arena(
                &mut module.sample_mem,
                raw_slice,
                &mut offset,
                &mut iend,
                &mut loop_start,
                &mut loop_end,
                &mut volume,
                bits,
                0,
                false,
                &mut play_mem_arena,
            );

            inst.offset = offset;
            inst.iend = iend;
            inst.loop_start = loop_start;
            inst.loop_end = loop_end;
            inst.volume = volume;
        }
    }

    play_mem_arena.finalize_post_load(&mut module.sample_mem);
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

