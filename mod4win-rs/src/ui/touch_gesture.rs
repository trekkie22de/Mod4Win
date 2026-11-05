//! Touch- & Gesten-Steuerung für Touchscreen und iPad-RDP:
//! 1. Mapping von `winit::event::WindowEvent::Touch` (Start, Move, End, Cancel) auf virtuelle Maus-Interaktionen
//! 2. Flüssiges Drag- & Kinetic-Scrolling (Trägheits-Nachlauf) durch direktes Ziehen in allen Listboxen (~5px Schwelle)
//! 3. Long-Press (~450 ms Halten am selben Ort) als automatischer Rechtsklick-Ersatz (z. B. Solo im Effektfenster, Stutter-Toggle).

use std::time::Instant;

use super::winit::event::MouseButton;
use super::winit::event_loop::ActiveEventLoop;

use super::app::{Mod4WinApp, WindowKind, HELP_WINDOW_HEIGHT, HELP_WINDOW_WIDTH};
use super::cpu_sim::CpuProfile;
use super::ini_config::Mod4WinIniConfig;

/// Minimale Bewegung in virtuellen Pixeln (~5px), ab der ein Ziehen in einer Liste als Scroll-Geste erkannt wird.
pub const DRAG_SCROLL_THRESHOLD_PX: i32 = 5;

/// Haltedauer in Millisekunden (~450 ms), nach der ein gehaltener Finger / Mausklick ohne Bewegung
/// automatisch als Rechtsklick (`MouseButton::Right`) ausgelöst wird.
pub const LONG_PRESS_DURATION_MS: u64 = 450;

/// Virtuelle Pixel pro Listenzeile beim Drag-Scrolling.
pub const PIXELS_PER_LIST_ROW: f32 = 12.0;

/// Zustand für Touch-Eingaben, Drag-/Kinetic-Scrolling und Long-Press-Erkennung.
#[derive(Clone, Debug)]
pub struct TouchGestureState {
    pub active_window: Option<WindowKind>,
    pub active_touch_id: Option<u64>,
    pub is_pressed: bool,
    pub press_started_at: Option<Instant>,
    pub start_pos: (i32, i32),
    pub last_pos: (i32, i32),
    pub drag_scroll_active: bool,
    pub long_press_fired: bool,
    pub accum_dy_px: f32,

    // Schnappschuss beim Press zum sauberen Rückgängigmachen eines Links-Toggles,
    // falls nach 450 ms ein Long-Press (Rechtsklick) ausgelöst wird
    pub saved_cpu_profile: u8,
    pub saved_channel_muted: [bool; 32],

    // Kinetic-Scrolling (Trägheits-Nachlauf nach schnellem Wischen)
    pub kinetic_window: Option<WindowKind>,
    pub kinetic_pos: (i32, i32),
    pub velocity_rows_per_sec: f32,
    pub kinetic_accum_rows: f32,
    pub last_move_instant: Option<Instant>,
    pub last_kinetic_instant: Option<Instant>,
}

impl Default for TouchGestureState {
    fn default() -> Self {
        Self {
            active_window: None,
            active_touch_id: None,
            is_pressed: false,
            press_started_at: None,
            start_pos: (0, 0),
            last_pos: (0, 0),
            drag_scroll_active: false,
            long_press_fired: false,
            accum_dy_px: 0.0,
            saved_cpu_profile: 0,
            saved_channel_muted: [false; 32],
            kinetic_window: None,
            kinetic_pos: (0, 0),
            velocity_rows_per_sec: 0.0,
            kinetic_accum_rows: 0.0,
            last_move_instant: None,
            last_kinetic_instant: None,
        }
    }
}

