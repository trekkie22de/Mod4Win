//! Kern-Fensterverwaltung, Event-Loop (`ApplicationHandler`) und `UiSharedState`
//! der MOD4WIN 2.40 Beta Multi-Window-Oberflaeche.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::pixels::{Pixels, ScalingMode, SurfaceTexture};
use super::winit::application::ApplicationHandler;
use super::winit::dpi::PhysicalSize;
use super::winit::event::{ElementState, Modifiers, MouseButton, TouchPhase, WindowEvent};
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::{CursorIcon, ResizeDirection, Window, WindowId};

use super::archive_mol::{extract_all_tracker_files_from_zip, load_mol_file};
use super::assets::EmbeddedAssets;
use super::cpu_sim::{CpuProfile, CpuTelemetryState};
use super::i18n::LocaleCatalog;
use super::ini_config::{
    default_hotkey_bindings, export_module_to_wav_file, Mod4WinIniConfig, DEV_HDD,
};
use super::theme_modern::SkinMode;
use super::touch_gesture::TouchGestureState;
use crate::engine::loader::load_module_with_ext;
use crate::engine::sequencer::Sequencer;
use crate::engine::types::SoundSettings;

// Re-Exporte aus den spezialisierten UI-Submodulen
pub use super::effects_win::{
    check_effect_active, effect_id_from_cmd_op, effect_name_by_id, note_to_str,
};
pub use super::file_mgr::{ArchiveSetupState, FileManItem, FileManagerState};
pub use super::help_win::{
    HelpClickResult, HelpViewerState, HELP_WINDOW_HEIGHT, HELP_WINDOW_WIDTH,
};
pub use super::hotkeys_dlg::HotkeyEditorState;
pub use super::info_win::InstrumentInfo;
pub use super::main_win::ConfirmAction;
use super::modern_skin::{
    MODERN_ABOUT_H, MODERN_ABOUT_W, MODERN_INFO_H, MODERN_INFO_W, MODERN_MAIN_H, MODERN_MAIN_W,
    MODERN_MINI_H, MODERN_MINI_W, MODERN_PLAYLIST_H, MODERN_PLAYLIST_W, MODERN_SETUP_H,
    MODERN_SETUP_W,
};
pub use super::open_dlg::{
    OpenBrowseContext, OpenDialogState, OpenDirItem, OpenDirKind, OpenFileItem, OpenFileSource,
    OpenPlaylistItem,
};
pub use super::setup_dlg::{
    FileSelectEntry, FileSelectState, FileSelectTarget, SetupEditFocus, WavAskEditFocus,
    WavAskState,
};

/// Gedrückter 3D-Button für unmittelbares visuelles Feedback bei gehaltener Maustaste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressedButton {
    Stop,
    Play,
    Pause,
    Prev,
    Rew,
    Ff,
    Next,
    Shuffle,
    Repeat,
    IntroScan,
    Time,
    Open,
    FileMan,
    Info,
    Effects,
    Mixer,
    Setup,
    Help,
    SysMenu,
    Minimize,
    MiniMode,
    Close,
    InfoMinus,
    InfoPlus,
    InstMinus,
    InstPlus,
    OpenLeftPlus,
    OpenLeftMinus,
    OpenLeftInvert,
    OpenRightPlus,
    OpenRightMinus,
    OpenRightInvert,
    OpenAdd,
    OpenDel,
    OpenOk,
    OpenSave,
    OpenCancel,
    OpenHelp,
    SetupDevUp,
    SetupDevDown,
    SetupSrUp,
    SetupSrDown,
    SetupBufUp,
    SetupBufDown,
    SetupIdoUp,
    SetupIdoDown,
    SetupRecBrowse,
    SetupMixBrowse,
    SetupHotkeys,
    SetupPacker,
    SetupRegister,
    SetupLang,
    SetupSkin,
    SetupOk,
    SetupSave,
    SetupCancel,
    SetupHelp,
    AskBrowse,
    AskOk,
    AskCancel,
    AskHelp,
    FselOk,
    FselCancel,
    FselHelp,
    FileManOk,
    FileManCancel,
    FileManHelp,
    HotkeyOk,
    HotkeyCancel,
    HotkeyDefault,
    HotkeyHelp,
    ArcBrowse0,
    ArcBrowse1,
    ArcBrowse2,
    ArcMore0,
    ArcMore1,
    ArcMore2,
    ArcOk,
    ArcCancel,
    ArcHelp,
    ArcMoreOk,
    ArcMoreCancel,
    ArcMoreDefault,
    ArcMoreHelp,
}

/// Abmessungen des Hauptfensters (`bm0.bmp`: 542 x 135)
pub const MAIN_WINDOW_WIDTH: u32 = 542;
pub const MAIN_WINDOW_HEIGHT: u32 = 135;

/// Abmessungen des Mini-Status-Fensters (`bm1.bmp`: 309 x 58)
pub const MINI_WINDOW_WIDTH: u32 = 309;
pub const MINI_WINDOW_HEIGHT: u32 = 58;

/// Abmessungen des Module-Info-Fensters (`bm10.bmp`: 452 x 373)
pub const INFO_WINDOW_WIDTH: u32 = 452;
pub const INFO_WINDOW_HEIGHT: u32 = 373;

/// Abmessungen des Instrumenten-Fensters (`bm11.bmp`: 204 x 182)
pub const INST_WINDOW_WIDTH: u32 = 204;
pub const INST_WINDOW_HEIGHT: u32 = 182;

/// Abmessungen des Effekt-Erklärungsfensters (`bm13.bmp`: 381 x 281)
pub const EFF_WINDOW_WIDTH: u32 = 381;
pub const EFF_WINDOW_HEIGHT: u32 = 281;

/// Abmessungen des Open-Files-Dialogs (`newopen.pas`: 558 x 358)
pub const OPEN_WINDOW_WIDTH: u32 = 558;
pub const OPEN_WINDOW_HEIGHT: u32 = 358;

/// Abmessungen der Quick-Selection (`MULBOX.PAS` / `bm3.bmp`: 166 x 212)
pub const QUICK_SELECT_WIDTH: u32 = 166;
pub const QUICK_SELECT_HEIGHT: u32 = 212;

/// Abmessungen des prozeduralen Setup-Dialogs (`dsetup.pas`: X_SETUP=525, Y_SETUP=318, CapDY=18 -> 525 x 336)
pub const SETUP_WINDOW_WIDTH: u32 = 525;
pub const SETUP_WINDOW_HEIGHT: u32 = 336;

/// Abmessungen des Direct-to-Disk WAVE-Dateinamen-Dialogs (`TAskWin` in `MULBOX.PAS`: 361 x 72)
pub const ASK_WINDOW_WIDTH: u32 = 361;
pub const ASK_WINDOW_HEIGHT: u32 = 72;

/// Abmessungen des Datei-/Verzeichnis-Auswahl-Dialogs (`T_Browse` / `T_DirBrowse` in `MULBOX.PAS` / `bm8.bmp`: 195 x 220)
pub const FSEL_WINDOW_WIDTH: u32 = 195;
pub const FSEL_WINDOW_HEIGHT: u32 = 220;

/// Abmessungen des prozeduralen Mini-File-Managers (`COPYWND.PAS`: 365 x 306)
pub const FILEMAN_WINDOW_WIDTH: u32 = 365;
pub const FILEMAN_WINDOW_HEIGHT: u32 = 306;

/// Abmessungen des prozeduralen Hotkey-Editors (`KEYSEL.PAS`: 336 x 248)
pub const HOTKEY_WINDOW_WIDTH: u32 = 336;
pub const HOTKEY_WINDOW_HEIGHT: u32 = 248;

/// Abmessungen des prozeduralen Archiv-Setups (`tarcopt` in `dsetup.pas`: 524 x 172)
pub const ARCHIVE_WINDOW_WIDTH: u32 = 524;
pub const ARCHIVE_WINDOW_HEIGHT: u32 = 172;

/// Legacy-Konstanten für Abwärtskompatibilität
pub const VIRTUAL_WIDTH: u32 = 542;
pub const VIRTUAL_HEIGHT: u32 = 135;
pub const COLOR_DESKTOP_TEAL: [u8; 4] = [0, 128, 128, 255];


/// Fenstertypen der echten Multi-Window-Architektur von MOD4WIN 2.40 Beta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowKind {
    Main,
    ModuleInfo,
    InstrumentInfo,
    Effects,
    EffectsHelp,
    QuickSelect,
    OpenDialog,
    SetupDialog,
    WavAskDialog,
    FileSelectDialog,
    FileManager,
    HotkeyEditor,
    ArchiveSetup,
    About,
    HelpViewer,
    FileMenu,
    ConfirmDialog,
}

#[cfg(feature = "retro-win32")]
#[derive(Clone)]
pub struct CoopPeakState(pub Arc<Mutex<(Sequencer, u32, u32, String)>>);

#[cfg(feature = "retro-win32")]
impl std::fmt::Debug for CoopPeakState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CoopPeakState(..)")
    }
}

/// Fortschritt und Anzeigezustand des asynchronen/inkrementellen PeakSearch (`PEAK SEARCH ... XX%`).
#[derive(Clone, Debug)]
pub struct PeakSearchProgress {
    pub active: bool,
    pub percent: u8,
    pub scanned_order: usize,
    pub total_orders: usize,
    pub started_at: Instant,
    pub duration_secs: f32,
    pub hold_100_until: Option<Instant>,
    pub computed_preamp: Option<u8>,
    #[cfg(feature = "retro-win32")]
    pub coop_state: Option<CoopPeakState>,
}

#[derive(Clone)]
pub struct PendingPeakSearch(
    pub Arc<crate::engine::loader::LoadedModule>,
    pub SoundSettings,
    pub u32,
    pub String,
);

impl std::fmt::Debug for PendingPeakSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PendingPeakSearch")
            .field(&self.2)
            .field(&self.3)
            .finish()
    }
}

/// Geteilter Zustand zwischen Audio-Wiedergabe-Thread und UI-Renderer.
#[derive(Clone, Debug)]
pub struct UiSharedState {
    pub title: String,
    pub filename: String,
    pub current_file_path: Option<PathBuf>,
    pub format_str: String,
    pub num_channels: usize,
    pub num_patterns: usize,
    pub num_orders: usize,
    pub num_unique_patterns: usize,
    pub file_size_kb: usize,
    pub song_mem_bytes: usize,
    pub total_time_secs: u32,
    pub instrument_names: Vec<String>,
    pub instruments: Vec<InstrumentInfo>,
    pub effects_used_set: [u8; 32],
    pub sound_settings: SoundSettings,
    pub settings_dirty: bool,
    pub channel_muted: [bool; 32],
    pub channel_muted_dirty: bool,
    pub curr_pos: usize,
    pub curr_pattern: usize,
    pub curr_row: usize,
    pub speed: u8,
    pub bpm: u16,
    pub elapsed_secs: u32,
    pub is_playing: bool,
    pub is_paused: bool,
    pub is_finished: bool,
    pub skin_mode: SkinMode,
    pub show_info: bool,
    pub show_effects: bool,
    pub show_filename: bool,
    pub minus_mode: bool,
    pub shuffle: bool,
    pub repeat_mode: u8, // 0 = Off, 1 = Repeat All, 2 = Repeat One
    pub intro_scan: bool,
    pub playlist: Vec<PathBuf>,
    pub playlist_idx: usize,
    /// `true`, wenn der aktuell noch zu Ende spielende Track aus der Playlist entfernt (oder die Playlist geleert) wurde.
    /// `playlist_idx` zeigt in diesem Zustand bereits auf den nachgerückten Folgetrack (`0..=playlist.len()`).
    pub playing_track_detached: bool,
    /// Permutationsliste (`numberlist` in `mod4win.pas`) für wiederholungsfreies Shuffle.
    pub number_list: Vec<usize>,
    /// Aktuelle Position innerhalb von `number_list` (`playlistpos` in `mod4win.pas`).
    pub playlist_pos: usize,
    pub pending_load_file: Option<PathBuf>,
    pub channel_vols: [(u8, u8); 32],
    /// Echt gemessener Stereo-Ausgangspegel `(L, R)` (`0..=64`) direkt aus dem gemischten PCM-Ausgabepuffer
    /// (berücksichtigt Mono/Stereo/Surround, Panning, Kanal-Mutes und PreAmp exakt so, wie es zu hören ist).
    pub output_vu_lr: (u8, u8),
    pub channel_notes: [u8; 32],
    pub channel_insts: [u8; 32],
    pub channel_effects: [u16; 32],
    pub channel_effect_hold: [(u16, Instant); 32],
    /// Relativer Pattern-Sprungwunsch aus UI/Tastatur (`+1` = nächstes Pattern, `-1` = vorheriges Pattern).
    pub pending_seek_delta: i32,
    /// Absoluter 1-basierter Pattern-Sprungwunsch aus UI/Tastatur (`Some(N)` springt zu Pattern `N`).
    pub pending_seek_pattern: Option<u16>,
    /// Komplette Setup- und INI-Konfiguration (`MOD4WIN.INI` & Peak-Datenbank).
    pub ini_config: Mod4WinIniConfig,
    /// Ausstehender Direct-to-Disk WAV-Export (`DEV_HDD`) aus der GUI.
    pub pending_export_wav: Option<PathBuf>,
    /// Zeigt an, ob gerade ein Direct-to-Disk WAV-Export läuft.
    pub is_exporting_wav: bool,
    /// Statusmeldung nach oder während eines Direct-to-Disk WAV-Exports.
    pub export_status_msg: Option<String>,
    /// Klassisches 1995er Maus-/Listen-Handling (`--classic-input` Flag) statt moderner Windows-Listenbedienung.
    pub classic_input: bool,
    /// Laufzeit-Telemetrie für die historische CPU- & Puffer-Simulation (Paket 6).
    pub cpu_telemetry: CpuTelemetryState,
    /// Echt gemessene Audio-Callback-Auslastung (`0.0..1.0`) für das Profil `Native (Modern CPU)`.
    pub native_callback_ratio: f32,
    /// Zeitpunkt der letzten Telemetrie-Aktualisierung zur Berechnung von `dt_secs`.
    pub last_telemetry_instant: Instant,
    /// Kurze Status-/Banner-Einblendung in `Main_Info` (z. B. beim Umschalten des CPU-Profils oder Authentic Stutter).
    pub status_banner: Option<(String, Instant)>,
    /// Laufender asynchroner PeakSearch-Zustand für die Dot-Matrix-Anzeige (`PEAK SEARCH ... XX%`) und das Pattern-LCD.
    pub peak_search: Option<PeakSearchProgress>,
    /// Ausstehender PeakSearch, der nach dem ersten Zeichnen des Hauptfensters (bzw. nach dem Laden eines neuen Moduls) im Hintergrund startet.
    pub pending_initial_peak_search: Option<PendingPeakSearch>,
    /// Zeitpunkt unmittelbar vor dem Öffnen der GUI (`run_ui`).
    pub gui_launch_instant: Instant,
}

impl UiSharedState {
    pub fn new(
        title: &str,
        format_str: &str,
        num_channels: usize,
        num_unique_patterns: usize,
        num_patterns: usize,
        num_orders: usize,
        instrument_names: Vec<String>,
        file_size_kb: usize,
    ) -> Self {
        let instruments = instrument_names
            .iter()
            .enumerate()
            .map(|(idx, name)| InstrumentInfo {
                index: idx + 1,
                name: name.clone(),
                length: 0,
                loop_start: 0,
                loop_length: 0,
                fine_tune: 0,
                volume: 64,
                is_16bit: false,
                c4_freq: 8363,
            })
            .collect();

        let mut sound_settings = SoundSettings::default();
        sound_settings.samplerate = 44100;
        sound_settings.stereo = 2;
        sound_settings.bits = 16;
        sound_settings.surround = false;
        sound_settings.panning = true;
        sound_settings.oversamp = 2;
        sound_settings.pre_amp = 0x80 | 20; // 0 dB + Clipping aktiv

        Self {
            title: title.to_string(),
            filename: String::new(),
            current_file_path: None,
            format_str: format_str.to_string(),
            num_channels: num_channels.min(32),
            num_patterns,
            num_orders,
            num_unique_patterns,
            file_size_kb,
            song_mem_bytes: file_size_kb * 1024,
            total_time_secs: 0,
            instrument_names,
            instruments,
            effects_used_set: [0; 32],
            sound_settings,
            settings_dirty: false,
            channel_muted: [false; 32],
            channel_muted_dirty: false,
            curr_pos: 1,
            curr_pattern: 0,
            curr_row: 1,
            speed: 6,
            bpm: 125,
            elapsed_secs: 0,
            is_playing: true,
            is_paused: false,
            is_finished: false,
            skin_mode: SkinMode::Classic240,
            show_info: false,
            show_effects: false,
            show_filename: false,
            minus_mode: false,
            shuffle: false,
            repeat_mode: 1,
            intro_scan: false,
            playlist: Vec::new(),
            playlist_idx: 0,
            playing_track_detached: false,
            number_list: Vec::new(),
            playlist_pos: 0,
            pending_load_file: None,
            channel_vols: [(0, 0); 32],
            output_vu_lr: (0, 0),
            channel_notes: [0; 32],
            channel_insts: [0; 32],
            channel_effects: [0xFF00; 32],
            channel_effect_hold: [(0xFF00, Instant::now()); 32],
            pending_seek_delta: 0,
            pending_seek_pattern: None,
            ini_config: Mod4WinIniConfig::default(),
            pending_export_wav: None,
            is_exporting_wav: false,
            export_status_msg: None,
            classic_input: false,
            cpu_telemetry: CpuTelemetryState::new(25),
            native_callback_ratio: 0.004,
            last_telemetry_instant: Instant::now(),
            status_banner: None,
            peak_search: None,
            pending_initial_peak_search: None,
            gui_launch_instant: Instant::now(),
        }
    }

    /// Aktualisiert den Stereo-Ausgangspegel `output_vu_lr` (`0..=64`) direkt aus den tatsächlich ausgegebenen
    /// 16-Bit-Stereo-PCM-Frames (`frames`), inklusive schnellem Attack und sanftem Release.
    pub fn update_output_vu_from_pcm(&mut self, frames: &[(i16, i16)]) {
        if !self.is_playing || self.is_paused || frames.is_empty() {
            self.output_vu_lr = (
                self.output_vu_lr.0.saturating_sub(6),
                self.output_vu_lr.1.saturating_sub(6),
            );
            return;
        }
        let mut peak_l = 0u32;
        let mut peak_r = 0u32;
        for &(l, r) in frames {
            let al = (l as i32).unsigned_abs();
            let ar = (r as i32).unsigned_abs();
            if al > peak_l {
                peak_l = al;
            }
            if ar > peak_r {
                peak_r = ar;
            }
        }
        // Skaliere 0..32768 mit leichter Wurzel-Kurve auf 0..=64, damit sowohl leise Passagen als auch 0-dBFS-Spitzen natürlich ausschlagen
        let map_peak = |p: u32| -> u8 {
            if p < 64 {
                0
            } else {
                let norm = (p as f32 / 32768.0).clamp(0.0, 1.0);
                let curved = norm.powf(0.65);
                ((curved * 64.0).round() as u8).clamp(1, 64)
            }
        };
        let mut target_l = map_peak(peak_l);
        let mut target_r = map_peak(peak_r);
        if self.sound_settings.stereo < 2 {
            let mono = ((target_l as u16 + target_r as u16) / 2) as u8;
            target_l = mono;
            target_r = mono;
        }
        let next_l = if target_l >= self.output_vu_lr.0 {
            target_l
        } else {
            self.output_vu_lr.0.saturating_sub(3).max(target_l)
        };
        let next_r = if target_r >= self.output_vu_lr.1 {
            target_r
        } else {
            self.output_vu_lr.1.saturating_sub(3).max(target_r)
        };
        self.output_vu_lr = (next_l, next_r);
    }

    /// Setzt eine kurze Banner-Nachricht (für ~2.2 Sekunden) in der Haupt-Anzeige `Main_Info`.
    pub fn set_status_banner(&mut self, msg: impl Into<String>) {
        self.status_banner = Some((msg.into(), Instant::now()));
    }

