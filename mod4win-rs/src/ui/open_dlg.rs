//! 3-Spalten-Commander MOD 4 WIN Open Files (`newopen.pas`, `558 x 358`)
//! inklusive virtueller `.MOL`-Playlist- und `.ZIP`-Archiv-Navigation.

use std::path::PathBuf;
use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{Mod4WinApp, PressedButton, UiSharedState, WindowKind, OPEN_WINDOW_WIDTH};
use super::archive_mol::{
    extract_zip_entry, is_supported_archive_ext, list_available_drives, list_zip_entries,
    load_mol_file, matches_wildcard, save_mol_file, ZipEntryInfo,
};
use super::widgets::{
    draw_3d_frame_only, draw_3d_panel, draw_bmp_button_2state, draw_oem_text,
    draw_oem_text_centered, draw_oem_text_mnemonic, fill_rect,
};

/// Virtueller oder physischer Navigationskontext im linken Browser des Open-Dialogs (`newopen.pas`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenBrowseContext {
    /// Normales Dateisystem-Verzeichnis
    Directory(PathBuf),
    /// Direktes Hineinwechseln in eine `.MOL`-Playlist wie in einen Ordner (`inplaylist = true` in `newopen.pas`)
    MolPlaylist {
        mol_path: PathBuf,
        parent_dir: PathBuf,
    },
    /// Direktes Hineinwechseln in ein `.ZIP`-Archiv wie in einen Ordner (`inarchive = true` in `newopen.pas`)
    ZipArchive {
        zip_path: PathBuf,
        parent_dir: PathBuf,
    },
}

/// Quelle einer Datei in der linken Dateiliste des Open-Dialogs.
#[derive(Clone, Debug)]
pub enum OpenFileSource {
    Filesystem(PathBuf),
    ZipEntry {
        zip_path: PathBuf,
        entry: ZipEntryInfo,
    },
}

/// Eintrag in der linken Dateiliste (`Filelist`) des Open-Dialogs.
#[derive(Clone, Debug)]
pub struct OpenFileItem {
    pub display_name: String,
    pub ext: String,
    pub size_bytes: u64,
    pub source: OpenFileSource,
    pub selected: bool,
}

impl OpenFileItem {
    /// Löst den Eintrag zu einem abspielbaren Dateipfad auf (extrahiert bei `.ZIP` in den Temp-Cache).
    pub fn resolve_to_path(&self) -> Option<PathBuf> {
        match &self.source {
            OpenFileSource::Filesystem(p) => Some(p.clone()),
            OpenFileSource::ZipEntry { zip_path, entry } => extract_zip_entry(zip_path, entry),
        }
    }
}

/// Typ eines Eintrags in der mittleren `Directories`-Liste des Open-Dialogs (`newopen.pas`).
#[derive(Clone, Debug)]
pub enum OpenDirKind {
    Parent(PathBuf),
    Subdir(PathBuf),
    MolFile(PathBuf),
    ZipFile(PathBuf),
    Drive(char),
}

/// Eintrag in der mittleren `Directories`-Liste (`<..>`, `<subdir>`, `.MOL`, `.ZIP`, `[-c-]`).
#[derive(Clone, Debug)]
pub struct OpenDirItem {
    pub label: String,
    pub kind: OpenDirKind,
}

/// Eintrag in der rechten Arbeits-Playlist (`Playlist`) des Open-Dialogs.
#[derive(Clone, Debug)]
pub struct OpenPlaylistItem {
    pub path: PathBuf,
    pub display_name: String,
    pub ext: String,
    pub selected: bool,
}

/// Vollständiger Zustand des Commander-Dialogs „MOD 4 WIN Open Files“ (`newopen.pas`).
#[derive(Clone, Debug)]
pub struct OpenDialogState {
    pub context: OpenBrowseContext,
    pub left_files: Vec<OpenFileItem>,
    pub left_scroll: usize,
    pub left_cursor: usize,
    pub left_anchor: usize,
    pub mid_dirs: Vec<OpenDirItem>,
    pub mid_scroll: usize,
    pub mid_cursor: usize,
    pub right_playlist: Vec<OpenPlaylistItem>,
    pub right_scroll: usize,
    pub right_cursor: usize,
    pub right_anchor: usize,
    /// Zuletzt angeklickte Liste (`0 = Filelist`, `1 = Directories`, `2 = Playlist`) für `Strg + A`
    pub active_list: u8,
    /// Für `--classic-input`: Gezogener Ordner-/Archiv-Eintrag aus `Directories` in die `Filelist`
    pub classic_drag_dir: Option<usize>,
    /// Aktive Such-/Wildcard-Maske (z.B. `*.*`, `*.XM`, `*.MOD`)
    pub filter_mask: String,
    pub editing_filter: bool,
    pub editing_right_filter: bool,
    pub filter_deselect: bool,
    pub filter_fresh: bool,
    /// Format-Toggles: `[MOD, XM, S3M, MTM, OTHER]` (alle `true` = alle Formate sichtbar)
    pub format_toggles: [bool; 5],
    /// Eingabemodus zum Speichern einer `.MOL`-Playlist (`Save`-Button)
    pub saving_mol: bool,
    pub save_mol_name: String,
    /// Statuszeile unten (`12, 330, 532, 16`)
    pub status_msg: String,
}

impl OpenDialogState {
    pub fn new(initial_dir: PathBuf, current_playlist: &[PathBuf]) -> Self {
        let mut st = Self::new_lazy(initial_dir, current_playlist);
        st.sync_from_playlist(current_playlist);
        st.refresh_browser();
        st
    }

    /// Erzeugt den Dialogzustand ohne sofortiges Verzeichnis-/Laufwerks-Scannen oder Playlist-Kopieren beim Programmstart.
    pub fn new_lazy(initial_dir: PathBuf, _current_playlist: &[PathBuf]) -> Self {
        Self {
            context: OpenBrowseContext::Directory(initial_dir),
            left_files: Vec::new(),
            left_scroll: 0,
            left_cursor: 0,
            left_anchor: 0,
            mid_dirs: Vec::new(),
            mid_scroll: 0,
            mid_cursor: 0,
            right_playlist: Vec::new(),
            right_scroll: 0,
            right_cursor: 0,
            right_anchor: 0,
            active_list: 1,
            classic_drag_dir: None,
            filter_mask: "*.*".to_string(),
            editing_filter: false,
            editing_right_filter: false,
            filter_deselect: false,
            filter_fresh: false,
            format_toggles: [true; 5],
            saving_mol: false,
            save_mol_name: "PLAYLIST.MOL".to_string(),
            status_msg: String::new(),
        }
    }