impl Mod4WinApp {
    /// Prüft, ob die virtuellen Koordinaten `(vx, vy)` innerhalb eines scrollbaren Listenbereichs
    /// des Fensters `kind` liegen (`Open`, `Quick-Select`, `File-Manager`, `Hotkeys`, `WinHelp`, `Module-Info`, `FileSelect`).
    pub fn is_point_in_scrollable_list(&self, kind: WindowKind, vx: i32, vy: i32) -> bool {
        match kind {
            WindowKind::OpenDialog => {
                let is_mod = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_mod {
                    let max_vis = self.modern_playlist_rows.clamp(5, 36) as i32;
                    (10..=554).contains(&vx) && (35..=(36 + max_vis * 16)).contains(&vy)
                } else {
                    // Linke Dateiliste (8..208, 63..285), mittlere Ordnerliste (214..340, 63..241), rechte Playlist (348..548, 63..285)
                    ((8..=208).contains(&vx) && (63..=285).contains(&vy))
                        || ((214..=340).contains(&vx) && (63..=241).contains(&vy))
                        || ((348..=548).contains(&vx) && (63..=285).contains(&vy))
                }
            }
            WindowKind::QuickSelect => {
                let is_mod = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_mod {
                    let max_vis = self.modern_playlist_rows.clamp(5, 36) as i32;
                    (10..=554).contains(&vx) && (35..=(36 + max_vis * 16)).contains(&vy)
                } else {
                    (8..=142).contains(&vx) && (25..=183).contains(&vy)
                }
            }
            WindowKind::FileManager => {
                ((10..=158).contains(&vx) && (46..=218).contains(&vy))
                    || ((200..=336).contains(&vx) && (46..=218).contains(&vy))
            }
            WindowKind::HotkeyEditor => (9..=311).contains(&vx) && (39..=203).contains(&vy),
            WindowKind::HelpViewer => {
                (0..=(HELP_WINDOW_WIDTH as i32)).contains(&vx)
                    && (50..=(HELP_WINDOW_HEIGHT as i32)).contains(&vy)
            }
            WindowKind::ModuleInfo => (18..=434).contains(&vx) && (152..=348).contains(&vy),
            WindowKind::FileSelectDialog => (8..=168).contains(&vx) && (40..=176).contains(&vy),
            _ => false,
        }
    }

