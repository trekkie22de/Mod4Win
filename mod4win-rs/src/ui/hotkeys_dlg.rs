//! Prozeduraler Hotkey-Editor (`KEYSEL.PAS`, `336 x 248`) und komplette Original-Tastatursteuerung.

use std::sync::Arc;
use super::winit::event::KeyEvent;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use super::winit::window::Window;

use super::app::{Mod4WinApp, PressedButton, UiSharedState, WindowKind, HOTKEY_WINDOW_WIDTH};
use super::ini_config::{default_hotkey_bindings, HotkeyBinding, Mod4WinIniConfig};
use super::main_win::ConfirmAction;
use super::setup_dlg::{FileSelectEntry, SetupEditFocus, WavAskEditFocus};
use super::widgets::{
    draw_3d_panel, draw_bmp_button_2state, draw_frame_3d_1px, draw_hkonoff_led, draw_oem_text,
    draw_oem_text_centered, draw_rect_outline, fill_rect,
};

/// Zustand des prozeduralen Hotkey-Editors (`KEYSEL.PAS`, `336 x 248`).
#[derive(Clone, Debug)]
pub struct HotkeyEditorState {
    pub working_keys: Vec<HotkeyBinding>,
    pub scroll: usize,
    pub cursor: usize,
    pub capturing: bool,
}

impl Default for HotkeyEditorState {
    fn default() -> Self {
        Self {
            working_keys: default_hotkey_bindings(),
            scroll: 0,
            cursor: 0,
            capturing: false,
        }
    }
}

/// Prüft, ob eine Tastenkombination systemweit reserviert und daher als benutzerdefinierter Hotkey verboten ist
/// (`Strg + A` ist für „Alles markieren“ in der Playlist / Dateiliste reserviert).
pub fn is_forbidden_hotkey(key_name: &str, state: u8) -> bool {
    state == 1 && key_name.eq_ignore_ascii_case("A")
}

