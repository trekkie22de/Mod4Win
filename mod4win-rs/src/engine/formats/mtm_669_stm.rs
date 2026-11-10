//! MultiTracker (`.MTM`), Composer 669 / UNIS 669 (`.669`) und ScreamTracker 2 (`.STM`) Loader.
//!
//! Portiert aus `legacy_src/PLAYER/loaders.pas` (`MTM_GetFileData`, `f669_GetFileData`, `STM_GetFileData`).

use crate::engine::tables::{DEF_MOD_TUNING_NTSC, PITCH_TABLE};
use crate::engine::types::{
    effect_ids, ModType, Note, DEF_CH_VOL, ID_INV_MOD_FILE, ID_NO_MOD_FILE, MAX_M4W_CHANNELS,
    MAX_PATTERNS,
};

use super::{
    bitset_contains, bitset_include, load_instrument_into_sample_mem,
    normalize_note_for_track_write, read_le_i32, read_le_u16, sconvert_and_reduce, LoadedModule,
};

/// Lädt eine MultiTracker (`.MTM`)-Datei aus einem `&[u8]`-Slice.
pub fn load_mtm(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 66 {
        return Err(ID_NO_MOD_FILE);
    }
    if &data[0..3] != b"MTM" {
        return Err(ID_NO_MOD_FILE);
    }

    let saved_tracks = read_le_u16(data, 24).unwrap_or(0) as usize;
    let last_pattern = data[26] as usize;
    let last_to_play = data[27] as usize;
    let comment_len = read_le_u16(data, 28).unwrap_or(0) as usize;
    let num_samples = data[30] as usize;
    let attribute = data[31];
    let beat_per_trck = data[32];
    let ntracks = data[33] as usize;

    if attribute != 0 || !(1..=32).contains(&ntracks) || last_pattern > 127 {
        return Err(ID_NO_MOD_FILE);
    }
    for i in 0..32 {
        if data[34 + i] > 0x0F {
            return Err(ID_NO_MOD_FILE);
        }
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::MTM;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.init_bpm_speed = 125;
    state.bpm_speed = 125;
    state.init_speed = 6;
    state.speed = 6;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[4..24], 20, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    state.mi.num_patterns = (last_to_play + 1) as u16;
    state.mi.num_tracks = ntracks as u16;
    state.mi.comment_len = comment_len as u16;

    for i in 0..32 {
        state.def_panning[i] = data[34 + i] * 17;
    }

    let pat_break = if beat_per_trck == 0 {
        63
    } else {
        beat_per_trck.saturating_sub(1)
    };
    state.ptrn_breaks = [pat_break; MAX_PATTERNS];
    state.ptrn_tempi = [0; MAX_PATTERNS];

    // Instrumenten-Header (num_samples * 37 Bytes ab Offset 66)
    let inst_table_end = 66 + num_samples * 37;
    let arr_end = inst_table_end + 128;
    if data.len() < arr_end {
        return Err(ID_NO_MOD_FILE);
    }

    state.mi.num_instruments = 0;
    for i in 0..num_samples {
        let base = 66 + i * 37;
        let inst = &mut state.mi.instruments[i];
        let mut name_buf = [0u8; 40];
        inst.name_len = sconvert_and_reduce(&data[base..base + 22], 22, false, &mut name_buf);
        inst.name = name_buf;

        let samplen = read_le_i32(data, base + 22).unwrap_or(0);
        let sampreps = read_le_i32(data, base + 26).unwrap_or(0);
        let samprepe = read_le_i32(data, base + 30).unwrap_or(0);
        let samptune = data[base + 34];
        let sampvol = data[base + 35];
        let sampattr = data[base + 36];

        if samplen > 0 {
            inst.iend = samplen;
            inst.loop_start = sampreps;
            inst.loop_end = samprepe;
            inst.fine_tune = samptune & 0x0F;
            inst.volume = sampvol;
            inst.bits = 8 * (sampattr & 0x01) + 8;
            inst.tuning = DEF_MOD_TUNING_NTSC;
            state.mi.num_instruments += 1;
        }
    }

    // Arrangement (128 Bytes ab inst_table_end)
    state.p_arrangement[0..128].copy_from_slice(&data[inst_table_end..arr_end]);
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    state.mi.diff_patterns = 0;
    for i in 0..(state.mi.num_patterns as usize).min(128) {
        let pat = state.p_arrangement[i] as usize;
        if !bitset_contains(&state.row_played[pat], 1) {
            bitset_include(&mut state.row_played[pat], 1);
            state.mi.diff_patterns += 1;
        }
    }

    // Gespeicherte Einzel-Tracks einlesen (saved_tracks * 192 Bytes, loaders.pas Zeilen 1725–1748)
    let tracks_ofs = arr_end;
    let seq_ofs = tracks_ofs + saved_tracks * 192;
    let num_phys_patterns = last_pattern + 1;
    let comment_ofs = seq_ofs + num_phys_patterns * 64;

    if data.len() < comment_ofs {
        return Err(ID_NO_MOD_FILE);
    }

    let mut decoded_tracks: Vec<[Note; 64]> = Vec::with_capacity(saved_tracks);
    for t_idx in 0..saved_tracks {
        let t_base = tracks_ofs + t_idx * 192;
        let mut trk = [Note::default(); 64];
        for row in 0..64 {
            let cell_ofs = t_base + row * 3;
            let byte1 = data[cell_ofs];
            let byte2 = data[cell_ofs + 1];
            let byte3 = data[cell_ofs + 2];

            let instrument = ((byte1 & 0x03) << 4) | (byte2 >> 4);
            let pitch_idx = ((byte1 >> 2) as usize) + 1 + 24;
            let mut ton = PITCH_TABLE.get(pitch_idx).copied().unwrap_or(0);
            if ton == PITCH_TABLE[25] {
                ton = 0;
            }
            let mut effekt = byte2 & 0x0F;
            let mut operands = byte3;

            if effekt == effect_ids::ENHANCED
                && (0x10 + (operands >> 4)) == effect_ids::NOTE_DELAY
            {
                operands = operands.wrapping_add(1);
            }
            if effekt == 0 && operands == 0 {
                effekt = effect_ids::NO_EFFEKT;
            }

            trk[row] = Note {
                instrument,
                ton,
                effekt,
                operands,
                chn_vol: DEF_CH_VOL,
            };
        }
        decoded_tracks.push(trk);
    }

    // Track-Sequenzierung zu 64-Zeilen-Patterns zusammensetzen (loaders.pas Zeilen 1751–1757)
    module.patterns = Vec::with_capacity(num_phys_patterns);
    for pat_idx in 0..num_phys_patterns {
        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        let pat_seq_base = seq_ofs + pat_idx * 64;
        for ch in 0..ntracks {
            let trk_num = read_le_u16(data, pat_seq_base + ch * 2).unwrap_or(0) as usize;
            for row in 0..64 {
                let mut note = if trk_num >= 1 && trk_num <= saved_tracks {
                    decoded_tracks[trk_num - 1][row]
                } else {
                    Note::default()
                };
                normalize_note_for_track_write(&mut note, state);
                rows[row][ch] = note;
            }
        }
        module.patterns.push(rows);
    }

    // Kommentar überspringen und Samples einlesen (loaders.pas Zeilen 1758–1768)
    let mut cursor = (comment_ofs + comment_len).min(data.len());
    for i in 0..num_samples {
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
            let ins_type = 2 * ((bits & 0x10) >> 4) + 1;

            load_instrument_into_sample_mem(
                &mut module.sample_mem,
                raw_slice,
                &mut offset,
                &mut iend,
                &mut loop_start,
                &mut loop_end,
                &mut volume,
                bits,
                ins_type,
                false,
            );

            inst.offset = offset;
            inst.iend = iend;
            inst.loop_start = loop_start;
            inst.loop_end = loop_end;
            inst.volume = volume;
        }
    }

    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

/// Lädt eine Composer-669- oder UNIS-669 (`.669`)-Datei aus einem `&[u8]`-Slice.
pub fn load_669(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 497 {
        return Err(ID_NO_MOD_FILE);
    }
    let f669_sign = read_le_u16(data, 0).unwrap_or(0);
    let ninstruments = data[110] as usize;
    let numpatts = data[111] as usize;
    let rep_start = data[112];

    if !matches!(f669_sign, 0x6669 | 0x4E4A)
        || !(1..=64).contains(&ninstruments)
        || !(1..=128).contains(&numpatts)
    {
        return Err(ID_NO_MOD_FILE);
    }

    let file_type = if f669_sign == 0x4E4A {
        ModType::UNIS
    } else {
        ModType::F669
    };

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = file_type;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.mi.num_tracks = 8;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    let arrangement = &data[113..241];
    let tempos = &data[241..369];
    let lengths = &data[369..497];

    state.p_arrangement[0..128].copy_from_slice(arrangement);
    let last_pattern = numpatts - 1;
    state.mi.diff_patterns = 0;
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    let mut num_patterns = 0usize;

    for i in 0..128 {
        let pat = arrangement[i] as usize;
        if pat < 128 {
            num_patterns = i + 1;
        } else {
            break;
        }
        if !bitset_contains(&state.row_played[pat], 1) {
            bitset_include(&mut state.row_played[pat], 1);
            state.mi.diff_patterns += 1;
        }
    }
    if num_patterns == 0 {
        return Err(ID_NO_MOD_FILE);
    }
    state.mi.num_patterns = num_patterns as u16;

    state.init_bpm_speed = 78;
    state.bpm_speed = 78;
    let first_pat = (arrangement[0] as usize).min(127);
    state.init_speed = tempos[first_pat].max(1);
    state.speed = state.init_speed;

    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [63; MAX_PATTERNS];
    for i in 0..num_patterns {
        let pat = (arrangement[i] as usize).min(127);
        state.ptrn_tempi[i] = tempos[pat];
        state.ptrn_breaks[i] = lengths[pat].min(63);
    }

    // Modulname aus den 3 Kommentarzeilen à 36 Zeichen (Offset 2..110, loaders.pas Zeilen 738–743)
    let mut mod_name = [0u8; 40];
    let mut mod_name_len = 0u8;
    for line_idx in 0..3 {
        let start = 2 + line_idx * 36;
        mod_name_len = sconvert_and_reduce(&data[start..start + 36], 36, true, &mut mod_name);
        if mod_name_len > 0 {
            break;
        }
    }
    state.mi.mod_name = mod_name;
    state.mi.mod_name_len = mod_name_len;
    state.mi.comment_len = 108;

    let loop_to = (rep_start as i16) + 1;
    if loop_to > (num_patterns as i16) {
        state.mi.loop_to = -1;
        state.mi.loop_from = -1;
    } else {
        state.mi.loop_to = loop_to;
        state.mi.loop_from = num_patterns as i16;
    }

    // Instrumenten-Header (ninstruments * 25 Bytes ab Offset 497, loaders.pas Zeilen 754–775)
    let inst_end = 497 + ninstruments * 25;
    if data.len() < inst_end {
        return Err(ID_NO_MOD_FILE);
    }

    state.mi.num_instruments = 0;
    for i in 0..ninstruments {
        let base = 497 + i * 25;
        let inst = &mut state.mi.instruments[i];
        let mut name_buf = [0u8; 40];
        inst.name_len = sconvert_and_reduce(&data[base..base + 13], 13, false, &mut name_buf);
        inst.name = name_buf;

        let samplen = read_le_i32(data, base + 13).unwrap_or(0);
        let sampreps = read_le_i32(data, base + 17).unwrap_or(0);
        let samprepend = read_le_i32(data, base + 21).unwrap_or(0);

        if samplen > 0 {
            inst.offset = 0;
            inst.iend = samplen;
            if samprepend <= samplen {
                inst.loop_start = sampreps;
                inst.loop_end = samprepend;
            }
            inst.volume = 0x40;
            inst.fine_tune = 0;
            inst.bits = 8;
            inst.tuning = if file_type == ModType::UNIS {
                8100
            } else {
                DEF_MOD_TUNING_NTSC
            };
            state.mi.num_instruments += 1;
        }
    }

    // Panning setzen (loaders.pas Zeilen 776–786)
    for i in 0..4 {
        if file_type == ModType::UNIS {
            state.def_panning[2 * i] = 2 * 34;
            state.def_panning[2 * i + 1] = 2 * 95;
        } else {
            state.def_panning[2 * i] = 0;
            state.def_panning[2 * i + 1] = 255;
        }
    }

    // Patterns einlesen (numpatts * 1536 Bytes ab inst_end, loaders.pas Zeilen 1779–1825)
    let pat_byte_size = 64 * 8 * 3;
    let mut cursor = inst_end;
    let num_phys_patterns = last_pattern + 1;
    if cursor + num_phys_patterns * pat_byte_size > data.len() {
        return Err(ID_NO_MOD_FILE);
    }

    module.patterns = Vec::with_capacity(num_phys_patterns);
    for pat_idx in 0..num_phys_patterns {
        let pat_slice = &data[cursor..cursor + pat_byte_size];
        cursor += pat_byte_size;

        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        if bitset_contains(&state.row_played[pat_idx], 1) {
            for row in 0..64 {
                for ch in 0..8 {
                    let cell_ofs = (row * 8 + ch) * 3;
                    let byte1 = pat_slice[cell_ofs];
                    let byte2 = pat_slice[cell_ofs + 1];
                    let byte3 = pat_slice[cell_ofs + 2];

                    let (ton, instrument, chn_vol) = if byte1 != 0xFF {
                        let v = byte2 & 0x0F;
                        let vol = (v << 2) + (v >> 2) + (v >> 3);
                        if byte1 != 0xFE {
                            let pitch_idx = ((byte1 >> 2) as usize) + 1 + 24;
                            let t = PITCH_TABLE.get(pitch_idx).copied().unwrap_or(0);
                            let inst = (byte2 >> 4) + ((byte1 & 0x03) << 4) + 1;
                            (t, inst, vol)
                        } else {
                            (0, 0, vol)
                        }
                    } else {
                        (0, 0, DEF_CH_VOL)
                    };

                    let mut operands = 0u8;
                    let effekt = if byte3 != 0xFF {
                        operands = byte3 & 0x0F;
                        match byte3 >> 4 {
                            0 => effect_ids::PORTAMENTO_UP,
                            1 => effect_ids::PORTAMENTO_DOWN,
                            2 => effect_ids::TONE_PORTAMENTO,
                            3 => effect_ids::FREQU_ADJUST,
                            4 => effect_ids::VIBRATO,
                            5 => effect_ids::SET_SPEED,
                            6 => match operands {
                                0 => effect_ids::FINE_PAN_LEFT,
                                1 => effect_ids::FINE_PAN_RIGHT,
                                _ => effect_ids::NO_EFFEKT,
                            },
                            7 => effect_ids::SLOT_RETRIGGER,
                            _ => effect_ids::NO_EFFEKT,
                        }
                    } else {
                        effect_ids::NO_EFFEKT
                    };

                    let mut note = Note {
                        instrument,
                        ton,
                        effekt,
                        operands,
                        chn_vol,
                    };
                    normalize_note_for_track_write(&mut note, state);
                    rows[row][ch] = note;
                }
            }
        }
        module.patterns.push(rows);
    }

    // Sample-Daten einlesen (ins_type = 1 = 8-Bit unsigned, loaders.pas Zeilen 1829–1831)
    for i in 0..ninstruments {
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

            load_instrument_into_sample_mem(
                &mut module.sample_mem,
                raw_slice,
                &mut offset,
                &mut iend,
                &mut loop_start,
                &mut loop_end,
                &mut volume,
                bits,
                1,
                false,
            );

            inst.offset = offset;
            inst.iend = iend;
            inst.loop_start = loop_start;
            inst.loop_end = loop_end;
            inst.volume = volume;
        }
    }

    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

/// Lädt eine ScreamTracker-2 (`.STM`)-Datei aus einem `&[u8]`-Slice.
pub fn load_stm(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 1168 {
        return Err(ID_NO_MOD_FILE);
    }
    let stm_sign = &data[20..28];
    let stm_sign2 = data[28];
    let file_type_byte = data[29];
    let song_tempo = data[32];
    let num_patts = data[33] as usize;

    if !matches!(stm_sign, b"!Scream!" | b"BMOD2STM" | b"SWavePro")
        || stm_sign2 != 0x1A
        || !(1..=128).contains(&num_patts)
    {
        return Err(ID_NO_MOD_FILE);
    }
    if file_type_byte != 2 {
        return Err(ID_INV_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::STM;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.mi.num_tracks = 4;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[0..20], 20, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    state.init_speed = (song_tempo >> 4).max(1);
    state.speed = state.init_speed;
    state.init_bpm_speed = 125;
    state.bpm_speed = 125;
    state.def_panning[0..4].fill(128);
    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [63; MAX_PATTERNS];

    // Arrangement (Offset 1040..1168)
    state.p_arrangement[0..128].copy_from_slice(&data[1040..1168]);
    let mut num_patterns = 0usize;
    let mut last_pattern = 0usize;
    state.mi.diff_patterns = 0;
    state.row_played = [[0u8; 32]; MAX_PATTERNS];

    for i in 0..128 {
        let pat = state.p_arrangement[i] as usize;
        if pat < 99 {
            num_patterns += 1;
        } else {
            break;
        }
        if pat > last_pattern {
            last_pattern = pat;
        }
        if !bitset_contains(&state.row_played[pat], 1) {
            bitset_include(&mut state.row_played[pat], 1);
            state.mi.diff_patterns += 1;
        }
    }
    if num_patterns == 0 || last_pattern > 128 {
        return Err(ID_NO_MOD_FILE);
    }
    state.mi.num_patterns = num_patterns as u16;
    state.mi.dummy_patterns = (num_patts as u16).wrapping_sub(state.mi.diff_patterns);

    // Instrumente (31 Einträge à 32 Bytes ab Offset 48, loaders.pas Zeilen 1235–1255)
    let mut stm_sample_mem_ofs = [0u16; 31];
    state.mi.num_instruments = 0;
    for i in 0..31 {
        let base = 48 + i * 32;
        let inst = &mut state.mi.instruments[i];
        let mut name_buf = [0u8; 40];
        inst.name_len = sconvert_and_reduce(&data[base..base + 12], 12, false, &mut name_buf);
        inst.name = name_buf;

        let smem_ofs = read_le_u16(data, base + 14).unwrap_or(0);
        let samplen = read_le_u16(data, base + 16).unwrap_or(0);
        let sampreps = read_le_u16(data, base + 18).unwrap_or(0);
        let samprepend = read_le_u16(data, base + 20).unwrap_or(0);
        let sampvol = data[base + 22];
        let nadjust = read_le_u16(data, base + 24).unwrap_or(DEF_MOD_TUNING_NTSC);

        if smem_ofs != 0 && samplen > 0 {
            stm_sample_mem_ofs[i] = smem_ofs;
            inst.offset = 0;
            inst.iend = samplen as i32;
            if sampreps != 0 || samprepend != 0xFFFF {
                inst.loop_start = sampreps as i32;
                inst.loop_end = samprepend as i32;
            }
            inst.volume = sampvol;
            inst.fine_tune = 0;
            inst.bits = 8;
            inst.tuning = if nadjust == 0 {
                DEF_MOD_TUNING_NTSC
            } else {
                nadjust
            };
            state.mi.num_instruments += 1;
        }
    }

    // Patterns einlesen ab Offset 1168 (64 * 4 * 4 = 1024 Bytes je Pattern, loaders.pas Zeilen 2075–2128)
    let pat_byte_size = 64 * 4 * 4;
    let mut cursor = 1168usize;
    let num_phys_patterns = last_pattern + 1;
    let max_stored_pats = state.mi.dummy_patterns.wrapping_add(state.mi.diff_patterns) as usize;

    module.patterns = Vec::with_capacity(num_phys_patterns);
    for pat_idx in 0..num_phys_patterns {
        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        if pat_idx >= max_stored_pats {
            module.patterns.push(rows);
            continue;
        }
        if cursor + pat_byte_size > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let pat_slice = &data[cursor..cursor + pat_byte_size];
        cursor += pat_byte_size;

        if bitset_contains(&state.row_played[pat_idx], 1) {
            for row in 0..64 {
                for ch in 0..4 {
                    let cell_ofs = (row * 4 + ch) * 4;
                    let byte1 = pat_slice[cell_ofs];
                    let byte2 = pat_slice[cell_ofs + 1];
                    let byte3 = pat_slice[cell_ofs + 2];
                    let byte4 = pat_slice[cell_ofs + 3];

                    let ton = match byte1 {
                        0xFE => 0xFFFF,
                        0xFF => 0,
                        _ => {
                            if (byte1 >> 4) > 7 || (byte1 & 0x0F) > 11 {
                                0
                            } else {
                                let idx = ((byte1 >> 4) as usize) * 12
                                    + ((byte1 & 0x0F) as usize)
                                    + 1
                                    + 24;
                                PITCH_TABLE.get(idx).copied().unwrap_or(0)
                            }
                        }
                    };
                    let instrument = byte2 >> 3;
                    let mut chn_vol = ((byte3 & 0xF0) >> 1) + (byte2 & 0x07);
                    if chn_vol > 64 {
                        chn_vol = DEF_CH_VOL;
                    }
                    let mut operands = byte4;
                    let effekt = match byte3 & 0x0F {
                        0 => effect_ids::NO_EFFEKT,
                        1 => {
                            operands = byte4 >> 4;
                            effect_ids::SET_SPEED
                        }
                        2 => effect_ids::POSITION_JUMP,
                        3 => effect_ids::PATTERN_BREAK,
                        4 => effect_ids::VOLUME_SLIDE,
                        5 => effect_ids::PORTAMENTO_DOWN,
                        6 => effect_ids::PORTAMENTO_UP,
                        7 => effect_ids::TONE_PORTAMENTO,
                        8 => effect_ids::VIBRATO,
                        9 => effect_ids::TREMOR,
                        10 => effect_ids::ARPEGGIO,
                        _ => effect_ids::NO_EFFEKT,
                    };

                    let mut note = Note {
                        instrument,
                        ton,
                        effekt,
                        operands,
                        chn_vol,
                    };
                    normalize_note_for_track_write(&mut note, state);
                    rows[row][ch] = note;
                }
            }
        }
        module.patterns.push(rows);
    }

    // Samples einlesen (Offset = 16 * STMSampleMemOfs[i], ins_type = 0, loaders.pas Zeilen 2134–2139)
    for i in 0..31 {
        let inst = &mut state.mi.instruments[i];
        let mut iend = inst.iend;
        if iend > 0 {
            let smp_ofs = (stm_sample_mem_ofs[i] as usize) * 16;
            if smp_ofs < data.len() {
                let avail = data.len() - smp_ofs;
                let read_len = (iend as usize).min(avail);
                let raw_slice = &data[smp_ofs..smp_ofs + read_len];

                let mut offset = inst.offset;
                let mut loop_start = inst.loop_start;
                let mut loop_end = inst.loop_end;
                let mut volume = inst.volume;
                let bits = inst.bits;

                load_instrument_into_sample_mem(
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
                );

                inst.offset = offset;
                inst.iend = iend;
                inst.loop_start = loop_start;
                inst.loop_end = loop_end;
                inst.volume = volume;
            } else {
                inst.offset = 0;
                inst.iend = 0;
                inst.loop_start = 0;
                inst.loop_end = 0;
            }
        }
    }

    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

