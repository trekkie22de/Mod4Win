//! Prozeduraler Setup-Dialog (`dsetup.pas`, `525 x 336`), Direct-to-Disk `.WAV`-Abfrage (`TAskWin`)
//! und Datei-/Verzeichnis-Auswahldialog (`fsel_*` in `MULBOX.PAS`, `195 x 220`).

use std::path::PathBuf;
use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{
    Mod4WinApp, PressedButton, UiSharedState, WindowKind, ASK_WINDOW_WIDTH, FSEL_WINDOW_WIDTH,
    SETUP_WINDOW_WIDTH,
};
use super::archive_mol::list_available_drives;
use super::i18n::LocaleCatalog;
use super::ini_config::{DEV_HDD, DEV_SOUNDCARD};
use super::theme_modern::SkinMode;
use super::widgets::{
    draw_3d_frame_only, draw_3d_panel, draw_bmp_button_2state, draw_bmp_lcd_number,
    draw_bmp_text_16x20_centered, draw_frame_3d_1px, draw_lrswitch, draw_oem_text,
    draw_oem_text_centered, draw_oem_text_mnemonic, draw_rect_outline, draw_up_down_btn,
    draw_vswitch, fill_rect,
};

/// Fokussiertes Eingabefeld im Setup-Dialog (`RecPathEdit` oder `MixPathEdit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupEditFocus {
    None,
    RecPath,
    MixPath,
}

/// Fokussiertes Eingabefeld im `TAskWin`-Dialog (`EPath` oder `EName`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavAskEditFocus {
    Path,
    Filename,
}

/// Zustand des `TAskWin`-Dialogs (`MULBOX.PAS` Zeilen 451–640) für Direct-to-Disk `.WAV`-Aufnahmen.
#[derive(Clone, Debug)]
pub struct WavAskState {
    pub path: String,
    pub filename: String,
    pub focus: WavAskEditFocus,
}

impl Default for WavAskState {
    fn default() -> Self {
        Self {
            path: ".".to_string(),
            filename: "MODULE".to_string(),
            focus: WavAskEditFocus::Filename,
        }
    }
}

/// Ziel eines `fsel_*` Datei-/Verzeichnis-Auswahldialogs (`T_Browse` / `T_DirBrowse` in `MULBOX.PAS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSelectTarget {
    SetupRecDir,
    SetupMixerExe,
    AskWavDir,
    ArchiveExe(u8),
    PlaylistAdd,
}


/// Eintrag in der Liste des `fsel_*` Auswahl-Dialogs (`MULBOX.PAS`).
#[derive(Clone, Debug)]
pub enum FileSelectEntry {
    Parent(PathBuf),
    Subdir(String, PathBuf),
    File(String, PathBuf),
    Drive(char),
}

/// Zustand des `tbrowse` (`Dateiauswahl`, `316 x 268`) bzw. `tDirbrowse` (`Verzeichniswahl`, `300 x 210`) Dialogs (`newopen.pas`).
#[derive(Clone, Debug)]
pub struct FileSelectState {
    pub target: FileSelectTarget,
    pub current_dir: PathBuf,
    pub entries: Vec<FileSelectEntry>,
    pub scroll: usize,
    pub cursor: usize,
    pub files: Vec<(String, PathBuf)>,
    pub file_scroll: usize,
    pub file_cursor: usize,
    pub active_list: u8,
}

impl FileSelectState {
    pub fn is_file_browse(&self) -> bool {
        matches!(
            self.target,
            FileSelectTarget::SetupMixerExe
                | FileSelectTarget::ArchiveExe(_)
                | FileSelectTarget::PlaylistAdd
        )
    }

    pub fn new(target: FileSelectTarget, start_path: &str) -> Self {
        let mut st = Self::new_lazy(target, start_path);
        st.refresh();
        st
    }

    pub fn new_lazy(target: FileSelectTarget, start_path: &str) -> Self {
        let p = PathBuf::from(start_path.trim());
        let dir = if !p.as_os_str().is_empty() {
            p
        } else {
            PathBuf::from(".")
        };
        Self {
            target,
            current_dir: dir,
            entries: Vec::new(),
            scroll: 0,
            cursor: 0,
            files: Vec::new(),
            file_scroll: 0,
            file_cursor: 0,
            active_list: 1,
        }
    }

    pub fn refresh(&mut self) {
        if !self.current_dir.is_dir() {
            if let Some(parent) = self.current_dir.parent() {
                if parent.is_dir() {
                    self.current_dir = parent.to_path_buf();
                } else {
                    self.current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                }
            } else {
                self.current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            }
        }
        self.entries.clear();
        self.files.clear();
        self.scroll = 0;
        self.cursor = 0;
        self.file_scroll = 0;
        self.file_cursor = 0;
        if let Some(parent) = self.current_dir.parent()
            && parent != self.current_dir {
                self.entries.push(FileSelectEntry::Parent(parent.to_path_buf()));
            }
        let mut subdirs = Vec::new();
        let mut files = Vec::new();
        let wants_exe = matches!(
            self.target,
            FileSelectTarget::SetupMixerExe | FileSelectTarget::ArchiveExe(_)
        );
        let wants_tracker = matches!(self.target, FileSelectTarget::PlaylistAdd);
        if let Ok(rd) = std::fs::read_dir(&self.current_dir) {
            for e in rd.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    subdirs.push((name.to_lowercase(), format!("<{}>", name.to_lowercase()), path));
                } else if wants_exe && path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if matches!(ext.as_str(), "exe" | "com" | "bat" | "cmd") {
                        files.push((name.to_lowercase(), path));
                    }
                } else if wants_tracker && path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if UiSharedState::is_tracker_file(&path)
                        || matches!(ext.as_str(), "zip" | "arj" | "lha" | "lzh" | "mol")
                    {
                        files.push((name.to_lowercase(), path));
                    }
                }
            }
        }
        subdirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, label, p) in subdirs {
            self.entries.push(FileSelectEntry::Subdir(label, p));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        self.files = files;
        for drv in list_available_drives() {
            self.entries.push(FileSelectEntry::Drive(drv));
        }
    }
}

impl Mod4WinApp {
    /// Rendert den prozeduralen Setup-Dialog exakt nach `dsetup.pas` (`525 x 336`, KEIN Handbuch-Bild `bm4.bmp`!).
    pub(crate) fn render_setup_dialog_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let cap_dy: i32 = 18;
        let cfg = &self.setup_cfg;
        let loc = &self.locale;

        // 1. Äußerer Rahmen + Titelleiste (v2.40) + Hauptrahmen (0, CapDY, 525, 318)
        draw_3d_panel(frame, dw, dh, 0, 0, dw, dh, false, [192, 192, 192, 255]);
        for y in 1..18 {
            for x in 1..(dw as i32 - 1) {
                let t = x as u32;
                let w_span = (dw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let idx = ((y as u32 * dw + x as u32) * 4) as usize;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 255;
                }
            }
        }
        fill_rect(frame, dw, dh, 1, 18, dw.saturating_sub(2), 1, [0, 0, 0, 255]);
        draw_oem_text(
            frame,
            dw,
            dh,
            22,
            4,
            loc.tr_or("win", "setup_title", "MOD4WIN 3.00 Setup"),
            [48, 56, 68, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);
        draw_3d_panel(
            frame,
            dw,
            dh,
            0,
            cap_dy,
            dw,
            dh.saturating_sub(cap_dy as u32),
            false,
            [192, 192, 192, 255],
        );

        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];
        let col_yellow = [255, 255, 0, 255];

        // 2. Schwarze Gruppenrahmen in der oberen Sektion (`dsetup.pas::wmEraseBkGnd`)
        draw_rect_outline(frame, dw, dh, 7, cap_dy + 43, 189, 41, col_black); // Sampling
        draw_rect_outline(frame, dw, dh, 200, cap_dy + 43, 61, 41, col_black); // Buffers
        draw_rect_outline(frame, dw, dh, 265, cap_dy + 43, 77, 41, col_black); // min. Memory
        draw_rect_outline(frame, dw, dh, 346, cap_dy + 43, 101, 41, col_black); // IDO / PAN / Clip
        draw_rect_outline(frame, dw, dh, 451, cap_dy + 43, 66, 41, col_black); // Load State

        // 3. Erhabene 3D-Gruppenrahmen für Mod4Win-Optionen, Rec. Path und Mixer Path (`Frame3D(..., 1, up)`)
        draw_frame_3d_1px(frame, dw, dh, 7, cap_dy + 90, 510, 102, false);
        draw_frame_3d_1px(frame, dw, dh, 7, cap_dy + 200, 510, 48, false);
        draw_frame_3d_1px(frame, dw, dh, 7, cap_dy + 256, 510, 29, false);