    pub fn current_dir_path(&self) -> PathBuf {
        match &self.context {
            OpenBrowseContext::Directory(d) => d.clone(),
            OpenBrowseContext::MolPlaylist { parent_dir, .. } => parent_dir.clone(),
            OpenBrowseContext::ZipArchive { parent_dir, .. } => parent_dir.clone(),
        }
    }

    pub fn path_bar_text(&self) -> String {
        match &self.context {
            OpenBrowseContext::Directory(d) => {
                let mut s = d.to_string_lossy().to_uppercase();
                if !s.ends_with('\\') {
                    s.push('\\');
                }
                s
            }
            OpenBrowseContext::MolPlaylist { mol_path, .. } => {
                mol_path.to_string_lossy().to_uppercase()
            }
            OpenBrowseContext::ZipArchive { zip_path, .. } => {
                zip_path.to_string_lossy().to_uppercase()
            }
        }
    }

    pub fn sync_from_playlist(&mut self, playlist: &[PathBuf]) {
        self.right_playlist = playlist
            .iter()
            .map(|p| {
                let display_name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("MODULE")
                    .to_uppercase();
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_uppercase();
                OpenPlaylistItem {
                    path: p.clone(),
                    display_name,
                    ext,
                    selected: false,
                }
            })
            .collect();
        self.right_scroll = 0;
        self.right_cursor = 0;
    }

    pub fn format_allowed(&self, ext: &str) -> bool {
        let idx = match ext.to_ascii_uppercase().as_str() {
            "MOD" | "WOW" => 0,
            "XM" => 1,
            "S3M" | "IT" | "UMX" => 2,
            "MTM" => 3,
            "669" | "STM" | "FAR" | "OKT" | "ULT" => 4,
            _ => return false,
        };
        self.format_toggles[idx]
    }

    /// Öffnet die `Sel?`-Auswahlmaske für links (`right_list = false`) oder rechts (`right_list = true`)
    /// im Selektier- (`deselect = false`, `+`) oder Deselektier-Modus (`deselect = true`, `-`).
    pub fn open_selection_mask(&mut self, right_list: bool, deselect: bool) {
        self.saving_mol = false;
        self.editing_filter = !right_list;
        self.editing_right_filter = right_list;
        self.filter_deselect = deselect;
        self.filter_mask = "*.*".to_string();
        self.filter_fresh = true;
    }

    /// Wendet die eingegebene `Sel?`-Wildcard-Maske (`*.*`, `*.XM`, etc.) auf die linke `Filelist`
    /// bzw. rechte `Playlist` an und markiert (`+`) bzw. demarkiert (`-`) alle passenden Einträge,
    /// exakt wie `dolistselections` in `newopen.pas`.
    pub fn apply_selection_mask(&mut self) {
        let mask = if self.filter_mask.trim().is_empty() {
            "*.*".to_string()
        } else {
            self.filter_mask.trim().to_string()
        };
        let target_state = !self.filter_deselect;
        if self.editing_right_filter {
            for p in &mut self.right_playlist {
                if matches_wildcard(&p.display_name, &mask) {
                    p.selected = target_state;
                }
            }
            self.active_list = 2;
        } else {
            for f in &mut self.left_files {
                if matches_wildcard(&f.display_name, &mask) {
                    f.selected = target_state;
                }
            }
            self.active_list = 0;
        }
        self.editing_filter = false;
        self.editing_right_filter = false;
        self.filter_deselect = false;
        self.filter_fresh = false;
        self.filter_mask = "*.*".to_string();
    }

