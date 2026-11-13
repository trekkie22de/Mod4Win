//! FastTracker II (`.XM`) Loader.
//!
//! Portiert aus `legacy_src/PLAYER/loaders.pas` (`GetInfo` Zeilen 1367–1537 & `FT2_GetFileData` Zeilen 2355–2461).

use crate::engine::tables::DEF_MOD_TUNING_NTSC;
use crate::engine::types::{
    effect_ids, EnvPoint, Ft2InstrumentHeader2, Ft2RuntimeInstrument, Ft2RuntimeSample,
    Ft2SampleHeader, ModType, Note, ID_INV_MOD_FILE, ID_NO_MOD_FILE, MAX_M4W_CHANNELS,
    MAX_PATTERNS, MAX_PATT_LEN,
};

use super::{
    load_instrument_into_sample_mem_with_top, normalize_note_for_track_write, read_le_i32,
    read_le_u16, read_le_u32, sconvert_and_reduce, LoadedModule, MAX_FT2_INSTS, MAX_FT2_SAMPLES,
};

/// Lädt eine FastTracker-II (`.XM`)-Datei aus einem `&[u8]`-Slice.
pub fn load_xm(data: &[u8]) -> Result<LoadedModule, u16> {
    if data.len() < 336 {
        return Err(ID_NO_MOD_FILE);
    }
    if &data[0..17] != b"Extended Module: " || data[37] != 0x1A {
        return Err(ID_NO_MOD_FILE);
    }

    let header_len = read_le_u32(data, 60).unwrap_or(0) as usize;
    let song_len = read_le_u16(data, 64).unwrap_or(0);
    let restart_pos = read_le_u16(data, 66).unwrap_or(0);
    let num_chans = read_le_u16(data, 68).unwrap_or(0);
    let num_patts = read_le_u16(data, 70).unwrap_or(0) as usize;
    let num_instrs = read_le_u16(data, 72).unwrap_or(0) as usize;
    let xm_flags = read_le_u16(data, 74).unwrap_or(0);
    let def_tempo = read_le_u16(data, 76).unwrap_or(6);
    let def_bpm = read_le_u16(data, 78).unwrap_or(125);

    if !(2..=32).contains(&num_chans) || num_patts > 256 || num_instrs > MAX_FT2_INSTS {
        return Err(ID_NO_MOD_FILE);
    }

    let mut module = LoadedModule::default();
    let state = &mut module.state;
    state.mi.file_type = ModType::FT2;
    state.mi.file_siz = (data.len() / 1024) as u16;
    state.use_linear_tune = xm_flags != 0;
    state.mi.num_patterns = song_len.min(MAX_PATTERNS as u16);
    state.mi.num_tracks = num_chans;
    state.mi.num_instruments = num_instrs as u16;
    // BIT-EXACTNESS INVARIANT (99.97% PLAYER32.DLL Parity — DO NOT "FIX"):
    // `def_tempo == 0` wird wie in `loaders.pas` Zeile 1388 bewusst nicht zu `6` konvertiert
    // (Fable-Review Pkt1-M5).
    state.init_speed = def_tempo as u8;
    state.speed = state.init_speed;
    state.init_bpm_speed = def_bpm.max(32);
    state.bpm_speed = state.init_bpm_speed;
    state.init_glob_vol = 64;
    state.glob_vol = 64;

    let mut mod_name = [0u8; 40];
    state.mi.mod_name_len = sconvert_and_reduce(&data[17..37], 20, true, &mut mod_name);
    state.mi.mod_name = mod_name;

    state.p_arrangement.copy_from_slice(&data[80..336]);

    // Restart-Position (loaders.pas Zeilen 1404–1406)
    let loop_to = (restart_pos as i16).saturating_add(1);
    if loop_to > (song_len as i16) || loop_to == 1 {
        state.mi.loop_to = -1;
        state.mi.loop_from = -1;
    } else {
        state.mi.loop_to = loop_to;
        state.mi.loop_from = song_len as i16;
    }

    state.ptrn_tempi = [0; MAX_PATTERNS];
    state.ptrn_breaks = [63; MAX_PATTERNS];
    state.def_panning = [128; MAX_M4W_CHANNELS];

    // Patterns einlesen ab Offset 60 + HeaderLen (FT2_GetFileData, loaders.pas Zeilen 2365–2437)
    let mut cursor = 60usize.checked_add(header_len).ok_or(ID_NO_MOD_FILE)?;
    let total_pats = num_patts.max(1);
    module.patterns = Vec::with_capacity(total_pats);

    for pat_idx in 0..num_patts {
        if cursor.checked_add(9).is_none_or(|e| e > data.len()) {
            return Err(ID_NO_MOD_FILE);
        }
        let pat_hdr_len = read_le_u32(data, cursor).unwrap_or(9) as usize;
        let pack_type = data[cursor + 4];
        let num_rows = read_le_u16(data, cursor + 5).unwrap_or(64) as usize;
        let packed_size = read_le_u16(data, cursor + 7).unwrap_or(0) as usize;
        if pack_type != 0 {
            return Err(ID_NO_MOD_FILE);
        }

        for j in 0..(song_len as usize).min(MAX_PATTERNS) {
            if state.p_arrangement[j] as usize == pat_idx && num_rows > 0 {
                state.ptrn_breaks[j] = (num_rows - 1) as u8;
            }
        }

        let pat_data_start = cursor.checked_add(pat_hdr_len).ok_or(ID_NO_MOD_FILE)?;
        let pat_data_end = pat_data_start.checked_add(packed_size).ok_or(ID_NO_MOD_FILE)?;
        if pat_data_end > data.len() {
            return Err(ID_NO_MOD_FILE);
        }
        let packed = &data[pat_data_start..pat_data_end];
        cursor = pat_data_end;

        let rows_len = num_rows.clamp(1, MAX_PATT_LEN);
        let default_ft2_note = Note {
            instrument: 0,
            ton: 0,
            effekt: effect_ids::NO_EFFEKT,
            operands: 0,
            chn_vol: 0,
        };
        let mut rows = vec![[default_ft2_note; MAX_M4W_CHANNELS]; rows_len];

        if packed_size > 0 {
            let mut p = 0usize;
            let mut ch = 0usize;
            let mut row = 0usize;

            while p < packed.len() && row < rows_len {
                let mut curr_flags = packed[p];
                if (curr_flags & 0x80) != 0 {
                    p += 1;
                } else {
                    curr_flags = 0xFF;
                }

                let mut ton = 0u16;
                let mut instrument = 0u8;
                let mut chn_vol = 0u8;
                let mut effekt = effect_ids::NO_EFFEKT;
                let mut operands = 0u8;

                if (curr_flags & 0x01) != 0 && p < packed.len() {
                    ton = packed[p] as u16;
                    if ton > 96 {
                        ton = 0xFFFF;
                    }
                    p += 1;
                }
                if (curr_flags & 0x02) != 0 && p < packed.len() {
                    instrument = packed[p];
                    p += 1;
                }
                if (curr_flags & 0x04) != 0 && p < packed.len() {
                    chn_vol = packed[p];
                    p += 1;
                }
                if (curr_flags & 0x08) != 0 && p < packed.len() {
                    effekt = packed[p];
                    p += 1;
                }
                if (curr_flags & 0x10) != 0 && p < packed.len() {
                    operands = packed[p];
                    p += 1;
                }

                // Effekt-Mapping für FT2 (loaders.pas Zeilen 2404–2426)
                effekt = match effekt {
                    effect_ids::NO_EFFEKT | 0 => {
                        if operands == 0 {
                            effect_ids::NO_EFFEKT
                        } else {
                            effect_ids::ARPEGGIO
                        }
                    }
                    0x01..=0x08 => effekt,
                    0x09 => {
                        if !(1..=96).contains(&ton) {
                            operands = 0;
                            effect_ids::NO_EFFEKT
                        } else {
                            effect_ids::PLAY_OFFSET
                        }
                    }
                    0x0A..=0x0F => effekt,
                    0x10 => effect_ids::SET_GLOB_VOL,
                    0x11 => effect_ids::GLOB_VOL_SLD,
                    0x14 => effect_ids::KEY_OFF,
                    0x15 => effect_ids::SET_ENVELOPE_POS,
                    0x19 => effect_ids::PAN_SLD,
                    0x1B => effect_ids::RETRIG_VOL_SLD,
                    0x1D => effect_ids::TREMOR,
                    0x21 => {
                        let sub = operands >> 4;
                        operands &= 0x0F;
                        match sub {
                            1 => effect_ids::XTRA_F_SLD_UP,
                            2 => effect_ids::XTRA_F_SLD_DWN,
                            _ => effect_ids::NO_EFFEKT,
                        }
                    }
                    _ => effect_ids::NO_EFFEKT,
                };

                if effekt == 0x0E && (0x10 + (operands >> 4)) == effect_ids::NOTE_DELAY {
                    operands = operands.wrapping_add(1);
                }

                let mut note = Note {
                    instrument,
                    ton,
                    effekt,
                    operands,
                    chn_vol,
                };
                normalize_note_for_track_write(&mut note, state);
                rows[row][ch] = note;

                ch += 1;
                if ch >= (num_chans as usize) {
                    ch = 0;
                    row += 1;
                }
            }
        }
        module.patterns.push(rows);
    }

    let default_ft2_note = Note {
        instrument: 0,
        ton: 0,
        effekt: effect_ids::NO_EFFEKT,
        operands: 0,
        chn_vol: 0,
    };
    if module.patterns.is_empty() {
        module
            .patterns
            .push(vec![[default_ft2_note; MAX_M4W_CHANNELS]; 64]);
    }

    // Instrumente und Samples einlesen (loaders.pas Zeilen 1446–1532 & 2441–2460)
    // Initiales PlayMemTop aus GetPlayMem(Longint(MI.FileSiz) * 1024) (player32.pas Zeile 551 & playdefs.pas Zeile 1394)
    let mut play_mem_top: usize = (((data.len() / 1024) * 1024 + 8192 + 65536) + 31) & !31;
    let mut effective_limit: usize = play_mem_top;
    let mut k_sample_idx: usize = 1;
    for inst_idx in 1..=num_instrs.min(MAX_FT2_INSTS) {
        if cursor + 4 > data.len() {
            break;
        }
        let inst_size = read_le_u32(data, cursor).unwrap_or(29) as usize;
        if cursor + 29 > data.len() {
            return Err(ID_NO_MOD_FILE);
        }

        let mut inst_name = [0u8; 40];
        state.mi.instruments[inst_idx - 1].name_len =
            sconvert_and_reduce(&data[cursor + 4..cursor + 26], 22, false, &mut inst_name);
        state.mi.instruments[inst_idx - 1].name = inst_name;

        let num_samples = read_le_u16(data, cursor + 27).unwrap_or(0) as usize;
        let next_header_ofs = cursor.checked_add(inst_size).ok_or(ID_NO_MOD_FILE)?;

        if num_samples == 0 {
            cursor = next_header_ofs.min(data.len());
            continue;
        }

        if cursor + 29 + 214 > data.len() {
            return Err(ID_NO_MOD_FILE);
        }

        let ext_slice = &data[cursor + 29..cursor + 29 + 214];
        let sample_header_size = read_le_i32(ext_slice, 0).unwrap_or(40).max(40) as usize;
        let mut samplenos = [0u8; 96];
        samplenos.copy_from_slice(&ext_slice[4..100]);

        let mut volenvpts = [EnvPoint::default(); 12];
        for p_idx in 0..12 {
            let o = 100 + p_idx * 4;
            volenvpts[p_idx] = EnvPoint {
                tick: read_le_u16(ext_slice, o).unwrap_or(0),
                value: read_le_u16(ext_slice, o + 2).unwrap_or(0),
            };
        }
        let mut panenvpts = [EnvPoint::default(); 12];
        for p_idx in 0..12 {
            let o = 148 + p_idx * 4;
            panenvpts[p_idx] = EnvPoint {
                tick: read_le_u16(ext_slice, o).unwrap_or(0),
                value: read_le_u16(ext_slice, o + 2).unwrap_or(0),
            };
        }

        let mut numvolpts = ext_slice[196];
        let mut numpanpts = ext_slice[197];
        let mut volsustain = ext_slice[198];
        let mut volloopst = ext_slice[199];
        let mut volloopend = ext_slice[200];
        let mut pansustain = ext_slice[201];
        let mut panloopst = ext_slice[202];
        let mut panloopend = ext_slice[203];
        let mut voltype = ext_slice[204];
        let mut pantype = ext_slice[205];
        let vibrtype = ext_slice[206];
        let vibrsweep = ext_slice[207];
        let vibrdepth = ext_slice[208];
        let vibrrate = ext_slice[209];
        let volfadeout = read_le_u16(ext_slice, 210).unwrap_or(0);

        // Hüllkurven-Anpassungen (loaders.pas Zeilen 1464–1499)
        if (voltype & 1) == 0 || numvolpts == 0 {
            voltype = 0;
            numvolpts = 0;
            volsustain = 0;
            volloopst = 0;
            volloopend = 0;
        } else {
            if (voltype & 2) == 0 {
                volsustain = 0;
            } else {
                volsustain = volsustain.wrapping_add(1);
            }
            if (voltype & 4) == 0 || volloopend < volloopst {
                volloopst = 0;
                volloopend = 0;
            } else {
                volloopst = volloopst.wrapping_add(1);
                volloopend = volloopend.wrapping_add(1);
            }
            // BIT-EXACTNESS INVARIANT (99.97% PLAYER32.DLL Parity — DO NOT "FIX"):
            // Sustain- und Loop-Punkt-Indizes (`volsustain`, `volloopst`, `volloopend` sowie
            // Pan-Pendants unten) werden wie in `loaders.pas` Zeilen 1473–1499 bewusst NICHT
            // gegen `numvolpts` / `numpanpts` geklammert (Fable-Review Pkt1-M5); `sequencer.rs`
            // sichert den Array-Zugriff per `.get()` ohne Panic ab.
            if numvolpts > 12 {
                numvolpts = 12;
            }
        }

        if (pantype & 1) == 0 || numpanpts == 0 {
            pantype = 0;
            numpanpts = 0;
            pansustain = 0;
            panloopst = 0;
            panloopend = 0;
        } else {
            if (pantype & 2) == 0 {
                pansustain = 0;
            } else {
                pansustain = pansustain.wrapping_add(1);
            }
            if (pantype & 4) == 0 || panloopend < panloopst {
                panloopst = 0;
                panloopend = 0;
            } else {
                panloopst = panloopst.wrapping_add(1);
                panloopend = panloopend.wrapping_add(1);
            }
            if numpanpts > 12 {
                numpanpts = 12;
            }
        }

        module.ft2_instruments[inst_idx] = Ft2RuntimeInstrument {
            sample_st: k_sample_idx as u16,
            num_samples: num_samples as u8,
            ext: Ft2InstrumentHeader2 {
                sampleheadersize: sample_header_size as i32,
                samplenos,
                volenvpts,
                panenvpts,
                numvolpts,
                numpanpts,
                volsustain,
                volloopst,
                volloopend,
                pansustain,
                panloopst,
                panloopend,
                voltype,
                pantype,
                vibrtype,
                vibrsweep,
                vibrdepth,
                vibrrate,
                volfadeout,
                reserved: 0,
            },
        };

        // Sample-Header lesen ab next_header_ofs
        let mut hdr_cursor = next_header_ofs;
        let first_k_for_inst = k_sample_idx;
        for _ in 0..num_samples {
            let Some(sh_end) = hdr_cursor.checked_add(40).filter(|&e| e <= data.len()) else {
                return Err(ID_INV_MOD_FILE);
            };
            if k_sample_idx > MAX_FT2_SAMPLES {
                return Err(ID_INV_MOD_FILE);
            }
            let sh = &data[hdr_cursor..sh_end];
            hdr_cursor = hdr_cursor
                .checked_add(sample_header_size.max(40))
                .ok_or(ID_INV_MOD_FILE)?;

            let samplelen = read_le_i32(sh, 0).unwrap_or(0);
            let mut loopstart = read_le_i32(sh, 4).unwrap_or(0);
            let mut loopend = read_le_i32(sh, 8).unwrap_or(0);
            let volume = sh[12];
            let mut finetune = sh[13] as i8;
            let sampletype = sh[14];
            let panning = sh[15];
            let relnote = sh[16] as i8;
            let mut sname = [0u8; 22];
            sname.copy_from_slice(&sh[18..40]);

            if !state.use_linear_tune {
                finetune = ((finetune as u8) >> 4) as i8;
            }
            loopend = loopend.wrapping_add(loopstart);
            if (sampletype & 0x03) == 0 {
                loopstart = 0;
                loopend = 0;
            }

            let mi_inst = &mut state.mi.instruments[inst_idx - 1];
            if mi_inst.iend < samplelen {
                mi_inst.iend = samplelen;
                if (sampletype & 0x10) != 0 || mi_inst.bits == 0 {
                    mi_inst.bits = if (sampletype & 0x10) != 0 { 16 } else { 8 };
                }
                mi_inst.loop_start = loopstart;
                mi_inst.loop_end = loopend;
                mi_inst.fine_tune = (finetune as u8) >> 4;
                mi_inst.volume = volume;
                mi_inst.tuning = DEF_MOD_TUNING_NTSC;
            } else if (sampletype & 0x10) != 0 && samplelen > 0 {
                mi_inst.bits = 16;
            }

            module.ft2_samples[k_sample_idx] = Ft2RuntimeSample {
                samp_offset: 0,
                hdr: Ft2SampleHeader {
                    samplelen,
                    loopstart,
                    loopend,
                    volume,
                    finetune,
                    sampletype,
                    panning,
                    relnote,
                    reserved: 0,
                    sname,
                },
            };
            k_sample_idx += 1;
        }

        // Direkt hinter den Sample-Headern folgen die Delta-kodierten Sample-Daten dieses Instruments
        let mut data_cursor = hdr_cursor;
        for s_idx in first_k_for_inst..k_sample_idx {
            let ft2_smp = &mut module.ft2_samples[s_idx];
            let mut samplelen = ft2_smp.hdr.samplelen;
            if samplelen > 0 && data_cursor < data.len() {
                let avail = data.len() - data_cursor;
                let read_len = (samplelen as usize).min(avail);
                let raw_delta = &data[data_cursor..data_cursor + read_len];
                data_cursor += read_len;

                let is_16bit = (ft2_smp.hdr.sampletype & 0x10) != 0;
                let bidi = (ft2_smp.hdr.sampletype & 0x02) != 0;
                let curr_flags = if is_16bit { 16 } else { 8 };
                let ins_type = if is_16bit { 8 } else { 7 };

                let mut samp_offset = 0i32;
                let mut loopstart = ft2_smp.hdr.loopstart;
                let mut loopend = ft2_smp.hdr.loopend;
                let mut volume = ft2_smp.hdr.volume;

                load_instrument_into_sample_mem_with_top(
                    &mut module.sample_mem,
                    raw_delta,
                    &mut samp_offset,
                    &mut samplelen,
                    &mut loopstart,
                    &mut loopend,
                    &mut volume,
                    curr_flags,
                    ins_type,
                    bidi,
                    &mut play_mem_top,
                    &mut effective_limit,
                );

                ft2_smp.samp_offset = samp_offset;
                ft2_smp.hdr.samplelen = samplelen;
                ft2_smp.hdr.loopstart = loopstart;
                ft2_smp.hdr.loopend = loopend;
                ft2_smp.hdr.volume = volume;
            } else {
                ft2_smp.samp_offset = 0;
                ft2_smp.hdr.samplelen = 0;
                ft2_smp.hdr.loopstart = 0;
                ft2_smp.hdr.loopend = 0;
            }
        }
        cursor = data_cursor;
    }

    Ok(module)
}

