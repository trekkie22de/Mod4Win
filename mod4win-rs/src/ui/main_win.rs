//! Hauptfenster (`bm0.bmp`), Mini-Statusfenster (`bm1.bmp`) und About-Ansicht (`ABOUT.PAS`).

use std::sync::Arc;
use super::winit::event_loop::ActiveEventLoop;
use super::winit::window::Window;

use super::app::{Mod4WinApp, PressedButton, UiSharedState, WindowKind};
use super::cpu_sim::CpuProfile;
use super::ini_config::Mod4WinIniConfig;
use super::widgets::{
    draw_3d_panel, draw_bmp_button_2state, draw_bmp_button_3state, draw_bmp_lcd_number,
    draw_bmp_text_16x20_marquee, draw_frame_3d, draw_frame_3d_1px, draw_oem_text,
    draw_oem_text_centered, draw_oem_text_mnemonic, draw_oem_text_right, draw_rect_outline,
    draw_vswitch, fill_rect,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    RemoveFromPlaylist,
    DeleteFromDisk,
}

impl Mod4WinApp {
    /// Rendert das Hauptfenster (`MODMAINFRAME.bmp` bei `(1, 19)` in beiden Modi; `--skin 2.30` ändert ausschließlich die obere Titelleiste).
    pub(crate) fn render_main_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        // 1. Rahmen + in BEIDEN Modi (v2.40 und v2.30) exakt das 2.40-MODMAINFRAME-Raster auf (1, 19)
        self.assets.bm0_main.blit_rect_to(0, 0, dw, dh, frame, dw, dh, 0, 0);
        self.assets.modmainframe.blit_rect_to(0, 0, 540, 115, frame, dw, dh, 1, 19);

        // 2. Modultitel, Dateiname oder temporäres Status-Banner (z.B. CPU-Profil / Stutter-Modus) in Main_Info (10, 29, 20 Zeichen in FONT_16X20.bmp)
        let banner_active = state
            .status_banner
            .as_ref()
            .and_then(|(msg, ts)| {
                if ts.elapsed().as_secs_f32() < 2.2 {
                    Some(msg.as_str())
                } else {
                    None
                }
            });
        let peak_disp = state.active_peak_search_display();
        let display_name = if let Some((ref ps_text, _, _)) = peak_disp {
            ps_text.as_str()
        } else if let Some(banner_msg) = banner_active {
            banner_msg
        } else if state.show_filename && !state.filename.is_empty() {
            state.filename.as_str()
        } else if !state.title.trim().is_empty() {
            state.title.trim()
        } else if !state.filename.is_empty() {
            state.filename.as_str()
        } else {
            "MOD4WIN 3.00"
        };
        let scroll_ms = state.main_title_scroll_elapsed_ms();
        draw_bmp_text_16x20_marquee(
            frame,
            dw,
            dh,
            10,
            29,
            20,
            display_name,
            scroll_ms,
            &self.assets.font_16x20,
        );

        // 3. Umschalter Dateiname / Modultitel (pSWName: 341, 28, 2-stufiger TSwitch 18x22 aus SWITCH.bmp: Oben=1=Dateiname, Unten=2=Songtitel)
        let sw_pos = if state.show_filename { 1 } else { 2 };
        draw_vswitch(frame, dw, dh, 341, 28, 2, sw_pos, &self.assets.switch_bmp);

        // 4. Spielzeit / Restzeit (LCD_15X24 bei (22, 69) und (56, 69), MINUS_MODE bei (12, 80))
        let disp_secs = if state.minus_mode && state.total_time_secs > 0 {
            state.total_time_secs.saturating_sub(state.elapsed_secs)
        } else {
            state.elapsed_secs
        };
        let mins = (disp_secs / 60).min(99);
        let secs = disp_secs % 60;
        let minus_sy = if state.minus_mode { 0 } else { 2 };
        self.assets
            .minus_mode
            .blit_rect_to(0, minus_sy, 8, 2, frame, dw, dh, 12, 80);
        draw_bmp_lcd_number(frame, dw, dh, 22, 69, mins, 2, false, &self.assets.lcd_15x24, 15, 24);
        draw_bmp_lcd_number(frame, dw, dh, 56, 69, secs, 2, true, &self.assets.lcd_15x24, 15, 24);