impl Mod4WinApp {
    /// Rendert den prozeduralen Hotkey-Editor (`KEYSEL.PAS`, `336 x 248`, KEIN Handbuch-Bild `bm7.bmp`!).
    pub(crate) fn render_hotkey_editor_window(&self, frame: &mut [u8], dw: u32, dh: u32, _state: &UiSharedState) {
        let cap_dy: i32 = 18;
        let hk = &self.hotkey_dlg;
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
            loc.tr_or("win", "hotkey_title", "MOD4WIN Hotkey Setup"),
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
        let col_gray = [192, 192, 192, 255];

        // Schwarzer Außenrahmen `(7, CapDY+7=25, 321, 185)` (`KEYSEL.PAS` Zeile 142)
        draw_rect_outline(frame, dw, dh, 7, cap_dy + 7, 321, 185, col_black);

        // Listbox `pselbox` (17, CapDY+16=34, 285, 116; 9 Zeilen à 12px) + Scrollbar (302, 34, 15, 116)
        draw_3d_panel(frame, dw, dh, 16, 33, 286, 118, true, col_black);
        let max_scroll = hk.working_keys.len().saturating_sub(9);
        let scroll = hk.scroll.min(max_scroll);

        for (row, (idx, item)) in hk
            .working_keys
            .iter()
            .enumerate()
            .skip(scroll)
            .take(9)
            .enumerate()
        {
            let ry = 35 + (row as i32) * 12;
            let is_cur = idx == hk.cursor;
            if is_cur {
                fill_rect(frame, dw, dh, 17, ry, 284, 12, col_green);
            }
            let lbl_str = loc.hotkey_label(item.action_id, item.label);
            let disp: String = lbl_str.chars().take(34).collect();
            draw_oem_text(
                frame,
                dw,
                dh,
                19,
                ry,
                &disp,
                if is_cur { col_black } else { col_green },
            );
        }
        Self::draw_vertical_scrollbar(
            frame,
            dw,
            dh,
            302,
            33,
            15,
            118,
            scroll,
            9,
            hk.working_keys.len(),
        );

        // Unteres Tasten-Panel `tkeywin` bei `(14, CapDY+142=160, 306, 42)` (`KEYSEL.PAS` Zeilen 266-281)
        draw_frame_3d_1px(frame, dw, dh, 14, 160, 306, 42, true);
        if hk.capturing {
            draw_rect_outline(frame, dw, dh, 16, 162, 302, 38, col_black);
        }

        let cur_binding = hk
            .working_keys
            .get(hk.cursor)
            .cloned()
            .unwrap_or_else(|| default_hotkey_bindings()[0].clone());

        // `keytext` Box bei `(34, 168, 266, 14)`: im Original (`pmycenterstatic`) grauer 3D-Rahmen mit schwarzer zentrierter Schrift
        fill_rect(frame, dw, dh, 34, 168, 266, 14, col_gray);
        draw_frame_3d_1px(frame, dw, dh, 33, 167, 268, 16, true);
        let key_disp = if hk.capturing {
            loc.tr_or("hotkeys", "press_new_key", ">>> PRESS NEW KEY <<<").to_string()
        } else {
            cur_binding.key_name.clone()
        };
        draw_oem_text_centered(frame, dw, dh, 34, 169, 266, &key_disp, col_black);

        // Grüne LEDs (`HKONOFF.bmp`, `10x5`) + schwarze Beschriftungen `Control` (64, 185), `Shift` (164, 185), `Alt` (264, 185)
        let has_ctrl = (cur_binding.state & 1) != 0;
        let has_shift = (cur_binding.state & 2) != 0;
        let has_alt = (cur_binding.state & 4) != 0;
        draw_hkonoff_led(frame, dw, dh, 50, 188, has_ctrl, &self.assets.hkonoff);
        draw_oem_text(frame, dw, dh, 64, 185, "Control", col_black);

        draw_hkonoff_led(frame, dw, dh, 150, 188, has_shift, &self.assets.hkonoff);
        draw_oem_text(frame, dw, dh, 164, 185, "Shift", col_black);

        draw_hkonoff_led(frame, dw, dh, 250, 188, has_alt, &self.assets.hkonoff);
        draw_oem_text(frame, dw, dh, 264, 185, "Alt", col_black);

        // Untere Zeile (`y = CapDY + 200 = 218`): volles `M_LABEL.bmp` (`94x20`) ohne Rahmen bei (35, 218) + OK (162), Cancel (203), Default (244), Help (285)
        let by = cap_dy + 200; // 218
        self.assets
            .m_label
            .blit_rect_to(0, 0, 94, 20, frame, dw, dh, 35, by);

        draw_frame_3d_1px(frame, dw, dh, 161, by - 1, 167, 23, true);
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            162,
            by,
            self.pressed_btn == Some(PressedButton::HotkeyOk),
            &self.assets.bsok,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            203,
            by,
            self.pressed_btn == Some(PressedButton::HotkeyCancel),
            &self.assets.bscancel,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            244,
            by,
            self.pressed_btn == Some(PressedButton::HotkeyDefault),
            &self.assets.b_default,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            285,
            by,
            self.pressed_btn == Some(PressedButton::HotkeyHelp),
            &self.assets.bshelp,
        );
    }

    pub(crate) fn commit_hotkey_editor_ok(&mut self) {
        self.setup_cfg.hotkeys = self.hotkey_dlg.working_keys.clone();
        {
            let mut st = self.shared_state.lock().unwrap();
            st.ini_config.hotkeys = self.hotkey_dlg.working_keys.clone();
            let ini_path = Mod4WinIniConfig::default_ini_path();
            let _ = st.ini_config.save_to_file(&ini_path);
        }
        self.hotkey_dlg.capturing = false;
        self.close_window(WindowKind::HotkeyEditor);
    }

    pub(crate) fn handle_hotkey_editor_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if vy <= 18 {
            if vx <= 20 || vx >= (HOTKEY_WINDOW_WIDTH as i32 - 20) {
                self.hotkey_dlg.capturing = false;
                self.close_window(WindowKind::HotkeyEditor);
            } else {
                let _ = window.drag_window();
            }
            return;
        }

        // Listbox `pselbox` (15..301, 32..150) & Scrollbar (302..317, 32..150)
        if (32..=150).contains(&vy) {
            if (15..=301).contains(&vx) {
                let row = ((vy - 35).max(0) / 12) as usize;
                let max_s = self.hotkey_dlg.working_keys.len().saturating_sub(9);
                let scroll = self.hotkey_dlg.scroll.min(max_s);
                let idx = scroll + row;
                if idx < self.hotkey_dlg.working_keys.len() {
                    self.hotkey_dlg.cursor = idx;
                    self.hotkey_dlg.capturing = is_double_click;
                }
                return;
            } else if (302..=317).contains(&vx) {
                let max_s = self.hotkey_dlg.working_keys.len().saturating_sub(9);
                if vy <= 46 {
                    self.hotkey_dlg.scroll = self.hotkey_dlg.scroll.saturating_sub(1);
                } else if vy >= 136 {
                    self.hotkey_dlg.scroll = (self.hotkey_dlg.scroll + 1).min(max_s);
                } else if vy < 91 {
                    self.hotkey_dlg.scroll = self.hotkey_dlg.scroll.saturating_sub(8);
                } else {
                    self.hotkey_dlg.scroll = (self.hotkey_dlg.scroll + 8).min(max_s);
                }
                return;
            }
        }

        // Klick auf `keytext` Box (34..300, 165..183) aktiviert Key-Capture
        if (165..=183).contains(&vy) && (34..=300).contains(&vx) {
            self.hotkey_dlg.capturing = !self.hotkey_dlg.capturing;
            return;
        }

        // Klick auf die grünen LEDs `Control` (45..135), `Shift` (145..220), `Alt` (245..305) bei y = 184..199
        if (184..=199).contains(&vy)
            && let Some(binding) = self.hotkey_dlg.working_keys.get_mut(self.hotkey_dlg.cursor) {
                let next_state = if (45..=135).contains(&vx) {
                    Some(binding.state ^ 1)
                } else if (145..=220).contains(&vx) {
                    Some(binding.state ^ 2)
                } else if (245..=305).contains(&vx) {
                    Some(binding.state ^ 4)
                } else {
                    None
                };
                if let Some(ns) = next_state {
                    if is_forbidden_hotkey(&binding.key_name, ns) {
                        if let Ok(mut st) = self.shared_state.lock() {
                            st.set_status_banner("CTRL+A RESERVED");
                        }
                    } else {
                        binding.state = ns;
                    }
                    return;
                }
            }

        // Untere Buttons (y = 217..240): OK (162..202), Cancel (203..243), Default (244..284), Help (285..326)
        if (217..=240).contains(&vy) {
            if (162..=202).contains(&vx) {
                self.pressed_btn = Some(PressedButton::HotkeyOk);
                self.commit_hotkey_editor_ok();
                return;
            } else if (203..=243).contains(&vx) {
                self.pressed_btn = Some(PressedButton::HotkeyCancel);
                self.hotkey_dlg.capturing = false;
                self.close_window(WindowKind::HotkeyEditor);
                return;
            } else if (244..=284).contains(&vx) {
                self.pressed_btn = Some(PressedButton::HotkeyDefault);
                self.hotkey_dlg.working_keys = default_hotkey_bindings();
                self.hotkey_dlg.capturing = false;
                return;
            } else if (285..=326).contains(&vx) {
                self.pressed_btn = Some(PressedButton::HotkeyHelp);
                self.open_help_topic(event_loop, "keyseldlg");
                return;
            }
        }

        self.drag_window_if_allowed(vy, window);
    }

    // =========================================================================
    // Komplette Original-Tastatursteuerung (Meilenstein 4)
    // =========================================================================

    pub(crate) fn key_event_to_vkey_and_name(event: &KeyEvent) -> Option<(u16, String)> {
        match &event.logical_key {
            Key::Named(NamedKey::Control)
            | Key::Named(NamedKey::Shift)
            | Key::Named(NamedKey::Alt)
            | Key::Named(NamedKey::Super) => return None,
            Key::Named(NamedKey::Space) => return Some((32, "SPACE".to_string())),
            Key::Named(NamedKey::Escape) => return Some((27, "ESCAPE".to_string())),
            Key::Named(NamedKey::Enter) => return Some((13, "ENTER".to_string())),
            Key::Named(NamedKey::Tab) => return Some((9, "TAB".to_string())),
            Key::Named(NamedKey::Backspace) => return Some((8, "BACKSPACE".to_string())),
            Key::Named(NamedKey::Insert) => return Some((45, "INSERT".to_string())),
            Key::Named(NamedKey::Delete) => return Some((46, "DELETE".to_string())),
            Key::Named(NamedKey::Home) => return Some((36, "HOME".to_string())),
            Key::Named(NamedKey::End) => return Some((35, "END".to_string())),
            Key::Named(NamedKey::PageUp) => return Some((33, "PAGE UP".to_string())),
            Key::Named(NamedKey::PageDown) => return Some((34, "PAGE DOWN".to_string())),
            Key::Named(NamedKey::ArrowLeft) => return Some((37, "CURSOR LEFT".to_string())),
            Key::Named(NamedKey::ArrowUp) => return Some((38, "CURSOR UP".to_string())),
            Key::Named(NamedKey::ArrowRight) => return Some((39, "CURSOR RIGHT".to_string())),
            Key::Named(NamedKey::ArrowDown) => return Some((40, "CURSOR DOWN".to_string())),
            Key::Named(NamedKey::F1) => return Some((112, "F1".to_string())),
            Key::Named(NamedKey::F2) => return Some((113, "F2".to_string())),
            Key::Named(NamedKey::F3) => return Some((114, "F3".to_string())),
            Key::Named(NamedKey::F4) => return Some((115, "F4".to_string())),
            Key::Named(NamedKey::F5) => return Some((116, "F5".to_string())),
            Key::Named(NamedKey::F6) => return Some((117, "F6".to_string())),
            Key::Named(NamedKey::F7) => return Some((118, "F7".to_string())),
            Key::Named(NamedKey::F8) => return Some((119, "F8".to_string())),
            Key::Named(NamedKey::F9) => return Some((120, "F9".to_string())),
            Key::Named(NamedKey::F10) => return Some((121, "F10".to_string())),
            Key::Named(NamedKey::F11) => return Some((122, "F11".to_string())),
            Key::Named(NamedKey::F12) => return Some((123, "F12".to_string())),
            _ => {}
        }
        if let PhysicalKey::Code(code) = event.physical_key {
            match code {
                KeyCode::NumpadAdd => return Some((107, "NUMPAD PLUS".to_string())),
                KeyCode::NumpadSubtract => return Some((109, "NUMPAD MINUS".to_string())),
                KeyCode::NumpadMultiply => return Some((106, "NUMPAD MULTIPLY".to_string())),
                KeyCode::NumpadDivide => return Some((111, "NUMPAD DIVIDE".to_string())),
                _ => {}
            }
        }
        if let Key::Character(ch) = &event.logical_key
            && let Some(c) = ch.chars().next() {
                let up = c.to_ascii_uppercase();
                if up.is_ascii_alphanumeric() {
                    return Some((up as u16, up.to_string()));
                }
                match up {
                    '+' => return Some((107, "NUMPAD PLUS".to_string())),
                    '-' => return Some((109, "NUMPAD MINUS".to_string())),
                    '*' => return Some((106, "NUMPAD MULTIPLY".to_string())),
                    '/' => return Some((111, "NUMPAD DIVIDE".to_string())),
                    _ => {}
                }
            }
        None
    }

    pub(crate) fn execute_hotkey_event(&mut self, event_loop: &ActiveEventLoop, action_id: u8) -> bool {
        match action_id {
            1 => {
                // 1. Stop playback
                let mut state = self.shared_state.lock().unwrap();
                state.intro_scan = false;
                state.is_playing = false;
                state.is_paused = false;
                state.pending_seek_pattern = Some(1);
            }
            2 => self.trigger_play_or_hdd_record(event_loop), // 2. Start playback
            3 => {
                // 3. Pause playback
                let mut state = self.shared_state.lock().unwrap();
                if state.is_playing || state.is_paused {
                    state.is_paused = !state.is_paused;
                    state.is_playing = !state.is_paused;
                }
            }
            4 => self.prev_song(), // 4. Previous song
            5 => {
                // 5. Rewind
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta -= 1;
            }
            6 => {
                // 6. Fast forward
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta += 1;
            }
            7 => self.next_song(), // 7. Next song
            8 => {
                // 8. First song
                let mut state = self.shared_state.lock().unwrap();
                if state.playlist.len() > 1 {
                    if state.number_list.len() != state.playlist.len() {
                        state.rebuild_number_list();
                    }
                    state.playlist_pos = 0;
                    state.playlist_idx = state.number_list[0];
                    let p = state.playlist[state.playlist_idx].clone();
                    state.pending_load_file = Some(p);
                    state.is_playing = true;
                    state.is_paused = false;
                } else {
                    state.pending_seek_pattern = Some(1);
                }
            }
            9 => {
                // 9. Last song
                let mut state = self.shared_state.lock().unwrap();
                if state.playlist.len() > 1 {
                    if state.number_list.len() != state.playlist.len() {
                        state.rebuild_number_list();
                    }
                    let last = state.playlist.len() - 1;
                    state.playlist_pos = last;
                    state.playlist_idx = state.number_list[last];
                    let p = state.playlist[state.playlist_idx].clone();
                    state.pending_load_file = Some(p);
                    state.is_playing = true;
                    state.is_paused = false;
                } else {
                    let last_pat = state.num_orders.max(1) as u16;
                    state.pending_seek_pattern = Some(last_pat);
                }
            }
            10 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    self.toggle_window(event_loop, WindowKind::OpenDialog);
                } else {
                    // Classic Skin: `O` öffnet den modalen Open-Dialog (oder fokussiert ihn), flackert niemals weg!
                    self.open_window(event_loop, WindowKind::OpenDialog);
                }
            }
            11 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    self.toggle_window(event_loop, WindowKind::QuickSelect);
                } else {
                    // Classic Skin: `L` öffnet die modale Quick Selection (oder fokussiert sie)
                    self.open_window(event_loop, WindowKind::QuickSelect);
                }
            }
            12 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    return false;
                }
                self.file_menu_open = !self.file_menu_open;
                self.file_menu_hover = None;
                if self.file_menu_open {
                    self.open_window(event_loop, WindowKind::FileMenu);
                } else {
                    self.close_window(WindowKind::FileMenu);
                }
            }
            13 => self.toggle_window(event_loop, WindowKind::ModuleInfo),
            14 => self.toggle_window(event_loop, WindowKind::Effects),
            15 => {
                let mut state = self.shared_state.lock().unwrap();
                state.minus_mode = !state.minus_mode;
            }
            16 => {
                let mut state = self.shared_state.lock().unwrap();
                state.show_filename = true;
                state.sync_runtime_to_ini();
            }
            17 => {
                let mut state = self.shared_state.lock().unwrap();
                state.show_filename = false;
                state.sync_runtime_to_ini();
            }
            18 => self.spawn_external_mixer(),
            19 => self.toggle_window(event_loop, WindowKind::SetupDialog),
            20 => {
                let topic = self.context_help_topic_for_window(self.active_window_kind);
                self.open_help_topic(event_loop, topic);
            }
            21 => {
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_shuffle();
            }
            22 => {
                let mut state = self.shared_state.lock().unwrap();
                state.repeat_mode = (state.repeat_mode + 1) % 3;
            }
            23 => {
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_intro_scan();
            }
            24 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    return false;
                }
                self.file_man_dlg.is_copy = true;
                self.open_window(event_loop, WindowKind::FileManager);
            }
            25 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    return false;
                }
                self.file_man_dlg.is_copy = false;
                self.open_window(event_loop, WindowKind::FileManager);
            }
            26 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    // Im Modern Skin entfernt F7 direkt den aktuell gespielten/ausgewählten Eintrag aus der Playlist ohne Classic-Popup
                    let idx = self.shared_state.lock().unwrap().playlist_idx;
                    self.remove_playlist_item_and_update_cursor(idx);
                } else {
                    self.confirm_action = Some(ConfirmAction::RemoveFromPlaylist);
                    self.open_window(event_loop, WindowKind::ConfirmDialog);
                }
            }
            27 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    return false;
                }
                self.confirm_action = Some(ConfirmAction::DeleteFromDisk);
                self.open_window(event_loop, WindowKind::ConfirmDialog);
            }
            28 => {
                let mut state = self.shared_state.lock().unwrap();
                state.step_clip_declick(1);
                self.setup_cfg.clipping = state.ini_config.clipping;
                self.setup_cfg.declick = state.ini_config.declick;
            }
            29 => {
                let mut state = self.shared_state.lock().unwrap();
                state.sound_settings.no_bpm = !state.sound_settings.no_bpm;
                state.settings_dirty = true;
            }
            30 => {
                let mut state = self.shared_state.lock().unwrap();
                state.sound_settings.surround = !state.sound_settings.surround;
                if state.sound_settings.surround {
                    state.sound_settings.stereo = 2;
                }
                state.settings_dirty = true;
                state.sync_runtime_to_ini();
                self.setup_cfg.channels = state.ini_config.channels;
                self.setup_cfg.surround = state.ini_config.surround;
            }
            31 => {
                let mut state = self.shared_state.lock().unwrap();
                state.step_oversamp(1);
                self.setup_cfg.over_sampling = state.ini_config.over_sampling;
            }
            32 => {
                let mut state = self.shared_state.lock().unwrap();
                state.sound_settings.panning = !state.sound_settings.panning;
                state.settings_dirty = true;
                state.sync_runtime_to_ini();
                self.setup_cfg.enable_panning = state.ini_config.enable_panning;
            }
            33 => self.step_preamp_db(-2),
            34 => self.step_preamp_db(2),
            35 => {
                let max_inst = self.shared_state.lock().unwrap().instruments.len().max(1);
                if self.info_inst_offset + 31 < max_inst {
                    self.info_inst_offset += 31;
                }
            }
            36 => {
                self.info_inst_offset = self.info_inst_offset.saturating_sub(31);
            }
            37..=40 => return false,
            41 => {
                let max_inst = self.shared_state.lock().unwrap().instruments.len().max(1);
                self.selected_instrument = (self.selected_instrument + 1).min(max_inst);
            }
            42 => {
                self.selected_instrument = self.selected_instrument.saturating_sub(1).max(1);
            }
            43 => {
                let is_modern = {
                    let st = self.shared_state.lock().unwrap();
                    self.is_modern_skin(&st)
                };
                if is_modern {
                    return false;
                }
                self.open_window(event_loop, WindowKind::About);
            }
            44 => {
                self.cycle_skin_live();
            }
            _ => return false,
        }
        true
    }

    #[inline]
    pub(crate) fn mark_escape_consumed(&mut self) {
        self.suppress_escape_stop = true;
        self.last_escape_close_at = Some(std::time::Instant::now());
    }

    #[inline]
    pub(crate) fn is_escape_stop_suppressed(&self) -> bool {
        self.last_escape_close_at.is_some_and(|t| {
            let ms = t.elapsed().as_millis();
            (self.suppress_escape_stop && ms < 1500) || ms < 300
        })
    }

    pub(crate) fn handle_key_press(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        let is_esc_key = matches!(&event.logical_key, Key::Named(NamedKey::Escape));

        // 0. Wenn gerade eine F7/F8 Sicherheitsabfrage offen ist:
        if let Some(action) = self.confirm_action {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if event.repeat {
                return;
            }
            if action == ConfirmAction::DeleteFromDisk {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::Escape)
                    | Key::Named(NamedKey::Space) => {
                        self.confirm_action = None;
                        self.close_window(WindowKind::ConfirmDialog);
                    }
                    Key::Character(ch) => {
                        let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                        if c == 'O' {
                            self.confirm_action = None;
                            self.close_window(WindowKind::ConfirmDialog);
                        }
                    }
                    _ => {}
                }
                return;
            }
            match &event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    self.confirm_action = None;
                    self.close_window(WindowKind::ConfirmDialog);
                    self.execute_confirm_action(action);
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.confirm_action = None;
                    self.close_window(WindowKind::ConfirmDialog);
                    return;
                }
                Key::Character(ch) => {
                    let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                    if c == 'Y' || c == 'J' {
                        self.confirm_action = None;
                        self.close_window(WindowKind::ConfirmDialog);
                        self.execute_confirm_action(action);
                    } else if c == 'N' {
                        self.confirm_action = None;
                        self.close_window(WindowKind::ConfirmDialog);
                    }
                    return;
                }
                _ => return,
            }
        }

        // Kontextsensitives F1 (ohne Modifikatoren, außer während der Tastenzuweisung im Hotkey-Editor):
        // Öffnet den integrierten WinHelp-Viewer direkt beim Thema des aktiven Dialogfensters!
        if !event.repeat
            && matches!(&event.logical_key, Key::Named(NamedKey::F1))
            && !self.modifiers.state().control_key()
            && !self.modifiers.state().alt_key()
            && !self.modifiers.state().shift_key()
            && !(self.kind_to_id.contains_key(&WindowKind::HotkeyEditor) && self.hotkey_dlg.capturing)
        {
            let topic = self.context_help_topic_for_window(self.active_window_kind);
            self.open_help_topic(event_loop, topic);
            return;
        }

        // Wenn das Hilfe-Fenster (`HelpViewer`) geöffnet ist: Tastaturnavigation (Esc, Backspace, Pfeile, Bild auf/ab)
        if self.kind_to_id.contains_key(&WindowKind::HelpViewer) {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.mark_escape_consumed();
                    if self.help_dlg.popup_topic_id.is_some() {
                        self.help_dlg.popup_topic_id = None;
                    } else {
                        self.close_window(WindowKind::HelpViewer);
                    }
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    if self.help_dlg.popup_topic_id.is_some() {
                        self.help_dlg.popup_topic_id = None;
                    } else {
                        self.help_dlg.go_back();
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.help_dlg.scroll_by_lines(-1, &self.assets);
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.help_dlg.scroll_by_lines(1, &self.assets);
                    return;
                }
                Key::Named(NamedKey::PageUp) => {
                    self.help_dlg.scroll_by_lines(-15, &self.assets);
                    return;
                }
                Key::Named(NamedKey::PageDown) => {
                    self.help_dlg.scroll_by_lines(15, &self.assets);
                    return;
                }
                Key::Named(NamedKey::Home) => {
                    self.help_dlg.scroll_y = 0;
                    return;
                }
                _ => {}
            }
        }

        // 0_fsel. Wenn der Datei-/Verzeichnis-Auswahldialog (`FileSelectDialog` / `tbrowse` / `tDirbrowse`) offen ist:
        if self.kind_to_id.contains_key(&WindowKind::FileSelectDialog) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            let is_fb = self.fsel_dlg.is_file_browse();
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.close_window(WindowKind::FileSelectDialog);
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    if !is_fb || self.fsel_dlg.active_list == 1 {
                        if let Some(entry) = self.fsel_dlg.entries.get(self.fsel_dlg.cursor).cloned() {
                            match entry {
                                FileSelectEntry::Parent(p) | FileSelectEntry::Subdir(_, p) => {
                                    self.fsel_dlg.current_dir = p;
                                    self.fsel_dlg.refresh();
                                    return;
                                }
                                FileSelectEntry::Drive(drv) => {
                                    self.fsel_dlg.current_dir =
                                        std::path::PathBuf::from(format!("{}:\\", drv));
                                    self.fsel_dlg.refresh();
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                    self.commit_file_select_ok();
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    if is_fb {
                        self.fsel_dlg.active_list = if self.fsel_dlg.active_list == 0 { 1 } else { 0 };
                    }
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    if let Some(parent) = self.fsel_dlg.current_dir.parent().map(|p| p.to_path_buf())
                        && parent != self.fsel_dlg.current_dir
                    {
                        self.fsel_dlg.current_dir = parent;
                        self.fsel_dlg.refresh();
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    if is_fb && self.fsel_dlg.active_list == 0 {
                        self.fsel_dlg.file_cursor = self.fsel_dlg.file_cursor.saturating_sub(1);
                        if self.fsel_dlg.file_cursor < self.fsel_dlg.file_scroll {
                            self.fsel_dlg.file_scroll = self.fsel_dlg.file_cursor;
                        }
                    } else {
                        self.fsel_dlg.cursor = self.fsel_dlg.cursor.saturating_sub(1);
                        if self.fsel_dlg.cursor < self.fsel_dlg.scroll {
                            self.fsel_dlg.scroll = self.fsel_dlg.cursor;
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    if is_fb && self.fsel_dlg.active_list == 0 {
                        let max_idx = self.fsel_dlg.files.len().saturating_sub(1);
                        self.fsel_dlg.file_cursor = (self.fsel_dlg.file_cursor + 1).min(max_idx);
                        if self.fsel_dlg.file_cursor >= self.fsel_dlg.file_scroll + 13 {
                            self.fsel_dlg.file_scroll = self.fsel_dlg.file_cursor.saturating_sub(12);
                        }
                    } else {
                        let max_idx = self.fsel_dlg.entries.len().saturating_sub(1);
                        self.fsel_dlg.cursor = (self.fsel_dlg.cursor + 1).min(max_idx);
                        if self.fsel_dlg.cursor >= self.fsel_dlg.scroll + 10 {
                            self.fsel_dlg.scroll = self.fsel_dlg.cursor.saturating_sub(9);
                        }
                    }
                    return;
                }
                Key::Character(ch) if self.modifiers.state().alt_key() => {
                    let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                    match c {
                        'O' => self.commit_file_select_ok(),
                        'Q' | 'C' => self.close_window(WindowKind::FileSelectDialog),
                        'H' => self.open_help_topic(event_loop, "FselDlg"),
                        'F' if is_fb => self.fsel_dlg.active_list = 0,
                        'D' => self.fsel_dlg.active_list = 1,
                        _ => {}
                    }
                    return;
                }
                _ => return,
            }
        }

        // 0a. Wenn der Hotkey-Editor (`KEYSEL.PAS`) geöffnet ist:
        if self.kind_to_id.contains_key(&WindowKind::HotkeyEditor) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if self.hotkey_dlg.capturing {
                if matches!(&event.logical_key, Key::Named(NamedKey::Escape)) {
                    self.hotkey_dlg.capturing = false;
                    return;
                }
                if let Some((_vkey, kname)) = Self::key_event_to_vkey_and_name(event) {
                    let ctrl = self.modifiers.state().control_key();
                    let shift = self.modifiers.state().shift_key();
                    let alt = self.modifiers.state().alt_key();
                    let mut st_bits = 0u8;
                    if ctrl {
                        st_bits |= 1;
                    }
                    if shift {
                        st_bits |= 2;
                    }
                    if alt {
                        st_bits |= 4;
                    }
                    if is_forbidden_hotkey(&kname, st_bits) {
                        if let Ok(mut st) = self.shared_state.lock() {
                            st.set_status_banner("CTRL+A RESERVED");
                        }
                    } else if let Some(binding) =
                        self.hotkey_dlg.working_keys.get_mut(self.hotkey_dlg.cursor)
                    {
                        binding.key_name = kname;
                        binding.state = st_bits;
                    }
                    self.hotkey_dlg.capturing = false;
                }
                return;
            } else {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.hotkey_dlg.capturing = true;
                        return;
                    }
                    Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                        if let Some(binding) =
                            self.hotkey_dlg.working_keys.get_mut(self.hotkey_dlg.cursor)
                        {
                            binding.key_name = "NONE".to_string();
                            binding.state = 0;
                        }
                        return;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.close_window(WindowKind::HotkeyEditor);
                        return;
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        self.hotkey_dlg.cursor = self.hotkey_dlg.cursor.saturating_sub(1);
                        if self.hotkey_dlg.cursor < self.hotkey_dlg.scroll {
                            self.hotkey_dlg.scroll = self.hotkey_dlg.cursor;
                        }
                        return;
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        let max_idx = self.hotkey_dlg.working_keys.len().saturating_sub(1);
                        self.hotkey_dlg.cursor = (self.hotkey_dlg.cursor + 1).min(max_idx);
                        if self.hotkey_dlg.cursor >= self.hotkey_dlg.scroll + 9 {
                            self.hotkey_dlg.scroll = self.hotkey_dlg.cursor.saturating_sub(8);
                        }
                        return;
                    }
                    Key::Named(NamedKey::PageUp) => {
                        self.hotkey_dlg.cursor = self.hotkey_dlg.cursor.saturating_sub(8);
                        if self.hotkey_dlg.cursor < self.hotkey_dlg.scroll {
                            self.hotkey_dlg.scroll = self.hotkey_dlg.cursor;
                        }
                        return;
                    }
                    Key::Named(NamedKey::PageDown) => {
                        let max_idx = self.hotkey_dlg.working_keys.len().saturating_sub(1);
                        self.hotkey_dlg.cursor = (self.hotkey_dlg.cursor + 8).min(max_idx);
                        if self.hotkey_dlg.cursor >= self.hotkey_dlg.scroll + 9 {
                            self.hotkey_dlg.scroll = self.hotkey_dlg.cursor.saturating_sub(8);
                        }
                        return;
                    }
                    _ => return,
                }
            }
        }

        // 0b. Wenn das Archiv-Setup (`tarcopt` oder Parameter-Subdialog `tmorearc`) offen ist:
        if self.kind_to_id.contains_key(&WindowKind::ArchiveSetup) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if self.archive_dlg.more_arc_type.is_some() {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.commit_archive_more_subdialog();
                        return;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.archive_dlg.more_arc_type = None;
                        return;
                    }
                    Key::Named(NamedKey::Tab) => {
                        self.archive_dlg.more_edit_focus =
                            (self.archive_dlg.more_edit_focus + 1) % 3;
                        return;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        match self.archive_dlg.more_edit_focus {
                            0 => {
                                self.archive_dlg.more_cmd_extract.pop();
                            }
                            1 => {
                                self.archive_dlg.more_cmd_pack.pop();
                            }
                            _ => {
                                self.archive_dlg.more_cmd_del.pop();
                            }
                        }
                        return;
                    }
                    Key::Named(NamedKey::Space) => {
                        let target = match self.archive_dlg.more_edit_focus {
                            0 => &mut self.archive_dlg.more_cmd_extract,
                            1 => &mut self.archive_dlg.more_cmd_pack,
                            _ => &mut self.archive_dlg.more_cmd_del,
                        };
                        if target.len() < 80 {
                            target.push(' ');
                        }
                        return;
                    }
                    Key::Character(ch) => {
                        for c in ch.chars() {
                            if !c.is_control() {
                                let target = match self.archive_dlg.more_edit_focus {
                                    0 => &mut self.archive_dlg.more_cmd_extract,
                                    1 => &mut self.archive_dlg.more_cmd_pack,
                                    _ => &mut self.archive_dlg.more_cmd_del,
                                };
                                if target.len() < 80 {
                                    target.push(c);
                                }
                            }
                        }
                        return;
                    }
                    _ => return,
                }
            } else if let Some(arc_row) = self.archive_dlg.edit_focus {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Escape) => {
                        self.archive_dlg.edit_focus = None;
                        return;
                    }
                    Key::Named(NamedKey::Tab) => {
                        self.archive_dlg.edit_focus = Some((arc_row + 1) % 3);
                        return;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        let is_pack = self.archive_dlg.pack_mode[arc_row.min(2)];
                        let s = match (arc_row, is_pack) {
                            (0, false) => &mut self.setup_cfg.arj_unpack,
                            (0, true) => &mut self.setup_cfg.arj_pack,
                            (1, false) => &mut self.setup_cfg.zip_unpack,
                            (1, true) => &mut self.setup_cfg.zip_pack,
                            (_, false) => &mut self.setup_cfg.lha_unpack,
                            (_, true) => &mut self.setup_cfg.lha_pack,
                        };
                        s.pop();
                        return;
                    }
                    Key::Named(NamedKey::Space) => {
                        let is_pack = self.archive_dlg.pack_mode[arc_row.min(2)];
                        let s = match (arc_row, is_pack) {
                            (0, false) => &mut self.setup_cfg.arj_unpack,
                            (0, true) => &mut self.setup_cfg.arj_pack,
                            (1, false) => &mut self.setup_cfg.zip_unpack,
                            (1, true) => &mut self.setup_cfg.zip_pack,
                            (_, false) => &mut self.setup_cfg.lha_unpack,
                            (_, true) => &mut self.setup_cfg.lha_pack,
                        };
                        if s.len() < 120 {
                            s.push(' ');
                        }
                        return;
                    }
                    Key::Character(ch) => {
                        let is_pack = self.archive_dlg.pack_mode[arc_row.min(2)];
                        let s = match (arc_row, is_pack) {
                            (0, false) => &mut self.setup_cfg.arj_unpack,
                            (0, true) => &mut self.setup_cfg.arj_pack,
                            (1, false) => &mut self.setup_cfg.zip_unpack,
                            (1, true) => &mut self.setup_cfg.zip_pack,
                            (_, false) => &mut self.setup_cfg.lha_unpack,
                            (_, true) => &mut self.setup_cfg.lha_pack,
                        };
                        for c in ch.chars() {
                            if !c.is_control() && s.len() < 120 {
                                s.push(c);
                            }
                        }
                        return;
                    }
                    _ => return,
                }
            } else {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.commit_archive_setup_ok();
                        return;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.setup_cfg = self.setup_backup_cfg.clone();
                        self.close_window(WindowKind::ArchiveSetup);
                        return;
                    }
                    Key::Character(ch) if self.modifiers.state().alt_key() => {
                        let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                        match c {
                            'O' => self.commit_archive_setup_ok(),
                            'C' | 'Q' => {
                                self.setup_cfg = self.setup_backup_cfg.clone();
                                self.close_window(WindowKind::ArchiveSetup);
                            }
                            'H' => self.open_help_topic(event_loop, "ArcOptDlg"),
                            _ => {}
                        }
                        return;
                    }
                    _ => return,
                }
            }
        }

        // 0c. Wenn der Mini-File-Manager (`COPYWND.PAS`) offen ist (strikt modal):
        if self.kind_to_id.contains_key(&WindowKind::FileManager) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if self.file_man_dlg.editing_target {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.file_man_dlg.editing_target = false;
                        self.execute_file_manager_ok();
                        return;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.file_man_dlg.editing_target = false;
                        return;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.file_man_dlg.target_path_edit.pop();
                        return;
                    }
                    Key::Named(NamedKey::Space) => {
                        if self.file_man_dlg.target_path_edit.len() < 120 {
                            self.file_man_dlg.target_path_edit.push(' ');
                        }
                        return;
                    }
                    Key::Character(ch) => {
                        for c in ch.chars() {
                            if !c.is_control() && self.file_man_dlg.target_path_edit.len() < 120 {
                                self.file_man_dlg.target_path_edit.push(c);
                            }
                        }
                        return;
                    }
                    _ => return,
                }
            } else {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.execute_file_manager_ok();
                        return;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.close_window(WindowKind::FileManager);
                        return;
                    }
                    Key::Named(NamedKey::Tab) => {
                        self.file_man_dlg.editing_target = true;
                        return;
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        self.file_man_dlg.file_cursor =
                            self.file_man_dlg.file_cursor.saturating_sub(1);
                        if self.file_man_dlg.file_cursor < self.file_man_dlg.file_scroll {
                            self.file_man_dlg.file_scroll = self.file_man_dlg.file_cursor;
                        }
                        return;
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        let max_idx = self.file_man_dlg.files.len().saturating_sub(1);
                        self.file_man_dlg.file_cursor =
                            (self.file_man_dlg.file_cursor + 1).min(max_idx);
                        if self.file_man_dlg.file_cursor >= self.file_man_dlg.file_scroll + 13 {
                            self.file_man_dlg.file_scroll =
                                self.file_man_dlg.file_cursor.saturating_sub(12);
                        }
                        return;
                    }
                    Key::Named(NamedKey::Space) | Key::Named(NamedKey::Insert) => {
                        let cur = self.file_man_dlg.file_cursor;
                        if let Some(f) = self.file_man_dlg.files.get_mut(cur) {
                            f.selected = !f.selected;
                        }
                        self.file_man_dlg.sync_target_edit();
                        return;
                    }
                    Key::Character(ch)
                        if self.modifiers.state().control_key()
                            && !self.modifiers.state().alt_key()
                            && ch.eq_ignore_ascii_case("a") =>
                    {
                        for f in &mut self.file_man_dlg.files {
                            f.selected = true;
                        }
                        self.file_man_dlg.sync_target_edit();
                        return;
                    }
                    Key::Character(ch) if self.modifiers.state().alt_key() => {
                        let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                        match c {
                            'O' => self.execute_file_manager_ok(),
                            'C' | 'Q' => self.close_window(WindowKind::FileManager),
                            'T' => self.file_man_dlg.editing_target = true,
                            _ => {}
                        }
                        return;
                    }
                    _ => return,
                }
            }
        }

        // 0d. Wenn das `TAskWin`-Fenster (Direct-to-Disk .WAV Dateiname) offen ist:
        if self.kind_to_id.contains_key(&WindowKind::WavAskDialog) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            match &event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    self.commit_wav_ask_ok();
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.close_window(WindowKind::WavAskDialog);
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    self.wav_ask_dlg.focus = match self.wav_ask_dlg.focus {
                        WavAskEditFocus::Path => WavAskEditFocus::Filename,
                        WavAskEditFocus::Filename => WavAskEditFocus::Path,
                    };
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    match self.wav_ask_dlg.focus {
                        WavAskEditFocus::Path => {
                            self.wav_ask_dlg.path.pop();
                        }
                        WavAskEditFocus::Filename => {
                            self.wav_ask_dlg.filename.pop();
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::Space) => {
                    match self.wav_ask_dlg.focus {
                        WavAskEditFocus::Path => {
                            if self.wav_ask_dlg.path.len() < 120 {
                                self.wav_ask_dlg.path.push(' ');
                            }
                        }
                        WavAskEditFocus::Filename => {
                            if self.wav_ask_dlg.filename.len() < 8 {
                                self.wav_ask_dlg.filename.push(' ');
                            }
                        }
                    }
                    return;
                }
                Key::Character(ch) => {
                    for c in ch.chars() {
                        if !c.is_control() {
                            match self.wav_ask_dlg.focus {
                                WavAskEditFocus::Path => {
                                    if self.wav_ask_dlg.path.len() < 120 {
                                        self.wav_ask_dlg.path.push(c);
                                    }
                                }
                                WavAskEditFocus::Filename => {
                                    if self.wav_ask_dlg.filename.len() < 8
                                        && !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                                    {
                                        self.wav_ask_dlg.filename.push(c.to_ascii_uppercase());
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        // 0e. Wenn im Setup-Dialog ein Edit-Feld (`RecPathEdit` oder `MixPathEdit`) fokussiert ist:
        if self.kind_to_id.contains_key(&WindowKind::SetupDialog)
            && self.setup_edit_focus != SetupEditFocus::None
        {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            match &event.logical_key {
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Escape) => {
                    self.setup_edit_focus = SetupEditFocus::None;
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    self.setup_edit_focus = match self.setup_edit_focus {
                        SetupEditFocus::RecPath => SetupEditFocus::MixPath,
                        SetupEditFocus::MixPath => SetupEditFocus::RecPath,
                        SetupEditFocus::None => SetupEditFocus::None,
                    };
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    match self.setup_edit_focus {
                        SetupEditFocus::RecPath => {
                            self.setup_cfg.hd_path.pop();
                        }
                        SetupEditFocus::MixPath => {
                            self.setup_cfg.mixer_path.pop();
                        }
                        SetupEditFocus::None => {}
                    }
                    return;
                }
                Key::Named(NamedKey::Space) => {
                    match self.setup_edit_focus {
                        SetupEditFocus::RecPath => {
                            if self.setup_cfg.hd_path.len() < 120 {
                                self.setup_cfg.hd_path.push(' ');
                            }
                        }
                        SetupEditFocus::MixPath => {
                            if self.setup_cfg.mixer_path.len() < 120 {
                                self.setup_cfg.mixer_path.push(' ');
                            }
                        }
                        SetupEditFocus::None => {}
                    }
                    return;
                }
                Key::Character(ch) => {
                    for c in ch.chars() {
                        if !c.is_control() {
                            match self.setup_edit_focus {
                                SetupEditFocus::RecPath => {
                                    if self.setup_cfg.hd_path.len() < 120 {
                                        self.setup_cfg.hd_path.push(c);
                                    }
                                }
                                SetupEditFocus::MixPath => {
                                    if self.setup_cfg.mixer_path.len() < 120 {
                                        self.setup_cfg.mixer_path.push(c);
                                    }
                                }
                                SetupEditFocus::None => {}
                            }
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        // 0e2. Wenn der Setup-Dialog (`SetupDialog`, ohne fokussiertes Edit-Feld) offen ist (strikt modal):
        if self.kind_to_id.contains_key(&WindowKind::SetupDialog) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.cancel_setup_dialog();
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    self.commit_setup_dialog(false);
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    let is_mod = {
                        let st = self.shared_state.lock().unwrap();
                        self.is_modern_skin(&st)
                    };
                    if !is_mod {
                        self.setup_edit_focus = SetupEditFocus::RecPath;
                    }
                    return;
                }
                Key::Character(ch) if self.modifiers.state().alt_key() => {
                    let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                    match c {
                        'O' => self.commit_setup_dialog(false),
                        'S' => self.commit_setup_dialog(true),
                        'C' | 'Q' => self.cancel_setup_dialog(),
                        'H' => self.open_help_topic(event_loop, "SetupDlg"),
                        'K' => self.open_window(event_loop, WindowKind::HotkeyEditor),
                        'A' | 'P' => self.open_window(event_loop, WindowKind::ArchiveSetup),
                        _ => {}
                    }
                    return;
                }
                _ => return,
            }
        }

        // 0e3. Wenn das About-Fenster (`About`) offen ist (modal: Escape/Enter/Space schließt):
        if self.kind_to_id.contains_key(&WindowKind::About) {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if !event.repeat
                && matches!(
                    &event.logical_key,
                    Key::Named(NamedKey::Escape)
                        | Key::Named(NamedKey::Enter)
                        | Key::Named(NamedKey::Space)
                )
            {
                self.close_window(WindowKind::About);
            }
            return;
        }

        // 0e4. Wenn `EffectsHelp` aktiv ist: Escape schließt
        if self.kind_to_id.contains_key(&WindowKind::EffectsHelp) && is_esc_key {
            self.mark_escape_consumed();
            if !event.repeat {
                self.close_window(WindowKind::EffectsHelp);
            }
            return;
        }

        // Wenn im Open-Dialog gerade der .MOL-Dateiname oder die Wildcard-Auswahl-Maske getippt wird:
        if self.kind_to_id.contains_key(&WindowKind::OpenDialog)
            && (self.open_dlg.saving_mol
                || self.open_dlg.editing_filter
                || self.open_dlg.editing_right_filter)
        {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            match &event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    if self.open_dlg.saving_mol {
                        self.save_current_mol_playlist();
                    } else {
                        self.open_dlg.apply_selection_mask();
                    }
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.open_dlg.saving_mol = false;
                    self.open_dlg.editing_filter = false;
                    self.open_dlg.editing_right_filter = false;
                    self.open_dlg.filter_fresh = false;
                    self.open_dlg.filter_mask = "*.*".to_string();
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    if self.open_dlg.saving_mol {
                        self.open_dlg.save_mol_name.pop();
                    } else if self.open_dlg.filter_fresh {
                        self.open_dlg.filter_mask.clear();
                        self.open_dlg.filter_fresh = false;
                    } else {
                        self.open_dlg.filter_mask.pop();
                    }
                    return;
                }
                Key::Named(NamedKey::Space) => {
                    if self.open_dlg.saving_mol {
                        if self.open_dlg.save_mol_name.len() < 32 {
                            self.open_dlg.save_mol_name.push(' ');
                        }
                    } else if self.open_dlg.editing_filter || self.open_dlg.editing_right_filter {
                        if self.open_dlg.filter_fresh {
                            self.open_dlg.filter_mask.clear();
                            self.open_dlg.filter_fresh = false;
                        }
                        if self.open_dlg.filter_mask.len() < 16 {
                            self.open_dlg.filter_mask.push(' ');
                        }
                    }
                    return;
                }
                Key::Character(ch) => {
                    for c in ch.chars() {
                        if !c.is_control() {
                            if self.open_dlg.saving_mol && self.open_dlg.save_mol_name.len() < 32 {
                                self.open_dlg.save_mol_name.push(c.to_ascii_uppercase());
                            } else if self.open_dlg.editing_filter
                                || self.open_dlg.editing_right_filter
                            {
                                // Drückt man während offener `+`-Maske nochmals `+` (oder bei `-`-Maske nochmals `-`),
                                // wird die Maske sofort angewendet – exakt wie im Original!
                                if c == '+' && !self.open_dlg.filter_deselect {
                                    self.open_dlg.apply_selection_mask();
                                    return;
                                }
                                if c == '-' && self.open_dlg.filter_deselect {
                                    self.open_dlg.apply_selection_mask();
                                    return;
                                }
                                if self.open_dlg.filter_fresh {
                                    self.open_dlg.filter_mask.clear();
                                    self.open_dlg.filter_fresh = false;
                                }
                                if self.open_dlg.filter_mask.len() < 16 {
                                    self.open_dlg.filter_mask.push(c.to_ascii_uppercase());
                                }
                            }
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        let shift = self.modifiers.state().shift_key();
        let ctrl = self.modifiers.state().control_key();
        let alt = self.modifiers.state().alt_key();
        let (classic_input, is_modern) = {
            let st = self.shared_state.lock().unwrap();
            (st.classic_input, self.is_modern_skin(&st))
        };

        // 0_open. Classic Open-Dialog (`newopen.pas`): strikt modal mit vollständiger Original-Tastatursteuerung!
        if self.kind_to_id.contains_key(&WindowKind::OpenDialog) && !is_modern {
            if is_esc_key {
                self.mark_escape_consumed();
            }
            if !classic_input
                && ctrl
                && !shift
                && !alt
                && let Key::Character(ch) = &event.logical_key
                && ch.eq_ignore_ascii_case("a")
            {
                if self.open_dlg.active_list == 2 {
                    for p in &mut self.open_dlg.right_playlist {
                        p.selected = true;
                    }
                    self.open_dlg.status_msg = format!(
                        "Selected all {} entries in Playlist.",
                        self.open_dlg.right_playlist.len()
                    );
                } else {
                    for f in &mut self.open_dlg.left_files {
                        f.selected = true;
                    }
                    self.open_dlg.status_msg = format!(
                        "Selected all {} files in Filelist.",
                        self.open_dlg.left_files.len()
                    );
                }
                return;
            }

            // Alt-Mnemonics & Alt+F1/F2/F3 (`newopen.pas` Zeilen 2036-2065)
            if alt && !ctrl {
                match &event.logical_key {
                    Key::Named(NamedKey::F1) => {
                        self.open_dlg.active_list = 0;
                        self.open_dlg.open_selection_mask(false, false);
                        return;
                    }
                    Key::Named(NamedKey::F2) => {
                        self.open_dlg.focus_current_drive();
                        return;
                    }
                    Key::Named(NamedKey::F3) => {
                        self.open_dlg.active_list = 2;
                        self.open_dlg.open_selection_mask(true, false);
                        return;
                    }
                    Key::Character(ch) => {
                        let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                        match c {
                            'F' => self.open_dlg.active_list = 0,
                            'D' => self.open_dlg.active_list = 1,
                            'P' => self.open_dlg.active_list = 2,
                            'O' => {
                                if self.open_dlg.right_playlist.is_empty()
                                    && self.open_dlg.active_list == 0
                                    && !self.open_dlg.left_files.is_empty()
                                {
                                    self.open_dlg.add_selected_to_playlist();
                                }
                                self.commit_open_dialog_ok();
                            }
                            'S' => {
                                self.open_dlg.saving_mol = true;
                                self.open_dlg.editing_filter = false;
                                self.open_dlg.editing_right_filter = false;
                            }
                            'C' | 'Q' => {
                                self.open_dlg.saving_mol = false;
                                self.open_dlg.editing_filter = false;
                                self.open_dlg.editing_right_filter = false;
                                self.close_window(WindowKind::OpenDialog);
                            }
                            'H' => self.open_help_topic(event_loop, "OpenDlg"),
                            _ => {}
                        }
                        return;
                    }
                    _ => return,
                }
            }

            // Numpad `+` / `-` / `*` für Auswahlmaske / Invertieren
            if !ctrl && !alt
                && let PhysicalKey::Code(code) = event.physical_key
            {
                let right_list = self.open_dlg.active_list == 2;
                match code {
                    KeyCode::NumpadAdd => {
                        self.open_dlg.open_selection_mask(right_list, false);
                        return;
                    }
                    KeyCode::NumpadSubtract => {
                        self.open_dlg.open_selection_mask(right_list, true);
                        return;
                    }
                    KeyCode::NumpadMultiply => {
                        if right_list {
                            for p in &mut self.open_dlg.right_playlist {
                                p.selected = !p.selected;
                            }
                        } else {
                            for f in &mut self.open_dlg.left_files {
                                f.selected = !f.selected;
                            }
                        }
                        return;
                    }
                    _ => {}
                }
            }

            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.open_dlg.saving_mol = false;
                    self.open_dlg.editing_filter = false;
                    self.open_dlg.editing_right_filter = false;
                    self.close_window(WindowKind::OpenDialog);
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    if shift {
                        self.open_dlg.active_list = match self.open_dlg.active_list {
                            0 => 2,
                            1 => 0,
                            _ => 1,
                        };
                    } else {
                        self.open_dlg.active_list = (self.open_dlg.active_list + 1) % 3;
                    }
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    if self.open_dlg.active_list == 1 {
                        let idx = self.open_dlg.mid_cursor;
                        self.navigate_open_dir_item(idx);
                    } else {
                        if self.open_dlg.right_playlist.is_empty()
                            && self.open_dlg.active_list == 0
                            && !self.open_dlg.left_files.is_empty()
                        {
                            self.open_dlg.add_selected_to_playlist();
                        }
                        self.commit_open_dialog_ok();
                    }
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    self.open_dlg.navigate_parent_dir();
                    return;
                }
                Key::Named(NamedKey::F5) => {
                    match self.open_dlg.active_list {
                        0 => self.open_dlg.add_selected_to_playlist(),
                        1 => self.open_dlg.append_highlighted_dir_to_playlist(),
                        _ => self.open_dlg.remove_selected_from_playlist(),
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowRight) => {
                    if ctrl {
                        self.open_dlg.add_all_to_playlist();
                    } else {
                        self.open_dlg.add_selected_to_playlist();
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    if ctrl {
                        self.open_dlg.clear_right_playlist();
                    } else {
                        self.open_dlg.remove_selected_from_playlist();
                    }
                    return;
                }
                Key::Named(NamedKey::Delete) => {
                    if self.open_dlg.active_list == 2 {
                        self.open_dlg.remove_selected_from_playlist();
                    }
                    return;
                }
                Key::Named(NamedKey::Space) | Key::Named(NamedKey::Insert) => {
                    match self.open_dlg.active_list {
                        0 => {
                            let len = self.open_dlg.left_files.len();
                            if len > 0 {
                                let cur = self.open_dlg.left_cursor.min(len - 1);
                                self.open_dlg.left_files[cur].selected =
                                    !self.open_dlg.left_files[cur].selected;
                                self.open_dlg.left_anchor = cur;
                                if cur + 1 < len {
                                    self.open_dlg.left_cursor = cur + 1;
                                    if self.open_dlg.left_cursor >= self.open_dlg.left_scroll + 20 {
                                        self.open_dlg.left_scroll =
                                            self.open_dlg.left_cursor.saturating_sub(19);
                                    }
                                }
                            }
                        }
                        2 => {
                            let len = self.open_dlg.right_playlist.len();
                            if len > 0 {
                                let cur = self.open_dlg.right_cursor.min(len - 1);
                                self.open_dlg.right_playlist[cur].selected =
                                    !self.open_dlg.right_playlist[cur].selected;
                                self.open_dlg.right_anchor = cur;
                                if cur + 1 < len {
                                    self.open_dlg.right_cursor = cur + 1;
                                    if self.open_dlg.right_cursor
                                        >= self.open_dlg.right_scroll + 20
                                    {
                                        self.open_dlg.right_scroll =
                                            self.open_dlg.right_cursor.saturating_sub(19);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    match self.open_dlg.active_list {
                        0 => {
                            let next = self.open_dlg.left_cursor.saturating_sub(1);
                            self.open_dlg.left_cursor = next;
                            if shift
                                && let Some(f) = self.open_dlg.left_files.get_mut(next)
                            {
                                f.selected = true;
                            }
                            if next < self.open_dlg.left_scroll {
                                self.open_dlg.left_scroll = next;
                            }
                        }
                        1 => {
                            let next = self.open_dlg.mid_cursor.saturating_sub(1);
                            self.open_dlg.mid_cursor = next;
                            if next < self.open_dlg.mid_scroll {
                                self.open_dlg.mid_scroll = next;
                            }
                        }
                        _ => {
                            let next = self.open_dlg.right_cursor.saturating_sub(1);
                            self.open_dlg.right_cursor = next;
                            if shift
                                && let Some(p) = self.open_dlg.right_playlist.get_mut(next)
                            {
                                p.selected = true;
                            }
                            if next < self.open_dlg.right_scroll {
                                self.open_dlg.right_scroll = next;
                            }
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    match self.open_dlg.active_list {
                        0 => {
                            let max_idx = self.open_dlg.left_files.len().saturating_sub(1);
                            let next = (self.open_dlg.left_cursor + 1).min(max_idx);
                            self.open_dlg.left_cursor = next;
                            if shift
                                && let Some(f) = self.open_dlg.left_files.get_mut(next)
                            {
                                f.selected = true;
                            }
                            if next >= self.open_dlg.left_scroll + 20 {
                                self.open_dlg.left_scroll = next.saturating_sub(19);
                            }
                        }
                        1 => {
                            let max_idx = self.open_dlg.mid_dirs.len().saturating_sub(1);
                            let next = (self.open_dlg.mid_cursor + 1).min(max_idx);
                            self.open_dlg.mid_cursor = next;
                            if next >= self.open_dlg.mid_scroll + 13 {
                                self.open_dlg.mid_scroll = next.saturating_sub(12);
                            }
                        }
                        _ => {
                            let max_idx = self.open_dlg.right_playlist.len().saturating_sub(1);
                            let next = (self.open_dlg.right_cursor + 1).min(max_idx);
                            self.open_dlg.right_cursor = next;
                            if shift
                                && let Some(p) = self.open_dlg.right_playlist.get_mut(next)
                            {
                                p.selected = true;
                            }
                            if next >= self.open_dlg.right_scroll + 20 {
                                self.open_dlg.right_scroll = next.saturating_sub(19);
                            }
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::PageUp) => {
                    if ctrl {
                        self.open_dlg.navigate_parent_dir();
                        return;
                    }
                    match self.open_dlg.active_list {
                        0 => {
                            let next = self.open_dlg.left_cursor.saturating_sub(19);
                            self.open_dlg.left_cursor = next;
                            self.open_dlg.left_scroll =
                                self.open_dlg.left_scroll.saturating_sub(19).min(next);
                        }
                        1 => {
                            let next = self.open_dlg.mid_cursor.saturating_sub(12);
                            self.open_dlg.mid_cursor = next;
                            self.open_dlg.mid_scroll =
                                self.open_dlg.mid_scroll.saturating_sub(12).min(next);
                        }
                        _ => {
                            let next = self.open_dlg.right_cursor.saturating_sub(19);
                            self.open_dlg.right_cursor = next;
                            self.open_dlg.right_scroll =
                                self.open_dlg.right_scroll.saturating_sub(19).min(next);
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::PageDown) => {
                    if ctrl {
                        if self.open_dlg.active_list == 1 {
                            let idx = self.open_dlg.mid_cursor;
                            self.navigate_open_dir_item(idx);
                        }
                        return;
                    }
                    match self.open_dlg.active_list {
                        0 => {
                            let max_idx = self.open_dlg.left_files.len().saturating_sub(1);
                            let next = (self.open_dlg.left_cursor + 19).min(max_idx);
                            self.open_dlg.left_cursor = next;
                            let max_s = self.open_dlg.left_files.len().saturating_sub(20);
                            self.open_dlg.left_scroll =
                                (self.open_dlg.left_scroll + 19).min(max_s);
                            if next >= self.open_dlg.left_scroll + 20 {
                                self.open_dlg.left_scroll = next.saturating_sub(19);
                            }
                        }
                        1 => {
                            let max_idx = self.open_dlg.mid_dirs.len().saturating_sub(1);
                            let next = (self.open_dlg.mid_cursor + 12).min(max_idx);
                            self.open_dlg.mid_cursor = next;
                            let max_s = self.open_dlg.mid_dirs.len().saturating_sub(13);
                            self.open_dlg.mid_scroll = (self.open_dlg.mid_scroll + 12).min(max_s);
                            if next >= self.open_dlg.mid_scroll + 13 {
                                self.open_dlg.mid_scroll = next.saturating_sub(12);
                            }
                        }
                        _ => {
                            let max_idx = self.open_dlg.right_playlist.len().saturating_sub(1);
                            let next = (self.open_dlg.right_cursor + 19).min(max_idx);
                            self.open_dlg.right_cursor = next;
                            let max_s = self.open_dlg.right_playlist.len().saturating_sub(20);
                            self.open_dlg.right_scroll =
                                (self.open_dlg.right_scroll + 19).min(max_s);
                            if next >= self.open_dlg.right_scroll + 20 {
                                self.open_dlg.right_scroll = next.saturating_sub(19);
                            }
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::Home) => {
                    match self.open_dlg.active_list {
                        0 => {
                            self.open_dlg.left_cursor = 0;
                            self.open_dlg.left_scroll = 0;
                        }
                        1 => {
                            self.open_dlg.mid_cursor = 0;
                            self.open_dlg.mid_scroll = 0;
                        }
                        _ => {
                            self.open_dlg.right_cursor = 0;
                            self.open_dlg.right_scroll = 0;
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::End) => {
                    match self.open_dlg.active_list {
                        0 => {
                            let len = self.open_dlg.left_files.len();
                            if len > 0 {
                                self.open_dlg.left_cursor = len - 1;
                                self.open_dlg.left_scroll = len.saturating_sub(20);
                            }
                        }
                        1 => {
                            let len = self.open_dlg.mid_dirs.len();
                            if len > 0 {
                                self.open_dlg.mid_cursor = len - 1;
                                self.open_dlg.mid_scroll = len.saturating_sub(13);
                            }
                        }
                        _ => {
                            let len = self.open_dlg.right_playlist.len();
                            if len > 0 {
                                self.open_dlg.right_cursor = len - 1;
                                self.open_dlg.right_scroll = len.saturating_sub(20);
                            }
                        }
                    }
                    return;
                }
                Key::Character(ch) => {
                    if ctrl && ch == "\\" {
                        self.open_dlg.navigate_root_dir();
                        return;
                    }
                    if !ctrl && !alt {
                        let right_list = self.open_dlg.active_list == 2;
                        if ch == "+" {
                            self.open_dlg.open_selection_mask(right_list, false);
                            return;
                        } else if ch == "-" {
                            self.open_dlg.open_selection_mask(right_list, true);
                            return;
                        } else if ch == "*" {
                            if right_list {
                                for p in &mut self.open_dlg.right_playlist {
                                    p.selected = !p.selected;
                                }
                            } else {
                                for f in &mut self.open_dlg.left_files {
                                    f.selected = !f.selected;
                                }
                            }
                            return;
                        }
                        if let Some(c) = ch.chars().next()
                            && !c.is_control()
                        {
                            self.open_dlg.type_ahead_jump(c);
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        // Modernes `Strg + A` zum Auswählen aller Einträge im Playlist-Dock / QuickSelect:
        let is_modern_playlist_open = is_modern
            && (self.kind_to_id.contains_key(&WindowKind::QuickSelect)
                || self.kind_to_id.contains_key(&WindowKind::OpenDialog));
        let is_modern_playlist_focused = is_modern_playlist_open
            && matches!(
                self.active_window_kind,
                WindowKind::QuickSelect | WindowKind::OpenDialog
            );
        let is_classic_quick_select =
            !is_modern && self.kind_to_id.contains_key(&WindowKind::QuickSelect);

        if !classic_input && ctrl && !shift && !alt
            && let Key::Character(ch) = &event.logical_key
            && ch.eq_ignore_ascii_case("a")
        {
            if is_modern_playlist_open || is_classic_quick_select {
                let len = self.shared_state.lock().unwrap().playlist.len();
                let has_items = len > 0;
                self.quick_select_all_selected = has_items;
                if is_modern_playlist_open {
                    self.playlist_selected = (0..len).collect();
                }
            }
            // `Strg + A` ist systemweit reserviert und darf niemals das About-Fenster (`A`) öffnen!
            return;
        }

        // Classic Quick Selection (`MULBOX.PAS`, modal!) oder fokussiertes Modern Playlist-Dock:
        if is_classic_quick_select || is_modern_playlist_focused {
            let sync_qs_pos = |app: &mut Self, new_vis_pos: usize, total_len: usize| {
                if is_modern_playlist_focused {
                    let max_vis = app.modern_playlist_rows.clamp(5, 36);
                    let max_scroll = total_len.saturating_sub(max_vis);
                    if app.quick_select_scroll > max_scroll {
                        app.quick_select_scroll = max_scroll;
                    }
                    if new_vis_pos < app.quick_select_scroll {
                        app.quick_select_scroll = new_vis_pos;
                    } else if new_vis_pos >= app.quick_select_scroll + max_vis {
                        app.quick_select_scroll = new_vis_pos + 1 - max_vis;
                    }
                } else {
                    app.ensure_classic_quick_select_visible(new_vis_pos, total_len);
                }
            };

            // Alt-Mnemonics für Sortierung (`MULBOX.PAS` Zeilen 332-354: Alt+N / Alt+O = Order, Alt+A = Name, Alt+E = Ext)
            if alt && !ctrl
                && let Key::Character(ch) = &event.logical_key
            {
                let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                match c {
                    'N' | 'O' => self.quick_select_sort = 0,
                    'A' => self.quick_select_sort = 1,
                    'E' => self.quick_select_sort = 2,
                    _ => {}
                }
                return;
            }

            match &event.logical_key {
                Key::Named(NamedKey::Delete) | Key::Named(NamedKey::F7) => {
                    if is_modern_playlist_focused {
                        self.remove_selected_modern_playlist_items();
                        return;
                    }
                    if self.quick_select_all_selected {
                        {
                            let mut st = self.shared_state.lock().unwrap();
                            st.clear_playlist();
                            let _ = st
                                .ini_config
                                .save_to_file(&Mod4WinIniConfig::default_ini_path());
                        }
                        self.quick_select_all_selected = false;
                        self.quick_select_cursor = 0;
                        self.quick_select_scroll = 0;
                    } else {
                        self.remove_playlist_item_and_update_cursor(self.quick_select_cursor);
                    }
                    if is_classic_quick_select {
                        let new_len = self.shared_state.lock().unwrap().playlist.len();
                        let (qw, qh, ..) = self.classic_quick_select_layout(new_len);
                        let scale = self.scale_factor;
                        if let Some(&win_id) = self.kind_to_id.get(&WindowKind::QuickSelect)
                            && let Some(surf) = self.windows.get_mut(&win_id)
                        {
                            surf.width = qw;
                            surf.height = qh;
                            let phys_w = ((qw as f64) * scale).round().max(1.0) as u32;
                            let phys_h = ((qh as f64) * scale).round().max(1.0) as u32;
                            let _ = surf.pixels.resize_buffer(qw, qh);
                            let _ = surf.pixels.resize_surface(phys_w, phys_h);
                            let _ = surf.window.request_inner_size(
                                super::winit::dpi::PhysicalSize::new(phys_w, phys_h),
                            );
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    self.quick_select_all_selected = false;
                    let orig_idx = self.quick_select_cursor;
                    if is_modern_playlist_focused {
                        self.playlist_selected.clear();
                        self.playlist_selected.insert(orig_idx);
                        self.playlist_anchor = orig_idx;
                    }
                    {
                        let mut state = self.shared_state.lock().unwrap();
                        if orig_idx < state.playlist.len() {
                            state.playlist_idx = orig_idx;
                            state.playing_track_detached = false;
                            state.rebuild_number_list();
                            if let Some(pos) = state.number_list.iter().position(|&i| i == orig_idx) {
                                state.playlist_pos = pos;
                            }
                            let path = state.playlist[orig_idx].clone();
                            state.pending_load_file = Some(path);
                            state.is_playing = true;
                            state.is_paused = false;
                        }
                    }
                    if is_classic_quick_select {
                        self.close_window(WindowKind::QuickSelect);
                    }
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.mark_escape_consumed();
                    self.quick_select_all_selected = false;
                    self.playlist_selected.clear();
                    self.playlist_reorder_drag = None;
                    self.close_window(WindowKind::QuickSelect);
                    if is_modern_playlist_focused {
                        self.close_window(WindowKind::OpenDialog);
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowUp)
                | Key::Named(NamedKey::ArrowDown)
                | Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::ArrowRight)
                | Key::Named(NamedKey::PageUp)
                | Key::Named(NamedKey::PageDown)
                | Key::Named(NamedKey::Home)
                | Key::Named(NamedKey::End) => {
                    self.quick_select_all_selected = false;
                    let items = {
                        let st = self.shared_state.lock().unwrap();
                        self.sorted_quick_select_items(&st)
                    };
                    if items.is_empty() {
                        return;
                    }
                    let total_len = items.len();
                    let cur_pos = items
                        .iter()
                        .position(|(orig_idx, _)| *orig_idx == self.quick_select_cursor)
                        .unwrap_or(0);
                    let col_step = if is_modern_playlist_focused {
                        self.modern_playlist_rows.clamp(5, 36)
                    } else {
                        let (_, _, vis_rows, vis_cols, ..) =
                            self.classic_quick_select_layout(total_len);
                        match &event.logical_key {
                            Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown) => {
                                (vis_rows * vis_cols).max(1)
                            }
                            _ => vis_rows.max(1),
                        }
                    };
                    let new_pos = match &event.logical_key {
                        Key::Named(NamedKey::ArrowUp) => cur_pos.saturating_sub(1),
                        Key::Named(NamedKey::ArrowDown) => (cur_pos + 1).min(total_len - 1),
                        Key::Named(NamedKey::ArrowLeft) if is_classic_quick_select => {
                            cur_pos.saturating_sub(col_step)
                        }
                        Key::Named(NamedKey::ArrowRight) if is_classic_quick_select => {
                            (cur_pos + col_step).min(total_len - 1)
                        }
                        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight) => {
                            cur_pos
                        }
                        Key::Named(NamedKey::PageUp) => cur_pos.saturating_sub(col_step),
                        Key::Named(NamedKey::PageDown) => (cur_pos + col_step).min(total_len - 1),
                        Key::Named(NamedKey::Home) => 0,
                        Key::Named(NamedKey::End) => total_len - 1,
                        _ => cur_pos,
                    };
                    if is_modern_playlist_focused
                        && matches!(
                            &event.logical_key,
                            Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight)
                        )
                    {
                        // Lass Links/Rechts im Modern Playlist-Dock zum Player-Seek durch!
                    } else {
                        let new_orig = items[new_pos].0;
                        self.quick_select_cursor = new_orig;
                        if is_modern_playlist_focused {
                            if shift {
                                let anchor_pos = items
                                    .iter()
                                    .position(|(idx, _)| *idx == self.playlist_anchor)
                                    .unwrap_or(cur_pos);
                                self.playlist_selected.clear();
                                let a = anchor_pos.min(new_pos);
                                let b = anchor_pos.max(new_pos);
                                for i in a..=b {
                                    self.playlist_selected.insert(items[i].0);
                                }
                            } else {
                                self.playlist_selected.clear();
                                self.playlist_selected.insert(new_orig);
                                self.playlist_anchor = new_orig;
                            }
                        }
                        sync_qs_pos(self, new_pos, total_len);
                        return;
                    }
                }
                Key::Character(ch) if !ctrl && !alt => {
                    if let Some(c) = ch.chars().next()
                        && c.is_ascii_alphanumeric()
                    {
                        let target_ch = c.to_ascii_uppercase();
                        let items = {
                            let st = self.shared_state.lock().unwrap();
                            self.sorted_quick_select_items(&st)
                        };
                        if !items.is_empty() {
                            let cur_pos = items
                                .iter()
                                .position(|(orig_idx, _)| *orig_idx == self.quick_select_cursor)
                                .unwrap_or(0);
                            let start = (cur_pos + 1) % items.len();
                            if let Some(found_pos) = (0..items.len()).find_map(|off| {
                                let i = (start + off) % items.len();
                                let fname = items[i].1.to_ascii_uppercase();
                                fname.starts_with(target_ch).then_some(i)
                            }) {
                                self.quick_select_all_selected = false;
                                self.quick_select_cursor = items[found_pos].0;
                                sync_qs_pos(self, found_pos, items.len());
                                return;
                            }
                        }
                    }
                    if is_classic_quick_select {
                        return;
                    }
                }
                _ => {
                    if is_classic_quick_select {
                        return;
                    }
                }
            }
        }

        // 0f. Wenn Module-Info oder Instrument-Info geöffnet ist:
        //     - Tasten `1..9` (ohne Ctrl/Shift/Alt und wenn Effects nicht offen ist) wählen Instrument `1..9` und öffnen Instrument-Info (`DETAILS.PAS`).
        //     - Numpad `+` / `-` schaltet das Instrument in Instrument-Info vor/zurück bzw. blättert in Module-Info.
        let info_open = self.kind_to_id.contains_key(&WindowKind::ModuleInfo);
        let inst_open = self.kind_to_id.contains_key(&WindowKind::InstrumentInfo);
        let eff_open = self.kind_to_id.contains_key(&WindowKind::Effects);

        if (info_open || inst_open) && !ctrl && !shift && !alt
            && let PhysicalKey::Code(code) = event.physical_key {
                match code {
                    KeyCode::NumpadAdd => {
                        let max_inst = self.shared_state.lock().unwrap().instruments.len().max(1);
                        if inst_open {
                            self.selected_instrument = (self.selected_instrument + 1).min(max_inst);
                        } else if self.info_inst_offset + 31 < max_inst {
                            self.info_inst_offset += 31;
                        }
                        return;
                    }
                    KeyCode::NumpadSubtract => {
                        if inst_open {
                            self.selected_instrument =
                                self.selected_instrument.saturating_sub(1).max(1);
                        } else {
                            self.info_inst_offset = self.info_inst_offset.saturating_sub(31);
                        }
                        return;
                    }
                    KeyCode::Digit1
                    | KeyCode::Digit2
                    | KeyCode::Digit3
                    | KeyCode::Digit4
                    | KeyCode::Digit5
                    | KeyCode::Digit6
                    | KeyCode::Digit7
                    | KeyCode::Digit8
                    | KeyCode::Digit9
                        if !eff_open =>
                    {
                        let digit = match code {
                            KeyCode::Digit1 => 1,
                            KeyCode::Digit2 => 2,
                            KeyCode::Digit3 => 3,
                            KeyCode::Digit4 => 4,
                            KeyCode::Digit5 => 5,
                            KeyCode::Digit6 => 6,
                            KeyCode::Digit7 => 7,
                            KeyCode::Digit8 => 8,
                            KeyCode::Digit9 => 9,
                            _ => 1,
                        };
                        let max_inst = self.shared_state.lock().unwrap().instruments.len().max(1);
                        self.selected_instrument = (self.info_inst_offset + digit).min(max_inst);
                        self.open_window(event_loop, WindowKind::InstrumentInfo);
                        return;
                    }
                    _ => {}
                }
            }

        // Offene Popups/Menüs (`SysMenu`, `FileMenu`, `About`) zuerst per Escape schließen,
        // bevor die Hotkey-Tabelle `ESCAPE -> Stop playback` auslöst.
        if is_esc_key {
            if self.sys_menu_open {
                self.mark_escape_consumed();
                self.sys_menu_open = false;
                self.sys_menu_hover = None;
                return;
            }
            if self.file_menu_open {
                self.mark_escape_consumed();
                self.file_menu_open = false;
                self.file_menu_hover = None;
                self.close_window(WindowKind::FileMenu);
                return;
            }
            if self.kind_to_id.contains_key(&WindowKind::About) {
                self.mark_escape_consumed();
                self.close_window(WindowKind::About);
                return;
            }
            // Falls `ESC` unmittelbar zuvor bereits einen Dialog geschlossen hat (z. B. Classic Playlist / QuickSelect),
            // ignoriert das Hauptfenster `ESC`, bis die Taste physisch losgelassen und neu gedrückt wurde!
            if self.is_escape_stop_suppressed() {
                return;
            }
        }

        // 0g. Benutzerdefinierte Hotkeys aus `state.ini_config.hotkeys` (`KEYSEL.PAS`) prüfen:
        let has_hotkey_table = {
            let st = self.shared_state.lock().unwrap();
            !st.ini_config.hotkeys.is_empty()
        };
        if let Some((_, kname)) = Self::key_event_to_vkey_and_name(event) {
            let mut st_bits = 0u8;
            if ctrl {
                st_bits |= 1;
            }
            if shift {
                st_bits |= 2;
            }
            if alt {
                st_bits |= 4;
            }
            let matched_action_id = {
                let st = self.shared_state.lock().unwrap();
                st.ini_config
                    .hotkeys
                    .iter()
                    .find(|hk| hk.key_name.eq_ignore_ascii_case(&kname) && hk.state == st_bits)
                    .map(|hk| hk.action_id)
            };
            if let Some(act_id) = matched_action_id {
                let repeatable = matches!(act_id, 4..=7 | 33..=36 | 41..=42);
                if event.repeat && !repeatable {
                    return;
                }
                if self.execute_hotkey_event(event_loop, act_id) {
                    return;
                }
            }
        }

        // 1. Numpad / und Numpad * für PreAmp sowie 1..0 für Kanal-Mute
        if let PhysicalKey::Code(code) = event.physical_key {
            match code {
                KeyCode::NumpadDivide if !has_hotkey_table && !ctrl && !alt && !shift => {
                    self.step_preamp_db(-2);
                    return;
                }
                KeyCode::NumpadMultiply if !has_hotkey_table && !ctrl && !alt && !shift => {
                    self.step_preamp_db(2);
                    return;
                }
                // Tasten 1..0 im Effekt-Fenster (bzw. global) schalten Kanäle 1..32 um; Alt+1/2/3 setzt Fenster-Skalierung (1x, 2x, 3x)
                KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
                | KeyCode::Digit0 => {
                    if event.repeat {
                        return;
                    }
                    if alt && !ctrl && !shift {
                        match code {
                            KeyCode::Digit1 => {
                                self.set_scale_factor_live(1.0);
                                return;
                            }
                            KeyCode::Digit2 => {
                                self.set_scale_factor_live(2.0);
                                return;
                            }
                            KeyCode::Digit3 => {
                                self.set_scale_factor_live(3.0);
                                return;
                            }
                            _ => return,
                        }
                    }
                    if alt {
                        return;
                    }
                    let base = match code {
                        KeyCode::Digit1 => 0,
                        KeyCode::Digit2 => 1,
                        KeyCode::Digit3 => 2,
                        KeyCode::Digit4 => 3,
                        KeyCode::Digit5 => 4,
                        KeyCode::Digit6 => 5,
                        KeyCode::Digit7 => 6,
                        KeyCode::Digit8 => 7,
                        KeyCode::Digit9 => 8,
                        KeyCode::Digit0 => 9,
                        _ => 0,
                    };
                    let offset = match (ctrl, shift) {
                        (false, false) => 0,
                        (false, true) => 10,
                        (true, false) => 20,
                        (true, true) => 30,
                    };
                    let ch_idx = base + offset;
                    if ch_idx < 32 {
                        let mut state = self.shared_state.lock().unwrap();
                        state.channel_muted[ch_idx] = !state.channel_muted[ch_idx];
                        state.channel_muted_dirty = true;
                    }
                    return;
                }
                _ => {}
            }
        }

        // Zusätzliche Fallback-Tastenkürzel, falls `ini_config.hotkeys` leer ist:
        if let Key::Character(ch) = &event.logical_key
            && !ctrl
            && !alt
        {
            let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
            match c {
                'G' if !shift && !has_hotkey_table => {
                    if !event.repeat {
                        self.cycle_skin_live();
                    }
                    return;
                }
                '/' | '-' if !has_hotkey_table => {
                    self.step_preamp_db(-2);
                    return;
                }
                '*' | '+' if !has_hotkey_table => {
                    self.step_preamp_db(2);
                    return;
                }
                _ => {}
            }
        }

        // Fallback-Hotkeys greifen NUR, wenn `ini_config.hotkeys` leer ist (damit umbelegte/gelöschte Hotkeys
        // respektiert werden) und KEINE Modifikatoren (`Ctrl`/`Alt`/`Shift`) gedrückt sind (verhindert z. B. `Alt+F4` -> `F4`).
        if has_hotkey_table || ctrl || alt || shift {
            return;
        }

        match &event.logical_key {
            // Space = Play
            Key::Named(NamedKey::Space) if !event.repeat => {
                self.trigger_play_or_hdd_record(event_loop);
            }
            // ESC = Stop
            Key::Named(NamedKey::Escape) if !event.repeat => {
                let mut state = self.shared_state.lock().unwrap();
                state.intro_scan = false;
                state.is_playing = false;
                state.is_paused = false;
                state.pending_seek_pattern = Some(1);
            }
            // Cursor Hoch/Runter = Voriger/Nächster Song
            Key::Named(NamedKey::ArrowUp) => self.prev_song(),
            Key::Named(NamedKey::ArrowDown) => self.next_song(),
            // Cursor Links/Rechts = Schneller Rück-/Vorlauf (Pattern -/+ 1)
            Key::Named(NamedKey::ArrowLeft) => {
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta -= 1;
            }
            Key::Named(NamedKey::ArrowRight) => {
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta += 1;
            }
            // Pos1 / Ende = Erster / Letzter Song (bzw. Pattern)
            Key::Named(NamedKey::Home) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 8);
            }
            Key::Named(NamedKey::End) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 9);
            }
            // Bild Hoch / Bild Runter = Dateiname / Songname
            Key::Named(NamedKey::PageUp) if !event.repeat => {
                let mut state = self.shared_state.lock().unwrap();
                state.show_filename = true;
                state.sync_runtime_to_ini();
            }
            Key::Named(NamedKey::PageDown) if !event.repeat => {
                let mut state = self.shared_state.lock().unwrap();
                state.show_filename = false;
                state.sync_runtime_to_ini();
            }
            // Einfg = Panning ein/aus
            Key::Named(NamedKey::Insert) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 32);
            }
            // F2 = Shuffle, F3 = Repeat-Modus, F4 = 15s Introscan
            Key::Named(NamedKey::F2) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 21);
            }
            Key::Named(NamedKey::F3) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 22);
            }
            Key::Named(NamedKey::F4) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 23);
            }
            // F5 = Copy, F6 = Move/Rename, F7 = Remove From Playlist, F8 = Delete from Disk
            Key::Named(NamedKey::F5) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 24);
            }
            Key::Named(NamedKey::F6) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 25);
            }
            Key::Named(NamedKey::F7) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 26);
            }
            Key::Named(NamedKey::F8) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 27);
            }
            // F9 = Clipping ein/aus
            Key::Named(NamedKey::F9) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 28);
            }
            // F10 = BPM-Speed ein/aus
            Key::Named(NamedKey::F10) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 29);
            }
            // F11 = Surround ein/aus
            Key::Named(NamedKey::F11) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 30);
            }
            // F12 = IDO-Modus durchschalten (0 = IDO OFF, 1 = IDO Linear, 2 = IDO CPU-Cubic, 3 = IDO FPU-Cubic)
            Key::Named(NamedKey::F12) if !event.repeat => {
                let _ = self.execute_hotkey_event(event_loop, 31);
            }
            // Buchstaben-Hotkeys (nur ohne Strg / Alt / Shift)
            Key::Character(ch) if !event.repeat => {
                let c = ch.chars().next().unwrap_or('\0').to_ascii_uppercase();
                match c {
                    'P' => {
                        let _ = self.execute_hotkey_event(event_loop, 3);
                    }
                    'O' => {
                        let _ = self.execute_hotkey_event(event_loop, 10);
                    }
                    'L' => {
                        let _ = self.execute_hotkey_event(event_loop, 11);
                    }
                    'F' => {
                        let _ = self.execute_hotkey_event(event_loop, 12);
                    }
                    'I' => self.toggle_window(event_loop, WindowKind::ModuleInfo),
                    'E' => self.toggle_window(event_loop, WindowKind::Effects),
                    'S' => self.toggle_window(event_loop, WindowKind::SetupDialog),
                    'M' => self.spawn_external_mixer(),
                    'A' => {
                        let _ = self.execute_hotkey_event(event_loop, 43);
                    }
                    'T' => {
                        let _ = self.execute_hotkey_event(event_loop, 15);
                    }
                    'G' => {
                        let _ = self.execute_hotkey_event(event_loop, 44);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