    /// Aktualisiert `left_files` und `mid_dirs` anhand des aktuellen `context`, `filter_mask` und `format_toggles`.
    pub fn refresh_browser(&mut self) {
        self.left_files.clear();
        self.mid_dirs.clear();
        self.left_scroll = 0;
        self.left_cursor = 0;
        self.mid_scroll = 0;
        self.mid_cursor = 0;

        match &self.context {
            OpenBrowseContext::Directory(dir) => {
                if let Some(parent) = dir.parent() {
                    if parent != dir {
                        self.mid_dirs.push(OpenDirItem {
                            label: "<..>".to_string(),
                            kind: OpenDirKind::Parent(parent.to_path_buf()),
                        });
                    }
                }

                let mut subdirs = Vec::new();
                let mut mols = Vec::new();
                let mut zips = Vec::new();
                let mut files = Vec::new();

                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let Ok(ft) = entry.file_type() else {
                            continue;
                        };
                        let path = entry.path();
                        let fname = entry.file_name().to_string_lossy().to_string();
                        if ft.is_dir() {
                            subdirs.push((fname.to_lowercase(), fname, path));
                        } else if ft.is_file() {
                            let ext = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_uppercase();
                            if ext == "MOL" {
                                mols.push((fname.to_uppercase(), path));
                            } else if is_supported_archive_ext(&ext) {
                                zips.push((fname.to_uppercase(), path));
                            } else if UiSharedState::is_tracker_file(&path)
                                && self.format_allowed(&ext)
                                && matches_wildcard(&fname, &self.filter_mask)
                            {
                                let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                files.push(OpenFileItem {
                                    display_name: fname.to_uppercase(),
                                    ext,
                                    size_bytes,
                                    source: OpenFileSource::Filesystem(path),
                                    selected: false,
                                });
                            }
                        }
                    }
                }

                subdirs.sort_by(|a, b| a.0.cmp(&b.0));
                for (_, fname, p) in subdirs {
                    self.mid_dirs.push(OpenDirItem {
                        label: format!("<{}>", fname.to_lowercase()),
                        kind: OpenDirKind::Subdir(p),
                    });
                }

                mols.sort_by(|a, b| a.0.cmp(&b.0));
                for (fname, p) in mols {
                    self.mid_dirs.push(OpenDirItem {
                        label: fname,
                        kind: OpenDirKind::MolFile(p),
                    });
                }

                zips.sort_by(|a, b| a.0.cmp(&b.0));
                for (fname, p) in zips {
                    self.mid_dirs.push(OpenDirItem {
                        label: fname,
                        kind: OpenDirKind::ZipFile(p),
                    });
                }

                for drv in list_available_drives() {
                    self.mid_dirs.push(OpenDirItem {
                        label: format!("[-{}-]", drv.to_ascii_lowercase()),
                        kind: OpenDirKind::Drive(drv),
                    });
                }

                files.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                self.left_files = files;
            }
            OpenBrowseContext::MolPlaylist {
                mol_path,
                parent_dir,
            } => {
                self.mid_dirs.push(OpenDirItem {
                    label: "<..>".to_string(),
                    kind: OpenDirKind::Parent(parent_dir.clone()),
                });
                for drv in list_available_drives() {
                    self.mid_dirs.push(OpenDirItem {
                        label: format!("[-{}-]", drv.to_ascii_lowercase()),
                        kind: OpenDirKind::Drive(drv),
                    });
                }
                let entries = load_mol_file(mol_path);
                for p in entries {
                    let fname = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("MODULE")
                        .to_uppercase();
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    if self.format_allowed(&ext) && matches_wildcard(&fname, &self.filter_mask) {
                        let size_bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                        self.left_files.push(OpenFileItem {
                            display_name: fname,
                            ext,
                            size_bytes,
                            source: OpenFileSource::Filesystem(p),
                            selected: false,
                        });
                    }
                }
            }
            OpenBrowseContext::ZipArchive {
                zip_path,
                parent_dir,
            } => {
                self.mid_dirs.push(OpenDirItem {
                    label: "<..>".to_string(),
                    kind: OpenDirKind::Parent(parent_dir.clone()),
                });
                for drv in list_available_drives() {
                    self.mid_dirs.push(OpenDirItem {
                        label: format!("[-{}-]", drv.to_ascii_lowercase()),
                        kind: OpenDirKind::Drive(drv),
                    });
                }
                let entries = list_zip_entries(zip_path);
                for entry in entries {
                    let fname = entry.file_name.to_uppercase();
                    let ext = std::path::Path::new(&fname)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    if self.format_allowed(&ext) && matches_wildcard(&fname, &self.filter_mask) {
                        self.left_files.push(OpenFileItem {
                            display_name: fname,
                            ext,
                            size_bytes: entry.uncompressed_size as u64,
                            source: OpenFileSource::ZipEntry {
                                zip_path: zip_path.clone(),
                                entry,
                            },
                            selected: false,
                        });
                    }
                }
            }
        }
    }

    /// Fügt alle links markierten Dateien (oder falls keine markiert ist, die aktuell fokussierte Datei) zur rechten Playlist hinzu.
    pub fn add_selected_to_playlist(&mut self) {
        let any_selected = self.left_files.iter().any(|f| f.selected);
        let mut added = 0usize;
        if any_selected {
            for f in &mut self.left_files {
                if f.selected {
                    if let Some(p) = f.resolve_to_path() {
                        self.right_playlist.push(OpenPlaylistItem {
                            path: p,
                            display_name: f.display_name.clone(),
                            ext: f.ext.clone(),
                            selected: false,
                        });
                        added += 1;
                    }
                    f.selected = false;
                }
            }
        } else if let Some(f) = self.left_files.get(self.left_cursor).cloned() {
            if let Some(p) = f.resolve_to_path() {
                self.right_playlist.push(OpenPlaylistItem {
                    path: p,
                    display_name: f.display_name,
                    ext: f.ext,
                    selected: false,
                });
                added += 1;
            }
            if self.left_cursor + 1 < self.left_files.len() {
                self.left_cursor += 1;
                if self.left_cursor >= self.left_scroll + 20 {
                    self.left_scroll = self.left_cursor.saturating_sub(19);
                }
            }
        }
        self.status_msg = format!("Added {} module(s) to playlist.", added);
    }

    /// Entfernt alle rechts markierten Einträge (oder den aktuellen Cursor-Eintrag) aus der Arbeits-Playlist.
    pub fn remove_selected_from_playlist(&mut self) {
        let any_selected = self.right_playlist.iter().any(|p| p.selected);
        if any_selected {
            let before = self.right_playlist.len();
            self.right_playlist.retain(|p| !p.selected);
            let removed = before.saturating_sub(self.right_playlist.len());
            self.status_msg = format!("Removed {} module(s) from playlist.", removed);
        } else if !self.right_playlist.is_empty() {
            let idx = self.right_cursor.min(self.right_playlist.len() - 1);
            let removed = self.right_playlist.remove(idx);
            self.status_msg = format!("Removed {} from playlist.", removed.display_name);
        }
        if self.right_cursor >= self.right_playlist.len() && !self.right_playlist.is_empty() {
            self.right_cursor = self.right_playlist.len() - 1;
        }
        if self.right_scroll > self.right_cursor {
            self.right_scroll = self.right_cursor;
        }
    }
}

impl Mod4WinApp {
    /// Rendert den vollständigen 3-Spalten-Commander „MOD4WIN Open Files“ (`newopen.pas`, 558 x 358).
    pub(crate) fn render_open_dialog_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let loc = &self.locale;
        let is_de = loc.is_german() || state.ini_config.language == 1;
        // 1. Äußerer 3D-Dialograhmen + Titelleiste (CapDY = 18)
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
            "MOD4WIN [3.0 Beta] Open Files",
            [0, 0, 0, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, dw as i32 - 20, 2);

        let col_green = [0, 255, 0, 255];
        let col_white = [255, 255, 255, 255];
        let col_black = [0, 0, 0, 255];
        let col_gray = [192, 192, 192, 255];

        // 2. Obere Pfadleiste (newopen.pas: Label "&Path:" bei (10, 28), Box bei (50, 26, 496, 16))
        draw_oem_text_mnemonic(frame, dw, dh, 10, 28, "&Path:", col_black);
        draw_3d_panel(frame, dw, dh, 50, 26, 496, 16, true, col_black);
        let path_str: String = self.open_dlg.path_bar_text().chars().take(59).collect();
        draw_oem_text(frame, dw, dh, 54, 28, &path_str, col_green);
        let cur_x = 54 + (path_str.chars().count() as i32) * 8;
        if cur_x + 8 <= 544 {
            draw_oem_text(frame, dw, dh, cur_x, 28, "_", col_white);
        }

