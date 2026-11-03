pub mod app;
pub mod archive_mol;
pub mod assets;
pub mod batch_cli;
pub mod bmp;
pub mod cpu_sim;
pub mod effects_win;
pub mod file_mgr;
pub mod font;
pub mod help_win;
pub mod hotkeys_dlg;
pub mod i18n;
pub mod info_win;
pub mod ini_config;
pub mod live_audio;
pub mod main_win;
pub mod modern_skin;
pub mod open_dlg;
pub mod setup_dlg;
pub mod theme_modern;
pub mod touch_gesture;
pub mod widgets;
pub mod winhelp;

#[cfg(feature = "retro-win32")]
pub mod retro_host;
#[cfg(feature = "retro-win32")]
pub use retro_host::{pixels, winit};

#[cfg(not(feature = "retro-win32"))]
pub use ::pixels;
#[cfg(not(feature = "retro-win32"))]
pub use ::winit;

pub use app::{
    note_to_str, InstrumentInfo, Mod4WinApp, PendingPeakSearch, UiSharedState, VIRTUAL_HEIGHT,
    VIRTUAL_WIDTH,
};
pub use archive_mol::{
    parse_zip_virtual_path, read_module_bytes_universal, resolve_path_case_insensitive,
};
pub use assets::EmbeddedAssets;
pub use batch_cli::{
    parse_wav_bytes_i16, run_headless_render_wav, ParsedWavData, RenderWavCliOptions,
};
#[cfg(feature = "diff-suite")]
pub use batch_cli::{
    render_ffmpeg_reference_pcm, resolve_ffmpeg_binary, run_batch_ffmpeg_diff, run_batch_m4w_diff,
    run_batch_verify, BatchFfmpegDiffOptions, BatchFfmpegDiffReport, BatchM4wDiffOptions,
    BatchVerifyReport,
};
pub use bmp::BmpImage;
pub use cpu_sim::{CpuProfile, CpuTelemetryState};
pub use i18n::LocaleCatalog;
pub use ini_config::{
    default_hotkey_bindings, export_module_to_wav_file, run_peak_search_isolated, HotkeyBinding,
    Mod4WinIniConfig, DEV_HDD, DEV_SOUNDCARD,
};
pub use live_audio::{
    apply_live_sound_settings, compute_module_total_time_ms_on_seq, handle_audio_cycle_and_sync,
    pcm8_to_i16, sync_ui_commands, LiveAudioProcessor,
};
pub use theme_modern::SkinMode;
pub use touch_gesture::TouchGestureState;

#[cfg(not(feature = "retro-win32"))]
use std::sync::{Arc, Mutex};
#[cfg(not(feature = "retro-win32"))]
use winit::event_loop::{ControlFlow, EventLoop};

/// Deaktiviert unter Windows 11 das Background Power-Throttling (EcoQoS) und Timer-Coalescing,
/// und setzt die System-Timer-Auflösung auf 1 ms (`timeBeginPeriod(1)`), damit alle offenen
/// MOD4WIN-Fenster (insb. der 60-FPS Effects-Analyzer) auch ohne Fensterfokus flüssig weiterlaufen.
#[cfg(all(target_os = "windows", not(feature = "retro-win32")))]
pub fn disable_windows_background_throttling() {
    #[repr(C)]
    struct ProcessPowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn timeBeginPeriod(u_period: u32) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(
            h_process: isize,
            process_information_class: i32,
            process_information: *const std::ffi::c_void,
            process_information_size: u32,
        ) -> i32;
    }

    const PROCESS_POWER_THROTTLING: i32 = 4;
    const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;
    const PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION: u32 = 0x4;

    unsafe {
        let _ = timeBeginPeriod(1);
        let h_proc = GetCurrentProcess();
        let state_both = ProcessPowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            state_mask: 0,
        };
        let ok = SetProcessInformation(
            h_proc,
            PROCESS_POWER_THROTTLING,
            &state_both as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
        );
        if ok == 0 {
            let state_exec = ProcessPowerThrottlingState {
                version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                state_mask: 0,
            };
            let _ = SetProcessInformation(
                h_proc,
                PROCESS_POWER_THROTTLING,
                &state_exec as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
            );
        }
    }
}

/// Startet die MOD4WIN Multi-Window-Oberfläche über Winit und Pixels auf dem Hauptthread.
#[cfg(not(feature = "retro-win32"))]
pub fn run_ui(shared_state: Arc<Mutex<UiSharedState>>) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    disable_windows_background_throttling();

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = Mod4WinApp::new(shared_state);
    event_loop.run_app(&mut app)?;
    Ok(())
}