        // 4. Alle 22 Original-Beschriftungs-Sprites aus `SGRAYPIECES.bmp` (`coords[1..22]` in `dsetup.pas` Zeilen 7-15)
        const SGRAY_COORDS: [(u32, u32, u32, u32, i32, i32); 22] = [
            (0, 0, 46, 11, 32, 39),     //  1: Sampling
            (44, 0, 36, 11, 210, 39),   //  2: Buffers
            (78, 0, 62, 11, 273, 39),   //  3: min. Memory
            (138, 0, 21, 9, 350, 35),   //  4: IDO
            (183, 11, 13, 11, 371, 47), //  5: Off
            (197, 11, 13, 9, 371, 59),  //  6: Lin
            (211, 11, 18, 9, 371, 71),  //  7: CUB
            (157, 0, 22, 11, 397, 39),  //  8: PAN
            (177, 0, 20, 11, 422, 39),  //  9: Clip
            (195, 0, 55, 11, 456, 39),  // 10: Load State
            (155, 11, 12, 9, 327, 62),  // 11: KB
            (1, 11, 18, 9, 66, 62),     // 12: kHz
            (20, 11, 19, 9, 116, 55),   // 13: 8 bit
            (40, 11, 25, 9, 110, 69),   // 14: 16 bit
            (66, 11, 26, 9, 162, 47),   // 15: Mono
            (93, 11, 30, 9, 162, 59),   // 16: Stereo
            (124, 11, 30, 11, 162, 71), // 17: Surrnd
            (0, 22, 53, 11, 30, 85),    // 18: Mod4Win
            (104, 22, 107, 11, 30, 195), // 19: Rec. Path
            (50, 22, 57, 11, 30, 251),  // 20: Mixer Path
            (169, 11, 13, 11, 490, 69), // 21: On
            (183, 11, 13, 11, 490, 55), // 22: Off
        ];
        for &(sx, sy, sw, sh, dx, dy) in &SGRAY_COORDS {
            self.assets
                .sgraypieces
                .blit_rect_to(sx, sy, sw, sh, frame, dw, dh, dx, cap_dy + dy);
        }

        // 5. Sound-Ausgabegerät (`devname` bei (10, CapDY+10), 30 Zeichen in `FONT_16X20.bmp` + `bName` bei (498, CapDY+8))
        draw_3d_panel(frame, dw, dh, 7, cap_dy + 7, 486, 27, true, col_black);
        let dev_title = if cfg.device_id == DEV_HDD {
            loc.tr_or("setup", "direct_to_disk", "DIRECT TO DISK RECORDING")
        } else {
            cfg.device_name.as_str()
        };
        draw_bmp_text_16x20_centered(
            frame,
            dw,
            dh,
            10,
            cap_dy + 10,
            30,
            dev_title,
            &self.assets.font_16x20,
        );
        draw_up_down_btn(
            frame,
            dw,
            dh,
            498,
            cap_dy + 8,
            self.pressed_btn == Some(PressedButton::SetupDevUp),
            self.pressed_btn == Some(PressedButton::SetupDevDown),
            &self.assets.b_up_down_1,
        );

