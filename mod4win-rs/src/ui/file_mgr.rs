//! Prozeduraler Mini-File-Manager (`COPYWND.PAS` & `FILES.PAS`, `365 x 306`)
//! und prozedurales Archiv-Setup (`tarcopt` & `tmorearc` in `dsetup.pas`).

use std::path::PathBuf;
use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{
    Mod4WinApp, PressedButton, UiSharedState, WindowKind, ARCHIVE_WINDOW_WIDTH,
    FILEMAN_WINDOW_WIDTH,
};
use super::archive_mol::list_available_drives;
use super::ini_config::Mod4WinIniConfig;
use super::setup_dlg::{FileSelectEntry, FileSelectState, FileSelectTarget};
use super::widgets::{
    draw_3d_panel, draw_bmp_button_2state, draw_frame_3d, draw_frame_3d_1px, draw_lrswitch,
    draw_oem_text, draw_oem_text_centered, draw_oem_text_mnemonic, draw_rect_outline, fill_rect,
};

/// Dateieintrag in der linken `Files`-Liste des Mini-File-Managers (`COPYWND.PAS`).
#[derive(Clone, Debug)]
pub struct FileManItem {
    pub path: PathBuf,
    pub display_name: String,
    pub size_bytes: u64,
    pub selected: bool,
}

/// Zustand des prozeduralen Mini-File-Managers (`COPYWND.PAS` & `FILES.PAS`, `365 x 306`).
#[derive(Clone, Debug)]
pub struct FileManagerState {
    pub is_copy: bool,
    pub files: Vec<FileManItem>,
    pub dest_files: Vec<(PathBuf, String)>,
    pub file_scroll: usize,
    pub file_cursor: usize,
    pub file_anchor: usize,
    pub target_dir: PathBuf,
    pub dirs: Vec<FileSelectEntry>,
    pub dir_scroll: usize,
    pub dir_cursor: usize,
    pub classic_drag_dir: Option<usize>,
    pub target_path_edit: String,
    pub editing_target: bool,
    pub status_msg: String,
}

impl FileManagerState {
    pub fn new(is_copy: bool, state: &UiSharedState) -> Self {
        let mut files = Vec::new();
        let mut cur_idx = 0usize;
        if !state.playlist.is_empty() {
            for (idx, p) in state.playlist.iter().enumerate() {
                let display_name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("MODULE")
                    .to_uppercase();
                let size_bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let is_curr = idx == state.playlist_idx;
                if is_curr {
                    cur_idx = idx;
                }
                files.push(FileManItem {
                    path: p.clone(),
                    display_name,
                    size_bytes,
                    selected: is_curr,
                });
            }
        } else if let Some(p) = &state.current_file_path {
            let display_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("MODULE")
                .to_uppercase();
            let size_bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            files.push(FileManItem {
                path: p.clone(),
                display_name,
                size_bytes,
                selected: true,
            });
        }

        let target_dir = std::env::current_dir()
            .ok()
            .or_else(|| {
                state
                    .current_file_path
                    .as_ref()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            })
            .unwrap_or_else(|| PathBuf::from("."));

        let mut st = Self {
            is_copy,
            files,
            dest_files: Vec::new(),
            file_scroll: cur_idx.saturating_sub(6),
            file_cursor: cur_idx,
            file_anchor: cur_idx,
            target_dir,
            dirs: Vec::new(),
            dir_scroll: 0,
            dir_cursor: 0,
            classic_drag_dir: None,
            target_path_edit: String::new(),
            editing_target: false,
            status_msg: String::new(),
        };
        st.refresh_dirs();
        st.sync_target_edit();
        st
    }

    pub fn new_lazy(is_copy: bool) -> Self {
        Self {
            is_copy,
            files: Vec::new(),
            dest_files: Vec::new(),
            file_scroll: 0,
            file_cursor: 0,
            file_anchor: 0,
            target_dir: PathBuf::from("."),
            dirs: Vec::new(),
            dir_scroll: 0,
            dir_cursor: 0,
            classic_drag_dir: None,
            target_path_edit: String::new(),
            editing_target: false,
            status_msg: String::new(),
        }
    }

    pub fn refresh_dirs(&mut self) {
        self.dirs.clear();
        self.dest_files.clear();
        self.dir_scroll = 0;
        self.dir_cursor = 0;
        if let Some(parent) = self.target_dir.parent()
            && parent != self.target_dir {
                self.dirs.push(FileSelectEntry::Parent(parent.to_path_buf()));
            }
        let mut subdirs = Vec::new();
        let mut dfiles = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.target_dir) {
            for e in rd.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    subdirs.push((name.to_lowercase(), format!("<{}>", name.to_lowercase()), path));
                } else if UiSharedState::is_tracker_file(&path) {
                    dfiles.push((name.to_lowercase(), path));
                }
            }
        }
        subdirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, label, p) in subdirs {
            self.dirs.push(FileSelectEntry::Subdir(label, p));
        }
        dfiles.sort_by(|a, b| a.0.cmp(&b.0));
        for (lower_name, p) in dfiles {
            self.dest_files.push((p, lower_name));
        }
        for drv in list_available_drives() {
            self.dirs.push(FileSelectEntry::Drive(drv));
        }
    }

    pub fn sync_target_edit(&mut self) {
        let sel_count = self.files.iter().filter(|f| f.selected).count();
        if sel_count == 1 {
            if let Some(f) = self.files.iter().find(|f| f.selected) {
                self.target_path_edit = self
                    .target_dir
                    .join(&f.display_name)
                    .to_string_lossy()
                    .into_owned();
                return;
            }
        } else if sel_count == 0
            && let Some(f) = self.files.get(self.file_cursor) {
                self.target_path_edit = self
                    .target_dir
                    .join(&f.display_name)
                    .to_string_lossy()
                    .into_owned();
                return;
            }
        self.target_path_edit = self.target_dir.to_string_lossy().into_owned();
    }
}