        // 5. Track / Tracks & Pattern / Patterns (LCD_13X20 bei y = 73, lz = false!)
        let curr_song = if state.playlist.is_empty() && state.current_file_path.is_none() {
            0
        } else {
            (state.playlist_idx + 1).max(1) as u32
        };
        let total_songs = if state.playlist.is_empty() && state.current_file_path.is_none() {
            0
        } else {
            state.playlist.len().max(1) as u32
        };
        let (disp_pos, disp_orders) = if let Some((_, scan_ord, max_ord)) = peak_disp {
            (scan_ord as u32, max_ord as u32)
        } else {
            (state.curr_pos as u32, state.num_orders as u32)
        };
        draw_bmp_lcd_number(frame, dw, dh, 95, 73, curr_song, 4, false, &self.assets.lcd_13x20, 13, 20);
        if (10_000..100_000).contains(&curr_song) {
            // Wenn "Song" durch den Dezimalpunkt um 2px nach rechts rückt, "of" um 1px nach rechts zentrieren
            fill_rect(frame, dw, dh, 149, 73, 1, 20, [0, 0, 0, 255]);
            self.assets
                .modmainframe
                .blit_rect_to(148, 54, 8, 20, frame, dw, dh, 150, 73);
        }
        draw_bmp_lcd_number(frame, dw, dh, 159, 73, total_songs, 4, false, &self.assets.lcd_13x20, 13, 20);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            226,
            73,
            disp_pos,
            3,
            false,
            &self.assets.lcd_13x20,
            13,
            20,
        );
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            275,
            73,
            disp_orders,
            3,
            false,
            &self.assets.lcd_13x20,
            13,
            20,
        );

        // 6. Shuffle, Repeat, IntroScan (SH_REP_SCAN.bmp bei x = 316)
        let sh_sy = if state.shuffle { 24 } else { 0 };
        self.assets
            .sh_rep_scan
            .blit_rect_to(0, sh_sy, 32, 8, frame, dw, dh, 316, 62);

        match state.repeat_mode {
            0 => {
                self.assets
                    .sh_rep_scan
                    .blit_rect_to(0, 8, 39, 8, frame, dw, dh, 316, 73);
            }
            1 => {
                self.assets
                    .sh_rep_scan
                    .blit_rect_to(0, 32, 32, 8, frame, dw, dh, 316, 73);
                self.assets
                    .sh_rep_scan
                    .blit_rect_to(33, 8, 6, 8, frame, dw, dh, 349, 73);
            }
            _ => {
                self.assets
                    .sh_rep_scan
                    .blit_rect_to(0, 32, 39, 8, frame, dw, dh, 316, 73);
            }
        }

        let scan_sy = if state.intro_scan { 40 } else { 16 };
        self.assets
            .sh_rep_scan
            .blit_rect_to(0, scan_sy, 44, 8, frame, dw, dh, 316, 84);

        // 7. Rechte Statusbox: SR, Bits, Channels, CPU %, Buffer Time (LCD_08X13, lz = false!)
        let sr_khz = ((state.sound_settings.samplerate + 500) / 1000).max(0) as u32;
        let bits = state.sound_settings.bits as u32;
        draw_bmp_lcd_number(frame, dw, dh, 375, 29, sr_khz, 2, false, &self.assets.lcd_08x13, 8, 13);
        draw_bmp_lcd_number(frame, dw, dh, 410, 29, bits, 2, false, &self.assets.lcd_08x13, 8, 13);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            375,
            47,
            state.num_channels as u32,
            2,
            false,
            &self.assets.lcd_08x13,
            8,
            13,
        );
        // CPU % (Main_Percnt: 375, 65 in LCD_08X13, 3-stellig 0..999%) & Vertikaler CPU-Balken (PBar: 371, 29, 4x51 wie `DrawPrcntBar` in `mod4win.pas`)
        let cpu_pct = state.cpu_telemetry.display_cpu_percent.min(999);
        draw_bmp_lcd_number(frame, dw, dh, 375, 65, cpu_pct, 3, false, &self.assets.lcd_08x13, 8, 13);

        let bar_usage = ((state.cpu_telemetry.smoothed_load_ratio * 100.0).round() as i32).clamp(0, 100);
        let hw = if bar_usage >= 100 || state.cpu_telemetry.buffer_underrun {
            51
        } else if bar_usage > 0 {
            3 * ((51 * bar_usage) / 300)
        } else {
            0
        };
        if hw < 51 {
            // Deckt die oberen (51 - hw) Pixel schwarz ab; darunter scheint der originale grün/gelb/rote Segmentbalken aus MODMAINFRAME.bmp durch!
            fill_rect(frame, dw, dh, 371, 29, 4, (51 - hw) as u32, [0, 0, 0, 255]);
        }

        // Buffer Time (`Main_BuffTime`: 410, 65 und 430, 65; blinkt bei Buffer-Underrun `00.0s`)
        let (btime_sec, btime_tenth) = state.cpu_telemetry.buffer_time_digits();
        let blink_hide = state.cpu_telemetry.buffer_underrun
            && ((state.cpu_telemetry.tick_counter / 8) % 2 == 1);
        if blink_hide {
            fill_rect(frame, dw, dh, 410, 65, 16, 13, [0, 0, 0, 255]);
            fill_rect(frame, dw, dh, 430, 65, 8, 13, [0, 0, 0, 255]);
        } else {
            let lead_zero = state.cpu_telemetry.buffer_underrun || btime_sec >= 10;
            draw_bmp_lcd_number(
                frame,
                dw,
                dh,
                410,
                65,
                btime_sec,
                2,
                lead_zero,
                &self.assets.lcd_08x13,
                8,
                13,
            );
            draw_bmp_lcd_number(
                frame,
                dw,
                dh,
                430,
                65,
                btime_tenth,
                1,
                true,
                &self.assets.lcd_08x13,
                8,
                13,
            );
        }

        // 8. Interaktive Status-Symbole im rechten Display
        // Clip / CL+DC (x_Qmix: 447, 35, 18x9 aus SH_REP_SCAN.bmp bzw. 23x7 CL+DC-Text bei (444, 35))
        let clip_on = (state.sound_settings.pre_amp & 0x80) != 0;
        if state.sound_settings.declick() {
            fill_rect(frame, dw, dh, 444, 34, 26, 10, [0, 0, 0, 255]);
            draw_cl_dc_lcd_badge(frame, dw, dh, 444, 35, [0, 0, 0, 255]);
        } else {
            let clip_sx = if clip_on { 0 } else { 26 };
            self.assets
                .sh_rep_scan
                .blit_rect_to(clip_sx, 83, 18, 9, frame, dw, dh, 447, 35);
        }

        // Stereo / Mono / Surround (x_m_st: 472, 31, 27x11 aus M_ST.bmp)
        let mst_idx = if state.sound_settings.stereo < 2 {
            0
        } else if state.sound_settings.surround {
            2
        } else {
            1
        };
        self.assets
            .m_st
            .blit_rect_to(0, mst_idx * 11, 27, 11, frame, dw, dh, 472, 31);

        // BPM (x_BPM: 414, 53, 26x9 aus SH_REP_SCAN.bmp)
        let bpm_sy = if state.sound_settings.no_bpm { 48 } else { 57 };
        self.assets
            .sh_rep_scan
            .blit_rect_to(0, bpm_sy, 26, 9, frame, dw, dh, 414, 53);

        // Panning (x_PAN: 443, 53, 23x9 aus SH_REP_SCAN.bmp)
        let pan_sy = if state.sound_settings.panning { 57 } else { 48 };
        self.assets
            .sh_rep_scan
            .blit_rect_to(26, pan_sy, 23, 9, frame, dw, dh, 443, 53);

        // IDO / IT (x_IDO: 473, 48, 26x17 aus SH_REP_SCAN.bmp für IDO 0..3 bzw. Impulse-Tracker-Badge für Native IT 4..6)
        let is_it_mod = state.is_it_module();
        let eff_oversamp = state.effective_oversamp();
        if eff_oversamp <= 3 {
            let ido_sx = (eff_oversamp as u32) * 26;
            self.assets
                .sh_rep_scan
                .blit_rect_to(ido_sx, 66, 26, 17, frame, dw, dh, 473, 48);
        } else {
            draw_it_lcd_badge(
                frame,
                dw,
                dh,
                473,
                48,
                eff_oversamp,
                [0, 0, 0, 255],
            );
        }

        // TMode (x_tmode: 448, 71, 23x9 aus TMODE.bmp)
        let tmode_sy = (state.sound_settings.timer_mode() as u32) * 9;
        self.assets
            .tmode
            .blit_rect_to(0, tmode_sy, 23, 9, frame, dw, dh, 448, 71);

        // 9. PreAmp-Schieberegler (509, 27, 23x72 aus PREAMP.bmp) + dB-Anzeige (515, 101)
        let raw_preamp = (state.sound_settings.pre_amp & 0x7F).min(40) as i32;
        let db_val = raw_preamp - 20;
        let pre_minus_sy = if db_val < 0 { 0 } else { 2 };
        self.assets
            .minus_mode
            .blit_rect_to(0, pre_minus_sy, 4, 2, frame, dw, dh, 510, 107);
        draw_bmp_lcd_number(
            frame,
            dw,
            dh,
            515,
            101,
            db_val.unsigned_abs(),
            2,
            false,
            &self.assets.lcd_08x13,
            8,
            13,
        );

        // Exakter 3-Slice BitBlt + Slider-Thumb aus DrawPreAmp (mod4win.pas Zeilen 720-733)
        let pre_w = 23u32;
        let pre_h = 72u32;
        let border = 36u32;
        let hw = (36 + ((32 * (20 - raw_preamp)) / 20)).clamp(0, 72) as u32;
        if hw < border {
            self.assets
                .preamp
                .blit_rect_to(pre_w, 0, pre_w, hw, frame, dw, dh, 509, 27);
            self.assets.preamp.blit_rect_to(
                0,
                hw,
                pre_w,
                border - hw,
                frame,
                dw,
                dh,
                509,
                27 + hw as i32,
            );
            self.assets.preamp.blit_rect_to(
                pre_w,
                border,
                pre_w,
                pre_h - border,
                frame,
                dw,
                dh,
                509,
                27 + border as i32,
            );
        } else {
            self.assets
                .preamp
                .blit_rect_to(pre_w, 0, pre_w, border, frame, dw, dh, 509, 27);
            self.assets.preamp.blit_rect_to(
                0,
                border,
                pre_w,
                hw - border,
                frame,
                dw,
                dh,
                509,
                27 + border as i32,
            );
            self.assets.preamp.blit_rect_to(
                pre_w,
                hw,
                pre_w,
                pre_h - hw,
                frame,
                dw,
                dh,
                509,
                27 + hw as i32,
            );
        }
        // Slider-Thumb (23x7 von (0, 72) in PREAMP.bmp)
        let thumb_y = (27 + hw as i32 - 3).clamp(24, 95);
        self.assets
            .preamp
            .blit_rect_to(0, 72, pre_w, 7, frame, dw, dh, 509, thumb_y);

        // 10. 3D-Buttons (Transport, 2x4-Funktionsblock und Titelleiste)
        let is_stopped = !state.is_playing && !state.is_paused;
        let scan_blink_on = !state.intro_scan
            || (self.about_opened_at.elapsed().as_millis() / 350).is_multiple_of(2);
        let is_play_active = state.is_playing && !state.is_paused && scan_blink_on;
        let is_pause_active = state.is_paused;

        // v2.40 Last Beta Titelleiste (`tmywindow.paintcaptiontext` / `Ex_NewWin` in `winunit.pas` & `mod4win.pas`):
        // Farbverlauf in der Titelleiste (y = 1..17), schwarze Trennlinie bei y = 18
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
            self.locale.tr_or("win", "main_title", "MOD4WIN 3.00"),
            [48, 56, 68, 255],
        );

        // System-Menü-Icon (16x14 aus SYSBUTTONS_W95.bmp, sx=0) bei (2, 2)
        let sys_sy = if self.pressed_btn == Some(PressedButton::SysMenu) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, sys_sy, 16, 14, frame, dw, dh, 2, 2);

        // Wenn ein .IT-Modul geladen ist: 28x12 Impulse-Tracker-Badge in der Titelleiste
        if is_it_mod {
            self.assets
                .ft_it
                .blit_rect_to(0, 0, 28, 12, frame, dw, dh, 347, 3);
        }

        // Mixer / Setup / Help (33x14 aus BMIXER95 / BSETUP95 / BHELP95) bei (379, 2), (414, 2), (449, 2)
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            379,
            2,
            self.pressed_btn == Some(PressedButton::Mixer),
            &self.assets.bmixer95,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            414,
            2,
            self.pressed_btn == Some(PressedButton::Setup),
            &self.assets.bsetup95,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            449,
            2,
            self.pressed_btn == Some(PressedButton::Help),
            &self.assets.bhelp95,
        );

        // Minimize (sx=16), Mini-Status (sx=48) und Close X (sx=64) aus SYSBUTTONS_W95.bmp (je 16x14) bei (487, 2), (503, 2), (521, 2)
        let min_sy = if self.pressed_btn == Some(PressedButton::Minimize) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(16, min_sy, 16, 14, frame, dw, dh, 487, 2);
        let mini_sy = if self.pressed_btn == Some(PressedButton::MiniMode) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(48, mini_sy, 16, 14, frame, dw, dh, 503, 2);
        let close_sy = if self.pressed_btn == Some(PressedButton::Close) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, close_sy, 16, 14, frame, dw, dh, 521, 2);

        // 2.40-Button-Layout (7x 50x25 Transport-Buttons bei y=102, 2x4 34x19 rechts)
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            9,
            102,
            self.pressed_btn == Some(PressedButton::Stop),
            is_stopped,
            &self.assets.bstop,
        );
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            59,
            102,
            self.pressed_btn == Some(PressedButton::Play),
            is_play_active,
            &self.assets.bplay,
        );
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            109,
            102,
            self.pressed_btn == Some(PressedButton::Pause),
            is_pause_active,
            &self.assets.bpause,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            159,
            102,
            self.pressed_btn == Some(PressedButton::Prev),
            &self.assets.bprev,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            209,
            102,
            self.pressed_btn == Some(PressedButton::Rew),
            &self.assets.bfr,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            259,
            102,
            self.pressed_btn == Some(PressedButton::Ff),
            &self.assets.bff,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            309,
            102,
            self.pressed_btn == Some(PressedButton::Next),
            &self.assets.bnext,
        );

        // Rechter 2x4 Block (34x19): Obere Reihe (y=89): Shuffle, Repeat, Scan, Time
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            368,
            89,
            self.pressed_btn == Some(PressedButton::Shuffle) || state.shuffle,
            &self.assets.bshuffle,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            401,
            89,
            self.pressed_btn == Some(PressedButton::Repeat),
            &self.assets.brepeat,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            434,
            89,
            self.pressed_btn == Some(PressedButton::IntroScan) || state.intro_scan,
            &self.assets.bintroscan,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            467,
            89,
            self.pressed_btn == Some(PressedButton::Time) || state.minus_mode,
            &self.assets.btime,
        );

        // Untere Reihe (y=107): Open, Datei, Info, Effects
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            368,
            107,
            self.pressed_btn == Some(PressedButton::Open),
            &self.assets.bopen,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            401,
            107,
            self.pressed_btn == Some(PressedButton::FileMan) || self.file_menu_open,
            &self.assets.bdatei,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            434,
            107,
            self.pressed_btn == Some(PressedButton::Info) || state.show_info,
            &self.assets.binfo,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            467,
            107,
            self.pressed_btn == Some(PressedButton::Effects) || state.show_effects,
            &self.assets.beffects,
        );

        // 11. Popup-Menü des `File`-Buttons (`tmod.handledatei` & `tsoundwindow.wmdrawitem` in `mod4win.pas`)
        if self.file_menu_open && !self.is_window_open(WindowKind::FileMenu) {
            self.render_file_popup_menu_at(frame, dw, dh, 326, 58);
        }

        // 11b. Klassisches Systemmenü oben links (`1..=18`, `vy <= 18`)
        if self.sys_menu_open {
            self.render_system_popup_menu(frame, dw, dh, state);
        }

        // 12. Sicherheitsabfrage (`OurMessageBox` / `TModalWindow` in `winunit.pas`) für `F7` (Remove) und `F8` (Delete)
        if let Some(action) = self.confirm_action
            && !self.is_window_open(WindowKind::ConfirmDialog) {
                let (cw, _) = self.confirm_dialog_size(action);
                let cx = ((dw as i32) - (cw as i32)) / 2;
                let cy = 5;
                self.render_confirm_dialog_at(frame, dw, dh, cx, cy, cw, 124, action, state);
            }
    }

    pub(crate) fn confirm_dialog_size(&self, action: ConfirmAction) -> (u32, u32) {
        match action {
            ConfirmAction::DeleteFromDisk => (284, 124),
            ConfirmAction::RemoveFromPlaylist => (398, 124),
        }
    }

    pub(crate) fn render_file_popup_menu_window(&self, frame: &mut [u8], dw: u32, dh: u32) {
        self.render_file_popup_menu_at(frame, dw, dh, 0, 0);
    }

    pub(crate) fn render_file_popup_menu_at(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        mx: i32,
        my: i32,
    ) {
        let mw = 212u32;
        let mh = 68u32;
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];
        fill_rect(frame, dw, dh, mx, my, mw, mh, col_gray);
        draw_rect_outline(frame, dw, dh, mx, my, mw, mh, col_black);
        draw_frame_3d_1px(frame, dw, dh, mx + 1, my + 1, mw - 2, mh - 2, false);

        let is_de = self.locale.is_german();
        let items: [(&str, &str); 4] = if is_de {
            [
                ("&Kopieren", "<F5>"),
                ("&Bewegen/Umbenennen", "<F6>"),
                ("&Von Liste entfernen", "<F7>"),
                ("&Löschen", "<F8>"),
            ]
        } else {
            [
                ("&Copy", "<F5>"),
                ("&Move/Rename", "<F6>"),
                ("&Remove From Playlist", "<F7>"),
                ("&Delete", "<F8>"),
            ]
        };
        let active_row = self.file_menu_hover.unwrap_or(0);
        for (i, (label, hk)) in items.iter().enumerate() {
            let ry = my + 2 + (i as i32) * 16;
            if i == active_row {
                draw_frame_3d(frame, dw, dh, mx + 3, ry, mw - 6, 16, 2, true);
            }
            draw_oem_text_mnemonic(frame, dw, dh, mx + 12, ry + 2, label, col_black);
            draw_oem_text_right(frame, dw, dh, mx + mw as i32 - 8, ry + 2, hk, col_black);
        }
    }

    pub(crate) fn render_system_popup_menu(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        let mx = 2i32;
        let my = 18i32;
        let mw = 196u32;
        let mh = 115u32;
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];
        let col_disabled = [128, 128, 128, 255];

        fill_rect(frame, dw, dh, mx, my, mw, mh, col_gray);
        draw_rect_outline(frame, dw, dh, mx, my, mw, mh, col_black);
        draw_frame_3d_1px(frame, dw, dh, mx + 1, my + 1, mw - 2, mh - 2, false);

        let is_de = self.locale.is_german();
        let top_prefix = if state.ini_config.mini_on_top { "» " } else { "  " };
        let info_prefix = if state.show_info { "» " } else { "  " };
        let eff_prefix = if state.show_effects { "» " } else { "  " };

        let rows: [(i32, String, bool); 8] = if is_de {
            [
                (21, "  &Wiederherstellen".to_string(), self.is_mini),
                (34, "  &Symbol".to_string(), true),
                (47, "  &Mini-Player".to_string(), !self.is_mini),
                (64, "  S&chließen     Alt+F4".to_string(), true),
                (81, format!("{}Immer im &Vordergrund", top_prefix), true),
                (94, format!("{}M&odul-Infos", info_prefix), true),
                (107, format!("{}&Effekt-Panel", eff_prefix), true),
                (120, "  Über &MOD4WIN...".to_string(), true),
            ]
        } else {
            [
                (21, "  &Restore".to_string(), self.is_mini),
                (34, "  Mi&nimize".to_string(), true),
                (47, "  &Mini-Player".to_string(), !self.is_mini),
                (64, "  &Close         Alt+F4".to_string(), true),
                (81, format!("{}&Always on Top", top_prefix), true),
                (94, format!("{}M&odule Info", info_prefix), true),
                (107, format!("{}&Effect Panel", eff_prefix), true),
                (120, "  &About MOD4WIN...".to_string(), true),
            ]
        };

        // Trennlinien nach Zeile 2 (y=61) und Zeile 3 (y=78)
        draw_frame_3d_1px(frame, dw, dh, mx + 3, 61, mw - 6, 2, true);
        draw_frame_3d_1px(frame, dw, dh, mx + 3, 78, mw - 6, 2, true);

        for (idx, (ry, text, enabled)) in rows.iter().enumerate() {
            if self.sys_menu_hover == Some(idx) && *enabled {
                draw_frame_3d(frame, dw, dh, mx + 3, *ry - 1, mw - 6, 14, 2, true);
            }
            draw_oem_text_mnemonic(
                frame,
                dw,
                dh,
                mx + 6,
                *ry,
                text,
                if *enabled { col_black } else { col_disabled },
            );
        }
    }

    pub(crate) fn render_confirm_dialog_window(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        state: &UiSharedState,
    ) {
        let action = self.confirm_action.unwrap_or(ConfirmAction::DeleteFromDisk);
        self.render_confirm_dialog_at(frame, dw, dh, 0, 0, dw, dh, action, state);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_confirm_dialog_at(
        &self,
        frame: &mut [u8],
        dw: u32,
        dh: u32,
        cx: i32,
        cy: i32,
        cw: u32,
        ch: u32,
        action: ConfirmAction,
        state: &UiSharedState,
    ) {
        let col_gray = [192, 192, 192, 255];
        let col_black = [0, 0, 0, 255];
        let is_de = self.locale.is_german();

        // 1px schwarzer Außenrahmen + 2.40-Titelleiste (`TModalwindow.WMEraseBkGnd` in `winunit.pas`)
        fill_rect(frame, dw, dh, cx, cy, cw, ch, col_gray);
        draw_rect_outline(frame, dw, dh, cx, cy, cw, ch, col_black);
        for y in 1..18 {
            for x in 1..(cw as i32 - 1) {
                let t = x as u32;
                let w_span = (cw.saturating_sub(2)).max(1);
                let r = (220 - (66 * t) / w_span) as u8;
                let g = (226 - (54 * t) / w_span) as u8;
                let b = (232 - (36 * t) / w_span) as u8;
                let px = cx + x;
                let py = cy + y;
                if px >= 0 && py >= 0 && (px as u32) < dw && (py as u32) < dh {
                    let idx = ((py as u32 * dw + px as u32) * 4) as usize;
                    if idx + 3 < frame.len() {
                        frame[idx] = r;
                        frame[idx + 1] = g;
                        frame[idx + 2] = b;
                        frame[idx + 3] = 255;
                    }
                }
            }
        }
        fill_rect(frame, dw, dh, cx + 1, cy + 18, cw.saturating_sub(2), 1, col_black);

        let title_str = match (action, is_de) {
            (ConfirmAction::DeleteFromDisk, true) => "MOD4WIN   Datei löschen",
            (ConfirmAction::DeleteFromDisk, false) => "MOD4WIN   Delete File",
            (ConfirmAction::RemoveFromPlaylist, true) => "MOD4WIN Datei entfernen",
            (ConfirmAction::RemoveFromPlaylist, false) => "MOD4WIN Remove File",
        };
        draw_oem_text(frame, dw, dh, cx + 22, cy + 4, title_str, [0, 0, 0, 255]);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, cx + 2, cy + 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, cx + cw as i32 - 19, cy + 2);

        // Hauptrahmen unter der Titelleiste (`Grayframefilled3d(0, CapDY, mainw, mainh-2, 2, up)`)
        draw_frame_3d(frame, dw, dh, cx + 1, cy + 19, cw - 2, ch - 20, 2, false);
        // Innerer abgesenkter 3D-Rahmen (`Grayframefilled3d(6, CapDY+6, mainw-12, mainh-19-buttonh, 1, down)`)
        draw_frame_3d_1px(frame, dw, dh, cx + 7, cy + 25, cw - 14, 68, true);

        // Fragezeichen-Icon (`MB_ICONQUESTION.bmp`, `60x60`) links bei `(cx + 11, cy + 29)`
        self.assets
            .mb_iconquestion
            .blit_rect_to(0, 0, 60, 60, frame, dw, dh, cx + 11, cy + 29);

        let full_path_lower = state
            .current_file_path
            .as_ref()
            .or_else(|| state.playlist.get(state.playlist_idx))
            .map(|p| p.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| state.filename.to_lowercase());

        let lines: [String; 3] = match (action, is_de) {
            (ConfirmAction::DeleteFromDisk, true) => [
                "Wollen Sie die Datei".to_string(),
                full_path_lower,
                "wirklich löschen?".to_string(),
            ],
            (ConfirmAction::DeleteFromDisk, false) => [
                "Do you really want".to_string(),
                full_path_lower,
                "to be deleted from disk?".to_string(),
            ],
            (ConfirmAction::RemoveFromPlaylist, true) => [
                "Wollen Sie das aktuelle Modul".to_string(),
                "wirklich aus der Playliste entfernen?".to_string(),
                "(Es wird nicht vom Datenträger entfernt.)".to_string(),
            ],
            (ConfirmAction::RemoveFromPlaylist, false) => [
                "Do you really want to remove".to_string(),
                "the current module from the playlist?".to_string(),
                "(It will not be deleted from disk.)".to_string(),
            ],
        };

        let text_area_x = cx + 72;
        let text_area_w = cw.saturating_sub(80);
        for (i, line) in lines.iter().enumerate() {
            let disp: String = if line.chars().count() > 38 {
                let skip = line.chars().count() - 38;
                line.chars().skip(skip).collect()
            } else {
                line.clone()
            };
            draw_oem_text_centered(
                frame,
                dw,
                dh,
                text_area_x,
                cy + 41 + (i as i32) * 12,
                text_area_w,
                &disp,
                col_black,
            );
        }

        // 2 breite `pmsgbutton` Buttons (`Ja` / `Nein` bzw. `Yes` / `No`, je `62x19`) bei `by = cy + 97`
        let button_w = 62u32;
        let button_h = 19u32;
        let buttdiff = ((cw as i32) - 11) / 2;
        let bx_yes = cx + ((cw as i32) - (button_w as i32) - buttdiff) / 2;
        let bx_no = bx_yes + buttdiff;
        let by = cy + 97;

        // `Ja` / `Yes` Button (mit 1px schwarzem Default-Rahmen wie in `TModalwindow`)
        draw_rect_outline(frame, dw, dh, bx_yes - 1, by - 1, button_w + 2, button_h + 2, col_black);
        draw_frame_3d(frame, dw, dh, bx_yes, by, button_w, button_h, 2, false);
        draw_oem_text_centered(
            frame,
            dw,
            dh,
            bx_yes,
            by + 4,
            button_w,
            if is_de { "Ja" } else { "Yes" },
            col_black,
        );

        // `Nein` / `No` Button
        draw_frame_3d_1px(frame, dw, dh, bx_no - 1, by - 1, button_w + 2, button_h + 2, true);
        draw_frame_3d(frame, dw, dh, bx_no, by, button_w, button_h, 2, false);
        draw_oem_text_centered(
            frame,
            dw,
            dh,
            bx_no,
            by + 4,
            button_w,
            if is_de { "Nein" } else { "No" },
            col_black,
        );
    }

    /// Rendert das Mini-Status-Fenster (`309 x 58`) exakt nach `mod4win.pas` (`paintministate`; `--skin 2.30` ändert nur die Titelleiste).
    pub(crate) fn render_mini_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        self.assets.bm1_mini.blit_rect_to(0, 0, dw, dh, frame, dw, dh, 0, 0);

        let disp_secs = if state.minus_mode && state.total_time_secs > 0 {
            state.total_time_secs.saturating_sub(state.elapsed_secs)
        } else {
            state.elapsed_secs
        };
        let mins = (disp_secs / 60).min(99);
        let secs = disp_secs % 60;
        let curr_song = if state.playlist.is_empty() && state.current_file_path.is_none() {
            0
        } else {
            (state.playlist_idx + 1).max(1) as u32
        };

        let is_stopped = !state.is_playing && !state.is_paused;
        let scan_blink_on = !state.intro_scan
            || (self.about_opened_at.elapsed().as_millis() / 350).is_multiple_of(2);
        let is_play_active = state.is_playing && !state.is_paused && scan_blink_on;
        let is_pause_active = state.is_paused;

        // Titelleisten-Buttons im Mini-Status (`HandleSwName` in `mod4win.pas:2496`: zeigt den Namen des laufenden Moduls!)
        let mini_caption = if state.show_filename && !state.filename.is_empty() {
            state.filename.as_str()
        } else if !state.title.trim().is_empty() && state.title.trim() != "MOD4WIN 3.00" {
            state.title.trim()
        } else if !state.filename.is_empty() {
            state.filename.as_str()
        } else {
            self.locale.tr_or("win", "main_title", "MOD4WIN 3.00")
        };
        let mini_caption_disp: String = mini_caption.chars().take(28).collect();
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
            &mini_caption_disp,
            [0, 0, 0, 255],
        );

        let sys_sy = if self.pressed_btn == Some(PressedButton::SysMenu) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, sys_sy, 16, 14, frame, dw, dh, 2, 2);
        let min_sy = if self.pressed_btn == Some(PressedButton::Minimize) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(16, min_sy, 16, 14, frame, dw, dh, 254, 2);
        let mini_sy = if self.pressed_btn == Some(PressedButton::MiniMode) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(32, mini_sy, 16, 14, frame, dw, dh, 270, 2);
        let close_sy = if self.pressed_btn == Some(PressedButton::Close) { 14 } else { 0 };
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, close_sy, 16, 14, frame, dw, dh, 288, 2);

        // Exakt nach `paintministate` (`mod4win.pas` Zeilen 2455-2462 & 2707-2709):
        // 3D-Panel ab y=19, links 1px abgesenkte schwarze Anzeige `(4, CapDY+3=21, 42, 32)` (lässt 1px schwarzen Rand oben/unten!),
        // rechts 2px abgesenkter Rahmen `(48, CapDY+4=22, 254, 30)` mit 5x 50x25 Buttons bei `(50..300, CapDY+6=24)`
        draw_3d_panel(frame, dw, dh, 0, 19, dw, dh - 19, false, [192, 192, 192, 255]);
        fill_rect(frame, dw, dh, 4, 21, 42, 32, [0, 0, 0, 255]);
        draw_frame_3d_1px(frame, dw, dh, 4, 21, 42, 32, true);
        draw_3d_panel(frame, dw, dh, 48, 22, 254, 30, true, [192, 192, 192, 255]);

        // Track (4 Stellen LCD_08X13 bei 10, CapDY+5=23) + Zeit (LCD_08X13 bei 10, CapDY+20=38 und 28, CapDY+20=38)
        draw_bmp_lcd_number(frame, dw, dh, 10, 23, curr_song, 4, false, &self.assets.lcd_08x13, 8, 13);
        let minus_sy = if state.minus_mode { 0 } else { 2 };
        self.assets
            .minus_mode
            .blit_rect_to(0, minus_sy, 4, 2, frame, dw, dh, 6, 43);
        draw_bmp_lcd_number(frame, dw, dh, 10, 38, mins, 2, false, &self.assets.lcd_08x13, 8, 13);
        self.assets
            .minus_mode
            .blit_rect_to(9, 0, 2, 4, frame, dw, dh, 27, 43);
        draw_bmp_lcd_number(frame, dw, dh, 28, 38, secs, 2, true, &self.assets.lcd_08x13, 8, 13);

        // 5 Transport-Buttons (50x25) bei x = 50, 100, 150, 200, 250, y = CapDY+6 = 24
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            50,
            24,
            self.pressed_btn == Some(PressedButton::Stop),
            is_stopped,
            &self.assets.bstop,
        );
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            100,
            24,
            self.pressed_btn == Some(PressedButton::Play),
            is_play_active,
            &self.assets.bplay,
        );
        draw_bmp_button_3state(
            frame,
            dw,
            dh,
            150,
            24,
            self.pressed_btn == Some(PressedButton::Pause),
            is_pause_active,
            &self.assets.bpause,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            200,
            24,
            self.pressed_btn == Some(PressedButton::Prev),
            &self.assets.bprev,
        );
        draw_bmp_button_2state(
            frame,
            dw,
            dh,
            250,
            24,
            self.pressed_btn == Some(PressedButton::Next),
            &self.assets.bnext,
        );
    }

    /// Rendert die originale About-Ansicht (`ABOUT.PAS` & `mod4win.pas`: `ABOUT.bmp` auf `542 x 135`, animierte Lautsprecher-Boxen `BOXES.bmp` und vertikale Laufschrift `laufschrift`).
    pub(crate) fn render_about_window(&self, frame: &mut [u8], dw: u32, dh: u32, state: &UiSharedState) {
        self.assets.bm0_main.blit_rect_to(0, 0, dw, dh, frame, dw, dh, 0, 0);
        self.assets.about.blit_rect_to(0, 0, 540, 115, frame, dw, dh, 1, 19);

        // Titelleiste wie im Hauptfenster (v2.40)
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
            self.locale.tr_or("win", "about_title", "MOD4WIN 3.00 About"),
            [48, 56, 68, 255],
        );
        self.assets
            .sysbuttons_w95
            .blit_rect_to(0, 0, 16, 14, frame, dw, dh, 2, 2);
        self.assets
            .sysbuttons_w95
            .blit_rect_to(64, 0, 16, 14, frame, dw, dh, 521, 2);

        // Animierte Lautsprecher-Boxen (`BOXES.bmp`, 3 Frames à 50x46) bei (26, 66) und (465, 66) (`tmod.animatebox` in `mod4win.pas:2010-2023`)
        let elapsed_ms = self.about_opened_at.elapsed().as_millis() as u32;
        let tick = elapsed_ms / 90;
        let (box_sy1, box_sy2) = if state.is_playing && !state.is_paused {
            let boxcount = tick % 4;
            if boxcount == 3 {
                (46, 46)
            } else {
                ((2 - boxcount) * 46, boxcount * 46)
            }
        } else {
            (0, 0)
        };
        self.assets
            .boxes
            .blit_rect_to(0, box_sy1, 50, 46, frame, dw, dh, 26, 66);
        self.assets
            .boxes
            .blit_rect_to(0, box_sy2, 50, 46, frame, dw, dh, 465, 66);

        // `FUNBMP.bmp` Lauflicht-Animation (`tmod.animatebox` in `mod4win.pas:2024-2033`):
        // Ein einzelner `39x3`-Farbbalken wandert in 2px-Schritten (`aktrunpos = 0..=12`, `run_step = 2`) durch die beiden schwarzen Fenster bei `(115, 94)` und `(389, 94)`!
        let aktrunpos = (tick % 13) as i32;
        let run_y = 94 + aktrunpos * 2;
        let fun_w = self.assets.funbmp.width.min(39);
        let max_fun_sy = self.assets.funbmp.height.saturating_sub(3);
        let sy_left = (3 + ((tick * 7 + 1) % 6) * 3).min(max_fun_sy);
        let sy_right = (3 + ((tick * 11 + 4) % 6) * 3).min(max_fun_sy);
        fill_rect(frame, dw, dh, 115, 94, fun_w, 27, [0, 0, 0, 255]);
        fill_rect(frame, dw, dh, 389, 94, fun_w, 27, [0, 0, 0, 255]);
        self.assets
            .funbmp
            .blit_rect_to(0, sy_left, fun_w, 3, frame, dw, dh, 115, run_y);
        self.assets
            .funbmp
            .blit_rect_to(0, sy_right, fun_w, 3, frame, dw, dh, 389, run_y);

        // Vertikale Credit-Laufschrift (`laufschrift` in `ABOUT.PAS`, `x_abscr=118, y_abscr=38, dx_abscr=308, dy_abscr=40`)
        let credits: [&str; 19] = [
            "MOD4WIN 3.00",
            "(c) 1993-96 by Kay Bruns",
            "",
            self.locale.tr_or("about", "prog_dsp", "Programming, DSP & Design:"),
            "Kay Bruns",
            "",
            self.locale.tr_or("about", "graphics", "Graphics & 3D Artwork:"),
            "Kay Bruns & Stefanomba",
            "",
            self.locale.tr_or("about", "formats", "Supported Formats:"),
            "MOD / XM / S3M / IT / UMX",
            "MTM / 669 / STM / FAR / OKT / ULT",
            "",
            self.locale.tr_or("about", "rust_port", "100% Bit-Exact Rust Port:"),
            "Multi-Window Winit + Pixels UI",
            "0 LSB Reference Audio Engine",
            "",
            self.locale.tr_or("about", "click_return", "Click anywhere to return..."),
            "",
        ];
        let scr_x = 118i32;
        let scr_y = 38i32;
        let scr_w = 308u32;
        let scr_h = 40u32;
        fill_rect(frame, dw, dh, scr_x, scr_y, scr_w, scr_h, [0, 0, 0, 255]);

        let line_h = 13i32;
        let total_scroll_h = (credits.len() as i32) * line_h + (scr_h as i32);
        let scroll_px = ((elapsed_ms / 45) as i32) % total_scroll_h;

        // Wir rendern in einen temporären 308x40 Puffer für sauberes vertikales Clipping an den Rändern des Scroller-Displays
        let mut scr_buf = vec![0u8; (scr_w * scr_h * 4) as usize];
        for b in scr_buf.as_chunks_mut::<4>().0 {
            b[3] = 255;
        }
        for (i, line) in credits.iter().enumerate() {
            let ly = (scr_h as i32) + (i as i32) * line_h - scroll_px;
            if ly > -line_h && ly < (scr_h as i32) {
                let lx = ((scr_w as i32 - (line.len() as i32 * 8)) / 2).max(4);
                let col = if i == 0 {
                    [0, 255, 0, 255]
                } else {
                    [255, 255, 0, 255]
                };
                draw_oem_text(&mut scr_buf, scr_w, scr_h, lx, ly, line, col);
            }
        }
        for y in 0..scr_h {
            for x in 0..scr_w {
                let s_idx = ((y * scr_w + x) * 4) as usize;
                let d_idx = (((scr_y as u32 + y) * dw + (scr_x as u32 + x)) * 4) as usize;
                if d_idx + 3 < frame.len() {
                    frame[d_idx..d_idx + 4].copy_from_slice(&scr_buf[s_idx..s_idx + 4]);
                }
            }
        }
    }

    pub(crate) fn execute_confirm_action(&mut self, action: ConfirmAction) {
        if action == ConfirmAction::RemoveFromPlaylist {
            let idx_opt = {
                let state = self.shared_state.lock().unwrap();
                if !state.playing_track_detached
                    && !state.playlist.is_empty()
                    && state.playlist_idx < state.playlist.len()
                {
                    Some(state.playlist_idx)
                } else {
                    None
                }
            };
            if let Some(idx) = idx_opt {
                self.remove_playlist_item_and_update_cursor(idx);
            }
            return;
        }
        let mut state = self.shared_state.lock().unwrap();
        let cur_path = state.current_file_path.clone().or_else(|| {
            if !state.playing_track_detached
                && !state.playlist.is_empty()
                && state.playlist_idx < state.playlist.len()
            {
                Some(state.playlist[state.playlist_idx].clone())
            } else {
                None
            }
        });
        let remove_idx = if !state.playing_track_detached
            && state.playlist_idx < state.playlist.len()
            && cur_path.as_ref() == Some(&state.playlist[state.playlist_idx])
        {
            Some(state.playlist_idx)
        } else {
            cur_path
                .as_ref()
                .and_then(|p| state.playlist.iter().position(|x| x == p))
        };
        if action == ConfirmAction::DeleteFromDisk
            && let Some(ref p) = cur_path
        {
            let _ = std::fs::remove_file(p);
        }
        if let Some(idx) = remove_idx {
            state.remove_playlist_entry(idx);
            if state.playlist.is_empty() {
                state.playlist_idx = 0;
                state.playing_track_detached = false;
                state.current_file_path = None;
                state.filename.clear();
                state.title = "MOD4WIN 3.00".to_string();
                state.is_playing = false;
                state.is_paused = false;
            } else {
                let next_idx = idx.min(state.playlist.len() - 1);
                state.playlist_idx = next_idx;
                state.playing_track_detached = false;
                if let Some(pos) = state.number_list.iter().position(|&v| v == next_idx) {
                    state.playlist_pos = pos;
                }
                let next_p = state.playlist[next_idx].clone();
                state.pending_load_file = Some(next_p);
            }
        } else if cur_path.is_some() {
            state.current_file_path = None;
            state.filename.clear();
            state.title = "MOD4WIN 3.00".to_string();
            state.is_playing = false;
            state.is_paused = false;
        }
    }

    pub(crate) fn handle_file_menu_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
    ) {
        self.file_menu_open = false;
        self.file_menu_hover = None;
        self.close_window(WindowKind::FileMenu);
        if (0..=212).contains(&vx) && (0..=68).contains(&vy) {
            let row = ((vy - 3).max(0) / 15).clamp(0, 3);
            match row {
                0 => {
                    self.file_man_dlg.is_copy = true;
                    self.open_window(event_loop, WindowKind::FileManager);
                }
                1 => {
                    self.file_man_dlg.is_copy = false;
                    self.open_window(event_loop, WindowKind::FileManager);
                }
                2 => {
                    self.confirm_action = Some(ConfirmAction::RemoveFromPlaylist);
                    self.open_window(event_loop, WindowKind::ConfirmDialog);
                }
                _ => {
                    self.confirm_action = Some(ConfirmAction::DeleteFromDisk);
                    self.open_window(event_loop, WindowKind::ConfirmDialog);
                }
            }
        }
    }

    pub(crate) fn handle_confirm_dialog_click(
        &mut self,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
    ) {
        let Some(action) = self.confirm_action else {
            self.close_window(WindowKind::ConfirmDialog);
            return;
        };
        let cw: i32 = match action {
            ConfirmAction::DeleteFromDisk => 284,
            ConfirmAction::RemoveFromPlaylist => 398,
        };
        if (5..=19).contains(&vy) && (cw - 19..=cw - 5).contains(&vx) {
            self.confirm_action = None;
            self.close_window(WindowKind::ConfirmDialog);
            return;
        }
        if (3..=20).contains(&vy) {
            let _ = window.drag_window();
            return;
        }
        let bx_yes = if cw > 320 { 80 } else { 44 };
        let bx_no = if cw > 320 { cw - 80 - 62 } else { cw - 44 - 62 };
        if (97..=116).contains(&vy) {
            if (bx_yes..=bx_yes + 62).contains(&vx) {
                self.confirm_action = None;
                self.close_window(WindowKind::ConfirmDialog);
                self.execute_confirm_action(action);
            } else if (bx_no..=bx_no + 62).contains(&vx) {
                self.confirm_action = None;
                self.close_window(WindowKind::ConfirmDialog);
            }
        }
    }

    pub(crate) fn handle_main_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        // 0a. Wenn gerade eine F7/F8 Sicherheitsabfrage offen ist (Fallback im Hauptfenster):
        if let Some(action) = self.confirm_action
            && !self.kind_to_id.contains_key(&WindowKind::ConfirmDialog) {
                let cw: i32 = match action {
                    ConfirmAction::DeleteFromDisk => 284,
                    ConfirmAction::RemoveFromPlaylist => 398,
                };
                let cx = (542 - cw) / 2;
                let cy = 5;
                let bx_yes = if cw > 320 { cx + 80 } else { cx + 44 };
                let bx_no = if cw > 320 { cx + cw - 80 - 62 } else { cx + cw - 44 - 62 };
                if (cy + 5..=cy + 19).contains(&vy) && (cx + cw - 19..=cx + cw - 5).contains(&vx) {
                    self.confirm_action = None;
                    return;
                }
                if (cy + 97..=cy + 116).contains(&vy) {
                    if (bx_yes..=bx_yes + 62).contains(&vx) {
                        self.confirm_action = None;
                        self.execute_confirm_action(action);
                        return;
                    } else if (bx_no..=bx_no + 62).contains(&vx) {
                        self.confirm_action = None;
                        return;
                    }
                }
                return;
            }

        // 0b. Wenn das System-Menü offen ist:
        if self.sys_menu_open {
            let mx = 2;
            let my = 18;
            let mw = 196;
            let mh = 115;
            self.sys_menu_open = false;
            self.sys_menu_hover = None;
            if (mx..=mx + mw).contains(&vx) && (my..=my + mh).contains(&vy) {
                let row = ((vy - (my + 3)).max(0) / 14).clamp(0, 7);
                match row {
                    0 => {
                        window.set_minimized(false);
                    }
                    1 => {
                        window.set_minimized(true);
                    }
                    2 => {
                        self.switch_mini_mode(event_loop, true);
                    }
                    3 => {
                        self.save_ini_on_exit();
                        let mut state = self.shared_state.lock().unwrap();
                        state.is_playing = false;
                        state.is_paused = false;
                        state.is_finished = true;
                        event_loop.exit();
                    }
                    4 => {
                        let new_aot = {
                            let mut state = self.shared_state.lock().unwrap();
                            state.ini_config.mini_on_top = !state.ini_config.mini_on_top;
                            self.setup_cfg.mini_on_top = state.ini_config.mini_on_top;
                            let _ = state.ini_config.save_to_file(&Mod4WinIniConfig::default_ini_path());
                            state.ini_config.mini_on_top
                        };
                        let level = if new_aot {
                            super::winit::window::WindowLevel::AlwaysOnTop
                        } else {
                            super::winit::window::WindowLevel::Normal
                        };
                        window.set_window_level(level);
                    }
                    5 => {
                        self.toggle_window(event_loop, WindowKind::ModuleInfo);
                    }
                    6 => {
                        self.toggle_window(event_loop, WindowKind::Effects);
                    }
                    _ => {
                        self.toggle_window(event_loop, WindowKind::About);
                    }
                }
                return;
            }
            if (1..=18).contains(&vx) && (1..=18).contains(&vy) && !is_double_click {
                return;
            }
        }

        // 0c. Wenn das Popup-Menü des `File`-Buttons offen ist:
        if self.file_menu_open {
            self.file_menu_open = false;
            self.file_menu_hover = None;
            if self.kind_to_id.contains_key(&WindowKind::FileMenu) {
                self.close_window(WindowKind::FileMenu);
                if (401..=433).contains(&vx) && (107..=126).contains(&vy) {
                    return;
                }
            } else {
                let mx = 326;
                let my = 39;
                let mw = 212;
                let mh = 68;
                if (mx..=mx + mw).contains(&vx) && (my..=my + mh).contains(&vy) {
                    let row = ((vy - (my + 3)).max(0) / 15).clamp(0, 3);
                    match row {
                        0 => {
                            self.file_man_dlg.is_copy = true;
                            self.open_window(event_loop, WindowKind::FileManager);
                        }
                        1 => {
                            self.file_man_dlg.is_copy = false;
                            self.open_window(event_loop, WindowKind::FileManager);
                        }
                        2 => {
                            self.confirm_action = Some(ConfirmAction::RemoveFromPlaylist);
                            self.open_window(event_loop, WindowKind::ConfirmDialog);
                        }
                        _ => {
                            self.confirm_action = Some(ConfirmAction::DeleteFromDisk);
                            self.open_window(event_loop, WindowKind::ConfirmDialog);
                        }
                    }
                    return;
                }
                if (401..=433).contains(&vx) && (107..=126).contains(&vy) {
                    return;
                }
            }
        }
        // 1. Obere Titelleisten-Buttons (y = 1..18)
        if (1..=18).contains(&vy) {
            // v2.40 Last Beta Titelleiste:
            // SysMenu (2..17), Mixer (379..411), Setup (414..446), Help (449..481),
            // Minimize (487..502), Mini-Status (503..518), Close X (521..537)
            if (1..=18).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SysMenu);
                if is_double_click {
                    self.save_ini_on_exit();
                    let mut state = self.shared_state.lock().unwrap();
                    state.is_playing = false;
                    state.is_paused = false;
                    state.is_finished = true;
                    event_loop.exit();
                } else {
                    self.sys_menu_open = !self.sys_menu_open;
                }
                return;
            } else if (379..=412).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Mixer);
                self.spawn_external_mixer();
                return;
            } else if (414..=447).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Setup);
                self.toggle_window(event_loop, WindowKind::SetupDialog);
                return;
            } else if (449..=482).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Help);
                self.open_help_topic(event_loop, "HauptDlg");
                return;
            } else if (487..=502).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Minimize);
                window.set_minimized(true);
                return;
            } else if (503..=519).contains(&vx) {
                self.pressed_btn = Some(PressedButton::MiniMode);
                self.switch_mini_mode(event_loop, true);
                return;
            } else if (520..=540).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Close);
                self.save_ini_on_exit();
                let mut state = self.shared_state.lock().unwrap();
                state.is_playing = false;
                state.is_paused = false;
                state.is_finished = true;
                event_loop.exit();
                return;
            } else if is_double_click {
                // Doppelklick auf die Titelleiste wechselt in den Mini-Player (`wmLButtonDblClk` in `mod4win.pas`)
                self.pressed_btn = Some(PressedButton::MiniMode);
                self.switch_mini_mode(event_loop, true);
                return;
            } else {
                let _ = window.drag_window();
                return;
            }
        }

        // 2. Main_Info (9..331, 27..50): Öffnet Schnellauswahl (bm3)
        if (9..=331).contains(&vx) && (27..=50).contains(&vy) {
            self.toggle_window(event_loop, WindowKind::QuickSelect);
            return;
        }

        // 3. Main_swname (339..360, 26..52): Dateiname (Oben=1) / Modultitel (Unten=2) umschalten
        if (339..=360).contains(&vx) && (26..=52).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            let clicked_top = vy < 39;
            if state.show_filename == clicked_top {
                state.show_filename = !state.show_filename;
            } else {
                state.show_filename = clicked_top;
            }
            state.reset_title_scroll();
            state.sync_runtime_to_ini();
            return;
        }

        // 4. Main_ModTime (10..88, 56..93): Spielzeit / Restzeit umschalten
        //    Track/Tracks (90..215, 56..93): Linksklick = +1 Song (`wmLButtonDown` in `mod4win.pas`)
        //    Pattern/Patterns (220..314, 56..93): Linksklick = +1 Pattern (`wmLButtonDown` in `mod4win.pas`)
        if (56..=93).contains(&vy) {
            if (10..=88).contains(&vx) {
                let mut state = self.shared_state.lock().unwrap();
                state.minus_mode = !state.minus_mode;
                state.sync_runtime_to_ini();
                return;
            } else if (90..=215).contains(&vx) {
                self.next_song();
                return;
            } else if (220..=314).contains(&vx) {
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta += 1;
                return;
            }
        }

        // 5. Shuffle / Repeat / IntroScan im mittleren Display (315..360, 60..93)
        if (315..=360).contains(&vx) {
            if (60..=70).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_shuffle();
                state.sync_runtime_to_ini();
                return;
            } else if (71..=81).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.repeat_mode = (state.repeat_mode + 1) % 3;
                state.sync_runtime_to_ini();
                return;
            } else if (82..=93).contains(&vy) {
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_intro_scan();
                state.sync_runtime_to_ini();
                return;
            }
        }

        // 6. Status-Symbole im rechten Display
        // main_SR (Samplerate kHz: 375..407, 28..43)
        if (375..=407).contains(&vx) && (28..=43).contains(&vy) {
            const STD_RATES: [i32; 4] = [11025, 22050, 44100, 48000];
            let mut state = self.shared_state.lock().unwrap();
            let cur_khz = (state.sound_settings.samplerate + 500) / 1000;
            let cur_idx = STD_RATES
                .iter()
                .position(|&r| (r + 500) / 1000 >= cur_khz)
                .unwrap_or(2);
            let next_sr = STD_RATES[(cur_idx + 1) % STD_RATES.len()];
            state.sound_settings.samplerate = next_sr;
            state.settings_dirty = true;
            state.sync_runtime_to_ini();
            self.setup_cfg.sample_rate = state.ini_config.sample_rate;
            return;
        }
        // main_Bits (8 / 16 Bit: 408..444, 28..43)
        if (408..=444).contains(&vx) && (28..=43).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.sound_settings.bits = if state.sound_settings.bits == 8 { 16 } else { 8 };
            state.settings_dirty = true;
            state.sync_runtime_to_ini();
            self.setup_cfg.bits_per_sample = state.ini_config.bits_per_sample;
            return;
        }
        // main_Clip / CL+DC (444..470, 33..45)
        if (444..=470).contains(&vx) && (33..=45).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.step_clip_declick(1);
            self.setup_cfg.clipping = state.ini_config.clipping;
            self.setup_cfg.declick = state.ini_config.declick;
            return;
        }
        // Main_MS (Stereo / Mono / Surround: 471..500, 30..43)
        if (471..=500).contains(&vx) && (30..=43).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            if state.sound_settings.stereo < 2 {
                state.sound_settings.stereo = 2;
                state.sound_settings.surround = false;
            } else if !state.sound_settings.surround {
                state.sound_settings.surround = true;
            } else {
                state.sound_settings.stereo = 1;
                state.sound_settings.surround = false;
            }
            state.settings_dirty = true;
            state.sync_runtime_to_ini();
            self.setup_cfg.channels = state.ini_config.channels;
            self.setup_cfg.surround = state.ini_config.surround;
            return;
        }
        // main_BPM (413..440, 51..63)
        if (413..=440).contains(&vx) && (51..=63).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.sound_settings.no_bpm = !state.sound_settings.no_bpm;
            state.settings_dirty = true;
            return;
        }
        // main_panning (442..467, 51..63)
        if (442..=467).contains(&vx) && (51..=63).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.sound_settings.panning = !state.sound_settings.panning;
            state.settings_dirty = true;
            state.sync_runtime_to_ini();
            self.setup_cfg.enable_panning = state.ini_config.enable_panning;
            return;
        }
        // main_IDO (472..500, 47..65): Zyklisch umschalten (0..3 bei klassischen Formaten, 0..6 bei .IT-Modulen)
        if (472..=500).contains(&vx) && (47..=65).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            state.step_oversamp(1);
            self.setup_cfg.over_sampling = state.ini_config.over_sampling;
            return;
        }
        // main_tmode (447..472, 70..81)
        if (447..=472).contains(&vx) && (70..=81).contains(&vy) {
            let mut state = self.shared_state.lock().unwrap();
            let next_tm = (state.sound_settings.timer_mode() + 1) % 3;
            state.sound_settings.set_timer_mode(next_tm);
            state.settings_dirty = true;
            state.sync_runtime_to_ini();
            self.setup_cfg.timer_mode = state.ini_config.timer_mode;
            return;
        }
        // Main_Percnt (370..407, 62..82) & PBar (368..374, 29..82):
        // Linksklick schaltet die 11 CPU-Profile vorwärts durch (Rechtsklick in `app.rs` schaltet rückwärts).
        if ((368..=374).contains(&vx) && (29..=82).contains(&vy))
            || ((370..=407).contains(&vx) && (62..=82).contains(&vy))
        {
            let mut state = self.shared_state.lock().unwrap();
            let next_prof = CpuProfile::from_u8(state.ini_config.cpu_sim_profile).next();
            state.ini_config.cpu_sim_profile = next_prof.to_u8();
            self.setup_cfg.cpu_sim_profile = state.ini_config.cpu_sim_profile;
            let banner = next_prof.lcd_banner_with_stutter(state.ini_config.authentic_stutter);
            state.set_status_banner(banner);
            let _ = state.ini_config.save_to_file(&Mod4WinIniConfig::default_ini_path());
            return;
        }
        // Main_BuffTime (408..444, 62..82) & BBar (446..452, 29..82):
        // Klick toggelt Authentic Stutter (Visual Only vs. 1995 DMA Stutter)
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

        // 7. Vertikaler PreAmp-Regler (main_PreAmp: 508..533, 26..100)
        if (508..=533).contains(&vx) && (26..=100).contains(&vy) {
            self.preamp_dragging = true;
            self.update_preamp_from_y(vy);
            return;
        }
        // 7b. Klick auf dB-Anzeige (main_preampdb: 508..555, 101..126) -> Manueller Peak-Search
        if (508..=555).contains(&vx) && (101..=126).contains(&vy) {
            let (mod_path_opt, ss_snap, mod_title) = {
                let st = self.shared_state.lock().unwrap();
                (
                    st.current_file_path.clone(),
                    st.sound_settings,
                    if !st.title.trim().is_empty() {
                        st.title.clone()
                    } else {
                        st.filename.clone()
                    },
                )
            };
            if let Some(mod_path) = mod_path_opt
                && let Ok((file_data, ext_opt)) =
                    super::archive_mol::read_module_bytes_universal(&mod_path)
                    && let Ok(m) =
                        crate::engine::loader::load_module_with_ext(&file_data, ext_opt.as_deref())
                    {
                        UiSharedState::start_or_resolve_peak_search(
                            &self.shared_state,
                            m,
                            ss_snap,
                            file_data.len() as u32,
                            mod_title,
                            true,
                        );
                    }
            return;
        }

        // 8a. Transport-Reihe in BEIDEN Modi (v2.40 und v2.30 zu 100% identisch: 7x 50x25 bei y = 102..127, x = 9..358)
        if (102..=127).contains(&vy) && (9..=358).contains(&vx) {
            if (9..=58).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Stop);
                let mut state = self.shared_state.lock().unwrap();
                state.intro_scan = false;
                state.is_playing = false;
                state.is_paused = false;
                state.pending_seek_pattern = Some(1);
                return;
            } else if (59..=108).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Play);
                self.trigger_play_or_hdd_record(event_loop);
                return;
            } else if (109..=158).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Pause);
                let mut state = self.shared_state.lock().unwrap();
                if state.is_playing || state.is_paused {
                    state.is_paused = !state.is_paused;
                    state.is_playing = !state.is_paused;
                }
                return;
            } else if (159..=208).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Prev);
                self.prev_song();
                return;
            } else if (209..=258).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Rew);
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta -= 1;
                return;
            } else if (259..=308).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Ff);
                let mut state = self.shared_state.lock().unwrap();
                state.pending_seek_delta += 1;
                return;
            } else if (309..=358).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Next);
                self.next_song();
                return;
            }
        }

        // 8b. Rechter 2x4 Button-Block in BEIDEN Modi (v2.40 und v2.30 identisch: x = 368..501, y = 89..126)
        if (368..=501).contains(&vx) && (89..=106).contains(&vy) {
            if (368..=400).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Shuffle);
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_shuffle();
                state.sync_runtime_to_ini();
                return;
            } else if (401..=433).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Repeat);
                let mut state = self.shared_state.lock().unwrap();
                state.repeat_mode = (state.repeat_mode + 1) % 3;
                state.sync_runtime_to_ini();
                return;
            } else if (434..=466).contains(&vx) {
                self.pressed_btn = Some(PressedButton::IntroScan);
                let mut state = self.shared_state.lock().unwrap();
                state.toggle_intro_scan();
                state.sync_runtime_to_ini();
                return;
            } else if (467..=501).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Time);
                let mut state = self.shared_state.lock().unwrap();
                state.minus_mode = !state.minus_mode;
                state.sync_runtime_to_ini();
                return;
            }
        }

        if (368..=501).contains(&vx) && (107..=126).contains(&vy) {
            if (368..=400).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Open);
                self.toggle_window(event_loop, WindowKind::OpenDialog);
                return;
            } else if (401..=433).contains(&vx) {
                self.pressed_btn = Some(PressedButton::FileMan);
                self.file_menu_open = !self.file_menu_open;
                self.file_menu_hover = None;
                if self.file_menu_open {
                    self.open_window(event_loop, WindowKind::FileMenu);
                } else {
                    self.close_window(WindowKind::FileMenu);
                }
                return;
            } else if (434..=466).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Info);
                self.toggle_window(event_loop, WindowKind::ModuleInfo);
                return;
            } else if (467..=501).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Effects);
                self.toggle_window(event_loop, WindowKind::Effects);
                return;
            }
        }

        // Doppelklick auf den Hauptfenster-Hintergrund öffnet die About-Ansicht (`wmLButtonDblClk` in `mod4win.pas`)
        if is_double_click && vy > 18 {
            self.toggle_window(event_loop, WindowKind::About);
            return;
        }

        // Klick auf sonstigen Hintergrund -> Fenster verschieben (setup_moveonclick)
        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn update_preamp_from_y(&mut self, vy: i32) {
        // hw = vy - 27 in 0..=72 -> raw_preamp in 0..=40 (20 = 0 dB)
        let hw = (vy - 27).clamp(0, 72);
        let raw = (20 - ((hw - 36) * 20) / 32).clamp(0, 40) as u8;
        let mut state = self.shared_state.lock().unwrap();
        let clip_bit = state.sound_settings.pre_amp & 0x80;
        state.sound_settings.pre_amp = clip_bit | raw;
        state.settings_dirty = true;
        state.sync_runtime_to_ini();
        self.setup_cfg.pre_amplification = state.ini_config.pre_amplification;
    }

    pub(crate) fn step_preamp_db(&mut self, delta_raw: i32) {
        let mut state = self.shared_state.lock().unwrap();
        let clip_bit = state.sound_settings.pre_amp & 0x80;
        let cur = (state.sound_settings.pre_amp & 0x7F) as i32;
        let next = (cur + delta_raw).clamp(0, 40) as u8;
        state.sound_settings.pre_amp = clip_bit | next;
        state.settings_dirty = true;
        state.sync_runtime_to_ini();
        self.setup_cfg.pre_amplification = state.ini_config.pre_amplification;
    }

    pub(crate) fn reset_preamp_to_zero_db(&mut self) {
        let mut state = self.shared_state.lock().unwrap();
        let clip_bit = state.sound_settings.pre_amp & 0x80;
        state.sound_settings.pre_amp = clip_bit | 20; // 20 = 0.0 dB
        state.settings_dirty = true;
        state.sync_runtime_to_ini();
        self.setup_cfg.pre_amplification = state.ini_config.pre_amplification;
    }

    pub(crate) fn handle_mini_click(
        &mut self,
        event_loop: &ActiveEventLoop,
        vx: i32,
        vy: i32,
        window: &Arc<Window>,
        is_double_click: bool,
    ) {
        if (1..=18).contains(&vy) {
            if (1..=18).contains(&vx) {
                self.pressed_btn = Some(PressedButton::SysMenu);
                let mut state = self.shared_state.lock().unwrap();
                state.is_playing = false;
                state.is_paused = false;
                state.is_finished = true;
                event_loop.exit();
                return;
            } else if (254..=269).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Minimize);
                window.set_minimized(true);
                return;
            } else if (270..=286).contains(&vx) || is_double_click {
                self.pressed_btn = Some(PressedButton::MiniMode);
                self.switch_mini_mode(event_loop, false);
                return;
            } else if (287..=306).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Close);
                let mut state = self.shared_state.lock().unwrap();
                state.is_playing = false;
                state.is_paused = false;
                state.is_finished = true;
                event_loop.exit();
                return;
            } else {
                let _ = window.drag_window();
                return;
            }
        }

        // Hit-Tests im Mini-Status:
        // Linkes Display (4..46, 22..54) -> ModuleInfo umschalten
        if (4..=46).contains(&vx) && (22..=54).contains(&vy) {
            self.toggle_window(event_loop, WindowKind::ModuleInfo);
            return;
        }
        // 5x 50x25 Transport-Buttons bei (50..300, 25..50)
        if (25..=50).contains(&vy) && (50..=300).contains(&vx) {
            if (50..=99).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Stop);
                let mut state = self.shared_state.lock().unwrap();
                state.intro_scan = false;
                state.is_playing = false;
                state.is_paused = false;
                state.pending_seek_pattern = Some(1);
            } else if (100..=149).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Play);
                self.trigger_play_or_hdd_record(event_loop);
            } else if (150..=199).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Pause);
                let mut state = self.shared_state.lock().unwrap();
                if state.is_playing || state.is_paused {
                    state.is_paused = !state.is_paused;
                    state.is_playing = !state.is_paused;
                }
            } else if (200..=249).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Prev);
                self.prev_song();
            } else if (250..=300).contains(&vx) {
                self.pressed_btn = Some(PressedButton::Next);
                self.next_song();
            }
            return;
        }

        self.drag_window_if_allowed(vy, window);
    }

    pub(crate) fn prev_song(&mut self) {
        let mut state = self.shared_state.lock().unwrap();
        let wrap = state.repeat_mode == 1;
        if state.playlist.len() > 1 {
            if state.playlist_pos == 0 && !wrap {
                state.pending_seek_pattern = Some(1);
            } else {
                state.step_song(-1, wrap);
            }
        } else {
            state.pending_seek_pattern = Some(1);
        }
    }

    pub(crate) fn next_song(&mut self) {
        let mut state = self.shared_state.lock().unwrap();
        let wrap = state.repeat_mode == 1;
        if state.playlist.len() > 1 {
            let _ = state.step_song(1, wrap);
        } else if wrap && !state.playlist.is_empty() {
            state.pending_seek_pattern = Some(1);
        }
    }
}