        // 6. Sampling-Parameter: Samplerate (`sr` + `bSR`), Bits (`bBits`), Mode (`bMS`), Buffers (`nBlocks` + `bnBlocks`), Memory (`mem`)
        draw_3d_panel(frame, dw, dh, 10, cap_dy + 53, 32, 27, true, col_black);
        let sr_khz = (cfg.sample_rate / 1000).clamp(1, 96);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            13,
            cap_dy + 56,
            sr_khz,
            2,
            false,
            &self.assets.lcd_13x20,
            13,
            20,
        );
        draw_up_down_btn(
            frame,
            dw,
            dh,
            45,
            cap_dy + 53,
            self.pressed_btn == Some(PressedButton::SetupSrUp),
            self.pressed_btn == Some(PressedButton::SetupSrDown),
            &self.assets.b_up_down_1,
        );

        // bBits (1 = 8 bit, 2 = 16 bit)
        let bits_pos = if cfg.bits_per_sample == 8 { 1 } else { 2 };
        draw_vswitch(
            frame,
            dw,
            dh,
            89,
            cap_dy + 55,
            2,
            bits_pos,
            &self.assets.switch_bmp,
        );

        // bMS (1 = Mono, 2 = Stereo, 3 = Surround)
        let ms_pos = if cfg.channels <= 1 {
            1
        } else if cfg.surround {
            3
        } else {
            2
        };
        draw_vswitch(
            frame,
            dw,
            dh,
            141,
            cap_dy + 47,
            3,
            ms_pos,
            &self.assets.switch_bmp,
        );

        // nBlocks (Buffers) + bnBlocks
        draw_3d_panel(frame, dw, dh, 203, cap_dy + 53, 32, 27, true, col_black);
        let num_bufs = if cfg.device_id == DEV_HDD {
            cfg.hdd_rec_buffers.clamp(1, 99) as u32
        } else {
            cfg.output_buffers.clamp(2, 99) as u32
        };
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            206,
            cap_dy + 56,
            num_bufs,
            2,
            false,
            &self.assets.lcd_13x20,
            13,
            20,
        );
        draw_up_down_btn(
            frame,
            dw,
            dh,
            238,
            cap_dy + 53,
            self.pressed_btn == Some(PressedButton::SetupBufUp),
            self.pressed_btn == Some(PressedButton::SetupBufDown),
            &self.assets.b_up_down_1,
        );

        // mem (min. Memory in KB, 4-stellig)
        draw_3d_panel(frame, dw, dh, 268, cap_dy + 53, 58, 27, true, col_black);
        let mem_kb = cfg.calc_min_memory_kb().min(9999);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            271,
            cap_dy + 56,
            mem_kb,
            4,
            false,
            &self.assets.lcd_13x20,
            13,
            20,
        );

        // 7. Obere Schiebeschalter rechts: bIDO (351, CapDY+47), bPAN (398, CapDY+55), bPAmp (423, CapDY+55), bloadstate (468, CapDY+55)
        // Im Setup gibt es optisch NUR die 3 originalen Beschriftungen Off (1), Lin (2) und Cub (3 für OverSamp 2 oder 3).
        let ido_pos = (cfg.over_sampling.min(2) as u32) + 1;
        draw_vswitch(
            frame,
            dw,
            dh,
            351,
            cap_dy + 47,
            3,
            ido_pos,
            &self.assets.switch_bmp,
        );
        let pan_pos = if cfg.enable_panning { 2 } else { 1 };
        draw_vswitch(
            frame,
            dw,
            dh,
            398,
            cap_dy + 55,
            2,
            pan_pos,
            &self.assets.switch_bmp,
        );
        let clip_pos = match (cfg.clipping, cfg.declick) {
            (false, _) => 1,
            (true, false) => 2,
            (true, true) => 3,
        };
        draw_vswitch(
            frame,
            dw,
            dh,
            423,
            cap_dy + 47,
            3,
            clip_pos,
            &self.assets.switch_bmp,
        );
        let load_pos = if cfg.state_saver { 2 } else { 1 };
        draw_vswitch(
            frame,
            dw,
            dh,
            468,
            cap_dy + 55,
            2,
            load_pos,
            &self.assets.switch_bmp,
        );

        // 8. Mittlere 5 Einstellungs-Zeilen (`comments[1..5]`, `whatsel[1..5]` und horizontale `LRSWITCH.bmp`)
        let is_de = loc.is_german() || state.ini_config.language == 1;
        let on_str = if is_de {
            "++++ EIN ++++".to_string()
        } else {
            "++++ ON ++++".to_string()
        };
        let off_str = if is_de {
            "---- AUS ----".to_string()
        } else {
            "---- OFF ----".to_string()
        };
        let row_labels: [(&str, String, u32, u32, i32); 5] = [
            (
                loc.tr_or("setup", "row1_comment", "Engine mode (Clean / Full Emulation)"),
                if cfg.clean_mode {
                    "-Clean Mode-".to_string()
                } else {
                    "-Emulation-".to_string()
                },
                2,
                if cfg.clean_mode { 2 } else { 1 },
                483,
            ),
            (
                loc.tr_or("setup", "row2_comment", "Change sample &rate in 1 kHz steps"),
                if cfg.override_rate_change {
                    on_str.clone()
                } else {
                    off_str.clone()
                },
                2,
                if cfg.override_rate_change { 2 } else { 1 },
                483,
            ),
            (
                loc.tr_or("setup", "row3_comment", "&Pause in Setup"),
                match (cfg.pause_while_setup, cfg.auto_preamp_peaksearch) {
                    (false, true) => {
                        if is_de {
                            "-PeakS: Ein-".to_string()
                        } else {
                            "-PeakS: On-".to_string()
                        }
                    }
                    (true, true) => "-Pause+PeakS-".to_string(),
                    (true, false) => on_str.clone(),
                    (false, false) => off_str.clone(),
                },
                2,
                if cfg.pause_while_setup { 2 } else { 1 },
                483,
            ),
            (
                loc.tr_or("setup", "row4_comment", "Play module immediately after &Open"),
                if cfg.play_on_load {
                    on_str.clone()
                } else {
                    off_str.clone()
                },
                2,
                if cfg.play_on_load { 2 } else { 1 },
                483,
            ),
            (
                loc.tr_or("setup", "row5_comment", "&Window movement by mouse click"),
                if cfg.full_window_move {
                    if is_de {
                        "* m. Inhalt *".to_string()
                    } else {
                        "* w. Content *".to_string()
                    }
                } else if cfg.move_on_click {
                    on_str.clone()
                } else {
                    off_str.clone()
                },
                3,
                if cfg.full_window_move {
                    3
                } else if cfg.move_on_click {
                    2
                } else {
                    1
                },
                471,
            ),
        ];

        for (idx, (comment_txt, sel_txt, num_pos, cur_pos, sw_x)) in row_labels.iter().enumerate() {
            let ry = cap_dy + 100 + (idx as i32) * 18;
            draw_frame_3d_1px(frame, dw, dh, 14, ry - 2, 316, 16, true);
            let clean_lbl = comment_txt.trim_end_matches(':').trim_end();
            draw_oem_text_mnemonic(frame, dw, dh, 16, ry, clean_lbl, col_black);
            draw_oem_text(frame, dw, dh, 320, ry, ":", col_black);

            fill_rect(frame, dw, dh, 349, ry - 2, 108, 16, col_black);
            draw_frame_3d_1px(frame, dw, dh, 349, ry - 2, 108, 16, true);
            let disp_sel: String = sel_txt.chars().take(13).collect();
            draw_oem_text_centered(frame, dw, dh, 351, ry, 104, &disp_sel, col_green);

            draw_lrswitch(
                frame,
                dw,
                dh,
                *sw_x,
                ry - 1,
                *num_pos,
                *cur_pos,
                &self.assets.lrswitch,
            );
        }

        // 9. Rec. Path Sektion (`RecPathEdit` bei (16, CapDY+208), `pRecBrowse` bei (492, CapDY+204), `pAskHD` bei (488, CapDY+229))
        let rec_y = cap_dy + 208;
        fill_rect(frame, dw, dh, 14, rec_y - 2, 474, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 14, rec_y - 2, 474, 16, true);
        let rec_focused = self.setup_edit_focus == SetupEditFocus::RecPath;
        let rec_disp: String = if rec_focused {
            format!("{}_", cfg.hd_path).chars().take(58).collect()
        } else {
            cfg.hd_path.chars().take(58).collect()
        };
        draw_oem_text(
            frame,
            dw,
            dh,
            16,
            rec_y,
            &rec_disp,
            if rec_focused { col_yellow } else { col_green },
        );
        draw_frame_3d_1px(frame, dw, dh, 491, cap_dy + 203, 23, 22, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            492,
            cap_dy + 204,
            self.pressed_btn == Some(PressedButton::SetupRecBrowse),
            &self.assets.b_browse,
        );

        let ask_y = cap_dy + 230;
        draw_frame_3d_1px(frame, dw, dh, 14, ask_y - 2, 316, 16, true);
        let ask_lbl = loc
            .tr_or("setup", "ask_hd_comment", "&Always ask for WAVE filename")
            .trim_end_matches(':')
            .trim_end();
        draw_oem_text_mnemonic(frame, dw, dh, 16, ask_y, ask_lbl, col_black);
        draw_oem_text(frame, dw, dh, 320, ask_y, ":", col_black);
        fill_rect(frame, dw, dh, 349, ask_y - 2, 108, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 349, ask_y - 2, 108, 16, true);
        draw_oem_text_centered(
            frame,
            dw,
            dh,
            351,
            ask_y,
            104,
            if cfg.ask_hd { &on_str } else { &off_str },
            col_green,
        );
        draw_lrswitch(
            frame,
            dw,
            dh,
            488,
            ask_y - 1,
            2,
            if cfg.ask_hd { 2 } else { 1 },
            &self.assets.lrswitch,
        );

        // 10. Mixer Path Sektion (`MixPathEdit` bei (16, CapDY+264), `pMixBrowse` bei (492, CapDY+260))
        let mix_y = cap_dy + 264;
        fill_rect(frame, dw, dh, 14, mix_y - 2, 474, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 14, mix_y - 2, 474, 16, true);
        let mix_focused = self.setup_edit_focus == SetupEditFocus::MixPath;
        let mix_disp: String = if mix_focused {
            format!("{}_", cfg.mixer_path).chars().take(58).collect()
        } else {
            cfg.mixer_path.chars().take(58).collect()
        };
        draw_oem_text(
            frame,
            dw,
            dh,
            16,
            mix_y,
            &mix_disp,
            if mix_focused { col_yellow } else { col_green },
        );
        draw_frame_3d_1px(frame, dw, dh, 491, cap_dy + 259, 23, 22, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            492,
            cap_dy + 260,
            self.pressed_btn == Some(PressedButton::SetupMixBrowse),
            &self.assets.b_browse,
        );

        // 11. Untere Button-Leiste (`y = CapDY + 290 = 308`, `IsShare = false` -> nur Hotkeys + Archives)
        let btn_y = cap_dy + 290;
        draw_frame_3d_1px(frame, dw, dh, 8, btn_y - 1, 124, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            9,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupHotkeys),
            &self.assets.bhotkeys,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            70,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupPacker),
            &self.assets.bpacker,
        );

        // Sprach-Anzeige (`dsetup.pas:503-511, 715-718`: durchgehender schwarzer Rahmen 179..305 mit `B_LANG.bmp` rechts darin)
        fill_rect(frame, dw, dh, 179, btn_y - 2, 126, 25, col_black);
        draw_frame_3d_1px(frame, dw, dh, 179, btn_y - 2, 126, 25, true);
        let lang_disp = if is_de { "Deutsch" } else { "English" };
        draw_oem_text_centered(
            frame,
            dw,
            dh,
            181,
            btn_y + 4,
            78,
            lang_disp,
            col_green,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            261,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupLang),
            &self.assets.b_lang,
        );

        // Live-Skin-Umschalter (`2.40` -> `Modern`) bei (309, btn_y - 1, 39, 23)
        let skin_pressed = self.pressed_btn == Some(PressedButton::SetupSkin);
        draw_frame_3d_1px(frame, dw, dh, 309, btn_y - 1, 39, 23, true);
        let src_y = if skin_pressed { 21 } else { 0 };
        self.assets
            .bsok
            .blit_rect_to(0, src_y, 8, 21, frame, dw, dh, 310, btn_y);
        for mx in (318..339).step_by(4) {
            let chunk_w = (339 - mx).min(4) as u32;
            self.assets
                .bsok
                .blit_rect_to(4, src_y, chunk_w, 21, frame, dw, dh, mx, btn_y);
        }
        self.assets
            .bsok
            .blit_rect_to(32, src_y, 8, 21, frame, dw, dh, 339, btn_y);

        let press_off = if skin_pressed { 1 } else { 0 };
        draw_classic_setup_btn_label(
            frame,
            dw,
            dh,
            310 + press_off,
            btn_y + press_off,
            37,
            "2.40",
            col_black,
        );

        // OK, Save, Cancel, Help (`x = 350, 391, 432, 473`, `y = 308`)
        draw_frame_3d_1px(frame, dw, dh, 349, btn_y - 1, 167, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            350,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupOk),
            &self.assets.bsok,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            391,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupSave),
            &self.assets.bssave,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            432,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupCancel),
            &self.assets.bscancel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            473,
            btn_y,
            self.pressed_btn == Some(PressedButton::SetupHelp),
            &self.assets.bshelp,
        );
    }

    /// Rendert den authentischen `TAskWin`-Dialog (`MULBOX.PAS` Zeilen 451–640, `361 x 72`) für Direct-to-Disk `.WAV`-Aufnahmen.
    pub(crate) fn render_wav_ask_window(&self, frame: &mut [u8], dw: u32, dh: u32, _state: &UiSharedState) {
        let cap_dy: i32 = 18;
        let loc = &self.locale;
        draw_3d_panel(frame, dw, dh, 0, 0, dw, dh, false, [192, 192, 192, 255]);
        for y in 1..18 {
            for x in 1..(dw as i32 - 1) {
                let t = x as u32;
                let w_span = (dw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let idx = ((y as u32 * dw + x as u32) * 4) as usize;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 255;
                }
            }
        }
        fill_rect(frame, dw, dh, 1, 18, dw.saturating_sub(2), 1, [0, 0, 0, 255]);
        draw_oem_text(
            frame,
            dw,
            dh,
            22,
            4,
            loc.tr_or("win", "wav_ask_title", "MOD4WIN Record Filename"),
            [48, 56, 68, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);
        draw_3d_panel(
            frame,
            dw,
            dh,
            0,
            cap_dy,
            dw,
            dh.saturating_sub(cap_dy as u32),
            false,
            [192, 192, 192, 255],
        );

        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];
        let col_yellow = [255, 255, 0, 255];

        // Zeile 1: "Path:" (10, CapDY+9, 40, 12) + EPath (82, CapDY+9, 248, 12) + pHDSel (335, CapDY+5, B_BROWSE.bmp)
        let y1 = cap_dy + 9;
        draw_frame_3d_1px(frame, dw, dh, 8, y1 - 2, 44, 16, true);
        draw_oem_text(frame, dw, dh, 10, y1, loc.tr_or("wav_ask", "path", "Path:"), col_black);

        fill_rect(frame, dw, dh, 80, y1 - 2, 252, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 80, y1 - 2, 252, 16, true);
        let path_focused = self.wav_ask_dlg.focus == WavAskEditFocus::Path;
        let p_disp: String = if path_focused {
            format!("{}_", self.wav_ask_dlg.path).chars().take(30).collect()
        } else {
            self.wav_ask_dlg.path.chars().take(30).collect()
        };
        draw_oem_text(
            frame,
            dw,
            dh,
            82,
            y1,
            &p_disp,
            if path_focused { col_yellow } else { col_green },
        );

        draw_frame_3d_1px(frame, dw, dh, 334, cap_dy + 4, 23, 22, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            335,
            cap_dy + 5,
            self.pressed_btn == Some(PressedButton::AskBrowse),
            &self.assets.b_browse,
        );

        // Zeile 2: "Filename:" (10, CapDY+31, 72, 12) + EName (82, CapDY+31, 64, 12) + ".WAV" (148, CapDY+31, 32, 12)
        let y2 = cap_dy + 31;
        draw_frame_3d_1px(frame, dw, dh, 8, y2 - 2, 74, 16, true);
        draw_oem_text(frame, dw, dh, 10, y2, loc.tr_or("wav_ask", "filename", "Filename:"), col_black);

        fill_rect(frame, dw, dh, 80, y2 - 2, 68, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 80, y2 - 2, 68, 16, true);
        let name_focused = self.wav_ask_dlg.focus == WavAskEditFocus::Filename;
        let n_disp: String = if name_focused {
            format!("{}_", self.wav_ask_dlg.filename).chars().take(8).collect()
        } else {
            self.wav_ask_dlg.filename.chars().take(8).collect()
        };
        draw_oem_text(
            frame,
            dw,
            dh,
            82,
            y2,
            &n_disp,
            if name_focused { col_yellow } else { col_green },
        );

        draw_frame_3d_1px(frame, dw, dh, 146, y2 - 2, 36, 16, true);
        draw_oem_text(frame, dw, dh, 148, y2, ".WAV", col_black);

        // Buttons: OK (207, CapDY+27), Cancel (248, CapDY+27), Help (289, CapDY+27)
        let by = cap_dy + 27;
        draw_frame_3d_1px(frame, dw, dh, 206, by - 1, 126, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            207,
            by,
            self.pressed_btn == Some(PressedButton::AskOk),
            &self.assets.bsok,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            248,
            by,
            self.pressed_btn == Some(PressedButton::AskCancel),
            &self.assets.bscancel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            289,
            by,
            self.pressed_btn == Some(PressedButton::AskHelp),
            &self.assets.bshelp,
        );
    }

    /// Rendert den Datei- (`tbrowse`, `316 x 268`) bzw. Verzeichnis-Auswahldialog (`tDirbrowse`, `300 x 210`) exakt nach `newopen.pas`.
    pub(crate) fn render_file_select_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let is_de = self.locale.is_german() || state.ini_config.language == 1;
        let is_file_browse = self.fsel_dlg.is_file_browse();
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];
        let col_white = [255, 255, 255, 255];

        draw_3d_panel(frame, dw, dh, 0, 0, dw, dh, false, col_gray);
        let title_str = if is_file_browse {
            if is_de {
                "MOD4WIN Dateiauswahl"
            } else {
                "MOD4WIN Select File"
            }
        } else if is_de {
            "MOD4WIN Verzeichniswahl"
        } else {
            "MOD4WIN Select Directory"
        };
        for y in 1..18 {
            for x in 1..(dw as i32 - 1) {
                let t = x as u32;
                let w_span = (dw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let idx = ((y as u32 * dw + x as u32) * 4) as usize;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 255;
                }
            }
        }
        fill_rect(frame, dw, dh, 1, 18, dw.saturating_sub(2), 1, col_black);
        draw_oem_text(frame, dw, dh, 22, 4, title_str, col_black);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);

        let mut cur_p = self.fsel_dlg.current_dir.to_string_lossy().to_uppercase();
        if !cur_p.ends_with('\\') && cur_p.len() <= 3 {
            cur_p.push('\\');
        }

        if is_file_browse {
            // === `tbrowse` (`Dateiauswahl`, `316 x 268`, `newopen.pas:3840-4300`) ===
            draw_oem_text_mnemonic(frame, dw, dh, 10, 28, "&Path:", col_black);
            draw_3d_panel(frame, dw, dh, 50, 26, 254, 16, true, col_black);
            let p_disp: String = cur_p.chars().take(29).collect();
            draw_oem_text(frame, dw, dh, 52, 28, &p_disp, col_green);
            let cur_x = 52 + (p_disp.chars().count() as i32) * 8;
            if cur_x + 8 <= 302 {
                draw_oem_text(frame, dw, dh, cur_x, 28, "_", col_white);
            }

            draw_oem_text_mnemonic(frame, dw, dh, 10, 53, "&Filelist:", col_black);
            draw_oem_text_mnemonic(frame, dw, dh, 148, 53, "&Directories:", col_black);

            // Linke Filelist (10, 67, 128, 162; 13 Zeilen à 12px) + Scrollbar (122, 69, 14, 158)
            draw_3d_frame_only(frame, dw, dh, 10, 67, 128, 162, true);
            fill_rect(frame, dw, dh, 12, 69, 110, 158, col_black);
            let f_max_scroll = self.fsel_dlg.files.len().saturating_sub(13);
            let f_scroll = self.fsel_dlg.file_scroll.min(f_max_scroll);
            for (row, (idx, (lbl, _))) in self
                .fsel_dlg
                .files
                .iter()
                .enumerate()
                .skip(f_scroll)
                .take(13)
                .enumerate()
            {
                let ry = 70 + (row as i32) * 12;
                let is_cur = idx == self.fsel_dlg.file_cursor && self.fsel_dlg.active_list == 0;
                if is_cur {
                    fill_rect(frame, dw, dh, 12, ry, 110, 12, col_green);
                }
                let disp: String = lbl.chars().take(13).collect();
                draw_oem_text(
                    frame,
                    dw,
                    dh,
                    14,
                    ry,
                    &disp,
                    if is_cur { col_black } else { col_green },
                );
            }
            Self::draw_vertical_scrollbar(
                frame,
                dw,
                dh,
                122,
                69,
                14,
                158,
                f_scroll,
                13,
                self.fsel_dlg.files.len(),
            );

            // Rechte Directories-Liste (146, 67, 160, 132; 10 Zeilen à 12px) + Scrollbar (290, 69, 14, 128)
            draw_3d_frame_only(frame, dw, dh, 146, 67, 160, 132, true);
            fill_rect(frame, dw, dh, 148, 69, 142, 128, col_black);
            let d_max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
            let d_scroll = self.fsel_dlg.scroll.min(d_max_scroll);
            for (row, (idx, entry)) in self
                .fsel_dlg
                .entries
                .iter()
                .enumerate()
                .skip(d_scroll)
                .take(10)
                .enumerate()
            {
                let ry = 70 + (row as i32) * 12;
                let is_cur = idx == self.fsel_dlg.cursor;
                if is_cur {
                    fill_rect(frame, dw, dh, 148, ry, 142, 12, col_green);
                }
                let tcol = if is_cur { col_black } else { col_green };
                let (icon_sx, lbl) = match entry {
                    FileSelectEntry::Parent(_) => (0, "<..>".to_string()),
                    FileSelectEntry::Subdir(s, _) => (0, s.to_lowercase()),
                    FileSelectEntry::File(s, _) => (0, s.to_lowercase()),
                    FileSelectEntry::Drive(drv) => (
                        if *drv == 'A' || *drv == 'B' { 84 } else { 112 },
                        format!("[-{}-]", drv.to_ascii_lowercase()),
                    ),
                };
                self.assets
                    .bmp_drives
                    .blit_rect_to(icon_sx, 0, 28, 12, frame, dw, dh, 149, ry);
                let disp: String = lbl.chars().take(12).collect();
                draw_oem_text(frame, dw, dh, 186, ry, &disp, tcol);
            }
            Self::draw_vertical_scrollbar(
                frame,
                dw,
                dh,
                290,
                69,
                14,
                128,
                d_scroll,
                10,
                self.fsel_dlg.entries.len(),
            );

            // Horizontale Buttons unter Directories: OK (147, 207), Quit (204, 207), Help (261, 207)
            for &(bx, btn_kind, bmp) in &[
                (147, PressedButton::FselOk, &self.assets.bsok),
                (204, PressedButton::FselCancel, &self.assets.bscancel),
                (261, PressedButton::FselHelp, &self.assets.bshelp),
            ] {
                draw_3d_frame_only(frame, dw, dh, bx - 2, 205, 45, 25, true);
                draw_bmp_button_2state(
                    frame,
                    dw,
                    dh,
                    bx,
                    207,
                    self.pressed_btn == Some(btn_kind),
                    bmp,
                );
            }

            // Untere Statusleiste (`INFOstatic`: 10, 240, 296, 18)
            draw_3d_panel(frame, dw, dh, 10, 240, 296, 18, true, col_gray);
            let status_txt = if is_de {
                "Enter - Datei auswählen"
            } else {
                "Enter - Select file"
            };
            draw_oem_text_centered(frame, dw, dh, 12, 243, 292, status_txt, col_black);
        } else {
            // === `tDirbrowse` (`Verzeichniswahl`, `300 x 210`, `newopen.pas:4300-4560`) ===
            draw_oem_text_mnemonic(frame, dw, dh, 10, 26, "&Path:", col_black);
            draw_3d_panel(frame, dw, dh, 56, 24, 234, 16, true, col_black);
            let p_disp: String = cur_p.chars().take(27).collect();
            draw_oem_text(frame, dw, dh, 58, 26, &p_disp, col_green);
            let cur_x = 58 + (p_disp.chars().count() as i32) * 8;
            if cur_x + 8 <= 288 {
                draw_oem_text(frame, dw, dh, cur_x, 26, "_", col_white);
            }

            draw_oem_text_mnemonic(frame, dw, dh, 10, 41, "&Directories:", col_black);

            // Directories-Liste links (8, 55, 236, 128; 10 Zeilen à 12px) + Scrollbar (228, 57, 14, 124)
            draw_3d_frame_only(frame, dw, dh, 8, 55, 236, 128, true);
            fill_rect(frame, dw, dh, 10, 57, 218, 124, col_black);
            let d_max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
            let d_scroll = self.fsel_dlg.scroll.min(d_max_scroll);
            for (row, (idx, entry)) in self
                .fsel_dlg
                .entries
                .iter()
                .enumerate()
                .skip(d_scroll)
                .take(10)
                .enumerate()
            {
                let ry = 58 + (row as i32) * 12;
                let is_cur = idx == self.fsel_dlg.cursor;
                if is_cur {
                    fill_rect(frame, dw, dh, 10, ry, 218, 12, col_green);
                }
                let tcol = if is_cur { col_black } else { col_green };
                let (icon_sx, lbl) = match entry {
                    FileSelectEntry::Parent(_) => (0, "<..>".to_string()),
                    FileSelectEntry::Subdir(s, _) => (0, s.to_lowercase()),
                    FileSelectEntry::File(s, _) => (0, s.to_lowercase()),
                    FileSelectEntry::Drive(drv) => (
                        if *drv == 'A' || *drv == 'B' { 84 } else { 112 },
                        format!("[-{}-]", drv.to_ascii_lowercase()),
                    ),
                };
                self.assets
                    .bmp_drives
                    .blit_rect_to(icon_sx, 0, 28, 12, frame, dw, dh, 11, ry);
                let disp: String = lbl.chars().take(21).collect();
                draw_oem_text(frame, dw, dh, 48, ry, &disp, tcol);
            }
            Self::draw_vertical_scrollbar(
                frame,
                dw,
                dh,
                228,
                57,
                14,
                124,
                d_scroll,
                10,
                self.fsel_dlg.entries.len(),
            );

            // Vertikal gestapelte Buttons rechts neben der Liste: OK (250, 55), Quit (250, 85), Help (250, 115)
            for &(by, btn_kind, bmp) in &[
                (55, PressedButton::FselOk, &self.assets.bsok),
                (85, PressedButton::FselCancel, &self.assets.bscancel),
                (115, PressedButton::FselHelp, &self.assets.bshelp),
            ] {
                draw_frame_3d_1px(frame, dw, dh, 249, by - 1, 43, 23, true);
                draw_bmp_button_2state(
                    frame,
                    dw,
                    dh,
                    250,
                    by,
                    self.pressed_btn == Some(btn_kind),
                    bmp,
                );
            }

            // Untere Statusleiste (`INFOstatic`: 8, 187, 284, 16)
            draw_3d_panel(frame, dw, dh, 8, 187, 284, 16, true, col_gray);
            let status_txt = if is_de {
                "Enter - Datei auswählen"
            } else {
                "Enter - Select directory"
            };
            draw_oem_text_centered(frame, dw, dh, 10, 189, 280, status_txt, col_black);
        }
    }

    /// Überträgt Änderungen in `self.setup_cfg` sofort live in `self.shared_state` (`ini_config` + `sound_settings`).
    pub(crate) fn sync_live_setup_to_state(&mut self) {
        let new_skin = SkinMode::from_u8(self.setup_cfg.skin_mode);
        if self.locale.language_id != self.setup_cfg.language {
            self.locale = LocaleCatalog::load_for_language_id(self.setup_cfg.language);
            self.help_dlg.set_language(self.setup_cfg.language);
        }
        {
            let mut st = self.shared_state.lock().unwrap();
            let old_peaks = st.ini_config.peaks.clone();
            let old_win_pos = st.ini_config.window_positions.clone();
            let old_x_main = st.ini_config.x_main;
            let old_y_main = st.ini_config.y_main;
            let old_x_mini = st.ini_config.x_mini;
            let old_y_mini = st.ini_config.y_mini;
            let old_playlist = st.playlist.clone();
            let old_idx = st.playlist_idx;
            let old_pos = st.curr_pos;
            st.ini_config = self.setup_cfg.clone();
            if st.ini_config.peaks.is_empty() {
                st.ini_config.peaks = old_peaks;
            }
            for (k, v) in old_win_pos {
                st.ini_config.window_positions.insert(k, v);
            }
            st.ini_config.x_main = old_x_main;
            st.ini_config.y_main = old_y_main;
            st.ini_config.x_mini = old_x_mini;
            st.ini_config.y_mini = old_y_mini;
            st.ini_config.gui_scale = self.scale_factor.clamp(0.5, 4.0);
            st.ini_config.saved_playlist = old_playlist;
            st.ini_config.saved_playlist_idx = old_idx;
            st.ini_config.saved_order_pos = old_pos;
            st.ini_config.saved_repeat_mode = st.repeat_mode;
            st.ini_config.saved_shuffle = st.shuffle;
            st.ini_config.saved_intro_scan = st.intro_scan;
            st.ini_config.saved_minus_mode = st.minus_mode;
            st.set_skin_mode(new_skin);
            st.apply_ini_to_sound_settings();
        };
        self.refresh_all_windows_for_skin();
    }

    pub(crate) fn commit_setup_dialog(&mut self, save_to_disk: bool) {
        self.sync_live_setup_to_state();
        self.close_window(WindowKind::SetupDialog);
        if save_to_disk {
            self.save_ini_on_exit();
        }
    }

    pub(crate) fn step_setup_samplerate(&mut self, dir: i32) {
        let cur_khz = (self.setup_cfg.sample_rate / 1000).clamp(4, 96) as i32;
        if self.setup_cfg.override_rate_change {
            let next_khz = (cur_khz + dir).clamp(11, 48) as u32;
            self.setup_cfg.sample_rate = match next_khz {
                11 => 11025,
                22 => 22050,
                44 => 44100,
                other => other * 1000,
            };
        } else {
            const STD_RATES: [u32; 4] = [11025, 22050, 44100, 48000];
            let cur_idx = STD_RATES
                .iter()
                .position(|&r| (r / 1000) as i32 >= cur_khz)
                .unwrap_or(2) as i32;
            let next_idx = (cur_idx + dir).clamp(0, (STD_RATES.len() - 1) as i32) as usize;
            self.setup_cfg.sample_rate = STD_RATES[next_idx];
        }
    }

    pub(crate) fn toggle_setup_device(&mut self) {
        if self.setup_cfg.device_id == DEV_HDD {
            self.setup_cfg.device_id = DEV_SOUNDCARD;
            if self.setup_cfg.device_name.eq_ignore_ascii_case("DIRECT TO DISK RECORDING") {
                self.setup_cfg.device_name = "Wave Mapper (CPAL Default)".to_string();
            }
        } else {
            self.setup_cfg.device_id = DEV_HDD;
            // Wie `CurrSettings := DefHDDSetting` in `dsetup.pas` / `modc.pas`:
            // 44.1 kHz, 16 Bit, Stereo, Surround = false!, Panning = true, gewählte IDO-Stufe beibehalten
            self.setup_cfg.sample_rate = 44100;
            self.setup_cfg.bits_per_sample = 16;
            self.setup_cfg.channels = 2;
            self.setup_cfg.surround = false;
            self.setup_cfg.over_sampling = self.setup_cfg.over_sampling.min(3);
            self.setup_cfg.enable_panning = true;
            self.setup_cfg.clipping = true;
        }
    }

    pub(crate) fn handle_setup_dialog_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        let cap_dy: i32 = 18;

        // 1. Titelleiste (y <= 18)
        if vy <= cap_dy {
            if vx <= 20 || vx >= (SETUP_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::SetupDialog);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        // Fokus von Edit-Feldern standardmäßig lösen, wenn woanders geklickt wird
        self.setup_edit_focus = SetupEditFocus::None;

        // 2. Sound-Ausgabegerät (`devname` 7..493, 25..52 oder `bName` Up/Down 497..518, 25..53)
        if (25..=53).contains(&vy) && (7..=518).contains(&vx) {
            if (497..=518).contains(&vx) {
                if vy < 26 + 13 {
                    self.pressed_btn = Some(PressedButton::SetupDevUp);
                } else {
                    self.pressed_btn = Some(PressedButton::SetupDevDown);
                }
            }
            self.toggle_setup_device();
            self.sync_live_setup_to_state();
            return;
        }

        // 3. Obere Sampling-Sektion (y = 58..102)
        if (58..=102).contains(&vy) {
            // Samplerate Up/Down (`bSR` bei 44..65, 70..98 oder Klick auf `sr` 10..42)
            if (44..=65).contains(&vx) && (70..=98).contains(&vy) {
                if vy < 71 + 13 {
                    self.pressed_btn = Some(PressedButton::SetupSrUp);
                    self.step_setup_samplerate(1);
                } else {
                    self.pressed_btn = Some(PressedButton::SetupSrDown);
                    self.step_setup_samplerate(-1);
                }
                self.sync_live_setup_to_state();
                return;
            } else if (10..=42).contains(&vx) && (70..=98).contains(&vy) {
                self.step_setup_samplerate(1);
                self.sync_live_setup_to_state();
                return;
            }

            // Auflösung `bBits` (87..136, 70..98: oben = 8 bit, unten = 16 bit)
            if (87..=136).contains(&vx) && (70..=98).contains(&vy) {
                let target = if vy < 84 { 8 } else { 16 };
                if (87..=109).contains(&vx) && self.setup_cfg.bits_per_sample == target {
                    self.setup_cfg.bits_per_sample =
                        if self.setup_cfg.bits_per_sample == 8 { 16 } else { 8 };
                } else {
                    self.setup_cfg.bits_per_sample = target;
                }
                self.sync_live_setup_to_state();
                return;
            }

            // Modus `bMS` (139..193, 62..100: oben = Mono, Mitte = Stereo, unten = Surround)
            if (139..=193).contains(&vx) && (62..=100).contains(&vy) {
                let cur_mode = if self.setup_cfg.channels <= 1 {
                    0
                } else if self.setup_cfg.surround {
                    2
                } else {
                    1
                };
                let clicked_mode = if vy < 76 {
                    0
                } else if vy <= 87 {
                    1
                } else {
                    2
                };
                let next_mode = if (139..=161).contains(&vx) && clicked_mode == cur_mode {
                    (cur_mode + 1) % 3
                } else {
                    clicked_mode
                };
                match next_mode {
                    0 => {
                        self.setup_cfg.channels = 1;
                        self.setup_cfg.surround = false;
                    }
                    1 => {
                        self.setup_cfg.channels = 2;
                        self.setup_cfg.surround = false;
                    }
                    _ => {
                        self.setup_cfg.channels = 2;
                        self.setup_cfg.surround = true;
                    }
                }
                self.sync_live_setup_to_state();
                return;
            }

            // Buffers Up/Down (`bnBlocks` bei 237..258, 70..98)
            if (237..=258).contains(&vx) && (70..=98).contains(&vy) {
                let delta: i32 = if vy < 71 + 13 {
                    self.pressed_btn = Some(PressedButton::SetupBufUp);
                    1
                } else {
                    self.pressed_btn = Some(PressedButton::SetupBufDown);
                    -1
                };
                if self.setup_cfg.device_id == DEV_HDD {
                    self.setup_cfg.hdd_rec_buffers =
                        (self.setup_cfg.hdd_rec_buffers as i32 + delta).clamp(1, 99) as u8;
                } else {
                    self.setup_cfg.output_buffers =
                        (self.setup_cfg.output_buffers as i32 + delta).clamp(2, 99) as u8;
                }
                self.sync_live_setup_to_state();
                return;
            }

            // `bIDO` (349..390, 58..100: 0 = Off [vy < 76], 1 = Lin [76..87], Cub = 3 (`IDO-FPU` auf modernem Build, bzw. 2 ohne FPU) [> 87])
            if (349..=395).contains(&vx) && (58..=100).contains(&vy) {
                let cub_val: u8 = if cfg!(any(target_arch = "x86_64", target_arch = "aarch64", target_feature = "sse")) {
                    3
                } else {
                    2
                };
                let cur_vis = self.setup_cfg.over_sampling.min(2);
                let clicked_vis = if vy < 76 {
                    0
                } else if vy <= 87 {
                    1
                } else {
                    2
                };
                let next_vis = if (349..=370).contains(&vx) && clicked_vis == cur_vis {
                    (cur_vis + 1) % 3
                } else {
                    clicked_vis
                };
                self.setup_cfg.over_sampling = match next_vis {
                    0 => 0,
                    1 => 1,
                    _ => cub_val,
                };
                self.sync_live_setup_to_state();
                return;
            }

            // `bPAN` (396..418, 70..98)
            if (396..=418).contains(&vx) && (70..=98).contains(&vy) {
                self.setup_cfg.enable_panning = !self.setup_cfg.enable_panning;
                self.sync_live_setup_to_state();
                return;
            }

            // `bPAmp` / Clipping & De-Click (421..444, 58..100: 0 = Clip Off [vy < 76], 1 = Clip On [76..87], 2 = CL+DC [> 87])
            if (421..=444).contains(&vx) && (58..=100).contains(&vy) {
                let cur_vis: u8 = match (self.setup_cfg.clipping, self.setup_cfg.declick) {
                    (false, _) => 0,
                    (true, false) => 1,
                    (true, true) => 2,
                };
                let clicked_vis: u8 = if vy < 76 {
                    0
                } else if vy <= 87 {
                    1
                } else {
                    2
                };
                let next_vis = if clicked_vis == cur_vis {
                    (cur_vis + 1) % 3
                } else {
                    clicked_vis
                };
                match next_vis {
                    0 => {
                        self.setup_cfg.clipping = false;
                        self.setup_cfg.declick = false;
                    }
                    1 => {
                        self.setup_cfg.clipping = true;
                        self.setup_cfg.declick = false;
                    }
                    _ => {
                        self.setup_cfg.clipping = true;
                        self.setup_cfg.declick = true;
                    }
                }
                self.sync_live_setup_to_state();
                return;
            }

            // `bloadstate` / StateSaver (466..506, 70..98)
            if (466..=506).contains(&vx) && (70..=98).contains(&vy) {
                self.setup_cfg.state_saver = !self.setup_cfg.state_saver;
                self.sync_live_setup_to_state();
                return;
            }
        }

        // 4. Mittlere 5 Zeilen (`I = 0..4`, `ry = CapDY + 100 + idx*18 = 118, 136, 154, 172, 190`)
        for idx in 0..5usize {
            let ry = cap_dy + 100 + (idx as i32) * 18;
            if (ry - 3..=ry + 15).contains(&vy) && (14..=512).contains(&vx) {
                match idx {
                    0 => {
                        // Engine-Modus (false = Full Emulation, true = Clean Mode)
                        self.setup_cfg.clean_mode = !self.setup_cfg.clean_mode;
                    }
                    1 => {
                        // Samplerate in 1-kHz-Schritten
                        self.setup_cfg.override_rate_change = !self.setup_cfg.override_rate_change;
                    }
                    2 => {
                        // Pause im Setup & Auto-PreAmp PeakSearch:
                        // Klick auf das grüne Statusfeld (349..457) schaltet Auto-PreAmp PeakSearch um,
                        // Klick auf den Schalter (483..507) oder Text schaltet PauseWhileSetup (bzw. beides) um.
                        if (349..=457).contains(&vx) {
                            self.setup_cfg.auto_preamp_peaksearch =
                                !self.setup_cfg.auto_preamp_peaksearch;
                        } else {
                            self.setup_cfg.pause_while_setup = !self.setup_cfg.pause_while_setup;
                        }
                    }
                    3 => {
                        // Auto-Play nach Open
                        self.setup_cfg.play_on_load = !self.setup_cfg.play_on_load;
                    }
                    4 => {
                        // Fensterbewegung (1 = Titelleiste, 2 = Überall, 3 = Mit Inhalt)
                        let next_mode = if (471..=507).contains(&vx) {
                            ((vx - 471) / 12).clamp(0, 2)
                        } else if self.setup_cfg.full_window_move {
                            0
                        } else if self.setup_cfg.move_on_click {
                            2
                        } else {
                            1
                        };
                        match next_mode {
                            0 => {
                                self.setup_cfg.move_on_click = false;
                                self.setup_cfg.full_window_move = false;
                            }
                            1 => {
                                self.setup_cfg.move_on_click = true;
                                self.setup_cfg.full_window_move = false;
                            }
                            _ => {
                                self.setup_cfg.move_on_click = true;
                                self.setup_cfg.full_window_move = true;
                            }
                        }
                    }
                    _ => {}
                }
                self.sync_live_setup_to_state();
                return;
            }
        }

        // 5. Rec. Path Sektion:
        //    - `RecPathEdit` (14..488, 224..240)
        //    - `pRecBrowse` (491..514, 221..243)
        //    - `pAskHD` Zeile 6 (14..514, 246..264)
        if (221..=243).contains(&vy) {
            if (14..=488).contains(&vx) {
                self.setup_edit_focus = SetupEditFocus::RecPath;
                return;
            } else if (491..=514).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupRecBrowse);
                self.fsel_dlg =
                    FileSelectState::new(FileSelectTarget::SetupRecDir, &self.setup_cfg.hd_path);
                self.open_window(event_loop, WindowKind::FileSelectDialog);
                return;
            }
        }
        if (246..=264).contains(&vy) && (14..=514).contains(&vx) {
            self.setup_cfg.ask_hd = !self.setup_cfg.ask_hd;
            self.sync_live_setup_to_state();
            return;
        }

        // 6. Mixer Path Sektion:
        //    - `MixPathEdit` (14..488, 280..296)
        //    - `pMixBrowse` (491..514, 277..299)
        if (277..=299).contains(&vy) {
            if (14..=488).contains(&vx) {
                self.setup_edit_focus = SetupEditFocus::MixPath;
                return;
            } else if (491..=514).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupMixBrowse);
                self.fsel_dlg = FileSelectState::new(
                    FileSelectTarget::SetupMixerExe,
                    &self.setup_cfg.mixer_path,
                );
                self.open_window(event_loop, WindowKind::FileSelectDialog);
                return;
            }
        }

        // 7. Untere Button-Leiste (`y = 307..330`)
        if (307..=330).contains(&vy) {
            if (9..=69).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupHotkeys);
                self.open_window(event_loop, WindowKind::HotkeyEditor);
                return;
            } else if (70..=131).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupPacker);
                self.open_window(event_loop, WindowKind::ArchiveSetup);
                return;
            } else if (179..=305).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupLang);
                self.setup_cfg.language = if self.setup_cfg.language == 0 { 1 } else { 0 };
                self.setup_backup_cfg.language = self.setup_cfg.language;
                self.sync_live_setup_to_state();
                return;
            } else if (309..=348).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupSkin);
                let next_skin = SkinMode::from_u8(self.setup_cfg.skin_mode).next();
                self.setup_cfg.skin_mode = next_skin.to_u8();
                self.setup_backup_cfg.skin_mode = self.setup_cfg.skin_mode;
                self.sync_live_setup_to_state();
                return;
            } else if (350..=390).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupOk);
                self.commit_setup_dialog(false);
                return;
            } else if (391..=431).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupSave);
                self.commit_setup_dialog(true);
                return;
            } else if (432..=472).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupCancel);
                self.setup_cfg = self.setup_backup_cfg.clone();
                self.sync_live_setup_to_state();
                self.close_window(WindowKind::SetupDialog);
                return;
            } else if (473..=515).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SetupHelp);
                self.open_help_topic(event_loop, "SetupDlg");
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn commit_wav_ask_ok(&mut self) {
        let mut fname = self.wav_ask_dlg.filename.trim().to_string();
        if fname.is_empty() {
            fname = "OUTPUT".to_string();
        }
        if !fname.to_ascii_uppercase().ends_with(".WAV") {
            fname.push_str(".WAV");
        }
        let dir_str = self.wav_ask_dlg.path.trim();
        let dir_path = if dir_str.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(dir_str)
        };
        let full_wav = dir_path.join(fname);
        {
            let mut st = self.shared_state.lock().unwrap();
            st.ini_config.hd_path = dir_path.to_string_lossy().into_owned();
        }
        self.close_window(WindowKind::WavAskDialog);
        self.execute_wav_export(full_wav);
    }

    pub(crate) fn handle_wav_ask_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        if vy <= 18 {
            if vx <= 20 || vx >= (ASK_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::WavAskDialog);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        // Zeile 1: EPath (80..332, 25..41) & pHDSel Browse (334..357, 22..44)
        if (22..=44).contains(&vy) {
            if (80..=332).contains(&vx) {
                self.wav_ask_dlg.focus = WavAskEditFocus::Path;
                return;
            } else if (334..=357).contains(&vx) {
                self.pressed_btn = Some(PressedButton::AskBrowse);
                self.fsel_dlg =
                    FileSelectState::new(FileSelectTarget::AskWavDir, &self.wav_ask_dlg.path);
                self.open_window(event_loop, WindowKind::FileSelectDialog);
                return;
            }
        }

        // Zeile 2: EName (80..148, 47..63) & Buttons OK (207..247), Cancel (248..288), Help (289..331) bei y = 45..67
        if (45..=67).contains(&vy) {
            if (80..=148).contains(&vx) {
                self.wav_ask_dlg.focus = WavAskEditFocus::Filename;
                return;
            } else if (207..=247).contains(&vx) {
                self.pressed_btn = Some(PressedButton::AskOk);
                self.commit_wav_ask_ok();
                return;
            } else if (248..=288).contains(&vx) {
                self.pressed_btn = Some(PressedButton::AskCancel);
                self.close_window(WindowKind::WavAskDialog);
                return;
            } else if (289..=331).contains(&vx) {
                self.pressed_btn = Some(PressedButton::AskHelp);
                self.open_help_topic(event_loop, "RecordingDlg");
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn apply_archive_exe_selection(&mut self, arc_idx: u8, path_str: String) {
        let is_pack = self.archive_dlg.pack_mode[arc_idx.min(2) as usize];
        match (arc_idx, is_pack) {
            (0, false) => self.setup_cfg.arj_unpack = path_str,
            (0, true) => self.setup_cfg.arj_pack = path_str,
            (1, false) => self.setup_cfg.zip_unpack = path_str,
            (1, true) => self.setup_cfg.zip_pack = path_str,
            (_, false) => self.setup_cfg.lha_unpack = path_str,
            (_, true) => self.setup_cfg.lha_pack = path_str,
        }
    }

    pub(crate) fn commit_file_select_ok(&mut self) {
        let wants_file = self.fsel_dlg.is_file_browse();
        let chosen_path = if wants_file {
            if let Some((_, p)) = self.fsel_dlg.files.get(self.fsel_dlg.file_cursor) {
                p.clone()
            } else {
                self.fsel_dlg.current_dir.clone()
            }
        } else {
            self.fsel_dlg.current_dir.clone()
        };
        let chosen = chosen_path.to_string_lossy().into_owned();
        match self.fsel_dlg.target {
            FileSelectTarget::SetupRecDir => {
                self.setup_cfg.hd_path = chosen;
            }
            FileSelectTarget::SetupMixerExe => {
                self.setup_cfg.mixer_path = chosen;
            }
            FileSelectTarget::AskWavDir => {
                self.wav_ask_dlg.path = chosen;
            }
            FileSelectTarget::ArchiveExe(arc_idx) => {
                self.apply_archive_exe_selection(arc_idx, chosen);
            }
            FileSelectTarget::PlaylistAdd => {
                let mut st = self.shared_state.lock().unwrap();
                st.ini_config.last_dir = self.fsel_dlg.current_dir.to_string_lossy().into_owned();
                st.handle_dropped_paths(&[chosen_path], false);
            }
        }
        self.close_window(WindowKind::FileSelectDialog);
    }

    pub(crate) fn handle_file_select_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        let is_file_browse = self.fsel_dlg.is_file_browse();
        let win_w = if is_file_browse { 316 } else { FSEL_WINDOW_WIDTH as i32 };
        if vy <= 18 {
            if vx <= 20 || vx >= (win_w - 20) {
                self.close_window(WindowKind::FileSelectDialog);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        let classic_input = self.shared_state.lock().unwrap().classic_input;

        if is_file_browse {
            // === `tbrowse` (`Dateiauswahl`, `316 x 268`) ===
            // 1. Linke Filelist (12..121, 69..227) & Scrollbar (122..136, 69..227)
            if (69..=227).contains(&vy) {
                if (12..=121).contains(&vx) {
                    self.fsel_dlg.active_list = 0;
                    let row = ((vy - 70).max(0) / 12) as usize;
                    let max_scroll = self.fsel_dlg.files.len().saturating_sub(13);
                    let scroll = self.fsel_dlg.file_scroll.min(max_scroll);
                    let idx = scroll + row;
                    if let Some((_, p)) = self.fsel_dlg.files.get(idx).cloned() {
                        let same = self.fsel_dlg.file_cursor == idx;
                        self.fsel_dlg.file_cursor = idx;
                        if is_double_click || (classic_input && same) {
                            match self.fsel_dlg.target {
                                FileSelectTarget::SetupMixerExe => {
                                    self.setup_cfg.mixer_path = p.to_string_lossy().into_owned();
                                    self.close_window(WindowKind::FileSelectDialog);
                                }
                                FileSelectTarget::ArchiveExe(arc_idx) => {
                                    self.apply_archive_exe_selection(
                                        arc_idx,
                                        p.to_string_lossy().into_owned(),
                                    );
                                    self.close_window(WindowKind::FileSelectDialog);
                                }
                                FileSelectTarget::PlaylistAdd => {
                                    let mut st = self.shared_state.lock().unwrap();
                                    st.ini_config.last_dir =
                                        self.fsel_dlg.current_dir.to_string_lossy().into_owned();
                                    st.handle_dropped_paths(&[p], false);
                                    drop(st);
                                    self.close_window(WindowKind::FileSelectDialog);
                                }
                                _ => {}
                            }
                        }
                    }
                    return;
                } else if (122..=136).contains(&vx) {
                    let max_scroll = self.fsel_dlg.files.len().saturating_sub(13);
                    if vy <= 83 {
                        self.fsel_dlg.file_scroll = self.fsel_dlg.file_scroll.saturating_sub(1);
                    } else if vy >= 213 {
                        self.fsel_dlg.file_scroll = (self.fsel_dlg.file_scroll + 1).min(max_scroll);
                    } else if vy < 148 {
                        self.fsel_dlg.file_scroll = self.fsel_dlg.file_scroll.saturating_sub(12);
                    } else {
                        self.fsel_dlg.file_scroll = (self.fsel_dlg.file_scroll + 12).min(max_scroll);
                    }
                    return;
                }
            }

            // 2. Rechte Directories-Liste (148..289, 69..197) & Scrollbar (290..304, 69..197)
            if (69..=197).contains(&vy) {
                if (148..=289).contains(&vx) {
                    self.fsel_dlg.active_list = 1;
                    let row = ((vy - 70).max(0) / 12) as usize;
                    let max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
                    let scroll = self.fsel_dlg.scroll.min(max_scroll);
                    let idx = scroll + row;
                    if let Some(entry) = self.fsel_dlg.entries.get(idx).cloned() {
                        let same = self.fsel_dlg.cursor == idx;
                        self.fsel_dlg.cursor = idx;
                        if is_double_click || (classic_input && same) {
                            match entry {
                                FileSelectEntry::Parent(p) | FileSelectEntry::Subdir(_, p) => {
                                    self.fsel_dlg.current_dir = p;
                                    self.fsel_dlg.refresh();
                                }
                                FileSelectEntry::Drive(drv) => {
                                    self.fsel_dlg.current_dir = PathBuf::from(format!("{}:\\", drv));
                                    self.fsel_dlg.refresh();
                                }
                                _ => {}
                            }
                        }
                    }
                    return;
                } else if (290..=304).contains(&vx) {
                    let max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
                    if vy <= 83 {
                        self.fsel_dlg.scroll = self.fsel_dlg.scroll.saturating_sub(1);
                    } else if vy >= 183 {
                        self.fsel_dlg.scroll = (self.fsel_dlg.scroll + 1).min(max_scroll);
                    } else if vy < 133 {
                        self.fsel_dlg.scroll = self.fsel_dlg.scroll.saturating_sub(9);
                    } else {
                        self.fsel_dlg.scroll = (self.fsel_dlg.scroll + 9).min(max_scroll);
                    }
                    return;
                }
            }

            // 3. Horizontale Buttons unter Directories: OK (145..190), Quit (202..247), Help (259..304) bei y = 205..230
            if (205..=230).contains(&vy) {
                if (145..=190).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::FselOk);
                    self.commit_file_select_ok();
                    return;
                } else if (202..=247).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::FselCancel);
                    self.close_window(WindowKind::FileSelectDialog);
                    return;
                } else if (259..=304).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::FselHelp);
                    self.open_help_topic(event_loop, "FselDlg");
                    return;
                }
            }
        } else {
            // === `tDirbrowse` (`Verzeichniswahl`, `300 x 210`) ===
            // 1. Directories-Liste (10..227, 57..181) & Scrollbar (228..242, 57..181)
            if (57..=181).contains(&vy) {
                if (10..=227).contains(&vx) {
                    let row = ((vy - 58).max(0) / 12) as usize;
                    let max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
                    let scroll = self.fsel_dlg.scroll.min(max_scroll);
                    let idx = scroll + row;
                    if let Some(entry) = self.fsel_dlg.entries.get(idx).cloned() {
                        let same = self.fsel_dlg.cursor == idx;
                        self.fsel_dlg.cursor = idx;
                        if is_double_click || (classic_input && same) {
                            match entry {
                                FileSelectEntry::Parent(p) | FileSelectEntry::Subdir(_, p) => {
                                    self.fsel_dlg.current_dir = p;
                                    self.fsel_dlg.refresh();
                                }
                                FileSelectEntry::Drive(drv) => {
                                    self.fsel_dlg.current_dir = PathBuf::from(format!("{}:\\", drv));
                                    self.fsel_dlg.refresh();
                                }
                                _ => {}
                            }
                        }
                    }
                    return;
                } else if (228..=242).contains(&vx) {
                    let max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
                    if vy <= 71 {
                        self.fsel_dlg.scroll = self.fsel_dlg.scroll.saturating_sub(1);
                    } else if vy >= 167 {
                        self.fsel_dlg.scroll = (self.fsel_dlg.scroll + 1).min(max_scroll);
                    } else if vy < 119 {
                        self.fsel_dlg.scroll = self.fsel_dlg.scroll.saturating_sub(9);
                    } else {
                        self.fsel_dlg.scroll = (self.fsel_dlg.scroll + 9).min(max_scroll);
                    }
                    return;
                }
            }

            // 2. Vertikal gestapelte Buttons rechts: OK (249..292, 54..78), Quit (249..292, 84..108), Help (249..292, 114..138)
            if (249..=292).contains(&vx) {
                if (54..=78).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::FselOk);
                    self.commit_file_select_ok();
                    return;
                } else if (84..=108).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::FselCancel);
                    self.close_window(WindowKind::FileSelectDialog);
                    return;
                } else if (114..=138).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::FselHelp);
                    self.open_help_topic(event_loop, "FselDlg");
                    return;
                }
            }
        }

        self.drag_window_if_allowed(vy, window);
    }
}