/// Zustand des prozeduralen Archiv-Setups (`tarcopt` `524 x 172` & Parameter-Subdialog `tmorearc` `336 x 148` in `dsetup.pas`).
#[derive(Clone, Debug, Default)]
pub struct ArchiveSetupState {
    /// Für `[ARJ, ZIP, LHA]`: `false` = Unpack-Pfad anzeigen/bearbeiten, `true` = Pack-Pfad anzeigen/bearbeiten
    pub pack_mode: [bool; 3],
    /// Aktuell fokussierte Pfad-Edit-Zeile (`Some(0..=2)` für ARJ, ZIP, LHA)
    pub edit_focus: Option<usize>,
    /// Wenn `Some(0..=2)`, wird der Parameter-Subdialog (`tmorearc`, `336 x 148`) für ARJ (`0`), ZIP (`1`) oder LHA (`2`) angezeigt
    pub more_arc_type: Option<u8>,
    /// Aktuell fokussiertes Parameter-Feld im `tmorearc`-Subdialog (`0 = Unpack`, `1 = Pack`, `2 = Delete`)
    pub more_edit_focus: usize,
    pub more_cmd_extract: String,
    pub more_cmd_pack: String,
    pub more_cmd_del: String,
}

impl Mod4WinApp {
    /// Rendert den prozeduralen Mini-File-Manager (`COPYWND.PAS` & `FILES.PAS`, `365 x 306`, KEIN Handbuch-Bild `bm2.bmp`!).
    pub(crate) fn render_file_manager_window(&self, frame: &mut [u8], dw: u32, dh: u32, _state: &UiSharedState) {
        let cap_dy: i32 = 18;
        let fm = &self.file_man_dlg;
        let loc = &self.locale;
        let title_str = if fm.is_copy {
            loc.tr_or("win", "file_copy_title", "MOD4WIN Copy File(s)")
        } else {
            loc.tr_or("win", "file_move_title", "MOD4WIN Move/Rename File(s)")
        };

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
        draw_oem_text(frame, dw, dh, 22, 4, title_str, [48, 56, 68, 255]);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);

        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];
        let col_gray = [192, 192, 192, 255];
        let col_white = [255, 255, 255, 255];
        let is_de = loc.is_german();

        // Obere 4 Zeilen (`von :`, `aus :`, `&nach:`, `in  :` aus `COPYWND.PAS` Zeilen 497-534)
        let sel_files: Vec<&FileManItem> = fm.files.iter().filter(|f| f.selected).collect();
        let (from_txt, src_arc_txt) = if let Some(f) = sel_files
            .first()
            .copied()
            .or_else(|| fm.files.get(fm.file_cursor))
        {
            let full_p = f.path.to_string_lossy().to_uppercase();
            if let Some((arc_p, inner_p)) = full_p.split_once("::") {
                let inner_disp = if inner_p.starts_with('\\') || inner_p.starts_with('/') {
                    inner_p.replace('/', "\\")
                } else {
                    format!("\\{}", inner_p.replace('/', "\\"))
                };
                (inner_disp, arc_p.to_string())
            } else {
                (full_p, String::new())
            }
        } else {
            (String::new(), String::new())
        };

        let to_raw = fm.target_path_edit.to_uppercase();
        let top_rows = [
            (
                cap_dy + 10,
                if is_de { "von :" } else { "from:" },
                from_txt,
                false,
            ),
            (
                cap_dy + 28,
                if is_de { "aus :" } else { "in  :" },
                src_arc_txt,
                false,
            ),
            (
                cap_dy + 46,
                if is_de { "&nach:" } else { "&to  :" },
                to_raw,
                true,
            ),
            (cap_dy + 64, "in  :", String::new(), false),
        ];

        for (ry, lbl, val, is_edit_row) in &top_rows {
            draw_oem_text_mnemonic(frame, dw, dh, 10, *ry, lbl, col_black);

            fill_rect(frame, dw, dh, 52, *ry - 2, 300, 16, col_black);
            draw_frame_3d_1px(frame, dw, dh, 52, *ry - 2, 300, 16, true);
            let disp: String = if val.chars().count() > 35 {
                let skip = val.chars().count() - 35;
                val.chars().skip(skip).collect()
            } else {
                val.clone()
            };
            draw_oem_text(
                frame,
                dw,
                dh,
                55,
                *ry,
                &disp,
                if *is_edit_row { col_green } else { col_gray },
            );
            if *is_edit_row {
                let cur_x = 55 + (disp.chars().count() as i32) * 8;
                draw_oem_text(frame, dw, dh, cur_x, *ry, "_", col_white);
            }
        }

        // Linke Spalte: "&Filelist:" Header (10, CapDY+82=100) + pfilelist (12, 116, 148, 156) + Scrollbar (160, 116, 14, 156)
        draw_oem_text_mnemonic(frame, dw, dh, 10, cap_dy + 82, "&Filelist:", col_black);

        draw_frame_3d(frame, dw, dh, 9, 113, 166, 162, 2, true);
        fill_rect(frame, dw, dh, 11, 115, 148, 158, col_black);
        let max_fscroll = fm.dest_files.len().saturating_sub(13);
        let fscroll = fm.file_scroll.min(max_fscroll);
        for (row, (path, lower_name)) in fm
            .dest_files
            .iter()
            .skip(fscroll)
            .take(13)
            .enumerate()
        {
            let ry = 116 + (row as i32) * 12;
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let badge = self.assets.filetype_badge(&ext);
            badge.blit_rect_to(0, 0, 28, 12, frame, dw, dh, 13, ry);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(lower_name)
                .to_lowercase();
            let mut s_pad: String = stem.chars().take(8).collect();
            while s_pad.chars().count() < 8 {
                s_pad.push(' ');
            }
            let disp = if ext.is_empty() {
                s_pad
            } else {
                format!("{}.{}", s_pad, ext)
            };
            draw_oem_text(frame, dw, dh, 45, ry, &disp, col_gray);
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            158,
            115,
            15,
            158,
            fscroll,
            13,
            fm.dest_files.len(),
        );

        // Rechte Spalte: "&Directories:" Header (188, CapDY+82=100) + pdirlist (188, 116, 140, 126) + Scrollbar (328, 116, 14, 126)
        draw_oem_text_mnemonic(frame, dw, dh, 188, cap_dy + 82, "&Directories:", col_black);

        draw_frame_3d(frame, dw, dh, 185, 113, 158, 132, 2, true);
        fill_rect(frame, dw, dh, 187, 115, 139, 128, col_black);
        let max_dscroll = fm.dirs.len().saturating_sub(10);
        let dscroll = fm.dir_scroll.min(max_dscroll);
        for (row, (idx, entry)) in fm
            .dirs
            .iter()
            .enumerate()
            .skip(dscroll)
            .take(10)
            .enumerate()
        {
            let ry = 116 + (row as i32) * 12;
            let is_cur = idx == fm.dir_cursor;
            if is_cur {
                fill_rect(frame, dw, dh, 188, ry, 138, 12, col_green);
            }
            let fg = if is_cur { col_black } else { col_green };
            match entry {
                FileSelectEntry::Parent(_) => {
                    self.assets
                        .bmp_drives
                        .blit_rect_to(0, 0, 28, 12, frame, dw, dh, 189, ry);
                    draw_oem_text(frame, dw, dh, 221, ry, "<..>", fg);
                }
                FileSelectEntry::Subdir(lbl, _) | FileSelectEntry::File(lbl, _) => {
                    self.assets
                        .bmp_drives
                        .blit_rect_to(0, 0, 28, 12, frame, dw, dh, 189, ry);
                    let disp: String = lbl.chars().take(13).collect();
                    draw_oem_text(frame, dw, dh, 221, ry, &disp, fg);
                }
                FileSelectEntry::Drive(drv) => {
                    let drv_sx = if *drv == 'A' || *drv == 'B' { 84 } else { 112 };
                    self.assets
                        .bmp_drives
                        .blit_rect_to(drv_sx, 0, 28, 12, frame, dw, dh, 189, ry);
                    draw_oem_text(
                        frame,
                        dw,
                        dh,
                        221,
                        ry,
                        &format!("[-{}-]", drv.to_ascii_lowercase()),
                        fg,
                    );
                }
            }
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            326,
            115,
            15,
            128,
            dscroll,
            10,
            fm.dirs.len(),
        );

        // Buttons unter `pdirlist`: OK (186, 252), Cancel/Quit (243, 252), Help (300, 252) (`COPYWND.PAS` Zeilen 148-153)
        let by = cap_dy + 234; // 252
        draw_frame_3d_1px(frame, dw, dh, 185, by - 1, 44, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            186,
            by,
            self.pressed_btn == Some(PressedButton::FileManOk),
            &self.assets.bsok,
        );
        draw_frame_3d_1px(frame, dw, dh, 242, by - 1, 44, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            243,
            by,
            self.pressed_btn == Some(PressedButton::FileManCancel),
            &self.assets.bscancel,
        );
        draw_frame_3d_1px(frame, dw, dh, 299, by - 1, 44, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            300,
            by,
            self.pressed_btn == Some(PressedButton::FileManHelp),
            &self.assets.bshelp,
        );

        // Untere Statuszeile `INFOstatic` (12, 282, 340, 14) im grauen 3D-Rahmen mit zentrierter schwarzer Schrift
        draw_frame_3d(frame, dw, dh, 9, 280, 346, 18, 2, true);
        fill_rect(frame, dw, dh, 11, 282, 342, 14, col_gray);
        let default_status = if fm.is_copy {
            if is_de {
                "Enter - Datei kopieren"
            } else {
                "Enter - copy file"
            }
        } else if is_de {
            "Enter - Datei bewegen"
        } else {
            "Enter - move file"
        };
        let status_txt = if fm.status_msg.is_empty()
            || fm.status_msg.starts_with("Select destination")
            || fm.status_msg.starts_with("Copy selected")
            || fm.status_msg.starts_with("Move/Rename selected")
        {
            default_status
        } else {
            fm.status_msg.as_str()
        };
        let st_disp: String = status_txt.chars().take(41).collect();
        draw_oem_text_centered(frame, dw, dh, 12, 283, 340, &st_disp, col_black);
    }

    /// Rendert das prozedurale Archiv-Setup (`tarcopt` `524 x 172` + Parameter-Subdialog `tmorearc` `336 x 148` nach `dsetup.pas`, KEIN `bm5.bmp`/`bm6.bmp`!).
    pub(crate) fn render_archive_setup_window(&self, frame: &mut [u8], dw: u32, dh: u32, _state: &UiSharedState) {
        let cap_dy: i32 = 18;
        let cfg = &self.setup_cfg;
        let arc = &self.archive_dlg;
        let loc = &self.locale;
        let is_de = loc.is_german();

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
            loc.tr_or("win", "archive_title", "MOD4WIN Archiver Setup"),
            [48, 56, 68, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);

        let col_black = [0, 0, 0, 255];
        let col_green = [0, 255, 0, 255];
        let col_yellow = [255, 255, 0, 255];
        let col_white = [255, 255, 255, 255];

        // Erhabener 3D-Gruppenrahmen `(7, CapDY+6=24, 508, 110)` (`dsetup.pas` Zeile 292)
        draw_frame_3d_1px(frame, dw, dh, 7, cap_dy + 6, 508, 110, false);

        // 3 Archiver-Zeilen: &ARJ (ry=36), &ZIP (ry=61), &LHA (ry=86)
        let archivers: [(&str, &str, &str, PressedButton, PressedButton); 3] = [
            (
                "&ARJ :",
                &cfg.arj_unpack,
                &cfg.arj_pack,
                PressedButton::ArcBrowse0,
                PressedButton::ArcMore0,
            ),
            (
                "&ZIP :",
                &cfg.zip_unpack,
                &cfg.zip_pack,
                PressedButton::ArcBrowse1,
                PressedButton::ArcMore1,
            ),
            (
                "&LHA :",
                &cfg.lha_unpack,
                &cfg.lha_pack,
                PressedButton::ArcBrowse2,
                PressedButton::ArcMore2,
            ),
        ];

        for (i, (lbl, ext_p, pack_p, browse_btn, more_btn)) in archivers.iter().enumerate() {
            let ry = cap_dy + 18 + (i as i32) * 25; // 36, 61, 86
            let is_pack = arc.pack_mode[i];

            // Label "&ARJ :" / "&ZIP :" / "&LHA :"
            draw_frame_3d_1px(frame, dw, dh, 14, ry - 2, 44, 16, true);
            draw_oem_text_mnemonic(frame, dw, dh, 16, ry, lbl, col_black);

            // LRSWITCH bei (64, ry - 1) mit 1px schwarzem Innenrahmen + 1px 3D-Außenrahmen (`dsetup.pas:293-301`)
            draw_frame_3d_1px(frame, dw, dh, 62, ry - 3, 28, 18, true);
            draw_rect_outline(frame, dw, dh, 63, ry - 2, 26, 16, col_black);
            draw_lrswitch(
                frame,
                dw,
                dh,
                64,
                ry - 1,
                2,
                if is_pack { 2 } else { 1 },
                &self.assets.lrswitch,
            );

            // Modus-Box ("Auspacken" / "Einpacken" bzw. "Unpack" / "Pack") bei (92, ry - 2, 76, 16)
            fill_rect(frame, dw, dh, 92, ry - 2, 76, 16, col_black);
            draw_frame_3d_1px(frame, dw, dh, 92, ry - 2, 76, 16, true);
            let mode_str = if is_pack {
                if is_de {
                    "Einpacken"
                } else {
                    loc.tr_or("archive", "mode_pack", "Pack")
                }
            } else if is_de {
                "Auspacken"
            } else {
                loc.tr_or("archive", "mode_unpack", "Unpack")
            };
            draw_oem_text_centered(frame, dw, dh, 92, ry, 76, mode_str, col_green);

            // Editierbares Pfad-Feld bei (172, ry - 2, 260, 16)
            fill_rect(frame, dw, dh, 172, ry - 2, 260, 16, col_black);
            draw_frame_3d_1px(frame, dw, dh, 172, ry - 2, 260, 16, true);
            let focused = arc.edit_focus == Some(i) && arc.more_arc_type.is_none();
            let raw_p = (if is_pack { *pack_p } else { *ext_p }).to_uppercase();
            let disp_p: String = raw_p.chars().take(30).collect();
            draw_oem_text(frame, dw, dh, 175, ry, &disp_p, col_green);
            if focused {
                let cur_x = 175 + (disp_p.chars().count() as i32) * 8;
                draw_oem_text(frame, dw, dh, cur_x, ry, "_", col_white);
            }

            // Browse-Button (`B_BROWSE.bmp` 21x20) bei (442, ry - 4) + More-Button (`B_MORE.bmp` 42x20) bei (466, ry - 4)
            draw_frame_3d_1px(frame, dw, dh, 441, ry - 5, 23, 22, true);
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                442,
                ry - 4,
                self.pressed_btn == Some(*browse_btn),
                &self.assets.b_browse,
            );
            draw_frame_3d_1px(frame, dw, dh, 465, ry - 5, 44, 22, true);
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                466,
                ry - 4,
                self.pressed_btn == Some(*more_btn),
                &self.assets.b_more,
            );
        }

        // Zeile 4 (ry = CapDY + 93 = 111): "&Ent/Pack-Fenster anzeigen :" + Statusbox ("++++ EIN ++++" / "---- AUS ----") + LRSWITCH
        let ry4 = cap_dy + 93;
        draw_frame_3d_1px(frame, dw, dh, 14, ry4 - 2, 316, 16, true);
        let row4_lbl = if is_de {
            "&Ent/Pack-Fenster anzeigen"
        } else {
            "&Show pack/unpack window"
        };
        draw_oem_text_mnemonic(frame, dw, dh, 16, ry4, row4_lbl, col_black);
        draw_oem_text(frame, dw, dh, 320, ry4, ":", col_black);

        fill_rect(frame, dw, dh, 349, ry4 - 2, 108, 16, col_black);
        draw_frame_3d_1px(frame, dw, dh, 349, ry4 - 2, 108, 16, true);
        let state_str = if cfg.show_pack_window {
            if is_de {
                "++++ EIN ++++"
            } else {
                "++++ ON ++++"
            }
        } else if is_de {
            "---- AUS ----"
        } else {
            "---- OFF ----"
        };
        draw_oem_text_centered(frame, dw, dh, 349, ry4, 108, state_str, col_green);

        draw_frame_3d_1px(frame, dw, dh, 481, ry4 - 3, 28, 18, true);
        draw_rect_outline(frame, dw, dh, 482, ry4 - 2, 26, 16, col_black);
        draw_lrswitch(
            frame,
            dw,
            dh,
            483,
            ry4 - 1,
            2,
            if cfg.show_pack_window { 2 } else { 1 },
            &self.assets.lrswitch,
        );

        // Untere Zeile (`y = CapDY + 122 = 140`): volles `M_LABEL.bmp` (`94x20`) ohne Rahmen bei (35, 140) + OK (389), Cancel (430), Help (471)
        let by = cap_dy + 122; // 140
        self.assets
            .m_label
            .blit_rect_to(0, 0, 94, 20, frame, dw, dh, 35, by);

        draw_frame_3d_1px(frame, dw, dh, 388, by - 1, 126, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            389,
            by,
            self.pressed_btn == Some(PressedButton::ArcOk),
            &self.assets.bsok,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            430,
            by,
            self.pressed_btn == Some(PressedButton::ArcCancel),
            &self.assets.bscancel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            471,
            by,
            self.pressed_btn == Some(PressedButton::ArcHelp),
            &self.assets.bshelp,
        );

        // Falls `More` geklickt wurde: Parameter-Subdialog (`tmorearc`, `336 x 148`, `dsetup.pas` Zeilen 881-1018) zentriert darüberlegen!
        if let Some(arc_idx) = arc.more_arc_type {
            let sx = 94;
            let sy = 12;
            let sw = 336u32;
            let sh = 148u32;
            draw_3d_panel(frame, dw, dh, sx, sy, sw, sh, false, [192, 192, 192, 255]);
            draw_rect_outline(frame, dw, dh, sx, sy, sw, sh, col_black);
            fill_rect(frame, dw, dh, sx + 2, sy + 2, sw - 4, 16, [0, 0, 128, 255]);
            let sub_title = match arc_idx {
                0 => loc.tr_or("archive", "params_arj", "MOD4WIN ARJ Parameters"),
                1 => loc.tr_or("archive", "params_zip", "MOD4WINPKZIP/PKUNZIP Parameters"),
                _ => loc.tr_or("archive", "params_lha", "MOD4WIN LHA Parameters"),
            };
            draw_oem_text(frame, dw, dh, sx + 12, sy + 4, sub_title, [255, 255, 255, 255]);

            draw_rect_outline(frame, dw, dh, sx + 7, sy + 25, 321, 85, col_black);
            let param_rows = [
                (0usize, loc.tr_or("archive", "lbl_unpack", "Unpack    :"), &arc.more_cmd_extract),
                (1usize, loc.tr_or("archive", "lbl_pack", "Pack      :"), &arc.more_cmd_pack),
                (2usize, loc.tr_or("archive", "lbl_delete", "Delete    :"), &arc.more_cmd_del),
            ];
            for (p_idx, p_lbl, p_val) in &param_rows {
                let pry = sy + 36 + (*p_idx as i32) * 25;
                draw_frame_3d_1px(frame, dw, dh, sx + 14, pry - 2, 92, 16, true);
                draw_oem_text(frame, dw, dh, sx + 16, pry, p_lbl, col_black);

                fill_rect(frame, dw, dh, sx + 118, pry - 2, 204, 16, col_black);
                draw_frame_3d_1px(frame, dw, dh, sx + 118, pry - 2, 204, 16, true);
                let is_foc = arc.more_edit_focus == *p_idx;
                let p_disp: String = if is_foc {
                    format!("{}_", p_val).chars().take(24).collect()
                } else {
                    p_val.chars().take(24).collect()
                };
                draw_oem_text(
                    frame,
                    dw,
                    dh,
                    sx + 121,
                    pry,
                    &p_disp,
                    if is_foc { col_yellow } else { col_green },
                );
            }

            let mby = sy + 118;
            self.assets
                .m_label
                .blit_rect_to(0, 0, 94, 20, frame, dw, dh, sx + 35, mby);

            draw_frame_3d_1px(frame, dw, dh, sx + 161, mby - 1, 167, 23, true);
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                sx + 162,
                mby,
                self.pressed_btn == Some(PressedButton::ArcMoreOk),
                &self.assets.bsok,
            );
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                sx + 203,
                mby,
                self.pressed_btn == Some(PressedButton::ArcMoreCancel),
                &self.assets.bscancel,
            );
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                sx + 244,
                mby,
                self.pressed_btn == Some(PressedButton::ArcMoreDefault),
                &self.assets.b_default,
            );
            draw_bmp_button_2state(
                frame,
                dw,
                dh,
                sx + 285,
                mby,
                self.pressed_btn == Some(PressedButton::ArcMoreHelp),
                &self.assets.bshelp,
            );
        }
    }

    pub(crate) fn execute_file_manager_ok(&mut self) {
        let is_copy = self.file_man_dlg.is_copy;
        let mut selected_indices: Vec<usize> = self
            .file_man_dlg
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if f.selected { Some(i) } else { None })
            .collect();
        if selected_indices.is_empty() && !self.file_man_dlg.files.is_empty() {
            selected_indices.push(
                self.file_man_dlg
                    .file_cursor
                    .min(self.file_man_dlg.files.len() - 1),
            );
        }

        let target_edit = PathBuf::from(self.file_man_dlg.target_path_edit.trim());
        let mut ok_count = 0usize;
        let mut last_err = None;

        for &idx in &selected_indices {
            let src = self.file_man_dlg.files[idx].path.clone();
            let dst = if selected_indices.len() == 1 && !target_edit.as_os_str().is_empty() {
                if target_edit.is_dir() {
                    target_edit.join(&self.file_man_dlg.files[idx].display_name)
                } else {
                    target_edit.clone()
                }
            } else {
                self.file_man_dlg
                    .target_dir
                    .join(&self.file_man_dlg.files[idx].display_name)
            };

            if src == dst {
                continue;
            }

            if is_copy {
                match std::fs::copy(&src, &dst) {
                    Ok(_) => ok_count += 1,
                    Err(e) => last_err = Some(e.to_string()),
                }
            } else {
                let moved = std::fs::rename(&src, &dst).or_else(|_| {
                    std::fs::copy(&src, &dst).and_then(|_| std::fs::remove_file(&src))
                });
                match moved {
                    Ok(_) => {
                        ok_count += 1;
                        self.file_man_dlg.files[idx].path = dst.clone();
                        if let Some(fname) = dst.file_name().and_then(|n| n.to_str()) {
                            self.file_man_dlg.files[idx].display_name = fname.to_uppercase();
                        }
                        let mut st = self.shared_state.lock().unwrap();
                        for p in &mut st.playlist {
                            if *p == src {
                                *p = dst.clone();
                            }
                        }
                        if st.current_file_path.as_ref() == Some(&src) {
                            st.current_file_path = Some(dst.clone());
                            if let Some(fname) = dst.file_name().and_then(|n| n.to_str()) {
                                st.filename = fname.to_uppercase();
                            }
                        }
                    }
                    Err(e) => last_err = Some(e.to_string()),
                }
            }
        }

        if let Some(err) = last_err {
            self.file_man_dlg.status_msg = format!("Error: {}", err);
        } else {
            self.file_man_dlg.status_msg = if is_copy {
                format!("Copied {} file(s).", ok_count)
            } else {
                format!("Moved/Renamed {} file(s).", ok_count)
            };
            self.close_window(WindowKind::FileManager);
        }
    }

    pub(crate) fn handle_file_manager_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if vy <= 18 {
            if vx <= 20 || vx >= (FILEMAN_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::FileManager);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        self.file_man_dlg.editing_target = false;
        self.file_man_dlg.classic_drag_dir = None;
        let classic_input = self.shared_state.lock().unwrap().classic_input;
        let ctrl = self.modifiers.state().control_key();
        let shift = self.modifiers.state().shift_key();

        // Zeile 3 (`to  :` Edit-Feld bei 52..352, 62..78)
        if (62..=78).contains(&vy) && (52..=352).contains(&vx) {
            self.file_man_dlg.editing_target = true;
            return;
        }

        // Linke `Files`-Liste (10..159, 114..274) & Scrollbar (160..175, 114..274)
        if (114..=274).contains(&vy) {
            if (10..=159).contains(&vx) {
                let row = ((vy - 116).max(0) / 12) as usize;
                let max_s = self.file_man_dlg.files.len().saturating_sub(13);
                let scroll = self.file_man_dlg.file_scroll.min(max_s);
                let idx = scroll + row;
                if idx < self.file_man_dlg.files.len() {
                    self.file_man_dlg.file_cursor = idx;
                    if classic_input {
                        self.file_man_dlg.files[idx].selected =
                            !self.file_man_dlg.files[idx].selected;
                        self.file_man_dlg.file_anchor = idx;
                    } else if shift {
                        if !ctrl {
                            for f in &mut self.file_man_dlg.files {
                                f.selected = false;
                            }
                        }
                        let a = self.file_man_dlg.file_anchor.min(idx);
                        let b = self.file_man_dlg.file_anchor.max(idx);
                        for i in a..=b {
                            if let Some(f) = self.file_man_dlg.files.get_mut(i) {
                                f.selected = true;
                            }
                        }
                    } else if ctrl {
                        self.file_man_dlg.files[idx].selected =
                            !self.file_man_dlg.files[idx].selected;
                        self.file_man_dlg.file_anchor = idx;
                    } else {
                        for (i, f) in self.file_man_dlg.files.iter_mut().enumerate() {
                            f.selected = i == idx;
                        }
                        self.file_man_dlg.file_anchor = idx;
                    }
                    self.file_man_dlg.sync_target_edit();
                }
                return;
            } else if (160..=175).contains(&vx) {
                let max_s = self.file_man_dlg.files.len().saturating_sub(13);
                if vy <= 128 {
                    self.file_man_dlg.file_scroll = self.file_man_dlg.file_scroll.saturating_sub(1);
                } else if vy >= 260 {
                    self.file_man_dlg.file_scroll = (self.file_man_dlg.file_scroll + 1).min(max_s);
                } else if vy < 194 {
                    self.file_man_dlg.file_scroll =
                        self.file_man_dlg.file_scroll.saturating_sub(12);
                } else {
                    self.file_man_dlg.file_scroll = (self.file_man_dlg.file_scroll + 12).min(max_s);
                }
                return;
            }
        }

        // Rechte `Dirs:`-Liste (186..327, 114..242) & Scrollbar (328..343, 114..242)
        if (114..=242).contains(&vy) {
            if (186..=327).contains(&vx) {
                let row = ((vy - 116).max(0) / 12) as usize;
                let max_s = self.file_man_dlg.dirs.len().saturating_sub(10);
                let scroll = self.file_man_dlg.dir_scroll.min(max_s);
                let idx = scroll + row;
                if let Some(entry) = self.file_man_dlg.dirs.get(idx).cloned() {
                    self.file_man_dlg.dir_cursor = idx;
                    if is_double_click {
                        match entry {
                            FileSelectEntry::Parent(p) | FileSelectEntry::Subdir(_, p) => {
                                self.file_man_dlg.target_dir = p;
                                self.file_man_dlg.refresh_dirs();
                                self.file_man_dlg.sync_target_edit();
                            }
                            FileSelectEntry::Drive(drv) => {
                                self.file_man_dlg.target_dir = PathBuf::from(format!("{}:\\", drv));
                                self.file_man_dlg.refresh_dirs();
                                self.file_man_dlg.sync_target_edit();
                            }
                            _ => {}
                        }
                    } else if classic_input {
                        self.file_man_dlg.classic_drag_dir = Some(idx);
                    }
                }
                return;
            } else if (328..=343).contains(&vx) {
                let max_s = self.file_man_dlg.dirs.len().saturating_sub(10);
                if vy <= 128 {
                    self.file_man_dlg.dir_scroll = self.file_man_dlg.dir_scroll.saturating_sub(1);
                } else if vy >= 228 {
                    self.file_man_dlg.dir_scroll = (self.file_man_dlg.dir_scroll + 1).min(max_s);
                } else if vy < 178 {
                    self.file_man_dlg.dir_scroll = self.file_man_dlg.dir_scroll.saturating_sub(9);
                } else {
                    self.file_man_dlg.dir_scroll = (self.file_man_dlg.dir_scroll + 9).min(max_s);
                }
                return;
            }
        }

        // Buttons unter `Dirs:` (y = 251..274): OK (185..228), Cancel/Quit (242..285), Help (299..342)
        if (251..=274).contains(&vy) {
            if (185..=228).contains(&vx) {
                self.pressed_btn = Some(PressedButton::FileManOk);
                self.execute_file_manager_ok();
                return;
            } else if (242..=285).contains(&vx) {
                self.pressed_btn = Some(PressedButton::FileManCancel);
                self.close_window(WindowKind::FileManager);
                return;
            } else if (299..=342).contains(&vx) {
                self.pressed_btn = Some(PressedButton::FileManHelp);
                self.open_help_topic(event_loop, "fmdlg");
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn open_archive_more_subdialog(&mut self, arc_idx: u8) {
        self.archive_dlg.edit_focus = None;
        self.archive_dlg.more_arc_type = Some(arc_idx);
        self.archive_dlg.more_edit_focus = 0;
        match arc_idx {
            0 => {
                self.archive_dlg.more_cmd_extract = self.setup_cfg.arj_cmd_extract.clone();
                self.archive_dlg.more_cmd_pack = self.setup_cfg.arj_cmd_pack.clone();
                self.archive_dlg.more_cmd_del = self.setup_cfg.arj_cmd_del.clone();
            }
            1 => {
                self.archive_dlg.more_cmd_extract = self.setup_cfg.zip_cmd_extract.clone();
                self.archive_dlg.more_cmd_pack = self.setup_cfg.zip_cmd_pack.clone();
                self.archive_dlg.more_cmd_del = self.setup_cfg.zip_cmd_del.clone();
            }
            _ => {
                self.archive_dlg.more_cmd_extract = self.setup_cfg.lha_cmd_extract.clone();
                self.archive_dlg.more_cmd_pack = self.setup_cfg.lha_cmd_pack.clone();
                self.archive_dlg.more_cmd_del = self.setup_cfg.lha_cmd_del.clone();
            }
        }
    }

    pub(crate) fn commit_archive_more_subdialog(&mut self) {
        if let Some(arc_idx) = self.archive_dlg.more_arc_type.take() {
            match arc_idx {
                0 => {
                    self.setup_cfg.arj_cmd_extract = self.archive_dlg.more_cmd_extract.clone();
                    self.setup_cfg.arj_cmd_pack = self.archive_dlg.more_cmd_pack.clone();
                    self.setup_cfg.arj_cmd_del = self.archive_dlg.more_cmd_del.clone();
                }
                1 => {
                    self.setup_cfg.zip_cmd_extract = self.archive_dlg.more_cmd_extract.clone();
                    self.setup_cfg.zip_cmd_pack = self.archive_dlg.more_cmd_pack.clone();
                    self.setup_cfg.zip_cmd_del = self.archive_dlg.more_cmd_del.clone();
                }
                _ => {
                    self.setup_cfg.lha_cmd_extract = self.archive_dlg.more_cmd_extract.clone();
                    self.setup_cfg.lha_cmd_pack = self.archive_dlg.more_cmd_pack.clone();
                    self.setup_cfg.lha_cmd_del = self.archive_dlg.more_cmd_del.clone();
                }
            }
        }
    }

    pub(crate) fn reset_archive_more_defaults(&mut self, arc_idx: u8) {
        let def = Mod4WinIniConfig::default();
        match arc_idx {
            0 => {
                self.archive_dlg.more_cmd_extract = def.arj_cmd_extract;
                self.archive_dlg.more_cmd_pack = def.arj_cmd_pack;
                self.archive_dlg.more_cmd_del = def.arj_cmd_del;
            }
            1 => {
                self.archive_dlg.more_cmd_extract = def.zip_cmd_extract;
                self.archive_dlg.more_cmd_pack = def.zip_cmd_pack;
                self.archive_dlg.more_cmd_del = def.zip_cmd_del;
            }
            _ => {
                self.archive_dlg.more_cmd_extract = def.lha_cmd_extract;
                self.archive_dlg.more_cmd_pack = def.lha_cmd_pack;
                self.archive_dlg.more_cmd_del = def.lha_cmd_del;
            }
        }
    }

    pub(crate) fn commit_archive_setup_ok(&mut self) {
        self.sync_live_setup_to_state();
        {
            let st = self.shared_state.lock().unwrap();
            let ini_path = Mod4WinIniConfig::default_ini_path();
            let _ = st.ini_config.save_to_file(&ini_path);
        }
        self.archive_dlg.edit_focus = None;
        self.archive_dlg.more_arc_type = None;
        self.close_window(WindowKind::ArchiveSetup);
    }

    pub(crate) fn handle_archive_setup_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        // Wenn der `tmorearc` Parameter-Subdialog (`sx=94, sy=12, 336x148`) aktiv ist:
        if let Some(arc_idx) = self.archive_dlg.more_arc_type {
            let sx = 94;
            let sy = 12;
            for p_idx in 0..3usize {
                let pry = sy + 36 + (p_idx as i32) * 25;
                if (pry - 3..=pry + 15).contains(&vy) && (sx + 118..=sx + 322).contains(&vx) {
                    self.archive_dlg.more_edit_focus = p_idx;
                    return;
                }
            }
            let mby = sy + 118;
            if (mby - 1..=mby + 22).contains(&vy) {
                if (sx + 162..=sx + 202).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::ArcMoreOk);
                    self.commit_archive_more_subdialog();
                    return;
                } else if (sx + 203..=sx + 243).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::ArcMoreCancel);
                    self.archive_dlg.more_arc_type = None;
                    return;
                } else if (sx + 244..=sx + 284).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::ArcMoreDefault);
                    self.reset_archive_more_defaults(arc_idx);
                    return;
                } else if (sx + 285..=sx + 326).contains(&vx) {
                    self.pressed_btn = Some(PressedButton::ArcMoreHelp);
                    self.open_help_topic(event_loop, "arcparm");
                    return;
                }
            }
            return;
        }

        if vy <= 18 {
            if vx <= 20 || vx >= (ARCHIVE_WINDOW_WIDTH as i32 - 20) {
                self.close_window(WindowKind::ArchiveSetup);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        self.archive_dlg.edit_focus = None;

        // 3 Archiver-Zeilen: ARJ (ry=36), ZIP (ry=61), LHA (ry=86)
        for i in 0..3usize {
            let ry = 36 + (i as i32) * 25;
            if (ry - 5..=ry + 17).contains(&vy) {
                if (60..=168).contains(&vx) {
                    self.archive_dlg.pack_mode[i] = !self.archive_dlg.pack_mode[i];
                    return;
                } else if (172..=432).contains(&vx) {
                    self.archive_dlg.edit_focus = Some(i);
                    return;
                } else if (441..=464).contains(&vx) {
                    self.pressed_btn = Some(match i {
                        0 => PressedButton::ArcBrowse0,
                        1 => PressedButton::ArcBrowse1,
                        _ => PressedButton::ArcBrowse2,
                    });
                    let cur_p = match (i, self.archive_dlg.pack_mode[i]) {
                        (0, false) => &self.setup_cfg.arj_unpack,
                        (0, true) => &self.setup_cfg.arj_pack,
                        (1, false) => &self.setup_cfg.zip_unpack,
                        (1, true) => &self.setup_cfg.zip_pack,
                        (_, false) => &self.setup_cfg.lha_unpack,
                        (_, true) => &self.setup_cfg.lha_pack,
                    };
                    self.fsel_dlg =
                        FileSelectState::new(FileSelectTarget::ArchiveExe(i as u8), cur_p);
                    self.open_window(event_loop, WindowKind::FileSelectDialog);
                    return;
                } else if (465..=509).contains(&vx) {
                    self.pressed_btn = Some(match i {
                        0 => PressedButton::ArcMore0,
                        1 => PressedButton::ArcMore1,
                        _ => PressedButton::ArcMore2,
                    });
                    self.open_archive_more_subdialog(i as u8);
                    return;
                }
            }
        }

        // Zeile 4 (`Show archiver progress in window`, ry = 111)
        if (108..=126).contains(&vy) && (14..=508).contains(&vx) {
            self.setup_cfg.show_pack_window = !self.setup_cfg.show_pack_window;
            return;
        }

        // Untere Buttons (`y = 139..162`): OK (389..429), Cancel (430..470), Help (471..513)
        if (139..=162).contains(&vy) {
            if (389..=429).contains(&vx) {
                self.pressed_btn = Some(PressedButton::ArcOk);
                self.commit_archive_setup_ok();
                return;
            } else if (430..=470).contains(&vx) {
                self.pressed_btn = Some(PressedButton::ArcCancel);
                self.setup_cfg = self.setup_backup_cfg.clone();
                self.close_window(WindowKind::ArchiveSetup);
                return;
            } else if (471..=513).contains(&vx) {
                self.pressed_btn = Some(PressedButton::ArcHelp);
                self.open_help_topic(event_loop, "ArcOptDlg");
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }
}