/// Zeichnet ein `26x17` Pixel-Art-`iT`-LCD-Badge im Stil von `SH_REP_SCAN.bmp` (`x_IDO`)
/// mit der aktiven Interpolations-/Mixer-Stufe (`ID0`/`ID1`/`CPU`/`FPU` für `0..=3`, `OFF`/`LIN`/`CUB` für `4..=6`).
pub(crate) fn draw_it_lcd_badge(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    oversamp: u8,
    bg_col: [u8; 4],
) {
    fill_rect(frame, dw, dh, x, y, 26, 17, bg_col);

    let is_dim = oversamp == 0 || oversamp == 4;
    let col_bright = if is_dim {
        [0, 148, 0, 255]
    } else {
        [0, 255, 0, 255]
    };
    let col_mid = [0, 128, 0, 255];
    let col_sub = if oversamp >= 4 {
        [0, 255, 128, 255]
    } else {
        [0, 220, 0, 255]
    };

    // Stilisierter Pixel-Art-'iT'-Schriftzug links oben (15x11) passend zur 'iD'-Grafik in SH_REP_SCAN.bmp
    const IT_LOGO: [&str; 10] = [
        " ..  ......... ",
        " ##  ######### ",
        " ..  .. ### .. ",
        " ##     ###    ",
        " ##     ###    ",
        " ##     ###    ",
        " ##     ###    ",
        " ##     ###    ",
        " ###    ###    ",
        "  ..    ...    ",
    ];
    for (ry, row_str) in IT_LOGO.iter().enumerate() {
        for (rx, ch) in row_str.chars().enumerate() {
            let c = match ch {
                '#' => Some(col_bright),
                '.' => Some(col_mid),
                _ => None,
            };
            if let Some(px_c) = c {
                let px = x + rx as i32;
                let py = y + 1 + ry as i32;
                if px >= 0 && py >= 0 && (px as u32) < dw && (py as u32) < dh {
                    let idx = ((py as u32 * dw + px as u32) * 4) as usize;
                    if idx + 3 < frame.len() {
                        frame[idx..idx + 4].copy_from_slice(&px_c);
                    }
                }
            }
        }
    }

    // 3x5 Mikro-Glyphen für die Stufen-Anzeige rechts unten (col 14..25, row 11..16)
    fn micro_3x5(ch: char) -> [u8; 5] {
        match ch {
            'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
            'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
            '0' | 'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
            '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
            'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
            'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
            'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
            'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
            'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
            'N' => [0b111, 0b101, 0b101, 0b101, 0b101],
            'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
            _ => [0; 5],
        }
    }

    let sub_lbl = match oversamp {
        0 => "ID0",
        1 => "ID1",
        2 => "CPU",
        3 => "FPU",
        4 => "OFF",
        5 => "LIN",
        _ => "CUB",
    };

    for (ci, ch) in sub_lbl.chars().enumerate() {
        let rows = micro_3x5(ch);
        let gx = x + 14 + (ci as i32) * 4;
        let gy = y + 11;
        for (ry, &bits) in rows.iter().enumerate() {
            for rx in 0..3i32 {
                if (bits & (1 << (2 - rx))) != 0 {
                    let px = gx + rx;
                    let py = gy + ry as i32;
                    if px >= 0 && py >= 0 && (px as u32) < dw && (py as u32) < dh {
                        let idx = ((py as u32 * dw + px as u32) * 4) as usize;
                        if idx + 3 < frame.len() {
                            frame[idx..idx + 4].copy_from_slice(&col_sub);
                        }
                    }
                }
            }
        }
    }
}