/// Zeichnet das Skin-Label (`"2.30"` / `"2.40"`) exakt im fetten `9px` hohen Windows-3.1/95-Button-Font
/// (`2px` vertikale Stämme, `1px` horizontale Balken, `y = btn_y + 5..=btn_y + 13`) passend zu `Lang`, `OK`, `Save`, `Quit`, `Help`.
fn draw_classic_setup_btn_label(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    btn_x: i32,
    btn_y: i32,
    btn_w: i32,
    text: &str,
    color: [u8; 4],
) {
    fn glyph_bold_9px(ch: char) -> (i32, [u8; 9]) {
        match ch {
            '2' => (
                6,
                [
                    0b011110, //  ####
                    0b110011, // ##  ##
                    0b000011, //     ##
                    0b000110, //    ##
                    0b001100, //   ##
                    0b011000, //  ##
                    0b110000, // ##
                    0b110000, // ##
                    0b111111, // ######
                ],
            ),
            '3' => (
                6,
                [
                    0b011110, //  ####
                    0b110011, // ##  ##
                    0b000011, //     ##
                    0b000011, //     ##
                    0b001110, //   ###
                    0b000011, //     ##
                    0b000011, //     ##
                    0b110011, // ##  ##
                    0b011110, //  ####
                ],
            ),
            '4' => (
                6,
                [
                    0b000110, //    ##
                    0b001110, //   ###
                    0b010110, //  # ##
                    0b100110, // #  ##
                    0b110110, // ## ##
                    0b111111, // ######
                    0b000110, //    ##
                    0b000110, //    ##
                    0b000110, //    ##
                ],
            ),
            '0' => (
                6,
                [
                    0b011110, //  ####
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b110011, // ##  ##
                    0b011110, //  ####
                ],
            ),
            '.' => (
                2,
                [
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0b11, // ##
                    0b11, // ##
                ],
            ),
            _ => (4, [0; 9]),
        }
    }

    let mut total_w = 0i32;
    for (i, ch) in text.chars().enumerate() {
        if i > 0 {
            total_w += 1;
        }
        total_w += glyph_bold_9px(ch).0;
    }

    let mut cur_x = btn_x + (btn_w - total_w) / 2;
    let base_y = btn_y + 5;
    let w_i32 = dw as i32;
    let h_i32 = dh as i32;

    for ch in text.chars() {
        let (gw, rows) = glyph_bold_9px(ch);
        for (ry, &mask) in rows.iter().enumerate() {
            let py = base_y + ry as i32;
            if py < 0 || py >= h_i32 {
                continue;
            }
            for rx in 0..gw {
                let px = cur_x + rx;
                if px < 0 || px >= w_i32 {
                    continue;
                }
                let bit = gw - 1 - rx;
                if (mask & (1 << bit)) != 0 {
                    let idx = ((py as u32 * dw + px as u32) * 4) as usize;
                    if idx + 3 < frame.len() {
                        frame[idx] = color[0];
                        frame[idx + 1] = color[1];
                        frame[idx + 2] = color[2];
                        frame[idx + 3] = color[3];
                    }
                }
            }
        }
        cur_x += gw + 1;
    }
}
