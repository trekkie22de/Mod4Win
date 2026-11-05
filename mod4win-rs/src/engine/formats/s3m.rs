//! ScreamTracker 3 (`.S3M`) Loader.
//!
//! Portiert aus `legacy_src/PLAYER/loaders.pas` (`GetInfo` Zeilen 903–1059 & `S3M_GetFileData` Zeilen 2143–2352).

use crate::engine::tables::{DEF_MOD_TUNING_NTSC, PITCH_TABLE};
use crate::engine::types::{
    effect_ids, ModType, Note, ID_FILE_CORRUPTED, ID_INV_MOD_FILE, ID_NO_MOD_FILE,
    MAX_M4W_CHANNELS, MAX_PATTERNS,
};

use super::{
    bitset_contains, bitset_include, load_instrument_into_sample_mem,
    normalize_note_for_track_write, read_le_i32, read_le_u16, read_le_u32, sconvert_and_reduce,
    LoadedModule,
};

/// Lädt eine ScreamTracker-3 (`.S3M`)-Datei aus einem `&[u8]`-Slice.
pub fn load_s3m(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 96 {
        return Err(ID_NO_MOD_FILE);
    }
    let s3m_sign1 = data[28];
    if !matches!(s3m_sign1, 0x00 | 0x1A) || &data[44..48] != b"SCRM" {
        return Err(ID_NO_MOD_FILE);
    }

    let file_type_byte = data[29];
    let ord_num = read_le_u16(data, 32).unwrap_or(0) as usize;
    let ins_num = read_le_u16(data, 34).unwrap_or(0) as usize;
    let pat_num = read_le_u16(data, 36).unwrap_or(0) as usize;
    let flags = read_le_u16(data, 38).unwrap_or(0);
    let cwt_v = read_le_u16(data, 40).unwrap_or(0);
    let ffv = read_le_u16(data, 42).unwrap_or(0);

    if file_type_byte != 16 || !matches!(ffv, 1 | 2) {
        return Err(ID_INV_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::S3M;
    state.mi.file_siz = (data.len() / 1024) as u16;

    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[0..28], 28, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    // Kanäle & Panning auswerten (loaders.pas Zeilen 919–930)
    let mut num_tracks = 0u16;
    for i in 0..32 {
        let ch_setting = data[64 + i];
        if (ch_setting & 0x7F) < 32 {
            num_tracks += 1;
            state.def_panning[i] = match ch_setting {
                0..=7 => 0,
                8..=15 => 255,
                _ => 128,
            };
        } else {
            break;
        }
    }
    if num_tracks == 0 {
        return Err(ID_NO_MOD_FILE);
    }
    state.mi.num_tracks = num_tracks;

    if (flags & 0x40) != 0 || cwt_v == 0x1300 {
        state.fast_slides = true;
    }

    let glob_vol = data[48].min(0x40);
    let init_speed = data[49];
    let init_tempo = data[50];
    let mast_vol = data[51];
    let def_panpos = data[53];

    state.init_glob_vol = glob_vol;
    state.glob_vol = glob_vol;
    state.init_speed = if init_speed == 0 { 6 } else { init_speed };
    state.speed = state.init_speed;
    state.init_bpm_speed = if init_tempo < 32 { 125 } else { init_tempo as u16 };
    state.bpm_speed = state.init_bpm_speed;
    state.master_vol = (mast_vol & 0x7F).max(0x10);

    if (mast_vol & 0x80) == 0 {
        for i in 0..(num_tracks as usize) {
            state.def_panning[i] = 128;
        }
    }

    // Arrangement lesen (Offset 96..96 + ord_num)
    let ord_ofs = 96usize;
    let ins_ptr_ofs = ord_ofs + ord_num;
    let pat_ptr_ofs = ins_ptr_ofs + ins_num * 2;
    let pan_table_ofs = pat_ptr_ofs + pat_num * 2;

    if data.len() < pan_table_ofs {
        return Err(ID_NO_MOD_FILE);
    }

    let copy_ord = ord_num.min(MAX_PATTERNS);
    state.p_arrangement[..copy_ord].copy_from_slice(&data[ord_ofs..ord_ofs + copy_ord]);

    let mut last_pattern = 0usize;
    state.mi.diff_patterns = 0;
    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    for i in 0..copy_ord {
        let p = state.p_arrangement[i];
        if !matches!(p, 0xFE | 0xFF) {
            if (p as usize) > last_pattern {
                last_pattern = p as usize;
            }
            if !bitset_contains(&state.row_played[p as usize], 1) {
                bitset_include(&mut state.row_played[p as usize], 1);
                state.mi.diff_patterns += 1;
            }
        }
    }

    let mut eff_num_patterns = copy_ord;
    while eff_num_patterns > 0 && state.p_arrangement[eff_num_patterns - 1] == 0xFF {
        eff_num_patterns -= 1;
    }
    if eff_num_patterns == 0 {
        return Err(ID_NO_MOD_FILE);
    }
    state.mi.num_patterns = eff_num_patterns as u16;

    // Optionale Default-Panning-Tabelle bei def_panpos == 0xFC (loaders.pas Zeilen 953 & 1041–1053)
    if def_panpos == 0xFC
        && (mast_vol & 0x80) != 0
        && data.len() >= pan_table_ofs + (num_tracks as usize)
    {
        for i in 0..(num_tracks as usize) {
            let p = data[pan_table_ofs + i];
            if (p & 0x20) != 0 {
                state.def_panning[i] = (p & 0x0F) * 17;
            }
        }
    }

    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [63; MAX_PATTERNS];

    // Gepackte S3M-Patterns entpacken (S3M_GetFileData, loaders.pas Zeilen 2173–2306)
    let num_phys_patterns = (last_pattern + 1).max(pat_num).min(MAX_PATTERNS);
    module.patterns = vec![vec![[Note::default(); MAX_M4W_CHANNELS]; 64]; num_phys_patterns];

    for pat_idx in 0..=last_pattern.min(num_phys_patterns.saturating_sub(1)) {
        if !bitset_contains(&state.row_played[pat_idx], 1) || pat_idx >= pat_num {
            continue;
        }
        let parapointer = read_le_u16(data, pat_ptr_ofs + pat_idx * 2).unwrap_or(0) as usize;
        if parapointer == 0 {
            continue;
        }
        let pat_file_ofs = parapointer * 16;
        if pat_file_ofs + 2 > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let packed_len = read_le_u16(data, pat_file_ofs).unwrap_or(0) as usize;
        if pat_file_ofs + packed_len > data.len() || packed_len > 10242 {
            return Err(ID_NO_MOD_FILE);
        }
        // In loaders.pas (Zeile 2186) wird SizeOf(TS3MPatternData) = 10242 Bytes ab pat_file_ofs gelesen;
        // PackedData[1..10240] beginnt bei pat_file_ofs + 2 und wird 1-basiert mit `l` adressiert.
        let mut packed_buf = [0u8; 10248];
        let avail_after_hdr = data.len().saturating_sub(pat_file_ofs + 2).min(10240);
        packed_buf[1..1 + avail_after_hdr]
            .copy_from_slice(&data[pat_file_ofs + 2..pat_file_ofs + 2 + avail_after_hdr]);

        let mut rows = vec![[Note::default(); MAX_M4W_CHANNELS]; 64];
        let mut l = 1usize;
        let len_limit = packed_len.saturating_sub(1);
        for row in 0..64 {
            while l <= 10240 && packed_buf[l] != 0 {
                let what = packed_buf[l];
                let ch = (what & 0x1F) as usize;
                if ch >= (num_tracks as usize) {
                    if (what & 0x20) != 0 {
                        l += 2;
                    }
                    if (what & 0x40) != 0 {
                        l += 1;
                    }
                    if (what & 0x80) != 0 {
                        l += 2;
                    }
                } else {
                    let cell = &mut rows[row][ch];
                    if (what & 0x20) != 0 {
                        let note_byte = packed_buf[l + 1];
                        let inst_byte = packed_buf[l + 2];
                        l += 2;
                        cell.ton = match note_byte {
                            0x00..=0x7F => {
                                if (note_byte & 0x0F) > 11 {
                                    0
                                } else {
                                    let idx = ((note_byte >> 4) as usize) * 12
                                        + ((note_byte & 0x0F) as usize)
                                        + 1;
                                    PITCH_TABLE.get(idx).copied().unwrap_or(0)
                                }
                            }
                            0xFE => 0xFFFF,
                            _ => 0,
                        };
                        cell.instrument = inst_byte;
                    }
                    if (what & 0x40) != 0 {
                        cell.chn_vol = packed_buf[l + 1];
                        l += 1;
                    }
                    if (what & 0x80) != 0 {
                        let raw_eff = packed_buf[l + 1];
                        let mut raw_ops = packed_buf[l + 2];
                        l += 2;
                        cell.effekt = map_s3m_effect(raw_eff, &mut raw_ops);
                        cell.operands = raw_ops;
                    }
                }
                l += 1;
                if l > len_limit {
                    break;
                }
            }
            l += 1;
            if l > len_limit {
                break;
            }
        }

        // Mehrfache Pattern-Loops (SBx) auf derselben Zeile entfernen (loaders.pas Zeilen 2287–2303)
        for row in rows.iter_mut() {
            let mut max_loop_op: u8 = 0;
            for k in 0..(num_tracks as usize) {
                if row[k].effekt == effect_ids::PATTRN_LOOP && row[k].operands != 0 {
                    if row[k].operands > max_loop_op {
                        max_loop_op = row[k].operands;
                        for prev_ch in 0..k {
                            row[prev_ch].effekt = effect_ids::NO_EFFEKT;
                            row[prev_ch].operands = 0;
                        }
                    } else {
                        row[k].effekt = effect_ids::NO_EFFEKT;
                        row[k].operands = 0;
                    }
                }
            }
        }

        // TrackWrite-Normalisierung auf alle Zellen anwenden
        for row in rows.iter_mut() {
            for ch in 0..(num_tracks as usize) {
                normalize_note_for_track_write(&mut row[ch], state);
            }
        }
        module.patterns[pat_idx] = rows;
    }

    // Instrumente und Samples einlesen (loaders.pas Zeilen 984–1037 & 2321–2347)
    state.mi.num_instruments = 0;
    for i in 0..ins_num.min(255) {
        let ins_para = read_le_u16(data, ins_ptr_ofs + i * 2).unwrap_or(0) as usize;
        if ins_para == 0 {
            continue;
        }
        let ins_ofs = ins_para * 16;
        if ins_ofs + 80 > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let smp_hdr = &data[ins_ofs..ins_ofs + 80];
        let sample_type = smp_hdr[0];

        let inst = &mut state.mi.instruments[i];
        let mut name_buf = [0u8; 40];
        inst.name_len = sconvert_and_reduce(&smp_hdr[48..76], 28, false, &mut name_buf);
        inst.name = name_buf;

        if sample_type == 1 {
            // PCM Sample ('SCRS' bei Offset 76..80)
            if &smp_hdr[76..80] != b"SCRS" {
                return Err(ID_NO_MOD_FILE);
            }
            // In loaders.pas (Zeile 2335) wird ausschließlich `LongInt(S3MSample^.MemSeg.w1) SHL 4`
            // (ohne das High-Byte `MemSeg.b1`) für den Dateioffset verwendet!
            let memseg_lo = read_le_u16(smp_hdr, 14).unwrap_or(0) as usize;
            let sample_data_ofs = memseg_lo * 16;

            let length = read_le_i32(smp_hdr, 16).unwrap_or(0);
            let loop_beg = read_le_i32(smp_hdr, 20).unwrap_or(0);
            let loop_end_val = read_le_i32(smp_hdr, 24).unwrap_or(0);
            let volume = smp_hdr[28];
            let pack = smp_hdr[30];
            let smp_flags = smp_hdr[31];
            let c2_speed = read_le_u32(smp_hdr, 32).unwrap_or(8363) as u16;

            if (pack & 0x01) != 0 {
                return Err(ID_FILE_CORRUPTED);
            }

            let is_16bit = (smp_flags & 0x04) != 0;
            let is_looped = (smp_flags & 0x01) != 0;
            let bits = 8
                + 2 * (smp_flags & 0x04)
                + ((smp_flags & 0x02) << 4)
                + ((pack & 0x01) << 6);

            let mut iend_bytes = if is_16bit {
                length.saturating_mul(2)
            } else {
                length
            };
            let mut loop_start_bytes = if is_looped {
                if is_16bit {
                    loop_beg.saturating_mul(2)
                } else {
                    loop_beg
                }
            } else {
                0
            };
            let mut loop_end_bytes = if is_looped {
                if is_16bit {
                    loop_end_val.saturating_mul(2)
                } else {
                    loop_end_val
                }
            } else {
                0
            };
            let mut vol = volume;
            let mut offset = 0i32;

            if iend_bytes > 0 {
                let raw_pcm = if sample_data_ofs < data.len() {
                    let avail = data.len() - sample_data_ofs;
                    let read_len = (iend_bytes as usize).min(avail);
                    &data[sample_data_ofs..sample_data_ofs + read_len]
                } else {
                    &[]
                };
                // InsType = 2 * ((Bits & 0x10) >> 4) + (Ffv - 1)
                let ins_type = (2 * ((bits & 0x10) >> 4)) + ((ffv as u8) - 1);

                load_instrument_into_sample_mem(
                    &mut module.sample_mem,
                    raw_pcm,
                    &mut offset,
                    &mut iend_bytes,
                    &mut loop_start_bytes,
                    &mut loop_end_bytes,
                    &mut vol,
                    bits,
                    ins_type,
                    false,
                );
                state.mi.num_instruments += 1;
            }

            inst.offset = offset;
            inst.iend = iend_bytes;
            inst.loop_start = loop_start_bytes;
            inst.loop_end = loop_end_bytes;
            inst.volume = vol;
            inst.bits = bits;
            inst.tuning = if c2_speed == 0 { DEF_MOD_TUNING_NTSC } else { c2_speed };
        }
    }

    state.row_played = [[0u8; 32]; MAX_PATTERNS];
    Ok(module)
}

/// Übersetzt einen S3M-Effektbuchstaben (`1..=26` entspricht `'A'..'Z'`) in die interne Effekt-ID
/// (`loaders.pas` Zeilen 2232–2277).
fn map_s3m_effect(raw_eff: u8, operands: &mut u8) -> u8 {
    if raw_eff == 0 || raw_eff > 26 {
        return effect_ids::NO_EFFEKT;
    }
    match b'A' + (raw_eff - 1) {
        b'A' => effect_ids::SET_SPEED,
        b'B' => effect_ids::POSITION_JUMP,
        b'C' => effect_ids::PATTERN_BREAK,
        b'D' => effect_ids::VOLUME_SLIDE,
        b'E' => effect_ids::PORTAMENTO_DOWN,
        b'F' => effect_ids::PORTAMENTO_UP,
        b'G' => effect_ids::TONE_PORTAMENTO,
        b'H' => effect_ids::VIBRATO,
        b'I' => effect_ids::TREMOR,
        b'J' => effect_ids::ARPEGGIO,
        b'K' => effect_ids::VIBRA_VOLSLID,
        b'L' => effect_ids::PORT_VOLSLIDE,
        b'O' => effect_ids::PLAY_OFFSET,
        b'Q' => effect_ids::RETRIG_VOL_SLD,
        b'R' => effect_ids::TREMOLO,
        b'S' => {
            let sub = *operands >> 4;
            let eff = match sub {
                0x00 => effect_ids::SET_FILTER,
                0x01 => effect_ids::GLISSANDO_CTRL,
                0x02 => effect_ids::FINE_TUNE,
                0x03 => effect_ids::VIBRATO_WAVE,
                0x04 => effect_ids::TREMOLO_WAVE,
                0x08 => effect_ids::E_PANNING,
                0x0A => effect_ids::STEREO_CONTROL,
                0x0B => effect_ids::PATTRN_LOOP,
                0x0C => effect_ids::NOTE_CUT,
                0x0D => {
                    *operands = operands.wrapping_add(1);
                    effect_ids::NOTE_DELAY
                }
                0x0E => effect_ids::PATTERN_DELAY,
                0x0F => effect_ids::INV_LOOP,
                _ => effect_ids::NO_EFFEKT,
            };
            *operands &= 0x0F;
            eff
        }
        b'T' => effect_ids::S3M_BPM,
        b'U' => effect_ids::FINE_VIBRATO,
        b'V' => effect_ids::SET_GLOB_VOL,
        b'X' => effect_ids::PANNING,
        _ => effect_ids::NO_EFFEKT,
    }
}

