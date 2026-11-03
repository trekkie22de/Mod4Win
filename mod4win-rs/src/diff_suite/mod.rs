#[cfg(not(all(
    any(target_os = "windows", target_os = "linux"),
    any(target_arch = "x86", target_arch = "x86_64")
)))]
compile_error!("The `diff-suite` feature is only supported on x86/x86_64 Windows and Linux targets.");

pub mod batch_runner;
pub mod compare;
pub mod patcher;

pub use batch_runner::{
    md5_short_hex_6, render_ffmpeg_reference_pcm, resolve_ffmpeg_available, resolve_ffmpeg_binary,
    resolve_mod4win_dir, resolve_otvdm_or_wine_runner, resolve_work_dir, run_batch_ffmpeg_diff,
    run_batch_m4w_diff, run_batch_verify, BatchFfmpegDiffEntry, BatchFfmpegDiffOptions,
    BatchFfmpegDiffReport, BatchM4wDiffOptions, BatchVerifyFileEntry, BatchVerifyReport,
};
pub use compare::{
    first_nonzero_frame, format_module_header, load_wav_i16, run_diff_suite,
    run_ffmpeg_stage_comparison_to_string, run_single_ido_diff_in_memory,
    run_stage_comparison_to_string, WavData,
};
pub use patcher::{
    ensure_mod4win_test_environment, patch_mod4win_exe_bytes, patch_player32_dll_bytes,
    MOD4WIN_EXE_SIZE, PLAYER32_DLL_SIZE,
};