    /// Scrollt die unter `(vx, vy)` liegende Liste im Fenster `kind` um `delta_rows` Zeilen
    /// (`delta_rows > 0` = nach unten / weiter in der Liste, `delta_rows < 0` = nach oben).
    pub fn scroll_list_at_point(
        &mut self,
        kind: WindowKind,
        vx: i32,
        vy: i32,
        delta_rows: i32,
    ) -> bool {
        if delta_rows == 0 {
            return false;
        }
        match kind {
            WindowKind::OpenDialog => {
                let (is_mod, pl_len) = {
                    let st = self.shared_state.lock().unwrap();
                    (self.is_modern_skin(&st), st.playlist.len())
                };
                if is_mod {
                    let max_vis = self.modern_playlist_rows.clamp(5, 36);
                    let max_scroll = pl_len.saturating_sub(max_vis);
                    if delta_rows > 0 {
                        self.quick_select_scroll =
                            (self.quick_select_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.quick_select_scroll = self
                            .quick_select_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                    true
                } else if vx < 210 {
                    let max_scroll = self.open_dlg.left_files.len().saturating_sub(17);
                    if delta_rows > 0 {
                        self.open_dlg.left_scroll =
                            (self.open_dlg.left_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.open_dlg.left_scroll = self
                            .open_dlg
                            .left_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                    true
                } else if vx < 346 {
                    let max_scroll = self.open_dlg.mid_dirs.len().saturating_sub(11);
                    if delta_rows > 0 {
                        self.open_dlg.mid_scroll =
                            (self.open_dlg.mid_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.open_dlg.mid_scroll = self
                            .open_dlg
                            .mid_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                    true
                } else {
                    let max_scroll = self.open_dlg.right_playlist.len().saturating_sub(17);
                    if delta_rows > 0 {
                        self.open_dlg.right_scroll =
                            (self.open_dlg.right_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.open_dlg.right_scroll = self
                            .open_dlg
                            .right_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                    true
                }
            }
            WindowKind::QuickSelect => {
                let (total, is_mod) = {
                    let st = self.shared_state.lock().unwrap();
                    (st.playlist.len(), self.is_modern_skin(&st))
                };
                let max_vis = if is_mod {
                    self.modern_playlist_rows.clamp(5, 36)
                } else {
                    12
                };
                let max_scroll = total.saturating_sub(max_vis);
                if delta_rows > 0 {
                    self.quick_select_scroll =
                        (self.quick_select_scroll + delta_rows as usize).min(max_scroll);
                } else {
                    self.quick_select_scroll = self
                        .quick_select_scroll
                        .saturating_sub((-delta_rows) as usize);
                }
                true
            }
            WindowKind::FileManager => {
                if vx < 185 {
                    let max_scroll = self.file_man_dlg.files.len().saturating_sub(13);
                    if delta_rows > 0 {
                        self.file_man_dlg.file_scroll =
                            (self.file_man_dlg.file_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.file_man_dlg.file_scroll = self
                            .file_man_dlg
                            .file_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                } else {
                    let max_scroll = self.file_man_dlg.dirs.len().saturating_sub(13);
                    if delta_rows > 0 {
                        self.file_man_dlg.dir_scroll =
                            (self.file_man_dlg.dir_scroll + delta_rows as usize).min(max_scroll);
                    } else {
                        self.file_man_dlg.dir_scroll = self
                            .file_man_dlg
                            .dir_scroll
                            .saturating_sub((-delta_rows) as usize);
                    }
                }
                true
            }
            WindowKind::HotkeyEditor => {
                let max_scroll = self.hotkey_dlg.working_keys.len().saturating_sub(11);
                if delta_rows > 0 {
                    self.hotkey_dlg.scroll =
                        (self.hotkey_dlg.scroll + delta_rows as usize).min(max_scroll);
                } else {
                    self.hotkey_dlg.scroll =
                        self.hotkey_dlg.scroll.saturating_sub((-delta_rows) as usize);
                }
                true
            }
            WindowKind::HelpViewer => {
                let _ = vy;
                self.help_dlg.scroll_by_lines(delta_rows, &self.assets);
                true
            }
            WindowKind::ModuleInfo => {
                let total_inst = {
                    let st = self.shared_state.lock().unwrap();
                    st.instrument_names.len()
                };
                let max_off = total_inst.saturating_sub(16);
                if delta_rows > 0 {
                    self.info_inst_offset =
                        (self.info_inst_offset + delta_rows as usize).min(max_off);
                } else {
                    self.info_inst_offset = self
                        .info_inst_offset
                        .saturating_sub((-delta_rows) as usize);
                }
                true
            }
            WindowKind::FileSelectDialog => {
                let max_scroll = self.fsel_dlg.entries.len().saturating_sub(10);
                if delta_rows > 0 {
                    self.fsel_dlg.scroll =
                        (self.fsel_dlg.scroll + delta_rows as usize).min(max_scroll);
                } else {
                    self.fsel_dlg.scroll =
                        self.fsel_dlg.scroll.saturating_sub((-delta_rows) as usize);
                }
                true
            }
            _ => false,
        }
    }

    /// Registriert den Beginn eines Finger-Taps oder linken Mausklicks bei `(vx, vy)`.
    pub fn on_pointer_press(&mut self, kind: WindowKind, vx: i32, vy: i32) {
        let (cpu_prof, muted) = {
            let st = self.shared_state.lock().unwrap();
            (st.ini_config.cpu_sim_profile, st.channel_muted)
        };
        let now = Instant::now();
        self.touch_gesture.active_window = Some(kind);
        self.touch_gesture.is_pressed = true;
        self.touch_gesture.press_started_at = Some(now);
        self.touch_gesture.start_pos = (vx, vy);
        self.touch_gesture.last_pos = (vx, vy);
        self.touch_gesture.drag_scroll_active = false;
        self.touch_gesture.long_press_fired = false;
        self.touch_gesture.accum_dy_px = 0.0;
        self.touch_gesture.saved_cpu_profile = cpu_prof;
        self.touch_gesture.saved_channel_muted = muted;
        // Stoppe laufendes Kinetic-Scrolling sofort beim Antippen
        self.touch_gesture.velocity_rows_per_sec = 0.0;
        self.touch_gesture.kinetic_accum_rows = 0.0;
        self.touch_gesture.last_move_instant = Some(now);
    }

    /// Verarbeitet eine Finger-/Mausbewegung bei gedrücktem Zustand (Drag-Scrolling in Listen).
    /// Gibt `true` zurück, wenn durch das Ziehen gescrollt wurde.
    pub fn on_pointer_move(&mut self, kind: WindowKind, vx: i32, vy: i32) -> bool {
        if !self.touch_gesture.is_pressed || self.touch_gesture.active_window != Some(kind) {
            return false;
        }

        let (sx, sy) = self.touch_gesture.start_pos;
        let (_, ly) = self.touch_gesture.last_pos;
        let total_dx = (vx - sx).abs();
        let total_dy = (vy - sy).abs();
        let step_dy = vy - ly;
        let now = Instant::now();
        let dt = self
            .touch_gesture
            .last_move_instant
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.016)
            .max(0.001);

        self.touch_gesture.last_pos = (vx, vy);
        self.touch_gesture.last_move_instant = Some(now);

        if !self.touch_gesture.drag_scroll_active {
            if (total_dx >= DRAG_SCROLL_THRESHOLD_PX || total_dy >= DRAG_SCROLL_THRESHOLD_PX)
                && self.is_point_in_scrollable_list(kind, sx, sy)
            {
                self.touch_gesture.drag_scroll_active = true;
                self.pressed_btn = None;
                // Initialen Offset seit Start übernehmen
                self.touch_gesture.accum_dy_px = -(vy - sy) as f32;
            } else {
                return false;
            }
        } else {
            // Nachziehen: Finger nach oben ziehen (step_dy < 0) scrollt Liste nach unten (+rows)
            self.touch_gesture.accum_dy_px += (-step_dy) as f32;
        }

        let rows = (self.touch_gesture.accum_dy_px / PIXELS_PER_LIST_ROW).trunc() as i32;
        let inst_vel = ((-step_dy as f32) / PIXELS_PER_LIST_ROW) / dt;
        self.touch_gesture.velocity_rows_per_sec =
            self.touch_gesture.velocity_rows_per_sec * 0.4 + inst_vel * 0.6;
        self.touch_gesture.kinetic_window = Some(kind);
        self.touch_gesture.kinetic_pos = (sx, sy);

        if rows != 0 {
            self.touch_gesture.accum_dy_px -= (rows as f32) * PIXELS_PER_LIST_ROW;
            return self.scroll_list_at_point(kind, sx, sy, rows);
        }
        true
    }

    /// Beendet eine Touch-/Maus-Press-Phase und startet ggf. den Kinetic-Scroll-Nachlauf.
    pub fn on_pointer_release(&mut self) -> bool {
        let was_consumed =
            self.touch_gesture.drag_scroll_active || self.touch_gesture.long_press_fired;
        if !self.touch_gesture.drag_scroll_active {
            self.touch_gesture.velocity_rows_per_sec = 0.0;
        } else {
            self.touch_gesture.last_kinetic_instant = Some(Instant::now());
        }
        self.touch_gesture.is_pressed = false;
        self.touch_gesture.press_started_at = None;
        self.touch_gesture.active_touch_id = None;
        was_consumed
    }

    /// Prüft im Event-Loop (`about_to_wait`), ob:
    /// 1. ein Long-Press (>= 450 ms ohne Bewegung > 5px) vorliegt -> löst Rechtsklick aus,
    /// 2. ein Kinetic-Scroll-Nachlauf aktiv ist -> führt sanftes Trägheits-Scrollen aus.
    pub fn poll_touch_long_press_and_kinetic(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let mut needs_redraw = false;

        // 1. Long-Press-Prüfung (~450 ms Halten bei echtem Touch-Input)
        if self.touch_gesture.is_pressed
            && self.touch_gesture.active_touch_id.is_some()
            && !self.touch_gesture.drag_scroll_active
            && !self.touch_gesture.long_press_fired
            && !self.preamp_dragging
            && self.pressed_btn.is_none()
            && let Some(started) = self.touch_gesture.press_started_at
                && started.elapsed().as_millis() as u64 >= LONG_PRESS_DURATION_MS {
                    let (vx, vy) = self.touch_gesture.start_pos;
                    let (lx, ly) = self.touch_gesture.last_pos;
                    if (lx - vx).abs() < DRAG_SCROLL_THRESHOLD_PX
                        && (ly - vy).abs() < DRAG_SCROLL_THRESHOLD_PX
                    {
                        self.touch_gesture.long_press_fired = true;
                        if let Some(kind) = self.touch_gesture.active_window {
                            self.execute_long_press_right_click(event_loop, kind, vx, vy);
                            needs_redraw = true;
                        }
                    }
                }

        // 2. Kinetic-Scrolling (Trägheits-Nachlauf)
        if !self.touch_gesture.is_pressed && self.touch_gesture.velocity_rows_per_sec.abs() >= 0.8 {
            let now = Instant::now();
            let dt = self
                .touch_gesture
                .last_kinetic_instant
                .map(|t| now.duration_since(t).as_secs_f32())
                .unwrap_or(0.016)
                .clamp(0.001, 0.1);
            self.touch_gesture.last_kinetic_instant = Some(now);

            self.touch_gesture.kinetic_accum_rows +=
                self.touch_gesture.velocity_rows_per_sec * dt;
            // Exponentielle Reibungs-Dämpfung
            self.touch_gesture.velocity_rows_per_sec *= (1.0 - 5.5 * dt).clamp(0.0, 0.96);

            let step_rows = self.touch_gesture.kinetic_accum_rows.trunc() as i32;
            if step_rows != 0 {
                self.touch_gesture.kinetic_accum_rows -= step_rows as f32;
                if let Some(kind) = self.touch_gesture.kinetic_window {
                    let (kx, ky) = self.touch_gesture.kinetic_pos;
                    if self.scroll_list_at_point(kind, kx, ky, step_rows) {
                        needs_redraw = true;
                    } else {
                        self.touch_gesture.velocity_rows_per_sec = 0.0;
                    }
                }
            }
        } else if !self.touch_gesture.is_pressed {
            self.touch_gesture.velocity_rows_per_sec = 0.0;
        }

        needs_redraw
    }

    /// Führt die Rechtsklick-Aktion bei einem erkannten Long-Press (450 ms) aus und macht
    /// einen beim initialen Touch-Down ausgelösten Links-Toggle (z. B. CPU-Profil oder Kanal-Mute)
    /// zuvor rückgängig, damit der Rechtsklick (Stutter-Toggle bzw. Kanal-Solo) sauber greift.
    pub fn execute_long_press_right_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        kind: WindowKind,
        vx: i32,
        vy: i32,
    ) {
        // 1. Falls Long-Press auf Main_Percnt / PBar / Main_BuffTime im Hauptfenster:
        //    Ursprüngliches CPU-Profil wiederherstellen, bevor Rechtsklick (Authentic Stutter) getoggelt wird
        let is_cpu_zone = ((368..=374).contains(&vx) && (29..=82).contains(&vy))
            || ((370..=444).contains(&vx) && (62..=82).contains(&vy));
        if kind == WindowKind::Main && !self.is_mini && is_cpu_zone {
            let mut st = self.shared_state.lock().unwrap();
            st.ini_config.cpu_sim_profile = self.touch_gesture.saved_cpu_profile;
            let _ = st
                .ini_config
                .save_to_file(&Mod4WinIniConfig::default_ini_path());
        }

        // 2. Falls Long-Press auf einer Kanalzeile im Effects-Fenster:
        //    Ursprünglichen Mute-Zustand wiederherstellen, bevor Solo (Rechtsklick) geschaltet wird
        if kind == WindowKind::Effects {
            let mut st = self.shared_state.lock().unwrap();
            st.channel_muted = self.touch_gesture.saved_channel_muted;
            st.channel_muted_dirty = true;
        }

        let win_opt = self
            .kind_to_id
            .get(&kind)
            .and_then(|id| self.windows.get(id))
            .map(|s| s.window.clone());

        if let Some(window) = win_opt {
            self.pressed_btn = None;
            self.handle_mouse_click(
                event_loop,
                kind,
                vx,
                vy,
                MouseButton::Right,
                &window,
                false,
            );
        } else {
            // Für Headless-Unit-Tests ohne geöffnetes OS-Fenster:
            // Direkt die Rechtsklick-Logik für Main_Percnt bzw. Effects ausführen
            if kind == WindowKind::Main && !self.is_mini && is_cpu_zone {
                let mut state = self.shared_state.lock().unwrap();
                state.ini_config.authentic_stutter = !state.ini_config.authentic_stutter;
                let prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile);
                let banner = prof.lcd_banner_with_stutter(state.ini_config.authentic_stutter);
                state.set_status_banner(banner);
            } else if kind == WindowKind::Effects {
                let mut state = self.shared_state.lock().unwrap();
                let num_ch = state.num_channels.clamp(4, 32);
                let ch_end_y = 80 + (num_ch as i32) * 9;
                if (8..=23).contains(&vx) && (80..ch_end_y).contains(&vy) {
                    let ch_idx = ((vy - 80) / 9) as usize;
                    for m in state.channel_muted.iter_mut() {
                        *m = true;
                    }
                    if ch_idx < 32 {
                        state.channel_muted[ch_idx] = false;
                    }
                    state.channel_muted_dirty = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::UiSharedState;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_drag_scrolling_and_kinetic_inertia_in_lists() {
        let mut st = UiSharedState::new("TEST", "XM", 8, 4, 4, 4, vec![], 10);
        for i in 0..30 {
            st.playlist.push(PathBuf::from(format!("SONG_{:02}.XM", i)));
        }
        let shared = Arc::new(Mutex::new(st));
        let mut app = Mod4WinApp::new(shared);

        assert_eq!(app.quick_select_scroll, 0);
        // Antippen bei (50, 120) im QuickSelect-Listenbereich
        app.on_pointer_press(WindowKind::QuickSelect, 50, 120);
        // Kleine Bewegung (< 5px) löst noch kein Drag-Scrolling aus
        assert!(!app.on_pointer_move(WindowKind::QuickSelect, 50, 117));
        assert_eq!(app.quick_select_scroll, 0);

        // Wischen um 48px nach oben (vy: 120 -> 72) scrollt die Liste um 4 Zeilen nach unten
        assert!(app.on_pointer_move(WindowKind::QuickSelect, 50, 72));
        assert!(app.touch_gesture.drag_scroll_active);
        assert_eq!(app.quick_select_scroll, 4);

        // Loslassen behält Kinetic-Geschwindigkeit bei
        assert!(app.on_pointer_release());
        assert!(app.touch_gesture.velocity_rows_per_sec > 0.0);
    }
}