        // 3. Spaltenüberschriften ("&Filelist:", "&Directories:", "&Playlist:") direkt auf Grau bei y = 46
        draw_oem_text_mnemonic(frame, dw, dh, 10, 46, "&Filelist:", col_black);
        draw_oem_text_mnemonic(frame, dw, dh, 194, 46, "&Directories:", col_black);
        draw_oem_text_mnemonic(frame, dw, dh, 374, 46, "&Playlist:", col_black);

        // 4. Linke Spalte: Filelist (12, 62, 156, 240; 20 Zeilen à 12px) + Scrollbar (168, 62, 14, 240)
        draw_3d_frame_only(frame, dw, dh, 10, 60, 174, 244, true);
        fill_rect(frame, dw, dh, 12, 62, 156, 240, col_black);
        let left_max_scroll = self.open_dlg.left_files.len().saturating_sub(20);
        let left_scroll = self.open_dlg.left_scroll.min(left_max_scroll);
        let in_zip = matches!(self.open_dlg.context, OpenBrowseContext::ZipArchive { .. });

        for (row, (_idx, item)) in self
            .open_dlg
            .left_files
            .iter()
            .enumerate()
            .skip(left_scroll)
            .take(20)
            .enumerate()
        {
            let ry = 62 + (row as i32) * 12;
            let hi = item.selected;
            if hi {
                fill_rect(frame, dw, dh, 12, ry, 156, 12, col_green);
            }
            let tcol = if hi { col_black } else { col_green };
            self.assets
                .filetype_badge(&item.ext)
                .blit_rect_to(0, 0, 28, 12, frame, dw, dh, 13, ry);
            if in_zip {
                self.assets
                    .arc_bmp
                    .blit_rect_to(0, 0, 8, 12, frame, dw, dh, 42, ry);
            }
            let disp_name: String = item.display_name.to_lowercase().chars().take(14).collect();
            draw_oem_text(frame, dw, dh, 51, ry, &disp_name, tcol);
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            168,
            62,
            14,
            240,
            left_scroll,
            20,
            self.open_dlg.left_files.len(),
        );

        // 5. Mittlere Spalte oben: Directories / .MOL / .ZIP / Drives (194, 62, 155, 156; 13 Zeilen à 12px) + Scrollbar (349, 62, 14, 156)
        draw_3d_frame_only(frame, dw, dh, 192, 60, 173, 160, true);
        fill_rect(frame, dw, dh, 194, 62, 155, 156, col_black);
        let mid_max_scroll = self.open_dlg.mid_dirs.len().saturating_sub(13);
        let mid_scroll = self.open_dlg.mid_scroll.min(mid_max_scroll);

        for (row, (idx, dir_item)) in self
            .open_dlg
            .mid_dirs
            .iter()
            .enumerate()
            .skip(mid_scroll)
            .take(13)
            .enumerate()
        {
            let ry = 62 + (row as i32) * 12;
            let is_cur = idx == self.open_dlg.mid_cursor;
            if is_cur {
                fill_rect(frame, dw, dh, 194, ry, 155, 12, col_green);
            }
            let tcol = if is_cur { col_black } else { col_green };

            let icon_sx = match &dir_item.kind {
                OpenDirKind::Parent(_) | OpenDirKind::Subdir(_) => 0,
                OpenDirKind::MolFile(_) => 28,
                OpenDirKind::ZipFile(_) => 56,
                OpenDirKind::Drive(drv) => {
                    if *drv == 'A' || *drv == 'B' {
                        84
                    } else {
                        112
                    }
                }
            };
            self.assets
                .bmp_drives
                .blit_rect_to(icon_sx, 0, 28, 12, frame, dw, dh, 195, ry);
            let disp: String = dir_item.label.to_lowercase().chars().take(14).collect();
            draw_oem_text(frame, dw, dh, 232, ry, &disp, tcol);
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            349,
            62,
            14,
            156,
            mid_scroll,
            13,
            self.open_dlg.mid_dirs.len(),
        );

        // 6. Mittlere Auswahl- und Transfer-Buttons mit 2px sunken 3D-Rahmen (newopen.pas Zeilen 1122-1127)
        // Links: +, -, * (je 14x14 bei x = 204, y = 229, 243, 257)
        draw_3d_frame_only(frame, dw, dh, 202, 227, 18, 46, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            204,
            229,
            self.pressed_btn == Some(PressedButton::OpenLeftPlus),
            &self.assets.boplus,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            204,
            243,
            self.pressed_btn == Some(PressedButton::OpenLeftMinus),
            &self.assets.bominus,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            204,
            257,
            self.pressed_btn == Some(PressedButton::OpenLeftInvert),
            &self.assets.bomal,
        );

        // Mitte: <- Del (BODEL.bmp 50x42 bei 229, 229) und -> Add (BOADD.bmp 50x42 bei 279, 229)
        draw_3d_frame_only(frame, dw, dh, 227, 227, 104, 46, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            229,
            229,
            self.pressed_btn == Some(PressedButton::OpenDel),
            &self.assets.bodel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            279,
            229,
            self.pressed_btn == Some(PressedButton::OpenAdd),
            &self.assets.boadd,
        );

        // Rechts: +, -, * (je 14x14 bei x = 339, y = 229, 243, 257)
        draw_3d_frame_only(frame, dw, dh, 337, 227, 18, 46, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            339,
            229,
            self.pressed_btn == Some(PressedButton::OpenRightPlus),
            &self.assets.boplus,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            339,
            243,
            self.pressed_btn == Some(PressedButton::OpenRightMinus),
            &self.assets.bominus,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            339,
            257,
            self.pressed_btn == Some(PressedButton::OpenRightInvert),
            &self.assets.bomal,
        );

