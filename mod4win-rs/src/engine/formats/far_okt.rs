//! Farandole Composer (`.FAR`) & Amiga Oktalyzer (`.OKT`) Loader.
//!
//! Portiert aus `legacy_src/PLAYER/loaders.pas` (`FAR_GetFileData` & `Okt_GetFileData`).

use crate::engine::tables::{DEF_MOD_TUNING_NTSC, PITCH_TABLE};
use crate::engine::types::{
    effect_ids, ModType, Note, DEF_CH_VOL, ID_INV_MOD_FILE, ID_NO_MOD_FILE, MAX_M4W_CHANNELS,
    MAX_PATTERNS, MAX_PATT_LEN,
};

use super::{
    bitset_contains, bitset_include, load_instrument_into_sample_mem,
    normalize_note_for_track_write, read_be_u16, read_be_u32, read_le_i32, read_le_u16,
    sconvert_and_reduce, LoadedModule,
};

/// Lädt eine Farandole-Composer (`.FAR`)-Datei aus einem `&[u8]`-Slice.
pub fn load_far(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 98 {
        return Err(ID_NO_MOD_FILE);
    }
    if &data[0..4] != b"FAR\xFE" || &data[44..47] != b"\r\n\x1A" {
        return Err(ID_NO_MOD_FILE);
    }

    let header_len = read_le_u16(data, 47).unwrap_or(0) as usize;
    let version = data[49];
    if version != 0x10 {
        return Err(ID_INV_MOD_FILE);
    }

    let def_tempo = data[75];
    let comment_len = read_le_u16(data, 96).unwrap_or(0) as usize;

    let hdr2_ofs = 98 + comment_len;
    if data.len() < hdr2_ofs + 771 || data.len() < header_len {
        return Err(ID_NO_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::FFar;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.mi.num_tracks = 16;
    state.mi.comment_len = comment_len as u16;

    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[4..44], 40, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    for i in 0..16 {
        state.def_panning[i] = (data[76 + i] & 0x0F) * 17;
    }

    state.init_speed = if def_tempo == 0 { 4 } else { def_tempo };
    state.speed = state.init_speed;
    state.far_tempo = state.init_speed;
    state.init_bpm_speed = 80;
    state.bpm_speed = 80;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    // Zweiter Header (TFarHeader2: 771 Bytes ab hdr2_ofs)
    let hdr2 = &data[hdr2_ofs..hdr2_ofs + 771];
    let orders = &hdr2[0..256];
    let num_patts = hdr2[256] as usize;
    let ord_length = hdr2[257] as usize;
    let loop_to_loc = hdr2[258] as i16;

    if ord_length == 0 {
        return Err(ID_NO_MOD_FILE);
    }

    state.p_arrangement[0..ord_length].copy_from_slice(&orders[0..ord_length]);
    state.mi.num_patterns = ord_length as u16;

    let mut last_pattern = num_patts.saturating_sub(1);
    state.mi.diff_patterns = 0;
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    for i in 0..ord_length {
        let p = orders[i] as usize;
        if p > last_pattern {
            last_pattern = p;
        }
        if !bitset_contains(&state.row_played[p], 1) {
            bitset_include(&mut state.row_played[p], 1);
            state.mi.diff_patterns += 1;
        }
    }

    let loop_to = loop_to_loc + 1;
    if loop_to > (ord_length as i16) {
        state.mi.loop_to = -1;
        state.mi.loop_from = -1;
    } else {
        state.mi.loop_to = loop_to;
        state.mi.loop_from = ord_length as i16;
    }

    let mut pat_lengths = [0u16; 256];
    for i in 0..256 {
        pat_lengths[i] = read_le_u16(hdr2, 259 + i * 2).unwrap_or(0);
    }

    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [0; MAX_PATTERNS];

    // Patterns ab Offset header_len einlesen (FAR_GetFileData, loaders.pas Zeilen 1955–2046)
    let mut cursor = header_len;
    let num_phys_patterns = last_pattern + 1;
    module.patterns = Vec::with_capacity(num_phys_patterns);

    for pat_idx in 0..num_phys_patterns {
        let pat_len = pat_lengths[pat_idx] as usize;
        if pat_len > 0 {
            if pat_len > 2 + 256 * 16 * 4 || cursor + pat_len > data.len() {
                return Err(ID_NO_MOD_FILE);
            }
            let pat_slice = &data[cursor..cursor + pat_len];
            cursor += pat_len;

            if pat_len >= 2 {
                let break_loc = pat_slice[0];
                for j in 0..ord_length {
                    if state.p_arrangement[j] as usize == pat_idx {
                        state.ptrn_breaks[j] = break_loc.wrapping_add(1);
                    }
                }

                let num_rows = ((pat_len - 2) / 64).clamp(1, MAX_PATT_LEN);
                let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; num_rows];
                if bitset_contains(&state.row_played[pat_idx], 1) {
                    for row in 0..((pat_len - 2) / 64).min(num_rows) {
                        for ch in 0..16 {
                            let cell_ofs = 2 + (row * 16 + ch) * 4;
                            let b1 = pat_slice[cell_ofs];
                            let b2 = pat_slice[cell_ofs + 1];
                            let b3 = pat_slice[cell_ofs + 2];
                            let b4 = pat_slice[cell_ofs + 3];

                            let chn_vol = if b3 != 0 {
                                let v = b3 - 1;
                                (v << 2) + (v >> 2) + (v >> 3)
                            } else {
                                DEF_CH_VOL
                            };
                            let (ton, instrument) = if b1 == 0 {
                                (0, 0)
                            } else {
                                let t = PITCH_TABLE
                                    .get((b1 as usize) + 12 + 24)
                                    .copied()
                                    .unwrap_or(0);
                                (t, b2.wrapping_add(1))
                            };

                            let mut operands = b4 & 0x0F;
                            let effekt = match b4 >> 4 {
                                0x00 => {
                                    if operands == 0 {
                                        effect_ids::NO_EFFEKT
                                    } else {
                                        effect_ids::FAR_SPECIAL
                                    }
                                }
                                0x01 => effect_ids::FAR_SLD_UP,
                                0x02 => effect_ids::FAR_SLD_DWN,
                                0x03 => effect_ids::FAR_PORTAMENTO,
                                0x04 => effect_ids::FAR_RETRIG,
                                0x05 => effect_ids::VIBRATO_DEPTH,
                                0x06 => effect_ids::FAR_VIBRATO,
                                0x07 => effect_ids::FAR_VOL_UP,
                                0x08 => effect_ids::FAR_VOL_DWN,
                                0x09 => effect_ids::VIBRATO_SUST,
                                0x0A => effect_ids::VOLUME_PORT,
                                0x0B => effect_ids::E_PANNING,
                                0x0C => {
                                    operands = operands.wrapping_add(1);
                                    effect_ids::NOTE_DELAY
                                }
                                0x0D => effect_ids::FINE_TEMPO_DWN,
                                0x0E => effect_ids::FINE_TEMPO_UP,
                                0x0F => effect_ids::SET_SPEED,
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
            } else {
                module
                    .patterns
                    .push(vec![[Note::default(); MAX_M4W_CHANNELS]; 64]);
            }
        } else {
            module
                .patterns
                .push(vec![[Note::default(); MAX_M4W_CHANNELS]; 64]);
        }
    }

    // Eventuelle Dummy-Patterns nach LastPattern überspringen (loaders.pas Zeile 2049)
    for pat_idx in (last_pattern + 1)..256 {
        cursor = cursor.saturating_add(pat_lengths[pat_idx] as usize);
    }

    // Sample-Map (8 Bytes = SET OF 0..63) und Instrumente einlesen (loaders.pas Zeilen 867–893 & 2052–2063)
    if cursor + 8 > data.len() {
        return Err(ID_NO_MOD_FILE);
    }
    let sample_map = &data[cursor..cursor + 8];
    cursor += 8;

    state.mi.num_instruments = 0;
    for i in 0..64 {
        let is_present = (sample_map[i >> 3] & (1 << (i & 7))) != 0;
        if is_present {
            if cursor + 48 > data.len() {
                return Err(ID_NO_MOD_FILE);
            }
            let ih = &data[cursor..cursor + 48];
            cursor += 48;

            let inst = &mut state.mi.instruments[i];
            let mut name_buf = [0u8; 40];
            inst.name_len = sconvert_and_reduce(&ih[0..32], 32, false, &mut name_buf);
            inst.name = name_buf;

            let length = read_le_i32(ih, 32).unwrap_or(0);
            let finetune = ih[36];
            let volume = ih[37];
            let repeat_start = read_le_i32(ih, 38).unwrap_or(0);
            let repeat_end = read_le_i32(ih, 42).unwrap_or(0);
            let sample_type = ih[46];
            let loop_mode = ih[47];

            if length > 0 {
                let mut iend = length;
                let mut loop_start = if (loop_mode & (1 << 3)) != 0 {
                    repeat_start
                } else {
                    0
                };
                let mut loop_end = if (loop_mode & (1 << 3)) != 0 {
                    repeat_end
                } else {
                    0
                };
                let mut vol = volume;
                let bits = if (sample_type & 1) != 0 { 16 } else { 8 };
                let ins_type = 2 * ((bits & 0x10) >> 4);
                let mut offset = 0i32;

                let avail = data.len().saturating_sub(cursor);
                let read_len = (length as usize).min(avail);
                let raw_slice = &data[cursor..cursor + read_len];
                cursor += read_len;

                load_instrument_into_sample_mem(
                    &mut module.sample_mem,
                    raw_slice,
                    &mut offset,
                    &mut iend,
                    &mut loop_start,
                    &mut loop_end,
                    &mut vol,
                    bits,
                    ins_type,
                    false,
                );

                inst.offset = offset;
                inst.iend = iend;
                inst.loop_start = loop_start;
                inst.loop_end = loop_end;
                inst.fine_tune = finetune;
                inst.volume = vol;
                inst.bits = bits;
                inst.tuning = DEF_MOD_TUNING_NTSC;
                state.mi.num_instruments += 1;
            }
        }
    }

    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

/// Lädt eine Amiga-Oktalyzer (`.OKT`)-Datei aus einem `&[u8]`-Slice.
pub fn load_okt(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 16 || &data[0..8] != b"OKTASONG" {
        return Err(ID_NO_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::Oktalyz;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.fast_slides = true;
    state.init_speed = 6;
    state.speed = 6;
    state.init_bpm_speed = 125;
    state.bpm_speed = 125;
    state.init_glob_vol = 64;
    state.glob_vol = 64;
    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [0; MAX_PATTERNS];

    let mut cursor = 8usize;
    let mut one_8 = false;
    let mut last_pattern = 0usize;
    let mut num_inst_slots = 0usize;

    // 1. Header-Chunks lesen bis zum ersten "PBOD"-Chunk (loaders.pas Zeilen 671–684)
    loop {
        if cursor + 8 > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let b4sign = &data[cursor..cursor + 4];
        let struct_size = read_be_u32(data, cursor + 4).unwrap_or(0) as usize;
        if b4sign == b"PBOD" {
            break;
        }
        cursor += 8;
        if cursor + struct_size > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let chunk = &data[cursor..cursor + struct_size];
        cursor += struct_size;

        match b4sign {
            b"CMOD" => {
                // GetChannels (loaders.pas Zeilen 477–505)
                if struct_size != 8 {
                    return Err(ID_NO_MOD_FILE);
                }
                let mut num_tracks = 0usize;
                for i in 0..4 {
                    let t_byte = chunk[i * 2 + 1];
                    let pan = if matches!(i, 0 | 3) { 0u8 } else { 255u8 };
                    match t_byte {
                        0 => {
                            num_tracks += 1;
                            state.def_panning[num_tracks - 1] = pan;
                            state.related_chn[num_tracks - 1] = num_tracks as u8;
                        }
                        1 => {
                            num_tracks += 2;
                            state.def_panning[num_tracks - 2] = pan;
                            state.def_panning[num_tracks - 1] = pan;
                            state.related_chn[num_tracks - 2] = num_tracks as u8;
                            state.related_chn[num_tracks - 1] = (num_tracks - 1) as u8;
                        }
                        _ => return Err(ID_NO_MOD_FILE),
                    }
                }
                state.mi.num_tracks = num_tracks as u16;
            }
            b"SAMP" => {
                // GetInstrData (loaders.pas Zeilen 508–547)
                if struct_size % 32 != 0 {
                    return Err(ID_NO_MOD_FILE);
                }
                one_8 = false;
                state.mi.num_instruments = 0;
                num_inst_slots = (struct_size / 32).min(256);
                for i in 0..num_inst_slots {
                    let base = i * 32;
                    let inst = &mut state.mi.instruments[i];
                    let mut name_buf = [0u8; 40];
                    inst.name_len =
                        sconvert_and_reduce(&chunk[base..base + 20], 20, false, &mut name_buf);
                    inst.name = name_buf;

                    let samplen = read_be_u32(chunk, base + 20).unwrap_or(0) as i32;
                    if samplen > 0 {
                        let sampreps = (read_be_u16(chunk, base + 24).unwrap_or(0) as i32) << 1;
                        let samprepl = (read_be_u16(chunk, base + 26).unwrap_or(0) as i32) << 1;
                        let defvol = chunk[base + 29];
                        // In Pascal auf Little-Endian x86 entspricht Hi(Sampres) dem Byte an Offset 31:
                        let sampres_hi = chunk[base + 31];

                        inst.offset = 0;
                        inst.iend = samplen;
                        if samprepl <= samplen {
                            inst.loop_start = sampreps;
                            inst.loop_end = sampreps + samprepl;
                        }
                        if inst.loop_end > samplen {
                            inst.loop_end = samplen - inst.loop_start;
                        }
                        inst.volume = defvol;
                        inst.fine_tune = 0;
                        if sampres_hi == 1 {
                            inst.bits = 8;
                            one_8 = true;
                        } else {
                            inst.bits = 7;
                        }
                        inst.tuning = DEF_MOD_TUNING_NTSC;
                        state.mi.num_instruments += 1;
                    }
                }
            }
            b"SPEE" => {
                // GetInitSpeed (loaders.pas Zeilen 549–558)
                if struct_size != 2 {
                    return Err(ID_NO_MOD_FILE);
                }
                let spd = if chunk[1] == 0 { 6 } else { chunk[1] };
                state.init_speed = spd;
                state.speed = spd;
                state.init_bpm_speed = 125;
                state.bpm_speed = 125;
            }
            b"SLEN" => {
                // GetSLen (loaders.pas Zeilen 560–566)
                if struct_size != 2 {
                    return Err(ID_NO_MOD_FILE);
                }
                last_pattern = read_be_u16(chunk, 0).unwrap_or(0) as usize;
            }
            b"PLEN" => {
                // GetArrangeSize (loaders.pas Zeilen 568–574)
                if struct_size != 2 {
                    return Err(ID_NO_MOD_FILE);
                }
                state.mi.num_patterns = read_be_u16(chunk, 0).unwrap_or(0);
            }
            b"PATT" => {
                // GetArrangeMent (loaders.pas Zeilen 576–596)
                if struct_size > 256 {
                    return Err(ID_NO_MOD_FILE);
                }
                state.p_arrangement[0..struct_size].copy_from_slice(chunk);
                state.ptrn_tempi = [0; MAX_PATTERNS];
                state.ptrn_breaks = [63; MAX_PATTERNS];
                state.row_played = [[0u8; 32]; MAX_PATTERNS];
                state.mi.diff_patterns = 0;
                for i in 0..(state.mi.num_patterns as usize).min(MAX_PATTERNS) {
                    let p = state.p_arrangement[i] as usize;
                    if p > last_pattern {
                        last_pattern = p;
                    }
                    if !bitset_contains(&state.row_played[p], 1) {
                        bitset_include(&mut state.row_played[p], 1);
                        state.mi.diff_patterns += 1;
                    }
                }
            }
            _ => return Err(ID_NO_MOD_FILE),
        }
    }

    if state.mi.num_tracks == 0 || state.mi.num_patterns == 0 {
        return Err(ID_NO_MOD_FILE);
    }

    // 2. Pattern-Bodies ("PBOD") für 0..=last_pattern einlesen (okt_getfiledata, loaders.pas Zeilen 1844–1906)
    let num_tracks = state.mi.num_tracks as usize;
    let num_phys_patterns = last_pattern + 1;
    module.patterns = Vec::with_capacity(num_phys_patterns);

    for pat_idx in 0..num_phys_patterns {
        if cursor + 8 > data.len() {
            break;
        }
        let b4sign = &data[cursor..cursor + 4];
        if b4sign == b"SBOD" {
            break;
        }
        if b4sign != b"PBOD" {
            return Err(ID_NO_MOD_FILE);
        }
        let struct_size = read_be_u32(data, cursor + 4).unwrap_or(0) as usize;
        cursor += 8;
        if struct_size < 2 || cursor + struct_size > data.len() {
            return Err(ID_NO_MOD_FILE);
        }

        let patt_len = (read_be_u16(data, cursor).unwrap_or(64) as usize).clamp(1, MAX_PATT_LEN);
        let pat_slice = &data[cursor + 2..cursor + struct_size];
        cursor += struct_size;

        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; patt_len];
        if pat_idx < MAX_PATTERNS && bitset_contains(&state.row_played[pat_idx], 1) {
            for j in 0..(state.mi.num_patterns as usize).min(MAX_PATTERNS) {
                if state.p_arrangement[j] as usize == pat_idx {
                    state.ptrn_breaks[j] = (patt_len - 1) as u8;
                }
            }

            for row in 0..patt_len {
                for ch in 0..num_tracks {
                    let cell_ofs = (row * num_tracks + ch) * 4;
                    if cell_ofs + 4 > pat_slice.len() {
                        break;
                    }
                    let byte1 = pat_slice[cell_ofs];
                    let byte2 = pat_slice[cell_ofs + 1];
                    let byte3 = pat_slice[cell_ofs + 2];
                    let byte4 = pat_slice[cell_ofs + 3];

                    let mut operands = byte4;
                    let effekt = match byte3 {
                        0x01 => effect_ids::PORTAMENTO_UP,
                        0x02 => effect_ids::PORTAMENTO_DOWN,
                        0x0A => effect_ids::OKT_ARPG2,
                        0x0B => effect_ids::OKT_ARPG1,
                        0x0C => effect_ids::ARPEGGIO,
                        0x0D => effect_ids::OKT_SLD_DWN,
                        0x0F => effect_ids::SET_FILTER,
                        0x11 => effect_ids::OKT_SLD1_UP,
                        0x15 => effect_ids::OKT_SLD1_DWN,
                        0x18 => effect_ids::OKT_OLD_VOL,
                        0x19 => {
                            operands = (operands & 0x0F)
                                .wrapping_add((operands >> 4).wrapping_mul(10))
                                .wrapping_add(1);
                            effect_ids::POSITION_JUMP
                        }
                        0x1B => effect_ids::RETRIG_NOTE,
                        0x1C => effect_ids::SET_SPEED,
                        0x1E => effect_ids::OKT_SLD_UP,
                        0x1F => {
                            if operands <= 0x40 {
                                effect_ids::SET_VOLUME
                            } else if operands < 0x50 {
                                operands -= 0x40;
                                effect_ids::VOLUME_SLIDE
                            } else if operands < 0x60 {
                                operands = (operands - 0x50) << 4;
                                effect_ids::VOLUME_SLIDE
                            } else if operands < 0x70 {
                                operands -= 0x60;
                                effect_ids::FINE_VOL_DWN
                            } else if operands < 0x80 {
                                operands -= 0x70;
                                effect_ids::FINE_VOL_UP
                            } else {
                                effect_ids::NO_EFFEKT
                            }
                        }
                        _ => effect_ids::NO_EFFEKT,
                    };

                    let (ton, instrument) = if byte1 != 0 {
                        let t = PITCH_TABLE
                            .get((byte1 as usize) + 12 + 24)
                            .copied()
                            .unwrap_or(0);
                        (t, byte2.wrapping_add(1))
                    } else {
                        (0, 0)
                    };

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

    // Eventuelle verbleibende Dummy-"PBOD"-Chunks bis zum ersten "SBOD"-Chunk überspringen (loaders.pas Zeilen 1909–1916)
    while cursor + 8 <= data.len() {
        let b4sign = &data[cursor..cursor + 4];
        let struct_size = read_be_u32(data, cursor + 4).unwrap_or(0) as usize;
        if b4sign == b"PBOD" {
            cursor = (cursor + 8 + struct_size).min(data.len());
        } else if b4sign == b"SBOD" {
            break;
        } else {
            return Err(ID_NO_MOD_FILE);
        }
    }

    // 3. Sample-Bodies ("SBOD") einlesen (loaders.pas Zeilen 1918–1936)
    for i in 0..num_inst_slots {
        if state.mi.instruments[i].iend > 0 {
            if cursor + 8 > data.len() || &data[cursor..cursor + 4] != b"SBOD" {
                for rem in i..num_inst_slots {
                    let rem_inst = &mut state.mi.instruments[rem];
                    rem_inst.offset = 0;
                    rem_inst.iend = 0;
                    rem_inst.loop_start = 0;
                    rem_inst.loop_end = 0;
                }
                break;
            }
            let struct_size = read_be_u32(data, cursor + 4).unwrap_or(0) as usize;
            cursor += 8;

            let avail = data.len().saturating_sub(cursor);
            let read_len = struct_size.min(avail);
            let raw_slice = &data[cursor..cursor + read_len];
            cursor += read_len;

            let inst = &mut state.mi.instruments[i];
            let mut iend = read_len as i32;
            let mut offset = inst.offset;
            let mut loop_start = inst.loop_start;
            let mut loop_end = inst.loop_end;
            let mut volume = inst.volume;
            let bits = inst.bits;
            let ins_type = if (bits & 0x1F) == 7 && !one_8 { 4 } else { 0 };

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