    /// Prüft bei aktivem `Load State` (`state_saver`), ob der Peak-Wert bereits in `MOD4WIN.INI` / `MOD4WIN.PKS`
    /// vorliegt (und übernimmt ihn sofort ohne Suchlauf), oder startet andernfalls den nicht-blockierenden
    /// Hintergrund-PeakSearch mit CPU-Profil-skalierter Fortschrittsanzeige (`PEAK SEARCH ... XX%`).
    pub fn start_or_resolve_peak_search(
        shared_state: &Arc<Mutex<UiSharedState>>,
        loaded_module: crate::engine::loader::LoadedModule,
        base_settings: SoundSettings,
        file_size_bytes: u32,
        mod_title: String,
        force_rescan: bool,
    ) {
        let pks_path = Mod4WinIniConfig::default_pks_path();
        {
            let mut st = shared_state.lock().unwrap();
            if !force_rescan && st.ini_config.state_saver
                && let Some(cached_pa) = st.ini_config.find_peak(file_size_bytes, &mod_title, &pks_path) {
                    let pa = cached_pa.min(40);
                    st.sound_settings.pre_amp = (st.sound_settings.pre_amp & 0x80) | pa;
                    st.ini_config.pre_amplification = pa;
                    st.settings_dirty = true;
                    st.peak_search = None;
                    return;
                }
            let profile = CpuProfile::from_u8(st.ini_config.cpu_sim_profile);
            let duration_secs = match profile {
                CpuProfile::I386Sx16 => 3.20,
                CpuProfile::I386Dx33 => 2.50,
                CpuProfile::I486Dx33 => 2.10,
                CpuProfile::I486Dx2_66 => 1.70,
                CpuProfile::I486Dx4_100 => 1.35,
                CpuProfile::Pentium90 => 1.05,
                CpuProfile::AmdK5Pr133 => 0.95,
                CpuProfile::Pentium133 => 0.80,
                CpuProfile::AmdK6_200 => 0.60,
                CpuProfile::AmdK6_2_450 => 0.35,
                CpuProfile::Native => 0.04,
            };
            let total_orders = st.num_orders.max(1);
            #[cfg(feature = "retro-win32")]
            let coop_state = {
                let mut ps_settings = base_settings;
                ps_settings.bits = 16;
                ps_settings.pre_amp = 0x80 | 40;
                let mut ps_seq = Sequencer::new(loaded_module, ps_settings);
                ps_seq.init_incremental_peak_search();
                Some(CoopPeakState(Arc::new(Mutex::new((
                    ps_seq,
                    0u32,
                    file_size_bytes,
                    mod_title,
                )))))
            };
            st.peak_search = Some(PeakSearchProgress {
                active: true,
                percent: 0,
                scanned_order: 1,
                total_orders,
                started_at: Instant::now(),
                duration_secs,
                hold_100_until: None,
                computed_preamp: None,
                #[cfg(feature = "retro-win32")]
                coop_state,
            });
        }

        #[cfg(not(feature = "retro-win32"))]
        {
            let state_arc = Arc::clone(shared_state);
            let _ = std::thread::Builder::new()
                .name("m4w-peak-search".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let mut ps_settings = base_settings;
                    ps_settings.bits = 16;
                    ps_settings.pre_amp = 0x80 | 40;
                    let mut ps_seq = Sequencer::new(loaded_module, ps_settings);
                    let found = (ps_seq.run_exact_peak_search() & 0x7F).min(40);
                    if let Ok(mut st) = state_arc.lock() {
                        st.ini_config
                            .store_peak(file_size_bytes, &mod_title, found, &pks_path);
                        let is_native =
                            CpuProfile::from_u8(st.ini_config.cpu_sim_profile) == CpuProfile::Native;
                        if is_native {
                            st.sound_settings.pre_amp = (st.sound_settings.pre_amp & 0x80) | found;
                            st.ini_config.pre_amplification = found;
                            st.settings_dirty = true;
                        }
                        if let Some(ref mut ps) = st.peak_search {
                            ps.computed_preamp = Some(found);
                        }
                    }
                });
        }
    }

    /// Aktualisiert den Fortschritt der `PEAK SEARCH ... XX%`-Anzeige und gibt `true` zurück,
    /// solange die Anzeige aktiv ist (damit das UI mit 60 FPS neu gezeichnet wird).
    pub fn step_peak_search_progress(&mut self) -> bool {
        let Some(mut ps) = self.peak_search.take() else {
            return false;
        };
        if !ps.active {
            return false;
        }
        let now = Instant::now();
        if let Some(until) = ps.hold_100_until {
            ps.percent = 100;
            ps.scanned_order = ps.total_orders.max(1);
            if now >= until {
                return false;
            }
            self.peak_search = Some(ps);
            return true;
        }

        #[cfg(feature = "retro-win32")]
        if ps.computed_preamp.is_none() {
            if let Some(CoopPeakState(ref coop_arc)) = ps.coop_state {
                let mut finished_found: Option<(u8, u32, String)> = None;
                if let Ok(mut guard) = coop_arc.lock() {
                    let (ref mut seq, ref mut ticks_run, fsize, ref mtitle) = *guard;
                    if let Some(raw_pa) = seq.step_incremental_peak_search(96, ticks_run) {
                        finished_found = Some(((raw_pa & 0x7F).min(40), fsize, mtitle.clone()));
                    }
                }
                if let Some((found, fsize, mtitle)) = finished_found {
                    let pks_path = Mod4WinIniConfig::default_pks_path();
                    self.ini_config.store_peak(fsize, &mtitle, found, &pks_path);
                    let is_native =
                        CpuProfile::from_u8(self.ini_config.cpu_sim_profile) == CpuProfile::Native;
                    if is_native {
                        self.sound_settings.pre_amp = (self.sound_settings.pre_amp & 0x80) | found;
                        self.ini_config.pre_amplification = found;
                        self.settings_dirty = true;
                    }
                    ps.computed_preamp = Some(found);
                    ps.coop_state = None;
                }
            }
        }

        let elapsed = now.saturating_duration_since(ps.started_at).as_secs_f32();
        let frac = (elapsed / ps.duration_secs.max(0.01)).clamp(0.0, 1.0);
        if frac >= 1.0 {
            if let Some(found) = ps.computed_preamp {
                let pa = found.min(40);
                self.sound_settings.pre_amp = (self.sound_settings.pre_amp & 0x80) | pa;
                self.ini_config.pre_amplification = pa;
                self.settings_dirty = true;
                ps.percent = 100;
                ps.scanned_order = ps.total_orders.max(1);
                ps.hold_100_until = Some(now + std::time::Duration::from_millis(250));
            } else {
                ps.percent = 99;
                ps.scanned_order = ps.total_orders.saturating_sub(1).max(1);
            }
        } else {
            ps.percent = ((frac * 99.0).floor() as u8).min(99);
            let ord_span = ps.total_orders.saturating_sub(1) as f32;
            ps.scanned_order =
                (1 + (frac * ord_span).round() as usize).clamp(1, ps.total_orders.max(1));
        }
        self.peak_search = Some(ps);
        true
    }

    /// Liefert den aktuellen Banner-String (`PEAK SEARCH ... XX%` bzw. `PEAK SEARCH 100%`)
    /// und die gescannte Order-Nummer sowie Gesamt-Orders für das Pattern-LCD, falls gerade ein PeakSearch läuft.
    pub fn active_peak_search_display(&self) -> Option<(String, usize, usize)> {
        let ps = self.peak_search.as_ref()?;
        if !ps.active {
            return None;
        }
        let txt = if ps.percent >= 100 {
            "PEAK SEARCH 100%".to_string()
        } else {
            format!("PEAK SEARCH ... {:>2}%", ps.percent)
        };
        Some((txt, ps.scanned_order.max(1), ps.total_orders.max(1)))
    }

    /// Setzt den aktiven UI-Skin (`2.40 Classic` oder `Modern`) und hält `ini_config.skin_mode` synchron.
    pub fn set_skin_mode(&mut self, mode: SkinMode) {
        let normalized = mode;
        self.skin_mode = normalized;
        self.ini_config.skin_mode = normalized.to_u8();
    }

    /// Überträgt die Werte aus `self.ini_config` in `self.sound_settings` und markiert `settings_dirty = true`.
    pub fn apply_ini_to_sound_settings(&mut self) {
        self.sound_settings.device_id = self.ini_config.device_id as i16;
        self.sound_settings.samplerate = self.ini_config.sample_rate.clamp(4000, 96000) as i32;
        self.sound_settings.bits = if self.ini_config.bits_per_sample == 8 { 8 } else { 16 };
        self.sound_settings.stereo = self.ini_config.channels.clamp(1, 2);
        self.sound_settings.surround = self.ini_config.surround && self.sound_settings.stereo == 2;
        self.sound_settings.n_buffers = if self.ini_config.device_id == DEV_HDD {
            self.ini_config.hdd_rec_buffers.clamp(1, 99)
        } else {
            self.ini_config.output_buffers.clamp(2, 99)
        };
        self.sound_settings.set_timer_mode(self.ini_config.timer_mode.min(2));
        self.sound_settings.set_clean_mode(self.ini_config.clean_mode);
        self.sound_settings.set_declick(self.ini_config.declick);
        self.sound_settings.oversamp = self.ini_config.over_sampling.min(6);
        self.sound_settings.panning = self.ini_config.enable_panning;
        let clip_bit = if self.ini_config.clipping || self.ini_config.declick {
            0x80
        } else {
            0x00
        };
        let pa = self.ini_config.pre_amplification.min(40);
        self.sound_settings.pre_amp = clip_bit | pa;
        self.show_filename = self.ini_config.force_filename_display;
        self.settings_dirty = true;
    }

    /// Synchronisiert die aktuellen Laufzeit-Einstellungen und (falls `state_saver` aktiv) den Wiedergabezustand zurück nach `self.ini_config`.
    pub fn sync_runtime_to_ini(&mut self) {
        self.ini_config.sample_rate = self.sound_settings.samplerate.max(4000) as u32;
        self.ini_config.bits_per_sample = self.sound_settings.bits;
        self.ini_config.channels = self.sound_settings.stereo.clamp(1, 2);
        self.ini_config.surround = self.sound_settings.surround;
        self.ini_config.timer_mode = self.sound_settings.timer_mode();
        self.ini_config.clean_mode = self.sound_settings.clean_mode();
        self.ini_config.declick = self.sound_settings.declick();
        self.ini_config.clipping = (self.sound_settings.pre_amp & 0x80) != 0;
        self.ini_config.pre_amplification = (self.sound_settings.pre_amp & 0x7F).min(40);
        self.ini_config.over_sampling = self.sound_settings.oversamp.min(6);
        self.ini_config.enable_panning = self.sound_settings.panning;
        self.ini_config.force_filename_display = self.show_filename;
        self.ini_config.saved_repeat_mode = self.repeat_mode;
        self.ini_config.saved_shuffle = self.shuffle;
        self.ini_config.saved_intro_scan = self.intro_scan;
        self.ini_config.saved_minus_mode = self.minus_mode;

        if self.ini_config.state_saver {
            self.ini_config.saved_playlist = self.playlist.clone();
            self.ini_config.saved_playlist_idx = self.playlist_idx;
            self.ini_config.saved_order_pos = self.curr_pos.max(1);
            self.ini_config.saved_playing = self.is_playing;
            self.ini_config.saved_paused = self.is_paused;
        }
    }

    /// Gibt zurück, ob aktuell ein Impulse-Tracker-Modul (`.IT`) geladen ist.
    pub fn is_it_module(&self) -> bool {
        self.format_str.eq_ignore_ascii_case("IT")
    }

    /// Liefert die effektive `oversamp`-Stufe (`0..=3` für klassische Formate, `0..=6` für `.IT`-Module).
    pub fn effective_oversamp(&self) -> u8 {
        if !self.is_it_module() && self.sound_settings.oversamp >= 4 {
            match self.sound_settings.oversamp {
                4 => 0,
                5 => 1,
                _ => 2,
            }
        } else {
            self.sound_settings.oversamp.min(6)
        }
    }

    /// Schaltet `sound_settings.oversamp` vorwärts (`delta > 0`) oder rückwärts (`delta < 0`):
    /// - Bei klassischen Formaten (`.mod`, `.s3m`, `.xm`, ...): Zyklus ausschließlich über `0..=3` (`MOD4WIN IDO 0..3`).
    /// - Bei `.IT`-Modulen: Zyklus über `0..=6` (`MOD4WIN IDO 0..3` + `Native IT 4..6`).
    pub fn step_oversamp(&mut self, delta: i32) {
        let is_it = self.is_it_module();
        let cur = self.effective_oversamp();
        let next = if is_it {
            if delta >= 0 {
                (cur + 1) % 7
            } else {
                (cur + 6) % 7
            }
        } else if delta >= 0 {
            (cur.min(3) + 1) % 4
        } else {
            (cur.min(3) + 3) % 4
        };
        self.sound_settings.oversamp = next;
        let banner = Self::oversamp_banner_text(next, is_it);
        self.set_status_banner(banner);
        self.settings_dirty = true;
        self.sync_runtime_to_ini();
    }

    /// Gibt den `20`-Zeichen-Statusbanner-Text für die aktuelle `oversamp`-Stufe (`0..=6`) zurück.
    pub fn oversamp_banner_text(oversamp: u8, _is_it_module: bool) -> &'static str {
        match oversamp {
            0 => "IDO 0: OFF",
            1 => "IDO 1: LINEAR",
            2 => "IDO 2: CPU CUBIC",
            3 => "IDO 3: FPU CUBIC",
            4 => "IT MODE: OFF",
            5 => "IT MODE: LINEAR",
            _ => "IT MODE: CUBIC",
        }
    }

    /// Schaltet die 3-stufige Clip / De-Click-Anzeige vorwärts (`delta > 0`) oder rückwärts (`delta < 0`):
    /// - Stufe `0`: `Clip aus` (`clipping = false`, `declick = false`)
    /// - Stufe `1`: `Clip an` (`clipping = true`, `declick = false`)
    /// - Stufe `2`: `CL+DC` (`clipping = true`, `declick = true`)
    pub fn step_clip_declick(&mut self, delta: i32) {
        let clip_on = (self.sound_settings.pre_amp & 0x80) != 0;
        let dc_on = self.sound_settings.declick();
        let cur: i32 = match (clip_on, dc_on) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        let next = if delta >= 0 {
            (cur + 1) % 3
        } else {
            (cur + 2) % 3
        };
        let (new_clip, new_dc, banner) = match next {
            0 => (false, false, "CLIP: OFF"),
            1 => (true, false, "CLIP: ON"),
            _ => (true, true, "CLIP + DE-CLICK"),
        };
        if new_clip {
            self.sound_settings.pre_amp |= 0x80;
        } else {
            self.sound_settings.pre_amp &= 0x7F;
        }
        self.sound_settings.set_declick(new_dc);
        self.set_status_banner(banner);
        self.settings_dirty = true;
        self.sync_runtime_to_ini();
    }

    /// Prüft, ob eine Datei ein unterstütztes Tracker-Modul ist.
    pub fn is_tracker_file(path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "mod" | "xm" | "s3m" | "mtm" | "669" | "stm" | "far" | "okt" | "wow" | "ult" | "it" | "umx"
                )
            })
            .unwrap_or(false)
    }

    /// Sammelt alle Tracker-Dateien aus einer einzelnen Datei, einer `.MOL`-Playlist, einem `.ZIP`-Archiv oder rekursiv aus einem Ordner.
    pub fn collect_tracker_files(path: &std::path::Path, out: &mut Vec<PathBuf>) {
        // 1. Fast-Path für bekannte Tracker-Dateiendungen: unter Windows 0 Disk-Syscalls (`GetFileAttributesW` entfällt komplett)!
        if Self::is_tracker_file(path) {
            #[cfg(target_os = "windows")]
            {
                out.push(path.to_path_buf());
                return;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let resolved = super::archive_mol::resolve_path_case_insensitive(path);
                if resolved.is_file() {
                    out.push(resolved);
                    return;
                }
            }
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("mol") {
                out.extend(load_mol_file(path));
                return;
            } else if super::archive_mol::is_supported_archive_ext(ext) {
                let resolved = super::archive_mol::resolve_path_case_insensitive(path);
                if resolved.is_file() {
                    out.extend(extract_all_tracker_files_from_zip(&resolved));
                    return;
                }
            }
        }
        let resolved = super::archive_mol::resolve_path_case_insensitive(path);
        if resolved.is_dir()
            && let Ok(entries) = std::fs::read_dir(&resolved) {
                let mut subdirs = Vec::new();
                let mut files = Vec::new();
                for entry in entries.flatten() {
                    let Ok(ft) = entry.file_type() else {
                        continue;
                    };
                    let p = entry.path();
                    if ft.is_dir() {
                        subdirs.push(p);
                    } else if ft.is_file() {
                        if Self::is_tracker_file(&p) {
                            files.push(p);
                        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                            if ext.eq_ignore_ascii_case("mol") {
                                files.extend(load_mol_file(&p));
                            } else if super::archive_mol::is_supported_archive_ext(ext) {
                                files.extend(extract_all_tracker_files_from_zip(&p));
                            }
                        }
                    }
                }
                files.sort();
                out.extend(files);
                subdirs.sort();
                for d in subdirs {
                    Self::collect_tracker_files(&d, out);
                }
            }
    }

    /// Baut die Permutationsliste `number_list` neu auf (`numberlist` in `mod4win.pas`).
    /// Im Shuffle-Modus wird eine echte Permutation erzeugt, sodass kein Song doppelt läuft,
    /// bevor alle Einträge der Playlist einmal gespielt wurden.
    pub fn rebuild_number_list(&mut self) {
        let len = self.playlist.len();
        if len == 0 {
            self.number_list.clear();
            self.playlist_pos = 0;
            self.playlist_idx = 0;
            return;
        }
        self.number_list = (0..len).collect();
        if self.shuffle && len > 1 {
            // Deterministisch-zeitbasierter LCG-Mix (Fisher-Yates) wie `Random(numfiles)` in Pascal
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
                .unwrap_or(0x1234_5678);
            if seed == 0 {
                seed = 0x9E37_79B9;
            }
            for i in (1..len).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = ((seed >> 32) as usize) % (i + 1);
                self.number_list.swap(i, j);
            }
            // Aktuellen Song an Position 0 setzen, damit er nicht sofort nochmal direkt dahinter kommt
            if let Some(pos) = self.number_list.iter().position(|&idx| idx == self.playlist_idx) {
                self.number_list.swap(0, pos);
            }
            self.playlist_pos = 0;
        } else {
            self.playlist_pos = self.playlist_idx.min(len - 1);
        }
    }

    /// Schaltet `shuffle` um und aktualisiert `number_list` wie `ID_SHUFFLE` in `mod4win.pas`.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.rebuild_number_list();
    }

    /// Schaltet den 15-Sekunden-Intro-Scan (`ID_SCAN` / `handleIntroscanbutton` in `mod4win.pas`) um.
    /// Wenn `intro_scan` aktiviert wird, wird ggf. das aktuelle Verzeichnis in die Playlist aufgenommen
    /// und die Wiedergabe sofort gestartet, damit jedes Modul 15 Sekunden angespielt und automatisch weitergeschaltet wird.
    pub fn toggle_intro_scan(&mut self) {
        self.intro_scan = !self.intro_scan;
        if self.intro_scan {
            if self.playlist.len() <= 1
                && let Some(ref cur_path) = self.current_file_path
                    && let Some(parent_dir) = cur_path.parent()
                        && let Ok(entries) = std::fs::read_dir(parent_dir) {
                            let mut list: Vec<PathBuf> = entries
                                .filter_map(|e| e.ok().map(|d| d.path()))
                                .filter(|p| Self::is_tracker_file(p))
                                .collect();
                            list.sort();
                            if list.len() > 1 {
                                if let Some(pos) = list.iter().position(|p| p == cur_path) {
                                    self.playlist_idx = pos;
                                }
                                self.playlist = list;
                                self.rebuild_number_list();
                            }
                        }
            if !self.is_playing || self.is_paused {
                self.is_playing = true;
                self.is_paused = false;
            }
        }
    }

    /// Entfernt den Eintrag an `remove_idx` aus der Playlist, OHNE die laufende Wiedergabe abzubrechen.
    /// Korrigiert `playlist_idx`, `playlist_pos`, `number_list` und setzt `playing_track_detached = true`,
    /// falls der aktuell spielende Track entfernt wurde, sodass beim automatischen Weiterschalten am Song-Ende
    /// (oder beim Klick auf Next) nahtlos der richtige Folgetrack gespielt wird.
    pub fn remove_playlist_entry(&mut self, remove_idx: usize) -> bool {
        if remove_idx >= self.playlist.len() {
            return false;
        }
        self.playlist.remove(remove_idx);
        if self.playlist.is_empty() {
            self.playlist_idx = 0;
            self.playlist_pos = 0;
            self.number_list.clear();
            self.playing_track_detached = true;
        } else {
            if !self.playing_track_detached {
                if remove_idx < self.playlist_idx {
                    self.playlist_idx -= 1;
                } else if remove_idx == self.playlist_idx {
                    self.playing_track_detached = true;
                    // `playlist_idx` bleibt auf `remove_idx` (`0..=self.playlist.len()`) und zeigt
                    // damit bereits direkt auf den nachgerückten Folgetrack.
                }
            } else if remove_idx < self.playlist_idx {
                self.playlist_idx -= 1;
            }
            if self.number_list.len() == self.playlist.len() + 1 {
                if let Some(pos_in_num) = self.number_list.iter().position(|&v| v == remove_idx) {
                    self.number_list.remove(pos_in_num);
                    if pos_in_num < self.playlist_pos && self.playlist_pos > 0 {
                        self.playlist_pos -= 1;
                    } else if self.playlist_pos >= self.number_list.len() {
                        self.playlist_pos = self.number_list.len().saturating_sub(1);
                    }
                }
                for v in &mut self.number_list {
                    if *v > remove_idx {
                        *v -= 1;
                    }
                }
            } else {
                self.rebuild_number_list();
            }
        }
        self.sync_runtime_to_ini();
        true
    }

    /// Leert die gesamte Playlist, OHNE die laufende Wiedergabe des aktuellen Songs abzubrechen.
    pub fn clear_playlist(&mut self) {
        self.playlist.clear();
        self.number_list.clear();
        self.playlist_idx = 0;
        self.playlist_pos = 0;
        self.playing_track_detached = true;
        self.sync_runtime_to_ini();
    }

    /// Wechselt um `delta` (`-1` = Prev, `+1` = Next) in der Playlist bzw. `number_list`.
    /// Gibt `true` zurück, wenn ein Song geladen/gewechselt wurde.
    pub fn step_song(&mut self, delta: i32, wrap: bool) -> bool {
        let len = self.playlist.len();
        if len == 0 {
            return false;
        }
        if self.number_list.len() != len {
            self.rebuild_number_list();
        }
        if self.playing_track_detached {
            let new_idx = if self.shuffle {
                self.playlist_pos = 0;
                self.number_list[0].min(len - 1)
            } else if delta > 0 {
                if self.playlist_idx < len {
                    self.playlist_idx
                } else if wrap {
                    0
                } else {
                    return false;
                }
            } else if delta < 0 {
                if self.playlist_idx > 0 {
                    (self.playlist_idx - 1).min(len - 1)
                } else if wrap {
                    len - 1
                } else {
                    0
                }
            } else {
                self.playlist_idx.min(len - 1)
            };
            self.playing_track_detached = false;
            self.playlist_idx = new_idx;
            if !self.shuffle {
                self.playlist_pos = new_idx;
            }
            let p = self.playlist[self.playlist_idx].clone();
            self.pending_load_file = Some(p);
            self.is_playing = true;
            self.is_paused = false;
            return true;
        }
        let cur = self.playlist_pos as i32;
        let next = cur + delta;
        let new_pos = if next < 0 {
            if wrap {
                len - 1
            } else {
                0
            }
        } else if next >= len as i32 {
            if wrap {
                if self.shuffle && len > 1 {
                    // Neue Runde mischen, wenn alle Songs einmal durchgelaufen sind
                    self.rebuild_number_list();
                }
                0
            } else {
                return false;
            }
        } else {
            next as usize
        };

        self.playing_track_detached = false;
        self.playlist_pos = new_pos;
        self.playlist_idx = self.number_list[new_pos].min(len - 1);
        let p = self.playlist[self.playlist_idx].clone();
        self.pending_load_file = Some(p);
        self.is_playing = true;
        self.is_paused = false;
        true
    }

    /// Wird vom Audio-Thread aufgerufen, wenn `player_repeats` (Liedende) oder `15s IntroScan` ausgelöst wird.
    pub fn on_auto_advance(&mut self, is_intro_scan_trigger: bool) {
        if self.playlist.is_empty() {
            if !self.playing_track_detached && !is_intro_scan_trigger && self.repeat_mode > 0 {
                self.pending_seek_pattern = Some(1);
            } else {
                self.intro_scan = false;
                self.is_playing = false;
                self.is_paused = false;
                self.pending_seek_pattern = Some(1);
            }
            return;
        }

        // Repeat One (repeat_mode == 2): Aktuellen Song von vorn spielen (außer IntroScan will weiter
        // oder der aktuell laufende Song wurde aus der Playlist gelöscht).
        if self.repeat_mode == 2 && !is_intro_scan_trigger && !self.playing_track_detached {
            self.pending_seek_pattern = Some(1);
            self.is_playing = true;
            self.is_paused = false;
            return;
        }

        let wrap = self.repeat_mode == 1 || is_intro_scan_trigger || self.intro_scan;
        if self.playlist.len() == 1 && !self.playing_track_detached {
            if wrap {
                self.pending_seek_pattern = Some(1);
                self.is_playing = true;
                self.is_paused = false;
            } else {
                self.is_playing = false;
                self.is_paused = false;
                self.pending_seek_pattern = Some(1);
            }
            return;
        }

        if !self.step_song(1, wrap) {
            // Ende der Playlist ohne Repeat erreicht -> Stopp und zurück zu Pattern 1
            self.intro_scan = false;
            self.is_playing = false;
            self.is_paused = false;
            self.pending_seek_pattern = Some(1);
        }
    }

    /// Fügt per Drag-and-Drop übergebene Dateien/Ordner zur Playlist hinzu und startet die Wiedergabe.
    pub fn handle_dropped_paths(&mut self, paths: &[PathBuf], replace_playlist: bool) {
        let mut found = Vec::with_capacity(paths.len());
        for p in paths {
            Self::collect_tracker_files(p, &mut found);
        }
        if found.is_empty() {
            return;
        }
        if replace_playlist || self.playlist.is_empty() {
            self.playlist = found;
            self.playlist_idx = 0;
        } else {
            let start_idx = self.playlist.len();
            self.playlist.extend(found);
            self.playlist_idx = start_idx;
        }
        self.playing_track_detached = false;
        self.rebuild_number_list();
        if let Some(pos) = self.number_list.iter().position(|&i| i == self.playlist_idx) {
            self.playlist_pos = pos;
        }
        if let Some(first_file) = self.playlist.get(self.playlist_idx).cloned() {
            self.pending_load_file = Some(first_file);
            self.is_playing = true;
            self.is_paused = false;
        }
    }

    /// Aktualisiert den Zustand aus der laufenden Sequencer-Engine.
    pub fn update_from_sequencer(&mut self, seq: &Sequencer, elapsed_secs: u32) {
        let state = &seq.module.state;
        self.curr_pos = (state.curr_pattern as usize) + 1;
        self.curr_pattern = state.mi.curr_pttrn as usize;
        self.curr_row = (state.curr_note as usize) + 1;
        self.speed = state.speed;
        self.bpm = state.bpm_speed;
        self.elapsed_secs = elapsed_secs;
        self.effects_used_set = state.mi.effects_used_set;
        if state.mi.total_time > 0 {
            self.total_time_secs = ((state.mi.total_time + 500) / 1000).max(0) as u32;
        }
        if state.mi.song_mem > 0 {
            self.song_mem_bytes = state.mi.song_mem as usize;
        }
        // Wichtig: self.sound_settings niemals rückwärts aus dem Audio-Thread überschreiben,
        // damit Änderungen aus dem UI/Setup-Dialog niemals durch einen Race-Condition-Tick zurückgesetzt werden!

        let now = Instant::now();
        let playing = self.is_playing && !self.is_paused;
        let is_ft2 = state.mi.file_type == crate::engine::types::ModType::FT2;
        let num_ch = self.num_channels.min(32);
        let mut active_mix_channels = 0usize;
        for i in 0..num_ch {
            let ch = &state.channels[i];
            let active = !self.channel_muted[i] && ch.note > 0 && ch.avg_vol > 0;
            if active {
                active_mix_channels += 1;
                self.channel_vols[i] = (ch.l_vol, ch.r_vol);
                self.channel_notes[i] = if ch.note_val != 0 {
                    ch.note_val
                } else if ch.note > 0 && ch.note <= 128 {
                    ch.note as u8
                } else {
                    0
                };
                self.channel_insts[i] = ch.inst;
            } else {
                self.channel_vols[i] = (0, 0);
            }

            if !playing || self.channel_muted[i] {
                self.channel_effects[i] = 0xFF00;
                continue;
            }

            let pat_note = seq.get_current_pattern_note(i);
            let mut raw_eff: Option<u16> = None;

            if ch.effkt != 0xFF && !(ch.effkt == 0 && ch.opnd == 0) {
                raw_eff = Some(((ch.effkt as u16) << 8) | (ch.opnd as u16));
            } else if state.curr_vblanc == 0
                && pat_note.effekt != 0xFF
                && !(pat_note.effekt == 0 && pat_note.operands == 0)
            {
                raw_eff = Some(((pat_note.effekt as u16) << 8) | (pat_note.operands as u16));
            } else if is_ft2 {
                let v = if state.curr_vblanc == 0 && pat_note.chn_vol != 0xFF && pat_note.chn_vol != 0 {
                    pat_note.chn_vol
                } else {
                    ch.old_chn_vol
                };
                raw_eff = match v {
                    0x10..=0x50 if state.curr_vblanc == 0 => {
                        Some(((crate::engine::types::effect_ids::SET_VOLUME as u16) << 8) | ((v - 0x10) as u16))
                    }
                    0x60..=0x7F => {
                        Some(((crate::engine::types::effect_ids::VOLUME_SLIDE as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    0x80..=0x8F => {
                        Some(((crate::engine::types::effect_ids::FINE_VOL_DWN as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    0x90..=0x9F => {
                        Some(((crate::engine::types::effect_ids::FINE_VOL_UP as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    0xB0..=0xBF => {
                        Some(((crate::engine::types::effect_ids::VIBRATO as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    0xC0..=0xCF if state.curr_vblanc == 0 => {
                        Some(((crate::engine::types::effect_ids::E_PANNING as u16) << 8) | ((v & 0x0F) as u16))
                    }
                    0xD0..=0xEF => {
                        Some(((crate::engine::types::effect_ids::PAN_SLD as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    0xF0..=0xFE => {
                        Some(((crate::engine::types::effect_ids::TONE_PORTAMENTO as u16) << 8) | ((v & 0x0F).max(1) as u16))
                    }
                    _ => None,
                };
            }

            if let Some(eff_word) = raw_eff {
                self.channel_effect_hold[i] = (eff_word, now);
                self.channel_effects[i] = eff_word;
            } else {
                let (held_eff, held_ts) = self.channel_effect_hold[i];
                if (held_eff >> 8) != 0xFF && now.saturating_duration_since(held_ts).as_millis() < 160 {
                    self.channel_effects[i] = held_eff;
                } else {
                    self.channel_effects[i] = 0xFF00;
                }
            }
        }

        let dt_secs = now
            .saturating_duration_since(self.last_telemetry_instant)
            .as_secs_f32()
            .clamp(0.002, 0.25);
        self.last_telemetry_instant = now;

        let profile = CpuProfile::from_u8(self.ini_config.cpu_sim_profile);
        let output_buffers = if self.ini_config.device_id == DEV_HDD {
            self.ini_config.hdd_rec_buffers.clamp(1, 99)
        } else {
            self.ini_config.output_buffers.clamp(2, 99)
        };
        self.cpu_telemetry.step_telemetry(
            profile,
            playing,
            active_mix_channels,
            &self.sound_settings,
            output_buffers,
            self.native_callback_ratio,
            dt_secs,
        );
    }

    /// Liefert die Liste der aktiven Effekt-Spalten-IDs (`0..=67`, ohne `14`) wie in `ana.pas`.
    pub fn active_effect_columns(&self) -> Vec<u8> {
        let mut cols = Vec::new();
        for i in 0..=67u8 {
            if i == 14 {
                continue;
            }
            let byte_idx = (i / 8) as usize;
            let bit_idx = i % 8;
            if (self.effects_used_set[byte_idx] & (1 << bit_idx)) != 0 {
                cols.push(i);
            }
        }
        if cols.is_empty() {
            // Standard-Spalten falls noch keine Effekte im Set eingetragen sind
            vec![0, 1, 2, 3, 10, 12, 15]
        } else {
            cols
        }
    }
}

/// Einzelnes OS-Fenster mit eigener `Pixels`-Oberfläche.
pub(crate) struct WindowSurface {
    pub(crate) kind: WindowKind,
    pub(crate) window: Arc<Window>,
    pub(crate) pixels: Pixels<'static>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) mouse_pos: (f64, f64),
    pub(crate) last_click_time: Instant,
    pub(crate) active_resize_dir: Option<ResizeDirection>,
    pub(crate) needs_aspect_snap: bool,
    pub(crate) last_outer_pos: Option<(i32, i32)>,
}

#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
unsafe extern "system" {
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn GetWindowLongW(hwnd: isize, n_index: i32) -> i32;
    fn SetWindowLongW(hwnd: isize, n_index: i32, dw_new_long: i32) -> i32;
}

/// Versteckt ein Unterfenster (`kind != WindowKind::Main`) zuverlässig aus der Windows-Taskleiste und `Alt+Tab`
/// mittels `WS_EX_TOOLWINDOW` (und Löschen von `WS_EX_APPWINDOW`), genau wie im Original-MOD4WIN.
pub(crate) fn hide_win32_window_from_taskbar(window: &Window) {
    #[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
    {
        use super::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw()
        {
            const GWL_EXSTYLE: i32 = -20;
            const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;
            const WS_EX_APPWINDOW: i32 = 0x0004_0000;
            const SWP_NOSIZE: u32 = 0x0001;
            const SWP_NOMOVE: u32 = 0x0002;
            const SWP_NOZORDER: u32 = 0x0004;
            const SWP_NOACTIVATE: u32 = 0x0010;
            const SWP_FRAMECHANGED: u32 = 0x0020;
            unsafe {
                let hwnd = h.hwnd.get();
                let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
                let new_ex = (cur_ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
                if new_ex != cur_ex {
                    SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex);
                    SetWindowPos(
                        hwnd,
                        0,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                    );
                }
            }
        }
    }
    #[cfg(any(not(target_os = "windows"), feature = "retro-win32"))]
    let _ = window;
}

/// Erzwingt unter Windows sofort die physische Fenstergröße (`SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE`),
/// damit auch nicht-fokussierte Hintergrundfenster beim Live-Skin-Wechsel ohne Verzögerung umskalieren.
pub(crate) fn force_win32_window_size(window: &Window, phys_w: u32, phys_h: u32) {
    #[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
    {
        use super::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw() {
                const SWP_NOMOVE: u32 = 0x0002;
                const SWP_NOZORDER: u32 = 0x0004;
                const SWP_NOACTIVATE: u32 = 0x0010;
                unsafe {
                    SetWindowPos(
                        h.hwnd.get(),
                        0,
                        0,
                        0,
                        phys_w as i32,
                        phys_h as i32,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
    }
    #[cfg(feature = "retro-win32")]
    window.force_size(phys_w, phys_h);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, phys_w, phys_h);
}

/// Hebt ein Fenster in der Z-Order ganz nach vorne (`HWND_TOP`), ohne den aktiven Eingabefokus zu stehlen (`SWP_NOACTIVATE`).
pub(crate) fn raise_win32_window_noactivate(window: &Window) {
    #[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
    {
        use super::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw() {
                const HWND_TOP: isize = 0;
                const SWP_NOSIZE: u32 = 0x0001;
                const SWP_NOMOVE: u32 = 0x0002;
                const SWP_NOACTIVATE: u32 = 0x0010;
                unsafe {
                    SetWindowPos(
                        h.hwnd.get(),
                        HWND_TOP,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
    }
    #[cfg(feature = "retro-win32")]
    window.raise_noactivate();
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// Erkennt, ob sich der Mauszeiger an einer ziehbaren Kante/Ecke befindet:
/// - Am Hauptfenster (`WindowKind::Main`): Alle 8 Kanten/Ecken für globales GUI-Scaling (außer in 32-Bit `retro-win32`/`x86`).
/// - Am Playlist-Dock im Modern Skin (`QuickSelect` / `OpenDialog`): Untere Fensterkante (`ResizeDirection::South`)
///   zum vertikalen Vergrößern/Verkleinern der sichtbaren Zeilenanzahl (`5..=36` Zeilen).
/// - An allen anderen Unterfenstern: Kein Ziehen an Kanten/Ecken (`None`).
fn detect_resize_direction_for_window(
    kind: WindowKind,
    is_modern_skin: bool,
    phys_x: f64,
    phys_y: f64,
    win_size: PhysicalSize<u32>,
) -> Option<ResizeDirection> {
    let w = win_size.width as f64;
    let h = win_size.height as f64;
    if w < 32.0 || h < 32.0 {
        return None;
    }
    if is_modern_skin && matches!(kind, WindowKind::QuickSelect | WindowKind::OpenDialog) {
        let bottom_band = 8.0;
        if phys_y <= h && phys_y >= (h - bottom_band).max(24.0) && phys_x >= 0.0 && phys_x <= w {
            return Some(ResizeDirection::South);
        }
        return None;
    }
    if kind != WindowKind::Main {
        return None;
    }
    #[cfg(any(feature = "retro-win32", target_arch = "x86"))]
    {
        let _ = (phys_x, phys_y);
        return None;
    }
    #[cfg(not(any(feature = "retro-win32", target_arch = "x86")))]
    {
        let edge = 5.0;
        let corner = 10.0;
        // In der oberen Titelleiste (außer an den äußersten 3 Pixeln oben) haben die System-/Close-Buttons Vorrang
        let top_band_limit = (h * (19.0 / 135.0)).clamp(18.0, 44.0);
        let in_titlebar_band = phys_y >= 3.0 && phys_y <= top_band_limit;

        let on_left = phys_x >= 0.0 && phys_x < edge && !in_titlebar_band;
        let on_right = phys_x <= w && phys_x > w - edge && !in_titlebar_band;
        let on_top = (0.0..3.0).contains(&phys_y);
        let on_bottom = phys_y <= h && phys_y > h - edge;

        let corner_left = phys_x >= 0.0 && phys_x < corner;
        let corner_right = phys_x <= w && phys_x > w - corner;
        let corner_bottom = phys_y <= h && phys_y > h - corner;

        if corner_right && corner_bottom {
            Some(ResizeDirection::SouthEast)
        } else if corner_left && corner_bottom {
            Some(ResizeDirection::SouthWest)
        } else if on_top && corner_right {
            Some(ResizeDirection::NorthEast)
        } else if on_top && corner_left {
            Some(ResizeDirection::NorthWest)
        } else if on_bottom {
            Some(ResizeDirection::South)
        } else if on_right {
            Some(ResizeDirection::East)
        } else if on_left {
            Some(ResizeDirection::West)
        } else if on_top {
            Some(ResizeDirection::North)
        } else {
            None
        }
    }
}

fn cursor_for_resize_dir(dir: Option<ResizeDirection>) -> CursorIcon {
    match dir {
        Some(ResizeDirection::East | ResizeDirection::West) => CursorIcon::EwResize,
        Some(ResizeDirection::North | ResizeDirection::South) => CursorIcon::NsResize,
        Some(ResizeDirection::NorthWest | ResizeDirection::SouthEast) => CursorIcon::NwseResize,
        Some(ResizeDirection::NorthEast | ResizeDirection::SouthWest) => CursorIcon::NeswResize,
        None => CursorIcon::Default,
    }
}

/// Hauptanwendung für die MOD4WIN 3.0 Beta Multi-Window-Oberfläche.
pub struct Mod4WinApp {
    pub(crate) shared_state: Arc<Mutex<UiSharedState>>,
    pub(crate) assets: EmbeddedAssets,
    pub(crate) windows: HashMap<WindowId, WindowSurface>,
    pub(crate) kind_to_id: HashMap<WindowKind, WindowId>,

    // UI-Status für Fenster und Unterdialoge
    pub(crate) is_mini: bool,
    pub(crate) info_inst_offset: usize, // 0 oder 16 (bzw. 31+) für [-]/[+] im Module-Info-Fenster
    pub(crate) selected_instrument: usize, // 1-basierter Index für das Instrumenten-Fenster (bm11)
    pub(crate) quick_select_sort: u8, // 0 = NONE, 1 = NAME, 2 = EXT
    pub(crate) quick_select_scroll: usize, // Scroll-Offset in der Quick Selection (bm3.bmp)
    pub(crate) quick_select_cursor: usize, // Aktuell markierter Eintrag in der Quick Selection (`orig_idx`)
    pub(crate) quick_select_all_selected: bool, // `Strg+A`: Alle Einträge in der Quick Selection / Modern Playlist markiert
    pub(crate) playlist_scroll_dragging: bool, // Ziehen des vertikalen Scrollbalkens im Modern Playlist-Dock
    pub(crate) modern_playlist_rows: usize, // Anzahl sichtbarer Zeilen im Modern Playlist-Dock (`5..=36`, Standard `9`)
    pub(crate) open_dlg: OpenDialogState, // Commander-Zustand für „Open Module“ (newopen.pas)
    pub(crate) setup_cfg: Mod4WinIniConfig, // Arbeitskopie der Einstellungen im geöffneten Setup-Dialog (`dsetup.pas`)
    pub(crate) setup_backup_cfg: Mod4WinIniConfig, // Backup zum Wiederherstellen bei Cancel im Setup-Dialog
    pub(crate) setup_edit_focus: SetupEditFocus,
    pub(crate) setup_paused_playback: bool,
    pub(crate) wav_ask_dlg: WavAskState, // Zustand des `TAskWin`-Dialogs für Direct-to-Disk `.WAV`-Export
    pub(crate) fsel_dlg: FileSelectState, // Zustand des `fsel_*` Datei-/Ordner-Browsers (`MULBOX.PAS`)
    pub(crate) file_menu_open: bool, // Popup-Menü des `File`-Buttons (`Copy F5`, `Move/Rename F6`, `Remove F7`, `Delete F8`)
    pub(crate) file_menu_hover: Option<usize>,
    pub(crate) sys_menu_open: bool,
    pub(crate) sys_menu_hover: Option<usize>,
    pub(crate) confirm_action: Option<ConfirmAction>, // Sicherheitsabfrage für `F7` / `F8`
    pub(crate) file_man_dlg: FileManagerState, // Prozeduraler Mini-File-Manager (`COPYWND.PAS`)
    pub(crate) hotkey_dlg: HotkeyEditorState, // Prozeduraler Hotkey-Editor (`KEYSEL.PAS`)
    pub(crate) archive_dlg: ArchiveSetupState, // Prozedurales Archiv-Setup (`tarcopt` & `tmorearc` in `dsetup.pas`)
    pub(crate) about_opened_at: Instant, // Zeitstempel für die vertikale Laufschrift (`ABOUT.PAS`)
    pub(crate) help_dlg: HelpViewerState, // Integrierter WinHelp-Viewer (`M4W_GER.HLP` + `bm0`..`bm15` + `hotspots.json`)
    pub(crate) active_window_kind: WindowKind, // Zuletzt aktives Fenster für kontextsensitives `F1`
    pub(crate) modifiers: Modifiers,
    pub(crate) scale_factor: f64,
    pub(crate) preamp_dragging: bool,
    pub(crate) pressed_btn: Option<PressedButton>,
    pub(crate) pressed_btn_tracker: Option<(PressedButton, Instant, Instant)>,
    pub(crate) last_drop_time: Option<Instant>,
    pub(crate) touch_gesture: TouchGestureState,
    pub(crate) locale: LocaleCatalog,
    pub(crate) suppress_move_snap: bool,
    pub(crate) bringing_to_front: bool,
    pub(crate) last_bring_to_front: Instant,
    pub(crate) last_redraw_poll: Instant,
    pub(crate) render_scratch_buf: Vec<u8>,
}

impl Mod4WinApp {
    pub fn new(shared_state: Arc<Mutex<UiSharedState>>) -> Self {
        let assets = EmbeddedAssets::load();
        let (initial_dir, initial_playlist, initial_ini, is_mini, st_clone) = {
            let st = shared_state.lock().unwrap();
            let dir = st
                .current_file_path
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .or_else(|| {
                    if !st.ini_config.last_dir.is_empty() {
                        Some(PathBuf::from(&st.ini_config.last_dir))
                    } else {
                        None
                    }
                })
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            (
                dir,
                st.playlist.clone(),
                st.ini_config.clone(),
                st.ini_config.mini_state,
                st.clone(),
            )
        };
        let init_scale = if cfg!(any(feature = "retro-win32", target_arch = "x86")) {
            1.0
        } else {
            initial_ini.gui_scale.clamp(0.5, 4.0)
        };
        let mut initial_ini = initial_ini;
        initial_ini.gui_scale = init_scale;
        let init_playlist_rows = initial_ini.modern_playlist_rows.clamp(5, 36) as usize;
        initial_ini.modern_playlist_rows = init_playlist_rows as u8;
        let now = Instant::now();
        Self {
            shared_state,
            assets,
            windows: HashMap::new(),
            kind_to_id: HashMap::new(),
            is_mini,
            info_inst_offset: 0,
            selected_instrument: 1,
            quick_select_sort: 0,
            quick_select_scroll: 0,
            quick_select_cursor: st_clone.playlist_idx,
            quick_select_all_selected: false,
            playlist_scroll_dragging: false,
            modern_playlist_rows: init_playlist_rows,
            open_dlg: OpenDialogState::new_lazy(initial_dir, &initial_playlist),
            setup_cfg: initial_ini.clone(),
            setup_backup_cfg: initial_ini.clone(),
            setup_edit_focus: SetupEditFocus::None,
            setup_paused_playback: false,
            wav_ask_dlg: WavAskState {
                path: initial_ini.hd_path.clone(),
                filename: "MODULE".to_string(),
                focus: WavAskEditFocus::Filename,
            },
            fsel_dlg: FileSelectState::new_lazy(FileSelectTarget::SetupRecDir, &initial_ini.hd_path),
            file_menu_open: false,
            file_menu_hover: None,
            sys_menu_open: false,
            sys_menu_hover: None,
            confirm_action: None,
            file_man_dlg: FileManagerState::new_lazy(true),
            hotkey_dlg: HotkeyEditorState {
                working_keys: initial_ini.hotkeys.clone(),
                scroll: 0,
                cursor: 0,
                capturing: false,
            },
            archive_dlg: ArchiveSetupState::default(),
            about_opened_at: now,
            help_dlg: HelpViewerState::new_with_language(initial_ini.language),
            active_window_kind: WindowKind::Main,
            modifiers: Modifiers::default(),
            scale_factor: init_scale,
            preamp_dragging: false,
            pressed_btn: None,
            pressed_btn_tracker: None,
            last_drop_time: None,
            touch_gesture: TouchGestureState::default(),
            locale: LocaleCatalog::load_for_language_id(initial_ini.language),
            suppress_move_snap: false,
            bringing_to_front: false,
            last_bring_to_front: now,
            last_redraw_poll: now,
            render_scratch_buf: Vec::with_capacity(580 * 505 * 4),
        }
    }

    /// Liefert den INI-Schlüssel unter `[Windows]` für das jeweilige Fenster.
    pub(crate) fn window_ini_key(&self, kind: WindowKind) -> &'static str {
        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    "MINI"
                } else {
                    "MAIN"
                }
            }
            WindowKind::ModuleInfo => "INFO",
            WindowKind::InstrumentInfo => "INSTRUMENTS",
            WindowKind::Effects => "EFFECTS",
            WindowKind::EffectsHelp => "EFFECTS_HELP",
            WindowKind::QuickSelect => "QUICK_SELECT",
            WindowKind::OpenDialog => "OPEN_DIALOG",
            WindowKind::SetupDialog => "SETUP",
            WindowKind::WavAskDialog => "WAV_ASK",
            WindowKind::FileSelectDialog => "FILE_SELECT",
            WindowKind::FileManager => "FILE_MANAGER",
            WindowKind::HotkeyEditor => "HOTKEYS",
            WindowKind::ArchiveSetup => "ARCHIVE_SETUP",
            WindowKind::About => "ABOUT",
            WindowKind::HelpViewer => "HELP",
            WindowKind::FileMenu => "FILE_MENU",
            WindowKind::ConfirmDialog => "CONFIRM_DIALOG",
        }
    }

    /// Wandelt einen INI-Fensterschlüssel (`[State]` -> `OpenWindows=...`) wieder in `WindowKind` um.
    pub(crate) fn window_kind_from_ini_key(key: &str) -> Option<WindowKind> {
        match key.trim().to_ascii_uppercase().as_str() {
            "INFO" => Some(WindowKind::ModuleInfo),
            "INSTRUMENTS" => Some(WindowKind::InstrumentInfo),
            "EFFECTS" => Some(WindowKind::Effects),
            "EFFECTS_HELP" => Some(WindowKind::EffectsHelp),
            "QUICK_SELECT" => Some(WindowKind::QuickSelect),
            "OPEN_DIALOG" => Some(WindowKind::OpenDialog),
            "SETUP" => Some(WindowKind::SetupDialog),
            "HELP" => Some(WindowKind::HelpViewer),
            _ => None,
        }
    }

    /// Sammelt alle aktuell geöffneten persistenten Unterfenster für `OpenWindows=` in `MOD4WIN.INI`.
    pub(crate) fn collect_open_window_keys(&self) -> Vec<String> {
        const PERSISTENT_KINDS: [WindowKind; 8] = [
            WindowKind::ModuleInfo,
            WindowKind::InstrumentInfo,
            WindowKind::Effects,
            WindowKind::EffectsHelp,
            WindowKind::QuickSelect,
            WindowKind::OpenDialog,
            WindowKind::SetupDialog,
            WindowKind::HelpViewer,
        ];
        let mut keys = Vec::new();
        for kind in PERSISTENT_KINDS {
            if self.is_window_open(kind) {
                keys.push(self.window_ini_key(kind).to_string());
            }
        }
        keys
    }

    /// Berechnet die virtuelle Fensterhöhe des Modern Playlist-Docks (`81 + rows * 16`, Standard `9` Zeilen = `MODERN_PLAYLIST_H = 225 px`).
    pub(crate) fn modern_playlist_height(&self) -> u32 {
        (MODERN_PLAYLIST_H - 9 * 16) + (self.modern_playlist_rows.clamp(5, 36) as u32) * 16
    }

    /// Setzt den aktiven Skin (`2.40 Classic`, `Modern`) live im Betrieb ohne Neustart
    /// und speichert ihn in `MOD4WIN.INI`.
    pub fn set_skin_live(&mut self, mode: SkinMode) {
        self.setup_cfg.skin_mode = mode.to_u8();
        if let Ok(mut st) = self.shared_state.lock() {
            st.set_skin_mode(mode);
            st.set_status_banner(mode.lcd_banner());
            let _ = st
                .ini_config
                .save_to_file(&Mod4WinIniConfig::default_ini_path());
        }
        self.apply_skin_to_all_open_windows();
    }

    /// Schaltet den aktiven Skin zyklisch weiter (`2.40 -> Modern -> 2.40`).
    pub fn cycle_skin_live(&mut self) -> SkinMode {
        let next_mode = {
            let st = self.shared_state.lock().unwrap();
            let cur = if st.skin_mode == SkinMode::Modern {
                SkinMode::Modern
            } else {
                SkinMode::Classic240
            };
            cur.next()
        };
        self.set_skin_live(next_mode);
        next_mode
    }

    /// Holt alle aktuell geöffneten MOD4WIN-Fenster gemeinsam vor fremde Hintergrundanwendungen (`Bring All To Front`),
    /// sobald ein beliebiges MOD4WIN-Fenster den Fokus erhält (`WindowEvent::Focused(true)`), ohne Fokus-Endlosschleifen.
    pub(crate) fn bring_all_windows_to_front(&mut self, focused_id: WindowId) {
        if self.bringing_to_front || self.last_bring_to_front.elapsed().as_millis() < 60 {
            return;
        }
        self.bringing_to_front = true;
        self.last_bring_to_front = Instant::now();

        for (&other_id, other_surf) in &self.windows {
            if other_id != focused_id {
                raise_win32_window_noactivate(&other_surf.window);
            }
        }
        if let Some(focused_surf) = self.windows.get(&focused_id) {
            raise_win32_window_noactivate(&focused_surf.window);
        }

        self.bringing_to_front = false;
    }

    /// Auto-Repeat für gedrückt gehaltene `+/-` bzw. `Up/Down`-Buttons im Setup- und Info-Fenster
    /// über alle Skins hinweg (Initiale Verzögerung `320 ms`, danach Wiederholung alle `65 ms`).
    pub(crate) fn poll_held_button_repeat(&mut self) {
        let Some(btn) = self.pressed_btn else {
            self.pressed_btn_tracker = None;
            return;
        };
        let is_repeatable = matches!(
            btn,
            PressedButton::SetupSrUp
                | PressedButton::SetupSrDown
                | PressedButton::SetupBufUp
                | PressedButton::SetupBufDown
                | PressedButton::SetupIdoUp
                | PressedButton::SetupIdoDown
                | PressedButton::InfoMinus
                | PressedButton::InfoPlus
                | PressedButton::InstMinus
                | PressedButton::InstPlus
        );
        if !is_repeatable {
            self.pressed_btn_tracker = None;
            return;
        }
        let now = Instant::now();
        let (since, last_step) = match self.pressed_btn_tracker {
            Some((tracked_btn, since, last_step)) if tracked_btn == btn => (since, last_step),
            _ => {
                self.pressed_btn_tracker = Some((btn, now, now));
                return;
            }
        };
        if now.duration_since(since).as_millis() < 320
            || now.duration_since(last_step).as_millis() < 65
        {
            return;
        }
        self.pressed_btn_tracker = Some((btn, since, now));
        match btn {
            PressedButton::SetupSrUp => {
                self.step_setup_samplerate(1);
                self.sync_live_setup_to_state();
            }
            PressedButton::SetupSrDown => {
                self.step_setup_samplerate(-1);
                self.sync_live_setup_to_state();
            }
            PressedButton::SetupBufUp => {
                if self.setup_cfg.device_id == DEV_HDD {
                    self.setup_cfg.hdd_rec_buffers =
                        (self.setup_cfg.hdd_rec_buffers as i32 + 1).clamp(1, 99) as u8;
                } else {
                    self.setup_cfg.output_buffers =
                        (self.setup_cfg.output_buffers as i32 + 1).clamp(2, 99) as u8;
                }
                self.sync_live_setup_to_state();
            }
            PressedButton::SetupBufDown => {
                if self.setup_cfg.device_id == DEV_HDD {
                    self.setup_cfg.hdd_rec_buffers =
                        (self.setup_cfg.hdd_rec_buffers as i32 - 1).clamp(1, 99) as u8;
                } else {
                    self.setup_cfg.output_buffers =
                        (self.setup_cfg.output_buffers as i32 - 1).clamp(2, 99) as u8;
                }
                self.sync_live_setup_to_state();
            }
            PressedButton::SetupIdoUp => {
                let is_it = self.shared_state.lock().unwrap().is_it_module();
                let max_ido: u8 = if is_it { 7 } else { 4 };
                self.setup_cfg.over_sampling = (self.setup_cfg.over_sampling + 1) % max_ido;
                self.sync_live_setup_to_state();
            }
            PressedButton::SetupIdoDown => {
                let is_it = self.shared_state.lock().unwrap().is_it_module();
                let max_ido: u8 = if is_it { 7 } else { 4 };
                self.setup_cfg.over_sampling =
                    (self.setup_cfg.over_sampling + max_ido - 1) % max_ido;
                self.sync_live_setup_to_state();
            }
            PressedButton::InfoMinus | PressedButton::InstMinus => {
                if self.selected_instrument > 1 {
                    self.selected_instrument -= 1;
                    if self.selected_instrument <= self.info_inst_offset {
                        self.info_inst_offset = self.info_inst_offset.saturating_sub(31);
                    }
                }
            }
            PressedButton::InfoPlus | PressedButton::InstPlus => {
                let max_inst = {
                    let st = self.shared_state.lock().unwrap();
                    st.instruments.len().max(1)
                };
                if self.selected_instrument < max_inst {
                    self.selected_instrument += 1;
                    if self.selected_instrument > self.info_inst_offset + 31 {
                        self.info_inst_offset += 31;
                    }
                }
            }
            _ => {}
        }
    }

    /// Setzt den globalen Skalierungsfaktor (Standard `1.0` = 1:1 Originalgröße aus `mod4win.pas`, stufenlos `0.5..4.0`)
    /// und skaliert alle aktuell geöffneten Fenster unter Beibehaltung des exakten Seitenverhältnisses.
    /// In der 32-Bit-Version (`retro-win32` / `x86`) ist der Skalierungsfaktor hart auf `1.00` fixiert.
    pub fn set_scale_factor_live(&mut self, new_scale: f64) {
        if cfg!(any(feature = "retro-win32", target_arch = "x86")) {
            let _ = new_scale;
            self.scale_factor = 1.0;
            self.setup_cfg.gui_scale = 1.0;
            return;
        }
        let clamped = new_scale.clamp(0.5, 4.0);
        self.scale_factor = clamped;
        self.setup_cfg.gui_scale = clamped;
        for surf in self.windows.values_mut() {
            let phys_w = ((surf.width as f64) * clamped).round().max(32.0) as u32;
            let phys_h = ((surf.height as f64) * clamped).round().max(32.0) as u32;
            surf.needs_aspect_snap = false;
            let _ = surf.window.request_inner_size(PhysicalSize::new(phys_w, phys_h));
            force_win32_window_size(&surf.window, phys_w, phys_h);
            let _ = surf.pixels.resize_surface(phys_w, phys_h);
            surf.window.request_redraw();
        }
        if let Ok(mut st) = self.shared_state.lock() {
            st.ini_config.gui_scale = clamped;
            st.set_status_banner(format!("SCALE {:.2}X", clamped));
            let _ = st
                .ini_config
                .save_to_file(&Mod4WinIniConfig::default_ini_path());
        }
    }

    /// Liefert die Basis-Fensterauflösung `(width, height)` abhängig von `WindowKind` und `SkinMode` (`Classic` vs. `Modern`).
    pub fn window_base_size(
        &self,
        kind: WindowKind,
        skin: SkinMode,
        state: &UiSharedState,
    ) -> (u32, u32) {
        if skin == SkinMode::Modern {
            match kind {
                WindowKind::Main => {
                    if self.is_mini {
                        return (MODERN_MINI_W, MODERN_MINI_H);
                    } else {
                        return (MODERN_MAIN_W, MODERN_MAIN_H);
                    }
                }
                WindowKind::ModuleInfo | WindowKind::InstrumentInfo => {
                    return (MODERN_INFO_W, MODERN_INFO_H);
                }
                WindowKind::Effects => {
                    let num_ch = state.num_channels.clamp(4, 32) as u32;
                    let num_fx = state.active_effect_columns().len().max(1) as u32;
                    let w = 350.max(115 + num_ch * 20);
                    let h = 90 + num_fx * 15;
                    return (w, h);
                }
                WindowKind::QuickSelect | WindowKind::OpenDialog => {
                    return (MODERN_PLAYLIST_W, self.modern_playlist_height());
                }
                WindowKind::SetupDialog => {
                    return (MODERN_SETUP_W, MODERN_SETUP_H);
                }
                WindowKind::About => {
                    return (MODERN_ABOUT_W, MODERN_ABOUT_H);
                }
                _ => {}
            }
        }
        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    (MINI_WINDOW_WIDTH, MINI_WINDOW_HEIGHT)
                } else {
                    (MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT)
                }
            }
            WindowKind::ModuleInfo => (INFO_WINDOW_WIDTH, INFO_WINDOW_HEIGHT),
            WindowKind::InstrumentInfo => (INST_WINDOW_WIDTH, INST_WINDOW_HEIGHT),
            WindowKind::Effects => {
                let cols = state.active_effect_columns();
                let dey = cols.len() as u32;
                let w = 124 + 14 * dey;
                let h = 65 + (state.num_channels.max(4) as u32 * 9) + 24;
                (w, h)
            }
            WindowKind::EffectsHelp => (EFF_WINDOW_WIDTH, EFF_WINDOW_HEIGHT),
            WindowKind::QuickSelect => {
                let rows = state.playlist.len().clamp(1, 12) as u32;
                (QUICK_SELECT_WIDTH, 56 + rows.saturating_sub(1) * 13)
            }
            WindowKind::OpenDialog => (OPEN_WINDOW_WIDTH, OPEN_WINDOW_HEIGHT),
            WindowKind::SetupDialog => (SETUP_WINDOW_WIDTH, SETUP_WINDOW_HEIGHT),
            WindowKind::WavAskDialog => (ASK_WINDOW_WIDTH, ASK_WINDOW_HEIGHT),
            WindowKind::FileSelectDialog => {
                if self.fsel_dlg.is_file_browse() {
                    (316, 268)
                } else {
                    (300, 210)
                }
            }
            WindowKind::FileManager => (FILEMAN_WINDOW_WIDTH, FILEMAN_WINDOW_HEIGHT),
            WindowKind::HotkeyEditor => (HOTKEY_WINDOW_WIDTH, HOTKEY_WINDOW_HEIGHT),
            WindowKind::ArchiveSetup => (ARCHIVE_WINDOW_WIDTH, ARCHIVE_WINDOW_HEIGHT),
            WindowKind::About => (MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT),
            WindowKind::HelpViewer => (HELP_WINDOW_WIDTH, HELP_WINDOW_HEIGHT),
            WindowKind::FileMenu => (212, 68),
            WindowKind::ConfirmDialog => match self.confirm_action.unwrap_or(ConfirmAction::DeleteFromDisk) {
                ConfirmAction::DeleteFromDisk => (284, 124),
                ConfirmAction::RemoveFromPlaylist => (398, 124),
            },
        }
    }

    pub(crate) fn window_dimensions(&self, kind: WindowKind, state: &UiSharedState) -> (u32, u32, String) {
        let skin = if self.is_modern_skin(state) {
            SkinMode::Modern
        } else {
            state.skin_mode
        };
        let (w, h) = self.window_base_size(kind, skin, state);
        if skin == SkinMode::Modern {
            match kind {
                WindowKind::Main => {
                    if self.is_mini {
                        return (
                            w,
                            h,
                            self.locale.tr_or("win", "mini_title", "MOD4WIN Mini-Status").to_string(),
                        );
                    } else {
                        return (w, h, "MOD4WIN [3.0 Beta]".to_string());
                    }
                }
                WindowKind::ModuleInfo | WindowKind::InstrumentInfo => {
                    return (w, h, "[10 + 12] Module & Instrument Info".to_string());
                }
                WindowKind::Effects => {
                    let num_ch = state.num_channels.clamp(4, 32) as u32;
                    return (w, h, format!("[11] Effects Analyzer (Channels 1-{})", num_ch));
                }
                WindowKind::QuickSelect | WindowKind::OpenDialog => {
                    return (w, h, "Playlist Dock (Drag & Drop / .ZIP / .MOL)".to_string());
                }
                WindowKind::SetupDialog => {
                    return (w, h, "[13] MOD4WIN Setup".to_string());
                }
                WindowKind::About => {
                    return (w, h, "[5] About MOD4WIN 3.0 Beta".to_string());
                }
                _ => {}
            }
        }
        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    (
                        w,
                        h,
                        self.locale.tr_or("win", "mini_title", "MOD4WIN Mini-Status").to_string(),
                    )
                } else {
                    (
                        w,
                        h,
                        self.locale.tr_or("win", "main_title", "MOD4WIN [3.0 Beta]").to_string(),
                    )
                }
            }
            WindowKind::ModuleInfo => (
                w,
                h,
                self.locale.tr_or("win", "info_title", "MOD4WIN [3.0 Beta] Module Info").to_string(),
            ),
            WindowKind::InstrumentInfo => (
                w,
                h,
                self.locale.tr_or("win", "inst_title", "Instrument Info").to_string(),
            ),
            WindowKind::Effects => (
                w,
                h,
                self.locale.tr_or("win", "effects_title", "MOD4WIN Effects").to_string(),
            ),
            WindowKind::EffectsHelp => (
                w,
                h,
                self.locale.tr_or("win", "effects_help_title", "The Effects in MOD 4 WIN").to_string(),
            ),
            WindowKind::QuickSelect => (
                w,
                h,
                self.locale.tr_or("win", "quick_select_title", "MOD4WIN Quick Selection").to_string(),
            ),
            WindowKind::OpenDialog => (
                w,
                h,
                self.locale.tr_or("win", "open_title", "MOD 4 WIN Open Files").to_string(),
            ),
            WindowKind::SetupDialog => (
                w,
                h,
                self.locale.tr_or("win", "setup_title", "MOD4WIN [3.0 Beta] Setup").to_string(),
            ),
            WindowKind::WavAskDialog => (
                w,
                h,
                self.locale.tr_or("win", "wav_ask_title", "MOD4WIN Record Filename").to_string(),
            ),
            WindowKind::FileSelectDialog => (
                w,
                h,
                self.locale.tr_or("win", "fsel_title", "MOD4WIN Select Path / File").to_string(),
            ),
            WindowKind::FileManager => (
                w,
                h,
                if self.file_man_dlg.is_copy {
                    self.locale.tr_or("win", "file_copy_title", "MOD4WIN Copy File(s)").to_string()
                } else {
                    self.locale.tr_or("win", "file_move_title", "MOD4WIN Move/Rename File(s)").to_string()
                },
            ),
            WindowKind::HotkeyEditor => (
                w,
                h,
                self.locale.tr_or("win", "hotkey_title", "MOD4WIN Hotkey Setup").to_string(),
            ),
            WindowKind::ArchiveSetup => (
                w,
                h,
                self.locale.tr_or("win", "archive_title", "MOD4WIN Archiver Setup").to_string(),
            ),
            WindowKind::About => (
                w,
                h,
                self.locale.tr_or("win", "about_title", "MOD4WIN [3.0 Beta] About").to_string(),
            ),
            WindowKind::HelpViewer => (
                w,
                h,
                self.locale.tr_or("win", "help_title", "MOD 4 WIN Online Help").to_string(),
            ),
            WindowKind::FileMenu => (w, h, "MOD4WIN File".to_string()),
            WindowKind::ConfirmDialog => (w, h, "MOD4WIN".to_string()),
        }
    }

    /// Prüft, ob das angegebene Fenster tatsächlich geöffnet und sichtbar ist.
    pub(crate) fn is_window_open(&self, kind: WindowKind) -> bool {
        self.kind_to_id
            .get(&kind)
            .and_then(|id| self.windows.get(id))
            .map(|surf| surf.window.is_visible().unwrap_or(true))
            .unwrap_or(false)
    }

    /// Fordert ein sofortiges Neuzeichnen des Hauptfensters an (z. B. nach Öffnen/Schließen von Unterfenstern).
    pub(crate) fn request_main_redraw(&self) {
        if let Some(&main_id) = self.kind_to_id.get(&WindowKind::Main)
            && let Some(surf) = self.windows.get(&main_id) {
                surf.window.request_redraw();
            }
    }

    /// Entfernt den Playlist-Eintrag `orig_idx` und rückt den Auswahl-Cursor (`quick_select_cursor`)
    /// sowie `quick_select_scroll` sauber auf den nächsten sichtbaren Eintrag nach.
    pub(crate) fn remove_playlist_item_and_update_cursor(&mut self, orig_idx: usize) {
        self.quick_select_all_selected = false;
        if self.quick_select_sort == 0 {
            let (removed, new_len, is_modern) = {
                let mut st = self.shared_state.lock().unwrap();
                let rem = st.remove_playlist_entry(orig_idx);
                (rem, st.playlist.len(), self.is_modern_skin(&st))
            };
            if !removed {
                return;
            }
            if new_len == 0 {
                self.quick_select_cursor = 0;
                self.quick_select_scroll = 0;
            } else {
                let new_vis_pos = orig_idx.min(new_len - 1);
                self.quick_select_cursor = new_vis_pos;
                let max_vis = if is_modern {
                    self.modern_playlist_rows.clamp(5, 36)
                } else {
                    13
                };
                let max_scroll = new_len.saturating_sub(max_vis);
                if self.quick_select_scroll > max_scroll {
                    self.quick_select_scroll = max_scroll;
                }
                if new_vis_pos < self.quick_select_scroll {
                    self.quick_select_scroll = new_vis_pos;
                } else if new_vis_pos >= self.quick_select_scroll + max_vis {
                    self.quick_select_scroll = new_vis_pos + 1 - max_vis;
                }
            }
            return;
        }
        let old_items = {
            let st = self.shared_state.lock().unwrap();
            self.sorted_quick_select_items(&st)
        };
        let old_vis_pos = old_items
            .iter()
            .position(|(idx, _)| *idx == orig_idx)
            .unwrap_or(0);

        let removed = {
            let mut st = self.shared_state.lock().unwrap();
            st.remove_playlist_entry(orig_idx)
        };
        if !removed {
            return;
        }

        let (new_items, is_modern) = {
            let st = self.shared_state.lock().unwrap();
            (self.sorted_quick_select_items(&st), self.is_modern_skin(&st))
        };
        if new_items.is_empty() {
            self.quick_select_cursor = 0;
            self.quick_select_scroll = 0;
        } else {
            let new_vis_pos = old_vis_pos.min(new_items.len() - 1);
            self.quick_select_cursor = new_items[new_vis_pos].0;
            let max_vis = if is_modern {
                self.modern_playlist_rows.clamp(5, 36)
            } else {
                13
            };
            let max_scroll = new_items.len().saturating_sub(max_vis);
            if self.quick_select_scroll > max_scroll {
                self.quick_select_scroll = max_scroll;
            }
            if new_vis_pos < self.quick_select_scroll {
                self.quick_select_scroll = new_vis_pos;
            } else if new_vis_pos >= self.quick_select_scroll + max_vis {
                self.quick_select_scroll = new_vis_pos + 1 - max_vis;
            }
        }
    }

    pub(crate) fn open_window(&mut self, event_loop: &ActiveEventLoop, kind: WindowKind) {
        if let Some(&existing_id) = self.kind_to_id.get(&kind)
            && let Some(surf) = self.windows.get(&existing_id) {
                surf.window.focus_window();
                surf.window.request_redraw();
                return;
            }

        let state = self.shared_state.lock().unwrap().clone();
        if kind == WindowKind::OpenDialog {
            if self.is_modern_skin(&state) {
                self.quick_select_all_selected = false;
                self.playlist_scroll_dragging = false;
                let max_vis = self.modern_playlist_rows.clamp(5, 36);
                let target_idx = state.playlist_idx.min(state.playlist.len().saturating_sub(1));
                self.quick_select_cursor = target_idx;
                let pos = if self.quick_select_sort == 0 {
                    Some(target_idx)
                } else {
                    let items = self.sorted_quick_select_items(&state);
                    items.iter().position(|(orig_idx, _)| *orig_idx == target_idx)
                };
                if let Some(p) = pos {
                    if p >= max_vis {
                        self.quick_select_scroll = p.saturating_sub(max_vis / 2);
                    } else {
                        self.quick_select_scroll = 0;
                    }
                } else {
                    self.quick_select_scroll = 0;
                }
            } else {
                if let Some(cur) = &state.current_file_path
                    && let Some(parent) = cur.parent()
                        && parent.is_dir()
                            && matches!(self.open_dlg.context, OpenBrowseContext::Directory(_))
                        {
                            self.open_dlg.context =
                                OpenBrowseContext::Directory(parent.to_path_buf());
                        }
                self.open_dlg.sync_from_playlist(&state.playlist);
                self.open_dlg.refresh_browser();
            }
        } else if kind == WindowKind::QuickSelect {
            self.quick_select_all_selected = false;
            self.playlist_scroll_dragging = false;
            let max_vis = if self.is_modern_skin(&state) {
                self.modern_playlist_rows.clamp(5, 36)
            } else {
                13
            };
            let target_idx = state.playlist_idx.min(state.playlist.len().saturating_sub(1));
            self.quick_select_cursor = target_idx;
            let pos = if self.quick_select_sort == 0 {
                Some(target_idx)
            } else {
                let items = self.sorted_quick_select_items(&state);
                items.iter().position(|(orig_idx, _)| *orig_idx == target_idx)
            };
            if let Some(p) = pos {
                if p >= max_vis {
                    self.quick_select_scroll = p.saturating_sub(max_vis / 2);
                } else {
                    self.quick_select_scroll = 0;
                }
            } else {
                self.quick_select_scroll = 0;
            }
        } else if kind == WindowKind::SetupDialog {
            let mut st = self.shared_state.lock().unwrap();
            st.sync_runtime_to_ini();
            self.setup_cfg = st.ini_config.clone();
            self.setup_backup_cfg = st.ini_config.clone();
            self.setup_edit_focus = SetupEditFocus::None;
            if st.ini_config.pause_while_setup && st.is_playing && !st.is_paused {
                st.is_paused = true;
                st.is_playing = false;
                self.setup_paused_playback = true;
            } else {
                self.setup_paused_playback = false;
            }
        } else if kind == WindowKind::FileSelectDialog {
            if self.fsel_dlg.entries.is_empty() {
                self.fsel_dlg.refresh();
            }
        } else if kind == WindowKind::FileManager {
            self.file_man_dlg = FileManagerState::new(self.file_man_dlg.is_copy, &state);
        } else if kind == WindowKind::HotkeyEditor {
            let hk = if self.kind_to_id.contains_key(&WindowKind::SetupDialog) {
                self.setup_cfg.hotkeys.clone()
            } else {
                state.ini_config.hotkeys.clone()
            };
            self.hotkey_dlg = HotkeyEditorState {
                working_keys: if hk.len() == 43 {
                    hk
                } else {
                    default_hotkey_bindings()
                },
                scroll: 0,
                cursor: 0,
                capturing: false,
            };
        } else if kind == WindowKind::ArchiveSetup {
            if !self.kind_to_id.contains_key(&WindowKind::SetupDialog) {
                self.setup_cfg = state.ini_config.clone();
                self.setup_backup_cfg = state.ini_config.clone();
            }
            self.archive_dlg = ArchiveSetupState::default();
        } else if kind == WindowKind::About {
            self.about_opened_at = Instant::now();
        }
        let (w, h, title) = self.window_dimensions(kind, &state);
        let scale = if cfg!(any(feature = "retro-win32", target_arch = "x86")) {
            1.0
        } else {
            self.scale_factor.clamp(0.5, 4.0)
        };
        let phys_w = ((w as f64) * scale).round().max(1.0) as u32;
        let phys_h = ((h as f64) * scale).round().max(1.0) as u32;

        let win_key = self.window_ini_key(kind);
        let main_pos = self
            .kind_to_id
            .get(&WindowKind::Main)
            .and_then(|id| self.windows.get(id))
            .and_then(|s| s.window.outer_position().ok().map(|p| (p.x, p.y)));

        let saved_pos = match kind {
            WindowKind::FileMenu => main_pos.map(|(mx, my)| {
                (
                    mx + (401.0 * scale).round() as i32,
                    my + (126.0 * scale).round() as i32,
                )
            }),
            WindowKind::ConfirmDialog => main_pos.map(|(mx, my)| {
                (
                    mx + (((542i32 - w as i32) as f64 / 2.0) * scale).round() as i32,
                    my + (6.0 * scale).round() as i32,
                )
            }),
            _ => state
                .ini_config
                .window_positions
                .get(win_key)
                .copied()
                .or(match kind {
                    WindowKind::Main if self.is_mini => {
                        Some((state.ini_config.x_mini, state.ini_config.y_mini))
                    }
                    WindowKind::Main => Some((state.ini_config.x_main, state.ini_config.y_main)),
                    _ => None,
                }),
        };

        let is_resizable = !cfg!(any(feature = "retro-win32", target_arch = "x86"));
        let mut window_attrs = Window::default_attributes()
            .with_title(title)
            .with_decorations(false)
            .with_resizable(is_resizable)
            .with_min_inner_size(super::winit::dpi::LogicalSize::new(
                (w as f64) * 0.25,
                (h as f64) * 0.25,
            ))
            .with_inner_size(super::winit::dpi::LogicalSize::new(
                phys_w as f64,
                phys_h as f64,
            ));
        #[cfg(not(feature = "retro-win32"))]
        {
            window_attrs = window_attrs.with_window_icon(load_mod4win_window_icon());
        }
        #[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
        if kind != WindowKind::Main {
            use super::winit::platform::windows::WindowAttributesExtWindows;
            window_attrs = window_attrs.with_skip_taskbar(true);
        }
        #[cfg(feature = "retro-win32")]
        if kind != WindowKind::Main {
            window_attrs = window_attrs.with_skip_taskbar(true);
        }
        if let Some((px, py)) = saved_pos {
            window_attrs =
                window_attrs.with_position(super::winit::dpi::PhysicalPosition::new(px.max(0), py.max(0)));
        }

        let window = match event_loop.create_window(window_attrs) {
            Ok(win) => Arc::new(win),
            Err(e) => {
                eprintln!("UI: Fehler beim Erstellen von {:?}: {}", kind, e);
                return;
            }
        };
        if kind != WindowKind::Main {
            hide_win32_window_from_taskbar(&window);
        }

        // Sicherstellen, dass die physische Innengröße exakt phys_w x phys_h beträgt (1.00 = 1:1 physische Pixel!)
        let _ = window.request_inner_size(PhysicalSize::new(phys_w, phys_h));
        force_win32_window_size(&window, phys_w, phys_h);
        let actual_size = window.inner_size();
        let surf_w = if actual_size.width > 0 { actual_size.width } else { phys_w };
        let surf_h = if actual_size.height > 0 { actual_size.height } else { phys_h };

        let surface_texture = SurfaceTexture::new(
            surf_w.max(1),
            surf_h.max(1),
            Arc::clone(&window),
        );

        #[cfg(target_os = "windows")]
        let wgpu_backends = super::pixels::wgpu::Backends::DX12;
        #[cfg(not(target_os = "windows"))]
        let wgpu_backends = super::pixels::wgpu::Backends::all();

        let pixels_res = super::pixels::PixelsBuilder::new(w, h, surface_texture)
            .wgpu_backend(wgpu_backends)
            .request_adapter_options(super::pixels::wgpu::RequestAdapterOptions {
                power_preference: super::pixels::wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .enable_vsync(false)
            .present_mode(super::pixels::wgpu::PresentMode::AutoNoVsync)
            .build();

        match pixels_res {
            Ok(mut pixels) => {
                pixels.set_scaling_mode(ScalingMode::Fill);
                let id = window.id();
                self.kind_to_id.insert(kind, id);

                // Unmittelbar den ersten Frame rendern und anzeigen, damit das Fenster ohne schwarzen Blitz erscheint!
                let mut first_buf = vec![0u8; (w * h * 4) as usize];
                self.render_window(kind, &mut first_buf, w, h, &state);
                {
                    let frame = pixels.frame_mut();
                    let copy_len = frame.len().min(first_buf.len());
                    frame[..copy_len].copy_from_slice(&first_buf[..copy_len]);
                }
                let _ = pixels.render();

                let init_outer_pos = window
                    .outer_position()
                    .ok()
                    .map(|p| (p.x, p.y))
                    .or(saved_pos);
                self.windows.insert(
                    id,
                    WindowSurface {
                        kind,
                        window,
                        pixels,
                        width: w,
                        height: h,
                        mouse_pos: (0.0, 0.0),
                        last_click_time: Instant::now(),
                        active_resize_dir: None,
                        needs_aspect_snap: false,
                        last_outer_pos: init_outer_pos,
                    },
                );
                if kind != WindowKind::Main {
                    let open_keys = self.collect_open_window_keys();
                    self.setup_cfg.saved_open_windows = open_keys.clone();
                    if let Ok(mut st) = self.shared_state.lock() {
                        st.ini_config.saved_open_windows = open_keys;
                    }
                }
                self.request_main_redraw();
            }
            Err(e) => {
                eprintln!("UI: Fehler beim Initialisieren von Pixels für {:?}: {}", kind, e);
            }
        }
    }

    /// Speichert die aktuelle Bildschirmposition eines Fensters in `ini_config` und `setup_cfg`.
    pub(crate) fn record_window_position(&mut self, kind: WindowKind, x: i32, y: i32) {
        let cx = x.max(0);
        let cy = y.max(0);
        let key = self.window_ini_key(kind).to_string();
        self.setup_cfg.window_positions.insert(key.clone(), (cx, cy));
        if kind == WindowKind::Main {
            if self.is_mini {
                self.setup_cfg.x_mini = cx;
                self.setup_cfg.y_mini = cy;
            } else {
                self.setup_cfg.x_main = cx;
                self.setup_cfg.y_main = cy;
            }
        }
        if let Ok(mut st) = self.shared_state.lock() {
            st.ini_config.window_positions.insert(key, (cx, cy));
            if kind == WindowKind::Main {
                if self.is_mini {
                    st.ini_config.x_mini = cx;
                    st.ini_config.y_mini = cy;
                } else {
                    st.ini_config.x_main = cx;
                    st.ini_config.y_main = cy;
                }
            }
        }
    }

    pub(crate) fn close_window(&mut self, kind: WindowKind) {
        if kind == WindowKind::FileMenu {
            self.file_menu_open = false;
            self.file_menu_hover = None;
        } else if kind == WindowKind::ConfirmDialog {
            self.confirm_action = None;
        }
        let closed_pos = self.kind_to_id.remove(&kind).and_then(|id| {
            self.windows
                .remove(&id)
                .and_then(|surf| surf.window.outer_position().ok().map(|p| (p.x, p.y)))
        });
        if let Some((px, py)) = closed_pos
            && !matches!(kind, WindowKind::FileMenu | WindowKind::ConfirmDialog) {
                self.record_window_position(kind, px, py);
            }
        let clamped_scale = self.scale_factor.clamp(0.5, 4.0);
        self.setup_cfg.gui_scale = clamped_scale;
        let open_keys = self.collect_open_window_keys();
        if kind != WindowKind::Main {
            self.setup_cfg.saved_open_windows = open_keys.clone();
        }
        if let Ok(mut state) = self.shared_state.lock() {
            match kind {
                WindowKind::ModuleInfo => state.show_info = false,
                WindowKind::Effects => state.show_effects = false,
                WindowKind::SetupDialog => {
                    self.setup_edit_focus = SetupEditFocus::None;
                    if self.setup_paused_playback {
                        state.is_paused = false;
                        state.is_playing = true;
                        self.setup_paused_playback = false;
                    }
                }
                _ => {}
            }
            state.ini_config.gui_scale = clamped_scale;
            state.ini_config.modern_playlist_rows = self.modern_playlist_rows.clamp(5, 36) as u8;
            if kind != WindowKind::Main {
                state.ini_config.saved_open_windows = open_keys;
            }
            let _ = state
                .ini_config
                .save_to_file(&Mod4WinIniConfig::default_ini_path());
        }
        self.request_main_redraw();
    }

    pub(crate) fn toggle_window(&mut self, event_loop: &ActiveEventLoop, kind: WindowKind) {
        if self.kind_to_id.contains_key(&kind) {
            self.close_window(kind);
        } else {
            if let Ok(mut state) = self.shared_state.lock() {
                match kind {
                    WindowKind::ModuleInfo => state.show_info = true,
                    WindowKind::Effects => state.show_effects = true,
                    _ => {}
                }
            }
            self.open_window(event_loop, kind);
        }
    }

    pub(crate) fn switch_mini_mode(&mut self, event_loop: &ActiveEventLoop, mini: bool) {
        if self.is_mini == mini {
            return;
        }
        self.close_window(WindowKind::Main);
        self.is_mini = mini;
        if let Ok(mut st) = self.shared_state.lock() {
            st.ini_config.mini_state = mini;
        }
        self.open_window(event_loop, WindowKind::Main);
    }

    pub(crate) fn spawn_external_mixer(&self) {
        #[cfg(target_os = "windows")]
        {
            let mixer_cmd = {
                let st = self.shared_state.lock().unwrap();
                let m = st.ini_config.mixer_path.trim();
                if m.is_empty() {
                    "sndvol.exe".to_string()
                } else {
                    m.to_string()
                }
            };
            if let Err(e) = std::process::Command::new(&mixer_cmd).spawn() {
                eprintln!(
                    "UI: Konnte externen Mixer ({}) nicht starten: {}",
                    mixer_cmd, e
                );
            }
        }
        #[cfg(target_os = "linux")]
        {
            let mixer_cmd = {
                let st = self.shared_state.lock().unwrap();
                let m = st.ini_config.mixer_path.trim();
                if m.is_empty() || m.eq_ignore_ascii_case("sndvol.exe") {
                    "pavucontrol".to_string()
                } else {
                    m.to_string()
                }
            };
            if std::process::Command::new(&mixer_cmd).spawn().is_err() {
                let _ = std::process::Command::new("alsamixer").spawn();
            }
        }
    }

    /// Speichert beim Beenden den aktuellen Zustand in `MOD4WIN.INI`.
    pub(crate) fn save_ini_on_exit(&mut self) {
        let open_positions: Vec<(WindowKind, i32, i32)> = self
            .windows
            .values()
            .filter_map(|surf| {
                surf.window
                    .outer_position()
                    .ok()
                    .map(|p| (surf.kind, p.x, p.y))
            })
            .collect();
        for (k, px, py) in open_positions {
            self.record_window_position(k, px, py);
        }
        let open_keys = self.collect_open_window_keys();
        let clamped_rows = self.modern_playlist_rows.clamp(5, 36) as u8;
        self.setup_cfg.saved_open_windows = open_keys.clone();
        self.setup_cfg.modern_playlist_rows = clamped_rows;
        if let Ok(mut st) = self.shared_state.lock() {
            st.ini_config.mini_state = self.is_mini;
            st.ini_config.gui_scale = self.scale_factor.clamp(0.5, 4.0);
            st.ini_config.modern_playlist_rows = clamped_rows;
            st.ini_config.saved_open_windows = open_keys;
            st.ini_config.last_dir = self
                .open_dlg
                .current_dir_path()
                .to_string_lossy()
                .into_owned();
            st.sync_runtime_to_ini();
            let ini_path = Mod4WinIniConfig::default_ini_path();
            let _ = st.ini_config.save_to_file(&ini_path);
        }
    }

    /// Prüft anhand von `MoveOnClick` im Setup, ob ein Fenster auch außerhalb der Titelleiste gezogen werden darf.
    pub(crate) fn drag_window_if_allowed(&self, vy: i32, window: &Arc<Window>) {
        let allow_anywhere = self
            .shared_state
            .lock()
            .map(|st| st.ini_config.move_on_click || st.ini_config.full_window_move)
            .unwrap_or(true);
        if vy <= 18 || allow_anywhere {
            let _ = window.drag_window();
        }
    }

    /// Führt den Direct-to-Disk `.WAV`-Export (`DEV_HDD`) direkt mit den aktuellen UI/Setup-Einstellungen aus.
    pub(crate) fn execute_wav_export(&mut self, wav_out_path: PathBuf) {
        if self.kind_to_id.contains_key(&WindowKind::SetupDialog) {
            self.sync_live_setup_to_state();
        }
        let (mod_path_opt, mut cfg_copy) = {
            let mut st = self.shared_state.lock().unwrap();
            st.sync_runtime_to_ini();
            st.is_playing = false;
            st.is_paused = false;
            st.is_exporting_wav = true;
            st.pending_export_wav = None;
            (st.current_file_path.clone(), st.ini_config.clone())
        };

        if let Some(mod_path) = mod_path_opt
            && let Ok(file_data) = std::fs::read(&mod_path) {
                let ext = mod_path.extension().and_then(|e| e.to_str());
                if let Ok(export_mod) = load_module_with_ext(&file_data, ext) {
                    match export_module_to_wav_file(
                        export_mod,
                        file_data.len() as u32,
                        &mut cfg_copy,
                        &wav_out_path,
                    ) {
                        Ok(frames) => {
                            let fname = wav_out_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("OUT.WAV");
                            println!(
                                "[DEV_HDD] Direct-to-Disk WAV-Export abgeschlossen: {} ({} Frames, OverSamp={}, SR={}, Bits={}, Ch={}, Surr={}, Pan={})",
                                wav_out_path.display(),
                                frames,
                                cfg_copy.over_sampling,
                                cfg_copy.sample_rate,
                                cfg_copy.bits_per_sample,
                                cfg_copy.channels,
                                cfg_copy.surround,
                                cfg_copy.enable_panning
                            );
                            let mut st = self.shared_state.lock().unwrap();
                            st.ini_config.peaks = cfg_copy.peaks.clone();
                            st.title = format!("SAVED {}", fname);
                            st.export_status_msg = Some(format!("WAV Saved: {}", fname));
                            st.is_exporting_wav = false;
                            return;
                        }
                        Err(e) => {
                            eprintln!("[DEV_HDD] Fehler beim WAV-Export: {}", e);
                            let mut st = self.shared_state.lock().unwrap();
                            st.title = "WAV EXPORT ERROR".to_string();
                            st.is_exporting_wav = false;
                            return;
                        }
                    }
                }
            }
        let mut st = self.shared_state.lock().unwrap();
        st.is_exporting_wav = false;
    }

    /// Startet entweder die normale Soundkarten-Wiedergabe oder (bei `DEV_HDD`) den Direct-to-Disk WAV-Export (`TAskWin`).
    pub(crate) fn trigger_play_or_hdd_record(&mut self, event_loop: &ActiveEventLoop) {
        if self.kind_to_id.contains_key(&WindowKind::SetupDialog) {
            self.sync_live_setup_to_state();
        }
        let (is_hdd, ask_hd, hd_path, mod_stem, has_mod) = {
            let mut st = self.shared_state.lock().unwrap();
            st.sync_runtime_to_ini();
            if st.current_file_path.is_none() && !st.playlist.is_empty() {
                let idx = st.playlist_idx.min(st.playlist.len() - 1);
                let p = st.playlist[idx].clone();
                st.current_file_path = Some(p.clone());
                if st.ini_config.device_id != DEV_HDD {
                    st.pending_load_file = Some(p);
                }
            }
            let has_mod = st.current_file_path.is_some();
            let stem = st
                .current_file_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("OUTPUT")
                .chars()
                .take(8)
                .collect::<String>()
                .to_uppercase();
            (
                st.ini_config.device_id == DEV_HDD,
                st.ini_config.ask_hd,
                st.ini_config.hd_path.clone(),
                stem,
                has_mod,
            )
        };

        if !has_mod {
            return;
        }

        if is_hdd {
            if ask_hd {
                self.wav_ask_dlg = WavAskState {
                    path: hd_path,
                    filename: mod_stem,
                    focus: WavAskEditFocus::Filename,
                };
                self.open_window(event_loop, WindowKind::WavAskDialog);
            } else {
                let out_path = PathBuf::from(hd_path).join(format!("{}.WAV", mod_stem));
                self.execute_wav_export(out_path);
            }
        } else {
            let mut st = self.shared_state.lock().unwrap();
            if !st.is_playing || st.is_paused {
                let nb = st.ini_config.output_buffers.clamp(2, 99);
                st.cpu_telemetry.reset_buffer(nb);
            }
            st.is_playing = true;
            st.is_paused = false;
        }
    }

    // =========================================================================
    // Rendering der einzelnen Fenster
    // =========================================================================

    pub(crate) fn render_window(&self, kind: WindowKind, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        let is_modern = self.is_modern_skin(state);
        if is_modern {
            match kind {
                WindowKind::Main => {
                    if self.is_mini {
                        self.render_modern_mini_window(frame, dw, dh, state);
                    } else {
                        self.render_modern_main_window(frame, dw, dh, state);
                    }
                    return;
                }
                WindowKind::ModuleInfo | WindowKind::InstrumentInfo => {
                    self.render_modern_module_info_window(frame, dw, dh, state);
                    return;
                }
                WindowKind::Effects => {
                    self.render_modern_effects_window(frame, dw, dh, state);
                    return;
                }
                WindowKind::QuickSelect | WindowKind::OpenDialog => {
                    self.render_modern_playlist_dock(frame, dw, dh, state);
                    return;
                }
                WindowKind::SetupDialog => {
                    self.render_modern_setup_window(frame, dw, dh, state);
                    return;
                }
                WindowKind::About => {
                    self.render_modern_about_window(frame, dw, dh, state);
                    return;
                }
                _ => {}
            }
        }

        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    self.render_mini_window(frame, dw, dh, state);
                } else {
                    self.render_main_window(frame, dw, dh, state);
                }
            }
            WindowKind::ModuleInfo => self.render_module_info_window(frame, dw, dh, state),
            WindowKind::InstrumentInfo => self.render_instrument_window(frame, dw, dh, state),
            WindowKind::Effects => self.render_effects_window(frame, dw, dh, state),
            WindowKind::EffectsHelp => {
                self.assets.bm13_effects_help.blit_rect_to(0, 0, dw, dh, frame, dw, dh, 0, 0);
            }
            WindowKind::QuickSelect => self.render_quick_select_window(frame, dw, dh, state),
            WindowKind::OpenDialog => self.render_open_dialog_window(frame, dw, dh, state),
            WindowKind::SetupDialog => self.render_setup_dialog_window(frame, dw, dh, state),
            WindowKind::WavAskDialog => self.render_wav_ask_window(frame, dw, dh, state),
            WindowKind::FileSelectDialog => self.render_file_select_window(frame, dw, dh, state),
            WindowKind::FileManager => self.render_file_manager_window(frame, dw, dh, state),
            WindowKind::HotkeyEditor => self.render_hotkey_editor_window(frame, dw, dh, state),
            WindowKind::ArchiveSetup => self.render_archive_setup_window(frame, dw, dh, state),
            WindowKind::About => self.render_about_window(frame, dw, dh, state),
            WindowKind::HelpViewer => {
                self.help_dlg
                    .render(frame, dw, dh, &self.assets, &self.locale);
            }
            WindowKind::FileMenu => self.render_file_popup_menu_window(frame, dw, dh),
            WindowKind::ConfirmDialog => self.render_confirm_dialog_window(frame, dw, dh, state),
        }
    }

    // =========================================================================
    // Maus-Interaktionen pro Fenster
    // =========================================================================

    pub(crate) fn handle_mouse_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        kind: WindowKind,
        vx: i32,
        vy: i32,
        button: MouseButton,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if !matches!(kind, WindowKind::HelpViewer | WindowKind::FileMenu | WindowKind::ConfirmDialog) {
            self.active_window_kind = kind;
        }
        if kind != WindowKind::FileMenu && kind != WindowKind::Main && self.file_menu_open {
            self.file_menu_open = false;
            self.file_menu_hover = None;
            self.close_window(WindowKind::FileMenu);
        }
        if kind != WindowKind::Main && self.sys_menu_open {
            self.sys_menu_open = false;
            self.sys_menu_hover = None;
        }

        let is_modern = {
            let st = self.shared_state.lock().unwrap();
            self.is_modern_skin(&st)
        };
        if is_modern {
            match kind {
                WindowKind::Main => {
                    if self.is_mini {
                        if button == MouseButton::Right {
                            self.toggle_window(event_loop, WindowKind::Effects);
                        } else if button == MouseButton::Left {
                            self.handle_modern_mini_click(
                                event_loop,
                                vx,
                                vy,
                                window,
                                is_double_click,
                            );
                        }
                    } else if button == MouseButton::Left || button == MouseButton::Right {
                        self.handle_modern_main_click(event_loop, vx, vy, button, window);
                    }
                    return;
                }
                WindowKind::ModuleInfo | WindowKind::InstrumentInfo => {
                    if button == MouseButton::Left {
                        self.handle_modern_module_info_click(event_loop, vx, vy, window);
                    }
                    return;
                }
                WindowKind::Effects => {
                    if button == MouseButton::Left || button == MouseButton::Right {
                        self.handle_modern_effects_click(event_loop, vx, vy, button, window);
                    }
                    return;
                }
                WindowKind::QuickSelect | WindowKind::OpenDialog => {
                    if button == MouseButton::Left {
                        self.handle_modern_playlist_dock_click(
                            event_loop,
                            vx,
                            vy,
                            window,
                            is_double_click,
                        );
                    }
                    return;
                }
                WindowKind::SetupDialog => {
                    if matches!(button, MouseButton::Left | MouseButton::Right) {
                        self.handle_modern_setup_click(event_loop, vx, vy, button, window);
                    }
                    return;
                }
                WindowKind::About => {
                    if button == MouseButton::Left {
                        if vy < 24 && vx < (MODERN_ABOUT_W as i32 - 26) {
                            let _ = window.drag_window();
                        } else {
                            self.close_window(WindowKind::About);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        if kind == WindowKind::HelpViewer && button == MouseButton::Right {
            if self.help_dlg.popup_topic_id.is_some() {
                self.help_dlg.popup_topic_id = None;
            } else {
                self.close_window(WindowKind::HelpViewer);
            }
            return;
        }

        // Rechtsklick im Effekt-Fenster (außer auf Kanalnummern / Header) schließt es
        if kind == WindowKind::Effects && button == MouseButton::Right {
            let num_ch = self.shared_state.lock().unwrap().num_channels.clamp(4, 32);
            let ch_end_y = 80 + (num_ch as i32) * 9;
            if (8..=23).contains(&vx) && (80..ch_end_y).contains(&vy) {
                let ch_idx = ((vy - 80) / 9) as usize;
                let mut state = self.shared_state.lock().unwrap();
                // Play Solo: Alle anderen stummschalten, gewählten Kanal aktivieren
                for m in state.channel_muted.iter_mut() {
                    *m = true;
                }
                if ch_idx < 32 {
                    state.channel_muted[ch_idx] = false;
                }
                state.channel_muted_dirty = true;
                return;
            } else if (8..=23).contains(&vx) && (65..=75).contains(&vy) {
                // Rechtsklick auf "Chn" Header: Alle stummschalten
                let mut state = self.shared_state.lock().unwrap();
                state.channel_muted = [true; 32];
                state.channel_muted_dirty = true;
                return;
            } else {
                self.close_window(WindowKind::Effects);
                return;
            }
        }

        if kind == WindowKind::EffectsHelp && button == MouseButton::Right {
            self.close_window(WindowKind::EffectsHelp);
            return;
        }

        // Mini-Status Rechtsklick im Display schaltet Effekte um (in beiden Modi identische Koordinaten!)
        if kind == WindowKind::Main && self.is_mini && button == MouseButton::Right {
            if (4..=46).contains(&vx) && (22..=54).contains(&vy) {
                self.toggle_window(event_loop, WindowKind::Effects);
            }
            return;
        }

        // Rechtsklick im Hauptfenster (`wmRButtonDown` in `mod4win.pas` Zeilen 881-940):
        // Track/Tracks -> -1 Song, Pattern/Patterns -> -1 Pattern, PreAmp -> Reset auf 0.0 dB
        if kind == WindowKind::Main && !self.is_mini && button == MouseButton::Right {
            // Rechtsklick auf den PreAmp/Boost-Schieberegler (507..534, 24..115) setzt PreAmp sofort auf 0.0 dB (raw = 20) zurück!
            if (507..=534).contains(&vx) && (24..=115).contains(&vy) {
                self.reset_preamp_to_zero_db();
                return;
            }
            if (56..=93).contains(&vy) {
                if (90..=215).contains(&vx) {
                    self.prev_song();
                    return;
                } else if (220..=314).contains(&vx) {
                    let mut state = self.shared_state.lock().unwrap();
                    state.pending_seek_delta -= 1;
                    return;
                }
            }
            if (9..=331).contains(&vx) && (27..=50).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.show_filename = !state.show_filename;
                return;
            }
            if (315..=360).contains(&vx) && (71..=81).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.repeat_mode = (state.repeat_mode + 2) % 3;
                return;
            }
            if (472..=500).contains(&vx) && (47..=65).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.step_oversamp(-1);
                self.setup_cfg.over_sampling = state.ini_config.over_sampling;
                return;
            }
            if (375..=407).contains(&vx) && (28..=43).contains(&vy) {
                const STD_RATES: [i32; 4] = [11025, 22050, 44100, 48000];
                let mut state = self.shared_state.lock().unwrap();
                let cur_khz = (state.sound_settings.samplerate + 500) / 1000;
                let cur_idx = STD_RATES
                    .iter()
                    .position(|&r| (r + 500) / 1000 >= cur_khz)
                    .unwrap_or(2);
                let prev_sr = STD_RATES[(cur_idx + STD_RATES.len() - 1) % STD_RATES.len()];
                state.sound_settings.samplerate = prev_sr;
                state.settings_dirty = true;
                state.sync_runtime_to_ini();
                self.setup_cfg.sample_rate = state.ini_config.sample_rate;
                return;
            }
            if (408..=444).contains(&vx) && (28..=43).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.sound_settings.bits = if state.sound_settings.bits == 8 { 16 } else { 8 };
                state.settings_dirty = true;
                state.sync_runtime_to_ini();
                self.setup_cfg.bits_per_sample = state.ini_config.bits_per_sample;
                return;
            }
            if (444..=470).contains(&vx) && (33..=45).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.step_clip_declick(-1);
                self.setup_cfg.clipping = state.ini_config.clipping;
                self.setup_cfg.declick = state.ini_config.declick;
                return;
            }
            // Rechtsklick auf CPU-Prozentanzeige (`Main_Percnt`) oder CPU-Balken (`PBar`) schaltet die Retro-CPU-Profile rückwärts durch
            if ((368..=374).contains(&vx) && (29..=82).contains(&vy))
                || ((370..=407).contains(&vx) && (62..=82).contains(&vy))
            {
                let mut state = self.shared_state.lock().unwrap();
                let prev_prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile).prev();
                state.ini_config.cpu_sim_profile = prev_prof.to_u8();
                self.setup_cfg.cpu_sim_profile = prev_prof.to_u8();
                let banner =
                    prev_prof.lcd_banner_with_stutter(state.ini_config.authentic_stutter);
                state.set_status_banner(banner);
                let _ = state.ini_config.save_to_file(&Mod4WinIniConfig::default_ini_path());
                return;
            }
            // Rechtsklick auf Pufferzeit-Anzeige (`Main_BuffTime`) oder Pufferbalken (`BBar`) toggelt Authentic Stutter
            if ((446..=452).contains(&vx) && (29..=82).contains(&vy))
                || ((408..=444).contains(&vx) && (62..=82).contains(&vy))
            {
                let mut state = self.shared_state.lock().unwrap();
                state.ini_config.authentic_stutter = !state.ini_config.authentic_stutter;
                self.setup_cfg.authentic_stutter = state.ini_config.authentic_stutter;
                let prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile);
                let banner = if state.ini_config.authentic_stutter {
                    format!("STUT:ON {}", prof.short_code())
                } else {
                    format!("STUT:OFF {}", prof.short_code())
                };
                state.set_status_banner(banner);
                let _ = state.ini_config.save_to_file(&Mod4WinIniConfig::default_ini_path());
                return;
            }
            return;
        }

        if button != MouseButton::Left {
            return;
        }

        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    self.handle_mini_click(event_loop, vx, vy, window, is_double_click);
                } else {
                    self.handle_main_click(event_loop, vx, vy, window, is_double_click);
                }
            }
            WindowKind::ModuleInfo => self.handle_module_info_click(event_loop, vx, vy, window),
            WindowKind::InstrumentInfo => self.handle_instrument_click(vx, vy, window),
            WindowKind::Effects => self.handle_effects_click(event_loop, vx, vy, window),
            WindowKind::EffectsHelp => {
                if vy <= 18 && vx >= (EFF_WINDOW_WIDTH as i32 - 20) {
                    self.close_window(WindowKind::EffectsHelp);
                } else {
                    let _ = window.drag_window();
                }
            }
            WindowKind::QuickSelect => {
                self.handle_quick_select_click(vx, vy, window, is_double_click);
            }
            WindowKind::OpenDialog => {
                self.handle_open_dialog_click(event_loop, vx, vy, window, is_double_click);
            }
            WindowKind::SetupDialog => self.handle_setup_dialog_click(event_loop, vx, vy, window),
            WindowKind::WavAskDialog => self.handle_wav_ask_click(event_loop, vx, vy, window),
            WindowKind::FileSelectDialog => {
                self.handle_file_select_click(event_loop, vx, vy, window, is_double_click);
            }
            WindowKind::FileManager => {
                self.handle_file_manager_click(event_loop, vx, vy, window, is_double_click);
            }
            WindowKind::HotkeyEditor => {
                self.handle_hotkey_editor_click(event_loop, vx, vy, window, is_double_click);
            }
            WindowKind::ArchiveSetup => {
                self.handle_archive_setup_click(event_loop, vx, vy, window);
            }
            WindowKind::About => {
                if vy <= 18 && vx > 20 && vx < (MAIN_WINDOW_WIDTH as i32 - 20) {
                    let _ = window.drag_window();
                } else {
                    self.close_window(WindowKind::About);
                }
            }
            WindowKind::HelpViewer => {
                let prev_lang = self.help_dlg.book.language_id;
                match self.help_dlg.handle_click(vx, vy, &self.assets) {
                    HelpClickResult::CloseWindow => self.close_window(WindowKind::HelpViewer),
                    HelpClickResult::DragWindow => {
                        let _ = window.drag_window();
                    }
                    HelpClickResult::Handled => {}
                }
                let new_lang = self.help_dlg.book.language_id;
                if new_lang != prev_lang {
                    self.setup_cfg.language = new_lang;
                    self.locale = LocaleCatalog::load_for_language_id(new_lang);
                    if let Ok(mut st) = self.shared_state.lock() {
                        st.ini_config.language = new_lang;
                        let _ = st
                            .ini_config
                            .save_to_file(&Mod4WinIniConfig::default_ini_path());
                    }
                    self.refresh_all_windows_for_skin();
                }
            }
            WindowKind::FileMenu => {
                self.handle_file_menu_click(event_loop, vx, vy);
            }
            WindowKind::ConfirmDialog => {
                self.handle_confirm_dialog_click(vx, vy, window);
            }
        }
    }
}

impl ApplicationHandler for Mod4WinApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.kind_to_id.contains_key(&WindowKind::Main) {
            let (restore_windows, is_modern) = {
                let st = self.shared_state.lock().unwrap();
                let list = if st.ini_config.state_saver {
                    st.ini_config.saved_open_windows.clone()
                } else {
                    Vec::new()
                };
                (list, self.is_modern_skin(&st))
            };

            self.open_window(event_loop, WindowKind::Main);

            for key in restore_windows {
                let Some(mut kind) = Self::window_kind_from_ini_key(&key) else {
                    continue;
                };
                if is_modern {
                    if kind == WindowKind::InstrumentInfo {
                        kind = WindowKind::ModuleInfo;
                    } else if kind == WindowKind::OpenDialog {
                        kind = WindowKind::QuickSelect;
                    }
                }
                if self.kind_to_id.contains_key(&kind) {
                    continue;
                }
                if let Ok(mut st) = self.shared_state.lock() {
                    match kind {
                        WindowKind::ModuleInfo => st.show_info = true,
                        WindowKind::Effects => st.show_effects = true,
                        _ => {}
                    }
                }
                self.open_window(event_loop, kind);
            }

            if let Some(&main_id) = self.kind_to_id.get(&WindowKind::Main)
                && let Some(surf) = self.windows.get(&main_id)
            {
                surf.window.focus_window();
            }

            let pending_ps = self
                .shared_state
                .lock()
                .unwrap()
                .pending_initial_peak_search
                .take();

            if let Some(PendingPeakSearch(module_arc, settings, file_size, module_name)) = pending_ps {
                let module = Arc::try_unwrap(module_arc).unwrap_or_else(|a| (*a).clone());
                UiSharedState::start_or_resolve_peak_search(
                    &self.shared_state,
                    module,
                    settings,
                    file_size,
                    module_name,
                    false,
                );
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(kind) = self.windows.get(&id).map(|w| w.kind) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                if kind == WindowKind::Main {
                    self.save_ini_on_exit();
                    let mut state = self.shared_state.lock().unwrap();
                    state.is_playing = false;
                    state.is_paused = false;
                    state.is_finished = true;
                    event_loop.exit();
                } else if kind == WindowKind::OpenDialog {
                    let is_modern = {
                        let st = self.shared_state.lock().unwrap();
                        self.is_modern_skin(&st)
                    };
                    if is_modern {
                        self.close_window(kind);
                    } else {
                        self.commit_open_dialog_ok();
                    }
                } else {
                    self.close_window(kind);
                }
            }
            WindowEvent::DroppedFile(dropped_path) => {
                // Drag & Drop aus dem Windows-Explorer (`wmDropFiles` in `mod4win.pas`):
                // Erster Drop eines Batches ersetzt die Playlist, weitere Drops desselben Batches (< 250ms) hängen an.
                let now = Instant::now();
                let replace = self
                    .last_drop_time
                    .map(|t| now.duration_since(t).as_millis() > 250)
                    .unwrap_or(true);
                self.last_drop_time = Some(now);
                let mut state = self.shared_state.lock().unwrap();
                state.handle_dropped_paths(&[dropped_path], replace);
            }
            WindowEvent::Focused(true) => {
                if !matches!(kind, WindowKind::HelpViewer | WindowKind::FileMenu | WindowKind::ConfirmDialog) {
                    self.active_window_kind = kind;
                }
                self.bring_all_windows_to_front(id);
            }
            WindowEvent::Moved(pos) => {
                // Speichere die neue Fensterposition (falls nicht minimiert auf -32000,-32000)
                if pos.x >= -100 && pos.y >= -100 {
                    let (sx, sy) = self.snap_window_position(id, pos.x, pos.y);
                    self.record_window_position(kind, sx, sy);
                }
            }
            WindowEvent::Resized(new_size) => {
                let mut updated_scale = None;
                let mut updated_playlist_rows = None;
                let is_mod_skin = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                let cur_scale = if cfg!(any(feature = "retro-win32", target_arch = "x86")) {
                    1.0
                } else {
                    self.scale_factor.clamp(0.5, 4.0)
                };
                if let Some(surf) = self.windows.get_mut(&id)
                    && new_size.width > 0 && new_size.height > 0 {
                        if let Some(rdir) = surf.active_resize_dir {
                            if surf.kind == WindowKind::Main {
                                let base_w = surf.width.max(1) as f64;
                                let base_h = surf.height.max(1) as f64;
                                let sx = (new_size.width as f64) / base_w;
                                let sy = (new_size.height as f64) / base_h;
                                let prop_scale = match rdir {
                                    ResizeDirection::North | ResizeDirection::South => sy,
                                    ResizeDirection::East | ResizeDirection::West => sx,
                                    _ => {
                                        if (sx - self.scale_factor).abs() >= (sy - self.scale_factor).abs() {
                                            sx
                                        } else {
                                            sy
                                        }
                                    }
                                }
                                .clamp(0.5, 4.0);
                                let target_w = ((surf.width as f64) * prop_scale)
                                    .round()
                                    .max(1.0) as u32;
                                let target_h = ((surf.height as f64) * prop_scale)
                                    .round()
                                    .max(1.0) as u32;
                                if (new_size.width as i32 - target_w as i32).abs() > 1
                                    || (new_size.height as i32 - target_h as i32).abs() > 1
                                {
                                    surf.needs_aspect_snap = true;
                                    let _ = surf
                                        .window
                                        .request_inner_size(PhysicalSize::new(target_w, target_h));
                                    let _ = surf.pixels.resize_surface(target_w, target_h);
                                } else {
                                    let _ = surf.pixels.resize_surface(new_size.width, new_size.height);
                                }
                                updated_scale = Some(prop_scale);
                            } else if is_mod_skin
                                && matches!(surf.kind, WindowKind::QuickSelect | WindowKind::OpenDialog)
                            {
                                let virt_h = (new_size.height as f64) / cur_scale;
                                let new_rows = (((virt_h - 81.0) / 16.0).round() as isize)
                                    .clamp(5, 36) as usize;
                                let target_virt_w = MODERN_PLAYLIST_W;
                                let target_virt_h = 81 + (new_rows as u32) * 16;
                                let target_phys_w = ((target_virt_w as f64) * cur_scale)
                                    .round()
                                    .max(32.0) as u32;
                                surf.width = target_virt_w;
                                surf.height = target_virt_h;
                                surf.needs_aspect_snap = true;
                                let _ = surf.pixels.resize_buffer(target_virt_w, target_virt_h);
                                let _ = surf
                                    .pixels
                                    .resize_surface(target_phys_w, new_size.height.max(1));
                                updated_playlist_rows = Some(new_rows);
                            } else {
                                let _ = surf.pixels.resize_surface(new_size.width, new_size.height);
                            }
                        } else {
                            let _ = surf.pixels.resize_surface(new_size.width, new_size.height);
                        }
                        surf.window.request_redraw();
                    }
                if let Some(sc) = updated_scale {
                    self.scale_factor = sc;
                    self.setup_cfg.gui_scale = sc;
                    if let Ok(mut st) = self.shared_state.lock() {
                        st.ini_config.gui_scale = sc;
                    }
                }
                if let Some(rows) = updated_playlist_rows {
                    self.modern_playlist_rows = rows;
                    self.setup_cfg.modern_playlist_rows = rows as u8;
                    let pl_len = if let Ok(mut st) = self.shared_state.lock() {
                        st.ini_config.modern_playlist_rows = rows as u8;
                        st.playlist.len()
                    } else {
                        0
                    };
                    let max_scroll = pl_len.saturating_sub(rows);
                    if self.quick_select_scroll > max_scroll {
                        self.quick_select_scroll = max_scroll;
                    }
                }
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut drag_vy = None;
                let mut playlist_drag_vy = None;
                let mut virt_pos = None;
                let mut finished_main_scale_resize = false;
                let mut finished_playlist_rows_resize = false;
                let is_mod_skin = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if let Some(surf) = self.windows.get_mut(&id) {
                    surf.mouse_pos = (position.x, position.y);
                    let cur_size = surf.window.inner_size();
                    if (surf.needs_aspect_snap || surf.active_resize_dir.is_some())
                        && cur_size.width > 0
                        && cur_size.height > 0
                    {
                        surf.needs_aspect_snap = false;
                        if surf.active_resize_dir.take().is_some() {
                            if surf.kind == WindowKind::Main {
                                finished_main_scale_resize = true;
                            } else if matches!(
                                surf.kind,
                                WindowKind::QuickSelect | WindowKind::OpenDialog
                            ) {
                                finished_playlist_rows_resize = true;
                            }
                        }
                    }
                    let rdir = detect_resize_direction_for_window(
                        surf.kind,
                        is_mod_skin,
                        position.x,
                        position.y,
                        cur_size,
                    );
                    surf.window.set_cursor(cursor_for_resize_dir(rdir));
                    let mapped = match surf
                        .pixels
                        .window_pos_to_pixel((position.x as f32, position.y as f32))
                    {
                        Ok((px, py)) => (px as i32, py as i32),
                        Err((px, py)) => (px as i32, py as i32),
                    };
                    virt_pos = Some(mapped);
                    if self.preamp_dragging && surf.kind == WindowKind::Main && !self.is_mini {
                        drag_vy = Some(mapped.1);
                    } else if self.playlist_scroll_dragging
                        && matches!(surf.kind, WindowKind::QuickSelect | WindowKind::OpenDialog)
                    {
                        playlist_drag_vy = Some(mapped.1);
                    }
                }
                if finished_main_scale_resize {
                    self.set_scale_factor_live(self.scale_factor);
                } else if finished_playlist_rows_resize {
                    if let Ok(mut st) = self.shared_state.lock() {
                        st.ini_config.modern_playlist_rows =
                            self.modern_playlist_rows.clamp(5, 36) as u8;
                        let _ = st
                            .ini_config
                            .save_to_file(&Mod4WinIniConfig::default_ini_path());
                    }
                    self.apply_skin_to_all_open_windows();
                }
                if let Some(vy) = drag_vy {
                    self.update_preamp_from_y(vy);
                } else if let Some(vy) = playlist_drag_vy {
                    self.update_modern_playlist_scroll_from_y(vy);
                } else if let Some((vx, vy)) = virt_pos {
                    if kind == WindowKind::FileMenu {
                        self.file_menu_hover = if (0..=212).contains(&vx) && (3..=62).contains(&vy) {
                            Some(((vy - 3) / 15).clamp(0, 3) as usize)
                        } else {
                            None
                        };
                    } else if kind == WindowKind::Main && self.sys_menu_open {
                        self.sys_menu_hover = if (2..=198).contains(&vx) && (21..=132).contains(&vy) {
                            Some(((vy - 21) / 14).clamp(0, 7) as usize)
                        } else {
                            None
                        };
                    }
                    let _ = self.on_pointer_move(kind, vx, vy);
                }
            }
            WindowEvent::Touch(touch) => {
                let mut virt_pos = None;
                let mut win_arc_opt = None;
                let mut is_double_click = false;
                if let Some(surf) = self.windows.get_mut(&id) {
                    surf.mouse_pos = (touch.location.x, touch.location.y);
                    let mapped = match surf
                        .pixels
                        .window_pos_to_pixel((touch.location.x as f32, touch.location.y as f32))
                    {
                        Ok((px, py)) => (px as i32, py as i32),
                        Err((px, py)) => (px as i32, py as i32),
                    };
                    virt_pos = Some(mapped);
                    if touch.phase == TouchPhase::Started {
                        let now = Instant::now();
                        is_double_click =
                            now.duration_since(surf.last_click_time).as_millis() < 350;
                        surf.last_click_time = now;
                        win_arc_opt = Some(Arc::clone(&surf.window));
                    }
                }
                if let Some((vx, vy)) = virt_pos {
                    match touch.phase {
                        TouchPhase::Started => {
                            self.touch_gesture.active_touch_id = Some(touch.id);
                            self.on_pointer_press(kind, vx, vy);
                            if let Some(win_arc) = win_arc_opt {
                                self.handle_mouse_click(
                                    event_loop,
                                    kind,
                                    vx,
                                    vy,
                                    MouseButton::Left,
                                    &win_arc,
                                    is_double_click,
                                );
                            }
                        }
                        TouchPhase::Moved => {
                            if self.preamp_dragging && kind == WindowKind::Main && !self.is_mini {
                                self.update_preamp_from_y(vy);
                            } else if self.playlist_scroll_dragging
                                && matches!(kind, WindowKind::QuickSelect | WindowKind::OpenDialog)
                            {
                                self.update_modern_playlist_scroll_from_y(vy);
                            } else {
                                let _ = self.on_pointer_move(kind, vx, vy);
                            }
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.preamp_dragging = false;
                            self.playlist_scroll_dragging = false;
                            self.pressed_btn = None;
                            self.pressed_btn_tracker = None;
                            let _ = self.on_pointer_release();
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: elem_state,
                button,
                ..
            } => {
                if elem_state == ElementState::Released {
                    if button == MouseButton::Left {
                        self.preamp_dragging = false;
                        self.playlist_scroll_dragging = false;
                        self.pressed_btn = None;
                        self.pressed_btn_tracker = None;
                        let _ = self.on_pointer_release();

                        let mut finished_main_scale_resize = false;
                        let mut finished_playlist_rows_resize = false;
                        if let Some(surf) = self.windows.get_mut(&id) {
                            surf.needs_aspect_snap = false;
                            if surf.active_resize_dir.take().is_some() {
                                if surf.kind == WindowKind::Main {
                                    finished_main_scale_resize = true;
                                } else if matches!(
                                    surf.kind,
                                    WindowKind::QuickSelect | WindowKind::OpenDialog
                                ) {
                                    finished_playlist_rows_resize = true;
                                }
                            }
                        }
                        if finished_main_scale_resize {
                            self.set_scale_factor_live(self.scale_factor);
                        } else if finished_playlist_rows_resize {
                            if let Ok(mut st) = self.shared_state.lock() {
                                st.ini_config.modern_playlist_rows =
                                    self.modern_playlist_rows.clamp(5, 36) as u8;
                                let _ = st
                                    .ini_config
                                    .save_to_file(&Mod4WinIniConfig::default_ini_path());
                            }
                            self.apply_skin_to_all_open_windows();
                        }

                        // `--classic-input` 1995er Drag-and-Drop:
                        // Zieht man einen Ordner/Laufwerk/.ZIP/.MOL aus `Directories` in die `Filelist` (bzw. `Files`), wird hineinnavigiert!
                        let drop_pos = self.windows.get(&id).and_then(|s| {
                            s.pixels
                                .window_pos_to_pixel((s.mouse_pos.0 as f32, s.mouse_pos.1 as f32))
                                .ok()
                        });
                        if let Some((px, py)) = drop_pos {
                            let vx = px as i32;
                            let vy = py as i32;
                            if kind == WindowKind::OpenDialog {
                                if let Some(dir_idx) = self.open_dlg.classic_drag_dir.take()
                                    && (12..=182).contains(&vx) && (62..=301).contains(&vy) {
                                        self.navigate_open_dir_item(dir_idx);
                                    }
                            } else if kind == WindowKind::FileManager
                                && let Some(dir_idx) = self.file_man_dlg.classic_drag_dir.take()
                                    && (10..=175).contains(&vx) && (114..=274).contains(&vy)
                                        && let Some(entry) = self.file_man_dlg.dirs.get(dir_idx).cloned() {
                                            match entry {
                                                FileSelectEntry::Parent(p) | FileSelectEntry::Subdir(_, p) => {
                                                    self.file_man_dlg.target_dir = p;
                                                    self.file_man_dlg.refresh_dirs();
                                                    self.file_man_dlg.sync_target_edit();
                                                }
                                                FileSelectEntry::Drive(drv) => {
                                                    self.file_man_dlg.target_dir =
                                                        PathBuf::from(format!("{}:\\", drv));
                                                    self.file_man_dlg.refresh_dirs();
                                                    self.file_man_dlg.sync_target_edit();
                                                }
                                                _ => {}
                                            }
                                        }
                        } else {
                            self.open_dlg.classic_drag_dir = None;
                            self.file_man_dlg.classic_drag_dir = None;
                        }
                    }
                    return;
                }

                let is_mod_skin = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                let Some(surf) = self.windows.get_mut(&id) else {
                    return;
                };
                if button == MouseButton::Left {
                    let cur_size = surf.window.inner_size();
                    if let Some(rdir) = detect_resize_direction_for_window(
                        surf.kind,
                        is_mod_skin,
                        surf.mouse_pos.0,
                        surf.mouse_pos.1,
                        cur_size,
                    ) {
                        surf.active_resize_dir = Some(rdir);
                        let _ = surf.window.drag_resize_window(rdir);
                        return;
                    }
                }
                let (px, py) = match surf
                    .pixels
                    .window_pos_to_pixel((surf.mouse_pos.0 as f32, surf.mouse_pos.1 as f32))
                {
                    Ok((px, py)) => (px as i32, py as i32),
                    Err((px, py)) => (
                        px.clamp(0, surf.width.saturating_sub(1) as isize) as i32,
                        py.clamp(0, surf.height.saturating_sub(1) as isize) as i32,
                    ),
                };
                let vx = px;
                let vy = py;

                let now = Instant::now();
                let is_double_click =
                    now.duration_since(surf.last_click_time).as_millis() < 350;
                surf.last_click_time = now;
                let win_arc = Arc::clone(&surf.window);

                if button == MouseButton::Left {
                    self.on_pointer_press(kind, vx, vy);
                }

                self.handle_mouse_click(
                    event_loop,
                    kind,
                    vx,
                    vy,
                    button,
                    &win_arc,
                    is_double_click,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines: i32 = match delta {
                    super::winit::event::MouseScrollDelta::LineDelta(_, y) => {
                        if y > 0.0 {
                            -3
                        } else if y < 0.0 {
                            3
                        } else {
                            0
                        }
                    }
                    super::winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        if pos.y > 0.0 {
                            -3
                        } else if pos.y < 0.0 {
                            3
                        } else {
                            0
                        }
                    }
                };
                if lines != 0 && self.modifiers.state().control_key() {
                    let step = if lines < 0 { 0.25 } else { -0.25 };
                    let next_scale = (self.scale_factor + step).clamp(0.5, 4.0);
                    self.set_scale_factor_live(next_scale);
                    return;
                }
                if lines != 0 {
                    if kind == WindowKind::Main && !self.is_mini {
                        let vx = self
                            .windows
                            .get(&id)
                            .and_then(|s| {
                                s.pixels
                                    .window_pos_to_pixel((s.mouse_pos.0 as f32, s.mouse_pos.1 as f32))
                                    .ok()
                            })
                            .map(|(px, _)| px as i32)
                            .unwrap_or(0);
                        if vx >= 507 {
                            self.step_preamp_db(if lines < 0 { 1 } else { -1 });
                            return;
                        }
                    }
                    if kind == WindowKind::QuickSelect
                        || (kind == WindowKind::OpenDialog
                            && self.is_modern_skin(&self.shared_state.lock().unwrap()))
                    {
                        let (is_mod, pl_len) = {
                            let st = self.shared_state.lock().unwrap();
                            (self.is_modern_skin(&st), st.playlist.len())
                        };
                        let max_vis = if is_mod {
                            self.modern_playlist_rows.clamp(5, 36)
                        } else {
                            13
                        };
                        let max_scroll = pl_len.saturating_sub(max_vis);
                        self.quick_select_scroll =
                            (self.quick_select_scroll as i32 + lines).clamp(0, max_scroll as i32) as usize;
                    } else if kind == WindowKind::OpenDialog {
                        let vx = self
                            .windows
                            .get(&id)
                            .and_then(|s| {
                                s.pixels
                                    .window_pos_to_pixel((s.mouse_pos.0 as f32, s.mouse_pos.1 as f32))
                                    .ok()
                            })
                            .map(|(px, _)| px as i32)
                            .unwrap_or(100);
                        if vx < 190 {
                            let max_s = self.open_dlg.left_files.len().saturating_sub(20);
                            self.open_dlg.left_scroll =
                                (self.open_dlg.left_scroll as i32 + lines).clamp(0, max_s as i32) as usize;
                        } else if vx < 370 {
                            let max_s = self.open_dlg.mid_dirs.len().saturating_sub(13);
                            self.open_dlg.mid_scroll =
                                (self.open_dlg.mid_scroll as i32 + lines).clamp(0, max_s as i32) as usize;
                        } else {
                            let max_s = self.open_dlg.right_playlist.len().saturating_sub(20);
                            self.open_dlg.right_scroll =
                                (self.open_dlg.right_scroll as i32 + lines).clamp(0, max_s as i32) as usize;
                        }
                    } else if kind == WindowKind::FileSelectDialog {
                        let max_s = self.fsel_dlg.entries.len().saturating_sub(11);
                        self.fsel_dlg.scroll =
                            (self.fsel_dlg.scroll as i32 + lines).clamp(0, max_s as i32) as usize;
                    } else if kind == WindowKind::FileManager {
                        let vx = self
                            .windows
                            .get(&id)
                            .and_then(|s| {
                                s.pixels
                                    .window_pos_to_pixel((s.mouse_pos.0 as f32, s.mouse_pos.1 as f32))
                                    .ok()
                            })
                            .map(|(px, _)| px as i32)
                            .unwrap_or(100);
                        if vx < 180 {
                            let max_s = self.file_man_dlg.files.len().saturating_sub(13);
                            self.file_man_dlg.file_scroll = (self.file_man_dlg.file_scroll as i32
                                + lines)
                                .clamp(0, max_s as i32)
                                as usize;
                        } else {
                            let max_s = self.file_man_dlg.dirs.len().saturating_sub(10);
                            self.file_man_dlg.dir_scroll = (self.file_man_dlg.dir_scroll as i32
                                + lines)
                                .clamp(0, max_s as i32)
                                as usize;
                        }
                    } else if kind == WindowKind::HotkeyEditor {
                        let max_s = self.hotkey_dlg.working_keys.len().saturating_sub(9);
                        self.hotkey_dlg.scroll =
                            (self.hotkey_dlg.scroll as i32 + lines).clamp(0, max_s as i32) as usize;
                    } else if kind == WindowKind::HelpViewer {
                        self.help_dlg.scroll_by_lines(lines, &self.assets);
                    } else if kind == WindowKind::ModuleInfo {
                        let total = self.shared_state.lock().unwrap().instruments.len();
                        if lines > 0 && self.info_inst_offset + 31 < total {
                            self.info_inst_offset += 31;
                        } else if lines < 0 {
                            self.info_inst_offset = self.info_inst_offset.saturating_sub(31);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.handle_key_press(event_loop, &event);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some((dw, dh)) = self.windows.get(&id).map(|s| (s.width, s.height)) else {
                    return;
                };
                let needed_len = (dw * dh * 4) as usize;
                let mut buf = std::mem::take(&mut self.render_scratch_buf);
                buf.resize(needed_len, 0);
                buf.fill(0);
                {
                    let state = self.shared_state.lock().unwrap();
                    self.render_window(kind, &mut buf, dw, dh, &state);
                }
                if let Some(surf) = self.windows.get_mut(&id) {
                    let frame = surf.pixels.frame_mut();
                    let copy_len = frame.len().min(buf.len());
                    frame[..copy_len].copy_from_slice(&buf[..copy_len]);
                    let _ = surf.pixels.render();
                }
                self.render_scratch_buf = buf;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_held_button_repeat();
        let _ = self.poll_touch_long_press_and_kinetic(event_loop);

        let pending_ps = {
            let mut st = self.shared_state.lock().unwrap();
            let _ = st.step_peak_search_progress();
            st.pending_initial_peak_search.take()
        };
        if let Some(PendingPeakSearch(module_arc, settings, file_size, module_name)) = pending_ps {
            let module = Arc::try_unwrap(module_arc).unwrap_or_else(|a| (*a).clone());
            UiSharedState::start_or_resolve_peak_search(
                &self.shared_state,
                module,
                settings,
                file_size,
                module_name,
                false,
            );
        }

        // Prüfen, ob sich durch Songwechsel oder Skin-Wechsel die erwartete Größe eines offenen Fensters
        // geändert hat; falls ja, sofort alle offenen Fenster aktualisieren.
        let needs_size_update = {
            let st_snapshot = self.shared_state.lock().unwrap();
            self.windows.values().any(|surf| {
                let (ew, eh, _) = self.window_dimensions(surf.kind, &st_snapshot);
                surf.width != ew || surf.height != eh
            })
        };
        if needs_size_update {
            self.apply_skin_to_all_open_windows();
        }

        // Garantierter 60-FPS-Redraw-Timer (16 ms) für ALLE offenen Fenster völlig unabhängig vom Fokus:
        let frame_interval = std::time::Duration::from_millis(16);
        if self.last_redraw_poll.elapsed() >= frame_interval {
            self.last_redraw_poll = Instant::now();
            for surf in self.windows.values() {
                surf.window.request_redraw();
            }
        }
        event_loop.set_control_flow(super::winit::event_loop::ControlFlow::WaitUntil(
            self.last_redraw_poll + frame_interval,
        ));
    }
}

/// Dekodiert das eingebettete Original-Icon `assets/MOD4WIN.ico` (32x32 4bpp + 1bpp AND-Maske)
/// in ein `winit::window::Icon` für Taskleiste, Alt+Tab und Linux/X11/Wayland.
#[cfg(not(feature = "retro-win32"))]
fn load_mod4win_window_icon() -> Option<super::winit::window::Icon> {
    const ICO: &[u8] = include_bytes!("../../assets/MOD4WIN.ico");
    if ICO.len() < 766 {
        return None;
    }
    let img_off = u32::from_le_bytes([ICO[18], ICO[19], ICO[20], ICO[21]]) as usize;
    if img_off + 744 > ICO.len() {
        return None;
    }
    let pal_off = img_off + 40;
    let xor_off = pal_off + 64;
    let and_off = xor_off + 512;

    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32usize {
        let src_row = 31 - y;
        let xor_row = &ICO[xor_off + src_row * 16..xor_off + (src_row + 1) * 16];
        let and_row = &ICO[and_off + src_row * 4..and_off + (src_row + 1) * 4];
        for x in 0..32usize {
            let byte_val = xor_row[x / 2];
            let pal_idx = if x % 2 == 0 {
                (byte_val >> 4) & 0x0F
            } else {
                byte_val & 0x0F
            } as usize;
            let b = ICO[pal_off + pal_idx * 4];
            let g = ICO[pal_off + pal_idx * 4 + 1];
            let r = ICO[pal_off + pal_idx * 4 + 2];
            let and_bit = (and_row[x / 8] >> (7 - (x % 8))) & 1;
            let a = if and_bit != 0 { 0u8 } else { 255u8 };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    super::winit::window::Icon::from_rgba(rgba, 32, 32).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_window_pixel_rendering() {
        let state = Arc::new(Mutex::new(UiSharedState::new(
            "AXEL F.",
            "ProTr",
            8,
            18,
            24,
            24,
            vec!["SYNTH LEAD".to_string(), "BASS".to_string()],
            142,
        )));
        let app = Mod4WinApp::new(Arc::clone(&state));
        let st = state.lock().unwrap().clone();

        for kind in [
            WindowKind::Main,
            WindowKind::ModuleInfo,
            WindowKind::InstrumentInfo,
            WindowKind::Effects,
            WindowKind::EffectsHelp,
            WindowKind::QuickSelect,
            WindowKind::OpenDialog,
            WindowKind::SetupDialog,
            WindowKind::WavAskDialog,
            WindowKind::FileSelectDialog,
            WindowKind::FileManager,
            WindowKind::HotkeyEditor,
            WindowKind::ArchiveSetup,
            WindowKind::About,
            WindowKind::HelpViewer,
        ] {
            let (w, h, _) = app.window_dimensions(kind, &st);
            let mut buf = vec![0u8; (w * h * 4) as usize];
            app.render_window(kind, &mut buf, w, h, &st);
            assert!(buf.iter().any(|&b| b != 0), "Window {:?} rendered empty frame", kind);
        }

        // Modern Skin (Option 4): Prüfe exakte Basis-Auflösungen aus `mockup.html` und Rendering
        let mut st_mod = st.clone();
        st_mod.set_skin_mode(SkinMode::Modern);
        assert_eq!(app.window_base_size(WindowKind::Main, SkinMode::Modern, &st_mod), (580, 148));
        assert_eq!(app.window_base_size(WindowKind::ModuleInfo, SkinMode::Modern, &st_mod), (580, 406));
        assert_eq!(app.window_base_size(WindowKind::InstrumentInfo, SkinMode::Modern, &st_mod), (580, 406));
        assert_eq!(app.window_base_size(WindowKind::QuickSelect, SkinMode::Modern, &st_mod), (580, 225));
        assert_eq!(app.window_base_size(WindowKind::OpenDialog, SkinMode::Modern, &st_mod), (580, 225));
        assert_eq!(app.window_base_size(WindowKind::SetupDialog, SkinMode::Modern, &st_mod), (530, 505));
        assert_eq!(app.window_base_size(WindowKind::About, SkinMode::Modern, &st_mod), (360, 185));

        for kind in [
            WindowKind::Main,
            WindowKind::ModuleInfo,
            WindowKind::InstrumentInfo,
            WindowKind::Effects,
            WindowKind::QuickSelect,
            WindowKind::OpenDialog,
            WindowKind::SetupDialog,
            WindowKind::About,
        ] {
            let (mw, mh, _) = app.window_dimensions(kind, &st_mod);
            let mut mbuf = vec![0u8; (mw * mh * 4) as usize];
            app.render_window(kind, &mut mbuf, mw, mh, &st_mod);
            assert!(mbuf.iter().any(|&b| b != 0), "Modern window {:?} rendered empty frame", kind);
        }
    }

    #[test]
    fn test_quick_select_sorting_preserves_orig_idx() {
        let state = Arc::new(Mutex::new(UiSharedState::new(
            "TEST", "Fast2", 8, 10, 10, 10, vec![], 100,
        )));
        {
            let mut st = state.lock().unwrap();
            st.playlist = vec![
                PathBuf::from("C:\\MODS\\ZETA.MOD"),
                PathBuf::from("C:\\MODS\\ALPHA.XM"),
                PathBuf::from("C:\\MODS\\BETA.S3M"),
            ];
            st.playlist_idx = 1;
        }
        let mut app = Mod4WinApp::new(Arc::clone(&state));
        let st = state.lock().unwrap().clone();

        // 0 = Order
        app.quick_select_sort = 0;
        let items_order = app.sorted_quick_select_items(&st);
        assert_eq!(items_order[0], (0, "zeta.mod".to_string()));
        assert_eq!(items_order[1], (1, "alpha.xm".to_string()));
        assert_eq!(items_order[2], (2, "beta.s3m".to_string()));

        // 1 = Name (alpha.xm hat orig_idx = 1!)
        app.quick_select_sort = 1;
        let items_name = app.sorted_quick_select_items(&st);
        assert_eq!(items_name[0], (1, "alpha.xm".to_string()));
        assert_eq!(items_name[1], (2, "beta.s3m".to_string()));
        assert_eq!(items_name[2], (0, "zeta.mod".to_string()));

        // 2 = Ext (mod < s3m < xm)
        app.quick_select_sort = 2;
        let items_ext = app.sorted_quick_select_items(&st);
        assert_eq!(items_ext[0], (0, "zeta.mod".to_string()));
        assert_eq!(items_ext[1], (2, "beta.s3m".to_string()));
        assert_eq!(items_ext[2], (1, "alpha.xm".to_string()));
    }

    #[test]
    fn test_shuffle_number_list_and_auto_advance() {
        let mut st = UiSharedState::new("TEST", "Fast2", 8, 10, 10, 10, vec![], 100);
        st.playlist = vec![
            PathBuf::from("A.XM"),
            PathBuf::from("B.XM"),
            PathBuf::from("C.XM"),
            PathBuf::from("D.XM"),
        ];
        st.playlist_idx = 0;
        st.toggle_shuffle();
        assert!(st.shuffle);
        assert_eq!(st.number_list.len(), 4);

        // Jedes Element 0..4 muss genau einmal in der Permutationsliste vorkommen
        let mut sorted = st.number_list.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3]);

        // Repeat One (repeat_mode = 2) springt bei Liedende zu Pattern 1, bei IntroScan jedoch zum nächsten Song
        st.repeat_mode = 2;
        st.on_auto_advance(false);
        assert_eq!(st.pending_seek_pattern.take(), Some(1));
        assert!(st.pending_load_file.is_none());

        st.on_auto_advance(true); // 15s IntroScan erzwingt Weiterblättern
        assert!(st.pending_load_file.take().is_some());
    }

    #[test]
    fn test_context_sensitive_help_and_hotspot_popup() {
        let state = Arc::new(Mutex::new(UiSharedState::new(
            "TEST", "Fast2", 8, 10, 10, 10, vec![], 100,
        )));
        let mut app = Mod4WinApp::new(Arc::clone(&state));
        assert_eq!(app.context_help_topic_for_window(WindowKind::Main), "HauptDlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::OpenDialog), "OpenDlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::SetupDialog), "SetupDlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::ModuleInfo), "InfoDlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::InstrumentInfo), "InstrWindow");
        assert_eq!(app.context_help_topic_for_window(WindowKind::Effects), "EffWnd");
        assert_eq!(app.context_help_topic_for_window(WindowKind::FileManager), "fmdlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::HotkeyEditor), "keyseldlg");
        assert_eq!(app.context_help_topic_for_window(WindowKind::ArchiveSetup), "ArcOptDlg");

        // Öffne `HauptDlg`, rendere und klicke auf den IDO-Hotspot in `bm0` -> Popup `main_IDO` muss aufgehen!
        app.help_dlg.open_topic("HauptDlg");
        let st = state.lock().unwrap().clone();
        let (w, h, _) = app.window_dimensions(WindowKind::HelpViewer, &st);
        let mut buf_before = vec![0u8; (w * h * 4) as usize];
        app.render_window(WindowKind::HelpViewer, &mut buf_before, w, h, &st);

        // In HauptDlg wird bm0 im Viewport gezeichnet; wir suchen die exakte Position des `main_IDO` Hotspots:
        let hs = app
            .assets
            .block("bm0")
            .expect("bm0 block in hotspots.json")
            .hotspots
            .iter()
            .find(|h| h.id.eq_ignore_ascii_case("main_IDO"))
            .expect("main_IDO hotspot in bm0");
        let (bm_x, bm_y) = app
            .help_dlg
            .bitmap_screen_origin("bm0", &app.assets)
            .expect("bm0 origin in HauptDlg");
        let click_x = bm_x + hs.x + (hs.width as i32) / 2;
        let click_y = bm_y + hs.y + (hs.height as i32) / 2;
        let res = app.help_dlg.handle_click(click_x, click_y, &app.assets);
        assert_eq!(res, HelpClickResult::Handled);
        assert!(
            app.help_dlg
                .popup_topic_id
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("main_IDO"))
                .unwrap_or(false),
            "Clicking main_IDO hotspot on bm0 must open the green-bordered popup for main_IDO"
        );

        let mut buf_after = vec![0u8; (w * h * 4) as usize];
        app.render_window(WindowKind::HelpViewer, &mut buf_after, w, h, &st);
        assert_ne!(
            buf_before, buf_after,
            "Rendered HelpViewer frame must show the green-bordered popup overlay"
        );
    }

    #[test]
    fn test_playlist_remove_and_clear_preserves_playback_and_advances_cleanly() {
        let mut st = UiSharedState::new("SONG B", "Fast2", 8, 10, 10, 10, vec![], 100);
        st.filename = "B.XM".to_string();
        st.playlist = vec![
            PathBuf::from("A.XM"),
            PathBuf::from("B.XM"),
            PathBuf::from("C.XM"),
            PathBuf::from("D.XM"),
        ];
        st.playlist_idx = 1; // spielt B.XM
        st.rebuild_number_list();
        st.repeat_mode = 0; // Repeat Off

        // 1. Eintrag VOR dem aktuellen Track löschen (Index 0 = A.XM)
        assert!(st.remove_playlist_entry(0));
        assert!(st.is_playing);
        assert!(st.pending_load_file.is_none());
        assert!(!st.playing_track_detached);
        assert_eq!(st.playlist_idx, 0); // B.XM ist jetzt auf Index 0
        assert_eq!(st.playlist[st.playlist_idx], PathBuf::from("B.XM"));

        // 2. Aktuell spielenden Track löschen (Index 0 = B.XM) -> Playlist ist jetzt [C.XM, D.XM]
        assert!(st.remove_playlist_entry(0));
        assert!(st.is_playing, "Wiedergabe darf beim Entfernen des aktiven Tracks nicht abbrechen");
        assert!(st.pending_load_file.is_none(), "Kein sofortiger Reload beim Entfernen des aktiven Tracks");
        assert!(st.playing_track_detached);
        assert_eq!(st.playlist_idx, 0); // zeigt bereits auf den nachgerückten Folgetrack C.XM

        // Wenn B.XM normal zu Ende gelaufen ist (`on_auto_advance`), muss nahtlos C.XM (Index 0) geladen werden!
        st.on_auto_advance(false);
        assert!(!st.playing_track_detached);
        assert_eq!(st.playlist_idx, 0);
        assert_eq!(st.pending_load_file.take(), Some(PathBuf::from("C.XM")));

        // 3. Spezialfall: 2 Tracks [C.XM, D.XM], C.XM (Index 0) läuft und wird gelöscht -> nur noch 1 Track [D.XM] übrig
        assert!(st.remove_playlist_entry(0));
        assert_eq!(st.playlist.len(), 1);
        assert!(st.playing_track_detached);
        st.on_auto_advance(false);
        assert!(!st.playing_track_detached);
        assert_eq!(st.playlist_idx, 0);
        assert_eq!(st.pending_load_file.take(), Some(PathBuf::from("D.XM")));

        // 4. Gesamte Playlist leeren (`clear_playlist`) während D.XM läuft
        st.repeat_mode = 1; // selbst bei Repeat All soll nach Clear am Liedende gestoppt werden
        st.clear_playlist();
        assert!(st.playlist.is_empty());
        assert!(st.is_playing, "Laufender Track muss nach Clear normal weiterlaufen");
        assert!(st.pending_load_file.is_none());
        assert!(st.playing_track_detached);

        // Am Ende des Tracks stoppt die Wiedergabe sauber
        st.on_auto_advance(false);
        assert!(!st.is_playing);
        assert_eq!(st.pending_seek_pattern.take(), Some(1));
    }
}