        // Aktions-Buttons: OK, Save, Quit, Help (je 41x21 bei y = 282, x = 196, 238, 280, 322)
        draw_3d_frame_only(frame, dw, dh, 194, 280, 171, 25, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            196,
            282,
            self.pressed_btn == Some(PressedButton::OpenOk),
            &self.assets.bsok,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            238,
            282,
            self.pressed_btn == Some(PressedButton::OpenSave) || self.open_dlg.saving_mol,
            &self.assets.bssave,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            280,
            282,
            self.pressed_btn == Some(PressedButton::OpenCancel),
            &self.assets.bscancel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            322,
            282,
            self.pressed_btn == Some(PressedButton::OpenHelp),
            &self.assets.bshelp,
        );

        // 7. Rechte Spalte: Playlist (374, 62, 156, 240; 20 Zeilen à 12px) + Scrollbar (530, 62, 14, 240)
        draw_3d_frame_only(frame, dw, dh, 372, 60, 174, 244, true);
        fill_rect(frame, dw, dh, 374, 62, 156, 240, col_black);
        let right_max_scroll = self.open_dlg.right_playlist.len().saturating_sub(20);
        let right_scroll = self.open_dlg.right_scroll.min(right_max_scroll);

        for (row, (idx, item)) in self
            .open_dlg
            .right_playlist
            .iter()
            .enumerate()
            .skip(right_scroll)
            .take(20)
            .enumerate()
        {
            let ry = 62 + (row as i32) * 12;
            let hi = item.selected;
            if hi {
                fill_rect(frame, dw, dh, 374, ry, 156, 12, col_green);
            }
            let tcol = if hi { col_black } else { col_green };
            self.assets
                .filetype_badge(&item.ext)
                .blit_rect_to(0, 0, 28, 12, frame, dw, dh, 375, ry);

            let status_ch = if state.current_file_path.as_ref() == Some(&item.path) {
                '»'
            } else if idx < state.playlist_idx {
                '☺'
            } else {
                ' '
            };
            let disp_str: String = format!("{} {}", status_ch, item.display_name.to_lowercase())
                .chars()
                .take(14)
                .collect();
            draw_oem_text(frame, dw, dh, 415, ry, &disp_str, tcol);
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            530,
            62,
            14,
            240,
            right_scroll,
            20,
            self.open_dlg.right_playlist.len(),
        );

        // 8. Untere Zähler-Boxen (newopen.pas Zeilen 1128-1129: 2px raised 3D-Panel mit schwarzem Text)
        draw_3d_panel(frame, dw, dh, 10, 310, 174, 16, false, col_gray);
        if self.open_dlg.editing_filter {
            draw_oem_text(frame, dw, dh, 13, 312, "Sel?", col_black);
            fill_rect(frame, dw, dh, 45, 312, 137, 12, col_black);
            let mask_disp: String = self.open_dlg.filter_mask.chars().take(16).collect();
            if self.open_dlg.filter_fresh && !mask_disp.is_empty() {
                let hi_w = (mask_disp.chars().count() as u32 * 8).min(137);
                fill_rect(frame, dw, dh, 45, 312, hi_w, 12, [166, 202, 240, 255]);
                draw_oem_text(frame, dw, dh, 45, 312, &mask_disp, col_black);
            } else {
                let with_cur: String = format!("{}_", mask_disp).chars().take(17).collect();
                draw_oem_text(frame, dw, dh, 45, 312, &with_cur, col_green);
            }
        } else {
            let sel_left = self.open_dlg.left_files.iter().filter(|f| f.selected).count();
            let left_box_str = format!("{:3} of {:5} Files", sel_left, self.open_dlg.left_files.len());
            let left_disp: String = left_box_str.chars().take(20).collect();
            draw_oem_text(frame, dw, dh, 20, 312, &left_disp, col_black);
        }

        // Mitte (228, 310, 100, 16): Nur sichtbar, wenn gerade eine .MOL-Liste gespeichert wird!
        if self.open_dlg.saving_mol {
            draw_3d_panel(frame, dw, dh, 228, 310, 100, 16, true, col_black);
            let mol_disp: String = format!("{}_", self.open_dlg.save_mol_name).chars().take(11).collect();
            draw_oem_text(frame, dw, dh, 232, 312, &mol_disp, col_green);
        }

        // Rechts: Playlist-Zähler "  X of     Y Files" (372, 310, 174, 16)
        draw_3d_panel(frame, dw, dh, 372, 310, 174, 16, false, col_gray);
        if self.open_dlg.editing_right_filter {
            draw_oem_text(frame, dw, dh, 375, 312, "Sel?", col_black);
            fill_rect(frame, dw, dh, 407, 312, 137, 12, col_black);
            let mask_disp: String = self.open_dlg.filter_mask.chars().take(16).collect();
            if self.open_dlg.filter_fresh && !mask_disp.is_empty() {
                let hi_w = (mask_disp.chars().count() as u32 * 8).min(137);
                fill_rect(frame, dw, dh, 407, 312, hi_w, 12, [166, 202, 240, 255]);
                draw_oem_text(frame, dw, dh, 407, 312, &mask_disp, col_black);
            } else {
                let with_cur: String = format!("{}_", mask_disp).chars().take(17).collect();
                draw_oem_text(frame, dw, dh, 407, 312, &with_cur, col_green);
            }
        } else {
            let sel_right = self
                .open_dlg
                .right_playlist
                .iter()
                .filter(|p| p.selected)
                .count();
            let right_str = format!(
                "{:3} of {:5} Files",
                sel_right,
                self.open_dlg.right_playlist.len()
            );
            draw_oem_text(frame, dw, dh, 382, 312, &right_str, col_black);
        }