/// Zeichnet den `23x7` Pixel-Art-`CL+DC`-Schriftzug ohne Glow passend zu `Bit` / `BPM` / `PAN`
/// für die 3. Stellung des Clip-Schalters (`Clip + De-Click`).
pub(crate) fn draw_cl_dc_lcd_badge(
    frame: &mut [u8],
    dw: u32,
    dh: u32,
    x: i32,
    y: i32,
    bg_col: [u8; 4],
) {
    fill_rect(frame, dw, dh, x, y, 23, 7, bg_col);
    let col_bright = [0, 255, 0, 255];

    const CL_DC_SPRITE: [&str; 7] = [
        " ### #        ###   ###",
        "#    #        #  # #   ",
        "#    #     #  #  # #   ",
        "#    #    ### #  # #   ",
        "#    #     #  #  # #   ",
        "#    #        #  # #   ",
        " ### ####     ###   ###",
    ];

    for (ry, row_str) in CL_DC_SPRITE.iter().enumerate() {
        for (rx, ch) in row_str.chars().enumerate() {
            if ch == '#' {
                let px = x + rx as i32;
                let py = y + ry as i32;
                if px >= 0 && py >= 0 && (px as u32) < dw && (py as u32) < dh {
                    let idx = ((py as u32 * dw + px as u32) * 4) as usize;
                    if idx + 3 < frame.len() {
                        frame[idx..idx + 4].copy_from_slice(&col_bright);
                    }
                }
            }
        }
    }
}