        // 9. Unterste Info-Leiste (INFOstatic: 10, 330, 536, 16; 2px sunken Grau mit zentriertem schwarzen Text)
        draw_3d_panel(frame, dw, dh, 10, 330, 536, 16, true, col_gray);
        let bottom_text = if self.open_dlg.saving_mol {
            if is_de {
                "Listennamen eingeben (Enter)".to_string()
            } else {
                "Enter playlist name (Enter)".to_string()
            }
        } else if self.open_dlg.editing_filter || self.open_dlg.editing_right_filter {
            if is_de {
                "Geben Sie eine Datei-Auswahl-Maske ein!  Enter - auswählen!".to_string()
            } else {
                "Enter a file selection mask!  Enter - select!".to_string()
            }
        } else if !self.open_dlg.status_msg.is_empty() {
            self.open_dlg.status_msg.clone()
        } else {
            match self.open_dlg.active_list {
                0 => {
                    if let Some(f) = self.open_dlg.left_files.get(self.open_dlg.left_cursor) {
                        format!(
                            "{}{}",
                            self.open_dlg.path_bar_text(),
                            f.display_name.to_lowercase()
                        )
                    } else if is_de {
                        "Keine gültigen Module im Verzeichnis".to_string()
                    } else {
                        "No valid modules in directory".to_string()
                    }
                }
                2 => {
                    if let Some(p) = self.open_dlg.right_playlist.get(self.open_dlg.right_cursor) {
                        p.path.to_string_lossy().to_string()
                    } else if is_de {
                        "Keine Module in der Playliste".to_string()
                    } else {
                        "No modules in playlist".to_string()
                    }
                }
                _ => {
                    if let Some(d) = self.open_dlg.mid_dirs.get(self.open_dlg.mid_cursor) {
                        match d.kind {
                            OpenDirKind::Parent(_) => {
                                if is_de {
                                    "Unterverzeichnis verlassen (Enter)".to_string()
                                } else {
                                    "Leave subdirectory (Enter)".to_string()
                                }
                            }
                            OpenDirKind::Subdir(_) => {
                                if is_de {
                                    "Verzeichnis wechseln (Enter), kopieren (F5)".to_string()
                                } else {
                                    "Change directory (Enter), copy (F5)".to_string()
                                }
                            }
                            OpenDirKind::MolFile(_) => {
                                if is_de {
                                    "MOD4WIN-Playliste öffnen (Enter), kopieren (F5)".to_string()
                                } else {
                                    "Open MOD4WIN playlist (Enter), copy (F5)".to_string()
                                }
                            }
                            OpenDirKind::ZipFile(_) => {
                                if is_de {
                                    "ZIP-Archiv öffnen (Enter), kopieren (F5)".to_string()
                                } else {
                                    "Open ZIP archive (Enter), copy (F5)".to_string()
                                }
                            }
                            OpenDirKind::Drive(drv) => {
                                if drv == 'A' || drv == 'B' {
                                    if is_de {
                                        "Diskettenlaufwerk wechseln (Enter), kopieren (F5)".to_string()
                                    } else {
                                        "Change floppy drive (Enter), copy (F5)".to_string()
                                    }
                                } else if is_de {
                                    "Festplattenlaufwerk wechseln (Enter), kopieren (F5)".to_string()
                                } else {
                                    "Change hard disk drive (Enter), copy (F5)".to_string()
                                }
                            }
                        }
                    } else {
                        String::new()
                    }
                }
            }
        };
        let bot_disp: String = bottom_text.chars().take(65).collect();
        draw_oem_text_centered(frame, dw, dh, 12, 332, 532, &bot_disp, col_black);
    }

    pub(crate) fn navigate_open_dir_item(&mut self, idx: usize) {
        let Some(item) = self.open_dlg.mid_dirs.get(idx).cloned() else {
            return;
        };
        let parent_dir = self.open_dlg.current_dir_path();
        match item.kind {
            OpenDirKind::Parent(p) | OpenDirKind::Subdir(p) => {
                self.open_dlg.context = OpenBrowseContext::Directory(p);
                self.open_dlg.refresh_browser();
                self.open_dlg.status_msg.clear();
            }
            OpenDirKind::MolFile(mol_path) => {
                self.open_dlg.context = OpenBrowseContext::MolPlaylist {
                    mol_path,
                    parent_dir,
                };
                self.open_dlg.refresh_browser();
                self.open_dlg.status_msg.clear();
            }
            OpenDirKind::ZipFile(zip_path) => {
                self.open_dlg.context = OpenBrowseContext::ZipArchive {
                    zip_path,
                    parent_dir,
                };
                self.open_dlg.refresh_browser();
                self.open_dlg.status_msg.clear();
            }
            OpenDirKind::Drive(drv) => {
                let root = PathBuf::from(format!("{}:\\", drv));
                self.open_dlg.context = OpenBrowseContext::Directory(root);
                self.open_dlg.refresh_browser();
                self.open_dlg.status_msg.clear();
            }
        }
    }

    pub(crate) fn commit_open_dialog_ok(&mut self) {
        let new_paths: Vec<PathBuf> = self
            .open_dlg
            .right_playlist
            .iter()
            .map(|p| p.path.clone())
            .collect();
        if !new_paths.is_empty() {
            let sel_idx = self
                .open_dlg
                .right_cursor
                .min(new_paths.len().saturating_sub(1));
            let mut state = self.shared_state.lock().unwrap();
            state.playlist = new_paths;
            state.playlist_idx = sel_idx;
            state.playing_track_detached = false;
            state.rebuild_number_list();
            if let Some(pos) = state.number_list.iter().position(|&i| i == sel_idx) {
                state.playlist_pos = pos;
            }
            let target = state.playlist[sel_idx].clone();
            state.pending_load_file = Some(target);
            state.is_playing = state.ini_config.play_on_load || state.is_playing || state.intro_scan;
            state.is_paused = false;
            state.sync_runtime_to_ini();
            let _ = state
                .ini_config
                .save_to_file(&super::ini_config::Mod4WinIniConfig::default_ini_path());
        } else {
            // Rechte Playlist wurde komplett geleert -> Haupt-Playlist ebenfalls leeren!
            {
                let mut state = self.shared_state.lock().unwrap();
                state.clear_playlist();
                let _ = state
                    .ini_config
                    .save_to_file(&super::ini_config::Mod4WinIniConfig::default_ini_path());
            }
            self.quick_select_cursor = 0;
            self.quick_select_scroll = 0;
            self.quick_select_all_selected = false;
        }
        self.close_window(WindowKind::OpenDialog);
    }

    pub(crate) fn save_current_mol_playlist(&mut self) {
        let mut fname = self.open_dlg.save_mol_name.trim().to_string();
        if fname.is_empty() {
            fname = "PLAYLIST.MOL".to_string();
        }
        if !fname.to_ascii_uppercase().ends_with(".MOL") {
            fname.push_str(".MOL");
        }
        let target_path = self.open_dlg.current_dir_path().join(&fname);
        let entries: Vec<PathBuf> = self
            .open_dlg
            .right_playlist
            .iter()
            .map(|p| p.path.clone())
            .collect();
        match save_mol_file(&target_path, &entries) {
            Ok(()) => {
                self.open_dlg.saving_mol = false;
                self.open_dlg.refresh_browser();
                self.open_dlg.status_msg = format!(
                    "Saved {} modules to {}",
                    entries.len(),
                    target_path.to_string_lossy()
                );
            }
            Err(e) => {
                self.open_dlg.saving_mol = false;
                self.open_dlg.status_msg = format!("Error saving .MOL: {}", e);
            }
        }
    }

    pub(crate) fn handle_open_dialog_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        // 1. Titelleiste (y <= 18)
        if vy <= 18 {
            if vx <= 20 || vx >= (OPEN_WINDOW_WIDTH as i32 - 20) {
                self.commit_open_dialog_ok();
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        let classic_input = self.shared_state.lock().unwrap().classic_input;
        let ctrl = self.modifiers.state().control_key();
        let shift = self.modifiers.state().shift_key();
        self.open_dlg.classic_drag_dir = None;

        // 2. Linke Filelist (x = 12..167, y = 62..301) & Linke Scrollbar (x = 168..182, y = 62..301)
        if (62..=301).contains(&vy) {
            if (12..=167).contains(&vx) {
                self.open_dlg.active_list = 0;
                let row = ((vy - 63).max(0) / 12) as usize;
                let max_scroll = self.open_dlg.left_files.len().saturating_sub(20);
                let scroll = self.open_dlg.left_scroll.min(max_scroll);
                let idx = scroll + row;
                if idx < self.open_dlg.left_files.len() {
                    self.open_dlg.left_cursor = idx;
                    if is_double_click {
                        // Doppelklick auf Datei links -> zur Playlist hinzufügen
                        if let Some(f) = self.open_dlg.left_files.get(idx).cloned() {
                            if let Some(p) = f.resolve_to_path() {
                                self.open_dlg.right_playlist.push(OpenPlaylistItem {
                                    path: p,
                                    display_name: f.display_name.clone(),
                                    ext: f.ext,
                                    selected: false,
                                });
                                self.open_dlg.right_cursor =
                                    self.open_dlg.right_playlist.len().saturating_sub(1);
                                self.open_dlg.status_msg =
                                    format!("Added {} to playlist.", f.display_name);
                            }
                        }
                    } else if classic_input {
                        // 1995er Verhalten (`--classic-input`): Klick toggelt die Auswahl
                        self.open_dlg.left_files[idx].selected =
                            !self.open_dlg.left_files[idx].selected;
                        self.open_dlg.left_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    } else if shift {
                        // Modernes Shift+Klick: Bereich von `left_anchor` bis `idx` markieren
                        if !ctrl {
                            for f in &mut self.open_dlg.left_files {
                                f.selected = false;
                            }
                        }
                        let a = self.open_dlg.left_anchor.min(idx);
                        let b = self.open_dlg.left_anchor.max(idx);
                        for i in a..=b {
                            if let Some(f) = self.open_dlg.left_files.get_mut(i) {
                                f.selected = true;
                            }
                        }
                        self.open_dlg.status_msg.clear();
                    } else if ctrl {
                        // Modernes Strg+Klick: Einzelnes Element toggeln
                        self.open_dlg.left_files[idx].selected =
                            !self.open_dlg.left_files[idx].selected;
                        self.open_dlg.left_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    } else {
                        // Moderner Einfachklick: Nur diesen Eintrag auswählen, andere abwählen
                        for (i, f) in self.open_dlg.left_files.iter_mut().enumerate() {
                            f.selected = i == idx;
                        }
                        self.open_dlg.left_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    }
                }
                return;
            } else if (168..=182).contains(&vx) {
                let max_scroll = self.open_dlg.left_files.len().saturating_sub(20);
                if vy <= 76 {
                    self.open_dlg.left_scroll = self.open_dlg.left_scroll.saturating_sub(1);
                } else if vy >= 287 {
                    self.open_dlg.left_scroll = (self.open_dlg.left_scroll + 1).min(max_scroll);
                } else if vy < 181 {
                    self.open_dlg.left_scroll = self.open_dlg.left_scroll.saturating_sub(19);
                } else {
                    self.open_dlg.left_scroll = (self.open_dlg.left_scroll + 19).min(max_scroll);
                }
                return;
            }
        }

        // 3. Mittlere Directories-Liste (x = 194..348, y = 62..217) & Scrollbar (x = 349..363, y = 62..217)
        if (62..=217).contains(&vy) {
            if (194..=348).contains(&vx) {
                self.open_dlg.active_list = 1;
                let row = ((vy - 63).max(0) / 12) as usize;
                let max_scroll = self.open_dlg.mid_dirs.len().saturating_sub(13);
                let scroll = self.open_dlg.mid_scroll.min(max_scroll);
                let idx = scroll + row;
                if idx < self.open_dlg.mid_dirs.len() {
                    self.open_dlg.mid_cursor = idx;
                    if is_double_click {
                        self.navigate_open_dir_item(idx);
                    } else {
                        if classic_input {
                            self.open_dlg.classic_drag_dir = Some(idx);
                        }
                        self.open_dlg.status_msg.clear();
                    }
                }
                return;
            } else if (349..=363).contains(&vx) {
                let max_scroll = self.open_dlg.mid_dirs.len().saturating_sub(13);
                if vy <= 76 {
                    self.open_dlg.mid_scroll = self.open_dlg.mid_scroll.saturating_sub(1);
                } else if vy >= 203 {
                    self.open_dlg.mid_scroll = (self.open_dlg.mid_scroll + 1).min(max_scroll);
                } else if vy < 140 {
                    self.open_dlg.mid_scroll = self.open_dlg.mid_scroll.saturating_sub(12);
                } else {
                    self.open_dlg.mid_scroll = (self.open_dlg.mid_scroll + 12).min(max_scroll);
                }
                return;
            }
        }

        // 4. Rechte Playlist (x = 374..529, y = 62..301) & Scrollbar (x = 530..544, y = 62..301)
        if (62..=301).contains(&vy) {
            if (374..=529).contains(&vx) {
                self.open_dlg.active_list = 2;
                let row = ((vy - 63).max(0) / 12) as usize;
                let max_scroll = self.open_dlg.right_playlist.len().saturating_sub(20);
                let scroll = self.open_dlg.right_scroll.min(max_scroll);
                let idx = scroll + row;
                if idx < self.open_dlg.right_playlist.len() {
                    self.open_dlg.right_cursor = idx;
                    if is_double_click {
                        // Doppelklick in der rechten Playlist -> sofort abspielen und Dialog schließen!
                        self.commit_open_dialog_ok();
                    } else if classic_input {
                        self.open_dlg.right_playlist[idx].selected =
                            !self.open_dlg.right_playlist[idx].selected;
                        self.open_dlg.right_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    } else if shift {
                        if !ctrl {
                            for p in &mut self.open_dlg.right_playlist {
                                p.selected = false;
                            }
                        }
                        let a = self.open_dlg.right_anchor.min(idx);
                        let b = self.open_dlg.right_anchor.max(idx);
                        for i in a..=b {
                            if let Some(p) = self.open_dlg.right_playlist.get_mut(i) {
                                p.selected = true;
                            }
                        }
                        self.open_dlg.status_msg.clear();
                    } else if ctrl {
                        self.open_dlg.right_playlist[idx].selected =
                            !self.open_dlg.right_playlist[idx].selected;
                        self.open_dlg.right_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    } else {
                        for (i, p) in self.open_dlg.right_playlist.iter_mut().enumerate() {
                            p.selected = i == idx;
                        }
                        self.open_dlg.right_anchor = idx;
                        self.open_dlg.status_msg.clear();
                    }
                }
                return;
            } else if (530..=544).contains(&vx) {
                let max_scroll = self.open_dlg.right_playlist.len().saturating_sub(20);
                if vy <= 76 {
                    self.open_dlg.right_scroll = self.open_dlg.right_scroll.saturating_sub(1);
                } else if vy >= 287 {
                    self.open_dlg.right_scroll = (self.open_dlg.right_scroll + 1).min(max_scroll);
                } else if vy < 181 {
                    self.open_dlg.right_scroll = self.open_dlg.right_scroll.saturating_sub(19);
                } else {
                    self.open_dlg.right_scroll = (self.open_dlg.right_scroll + 19).min(max_scroll);
                }
                return;
            }
        }

        // 5. Mittlere Auswahl- und Transfer-Buttons (y = 229..271)
        if (229..=271).contains(&vy) {
            // Linke +, -, * Buttons (x = 204..218)
            if (204..=218).contains(&vx) {
                if (229..=242).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenLeftPlus);
                    if self.open_dlg.editing_filter && !self.open_dlg.filter_deselect {
                        self.open_dlg.apply_selection_mask();
                    } else {
                        self.open_dlg.open_selection_mask(false, false);
                    }
                } else if (243..=256).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenLeftMinus);
                    if self.open_dlg.editing_filter && self.open_dlg.filter_deselect {
                        self.open_dlg.apply_selection_mask();
                    } else {
                        self.open_dlg.open_selection_mask(false, true);
                    }
                } else if (257..=271).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenLeftInvert);
                    self.open_dlg.editing_filter = false;
                    self.open_dlg.editing_right_filter = false;
                    for f in &mut self.open_dlg.left_files {
                        f.selected = !f.selected;
                    }
                }
                self.open_dlg.status_msg.clear();
                return;
            }

            // <- Del Button (x = 229..278, y = 229..271)
            if (229..=278).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenDel);
                self.open_dlg.remove_selected_from_playlist();
                self.open_dlg.status_msg.clear();
                return;
            }

            // -> Add Button (x = 279..328, y = 229..271)
            if (279..=328).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenAdd);
                self.open_dlg.add_selected_to_playlist();
                self.open_dlg.status_msg.clear();
                return;
            }

            // Rechte +, -, * Buttons (x = 339..353)
            if (339..=353).contains(&vx) {
                if (229..=242).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenRightPlus);
                    if self.open_dlg.editing_right_filter && !self.open_dlg.filter_deselect {
                        self.open_dlg.apply_selection_mask();
                    } else {
                        self.open_dlg.open_selection_mask(true, false);
                    }
                } else if (243..=256).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenRightMinus);
                    if self.open_dlg.editing_right_filter && self.open_dlg.filter_deselect {
                        self.open_dlg.apply_selection_mask();
                    } else {
                        self.open_dlg.open_selection_mask(true, true);
                    }
                } else if (257..=271).contains(&vy) {
                    self.pressed_btn = Some(PressedButton::OpenRightInvert);
                    self.open_dlg.editing_filter = false;
                    self.open_dlg.editing_right_filter = false;
                    for p in &mut self.open_dlg.right_playlist {
                        p.selected = !p.selected;
                    }
                }
                self.open_dlg.status_msg.clear();
                return;
            }
        }

        // 6. Aktions-Buttons: OK (196..237), Save (238..279), Quit (280..321), Help (322..363) bei y = 282..303
        if (282..=303).contains(&vy) {
            if (196..=237).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenOk);
                self.commit_open_dialog_ok();
                return;
            } else if (238..=279).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenSave);
                if self.open_dlg.saving_mol {
                    self.save_current_mol_playlist();
                } else {
                    self.open_dlg.saving_mol = true;
                    self.open_dlg.editing_filter = false;
                    self.open_dlg.editing_right_filter = false;
                }
                return;
            } else if (280..=321).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenCancel);
                self.open_dlg.saving_mol = false;
                self.open_dlg.editing_filter = false;
                self.open_dlg.editing_right_filter = false;
                self.close_window(WindowKind::OpenDialog);
                return;
            } else if (322..=363).contains(&vx) {
                self.pressed_btn = Some(PressedButton::OpenHelp);
                self.open_help_topic(event_loop, "OpenDlg");
                return;
            }
        }

        // 7. Untere Auswahl-Masken: Links (10..184, 310..326) und Rechts (372..546, 310..326)
        if (310..=326).contains(&vy) {
            if (10..=184).contains(&vx) {
                self.open_dlg.open_selection_mask(false, false);
            } else if (372..=546).contains(&vx) {
                self.open_dlg.open_selection_mask(true, false);
            }
        }
    }
}
