//! Multi-Core Batch-Diff-Suite (`--batch-diff`, `--batch-ffmpeg-diff`, `--batch-verify`).
//!
//! Integriert die komplette Logik aus `diff-suite/Mod_Batch.ps1` direkt nativ in Rust:
//! - Automatischer Selbst-Patcher für `MOD4WIN_IDO0..3.EXE` und `PLAYER32.DLL`
//! - Parallele Worker mit intelligentem CPU-Idle- und WAV-Stall-Watchdog
//! - Direktes Einlesen der temporären WAV in den RAM und In-Process-Diff ohne Prozess-Spawn
//! - Automatischer Modus-Wechsel auf `FFmpeg` (`libopenmpt` In-RAM-Pipe) bei `.IT`- und `.UMX`-Modulen
//! - Unterstützung für `otvdmw.exe` (x86/x86_64 Windows) sowie `wine` + `/dev/shm` RAM-Disk (x86/x86_64 Linux)

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::engine::loader::load_module_with_ext;
use crate::engine::mixer::{process_sample_block, MixOutputBuffer};
use crate::engine::sequencer::Sequencer;
use crate::engine::types::{ModType, SoundSettings, DEV_HDD};
use crate::ui::app::UiSharedState;
use crate::ui::archive_mol::{
    extract_zip_entry_bytes, is_supported_archive_ext, parse_zip_entries,
};
use crate::ui::batch_cli::parse_wav_bytes_i16;
use crate::ui::ini_config::run_peak_search_isolated;

use super::compare::{
    format_module_header, run_ffmpeg_stage_comparison_to_string, run_single_ido_diff_in_memory,
    WavData,
};
use super::patcher::{build_headless_mod4win_ini, ensure_mod4win_test_environment};

// ============================================================================
// 1. Reines Rust-MD5 (RFC 1321) für 100% kompatible Duplikat-Hashes zu Mod_Batch.ps1
// ============================================================================

/// Berechnet den ersten 6-Zeichen-Hex-MD5-Hash (`Substring(0, 6)`) wie in `Mod_Batch.ps1`.
pub fn md5_short_hex_6(data: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
        5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(K[i])
                .wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let bytes = a0.to_le_bytes();
    format!("{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2])
}

// ============================================================================
// 2. Plattform-Helfer für Prozess-CPU-Zeit & Prozessbaum-Terminierung
// ============================================================================

#[cfg(target_os = "windows")]
mod os_proc {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    struct FileTime {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessTimes(
            h_process: isize,
            lp_creation_time: *mut FileTime,
            lp_exit_time: *mut FileTime,
            lp_kernel_time: *mut FileTime,
            lp_user_time: *mut FileTime,
        ) -> i32;
        fn CreateJobObjectW(lp_job_attributes: *const std::ffi::c_void, lp_name: *const u16) -> isize;
        fn SetInformationJobObject(
            h_job: isize,
            job_object_info_class: i32,
            lp_job_object_info: *const std::ffi::c_void,
            cb_job_object_info_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(h_job: isize, h_process: isize) -> i32;
        fn CloseHandle(h_object: isize) -> i32;
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x00002000;

    /// Verwaltet ein Win32 Job Object, das bei Programmende (oder `Strg+C`) automatisch alle
    /// noch laufenden `otvdmw.exe`-Worker-Prozesse beendet.
    pub struct WorkerJobGuard {
        handle: isize,
    }

    impl WorkerJobGuard {
        pub fn new() -> Self {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h != 0 {
                    let mut info = JobObjectExtendedLimitInformation::default();
                    info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    let _ = SetInformationJobObject(
                        h,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        &info as *const _ as *const std::ffi::c_void,
                        std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
                    );
                }
                Self { handle: h }
            }
        }

        pub fn assign(&self, child: &Child) {
            if self.handle != 0 {
                unsafe {
                    let h_proc = child.as_raw_handle() as isize;
                    let _ = AssignProcessToJobObject(self.handle, h_proc);
                }
            }
        }
    }

    impl Drop for WorkerJobGuard {
        fn drop(&mut self) {
            if self.handle != 0 {
                unsafe {
                    let _ = CloseHandle(self.handle);
                }
            }
        }
    }

    /// Liest die verbrauchte CPU-Zeit (`KernelTime + UserTime`) eines Kindprozesses in Millisekunden.
    pub fn get_process_cpu_ms(child: &Child, _worker_tag: Option<&str>) -> Option<f64> {
        unsafe {
            let h_proc = child.as_raw_handle() as isize;
            let mut creation = FileTime::default();
            let mut exit = FileTime::default();
            let mut kernel = FileTime::default();
            let mut user = FileTime::default();
            if GetProcessTimes(h_proc, &mut creation, &mut exit, &mut kernel, &mut user) != 0 {
                let k_100ns =
                    ((kernel.dw_high_date_time as u64) << 32) | (kernel.dw_low_date_time as u64);
                let u_100ns =
                    ((user.dw_high_date_time as u64) << 32) | (user.dw_low_date_time as u64);
                Some((k_100ns + u_100ns) as f64 / 10_000.0)
            } else {
                None
            }
        }
    }

    /// Beendet einen hängenden Worker-Prozessbaum (`taskkill /F /T /PID`).
    pub fn kill_process_tree(child: &mut Child, _wine_prefix: Option<&std::path::Path>) {
        let pid = child.id();
        let _ = Command::new("taskkill.exe")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(target_os = "linux")]
mod os_proc {
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    pub struct WorkerJobGuard;

    impl WorkerJobGuard {
        pub fn new() -> Self {
            Self
        }
        pub fn assign(&self, _child: &Child) {}
    }

    fn parse_proc_stat_ticks_and_pgrp(pid: u32) -> Option<(u64, i32)> {
        let stat_str = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        let close_paren = stat_str.rfind(')')?;
        let rest = stat_str.get(close_paren + 2..)?;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        // Nach `(comm)` ist Feld 0 = state (Feld 3 in man 5 proc),
        // Feld 2 = pgrp (Feld 5), Feld 11 = utime (14), Feld 12 = stime (15).
        let pgrp: i32 = fields.get(2)?.parse().ok()?;
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        Some((utime.saturating_add(stime), pgrp))
    }

    fn proc_has_worker_tag(pid: u32, tag_needle: &[u8]) -> bool {
        let Ok(env_bytes) = fs::read(format!("/proc/{}/environ", pid)) else {
            return false;
        };
        env_bytes
            .split(|&b| b == 0)
            .any(|entry| entry == tag_needle)
    }

    /// Liest die verbrauchte CPU-Zeit (`utime + stime` über Prozessgruppe & Wine-Worker-Tag) in Millisekunden.
    ///
    /// Unter Wine wird 16-Bit `winevdm.exe` vom `wineserver` außerhalb der direkten Kind-PID
    /// gestartet; über `M4W_WTAG=<worker>` werden `wine`, `winevdm.exe` und `wineserver` dieses
    /// Workers exakt zusammengezählt.
    pub fn get_process_cpu_ms(child: &Child, worker_tag: Option<&str>) -> Option<f64> {
        let root_pid = child.id();
        let root_pgrp = root_pid as i32;
        let tag_str = worker_tag.map(|t| format!("M4W_WTAG={}", t));
        let tag_bytes = tag_str.as_deref().map(str::as_bytes);

        let Ok(entries) = fs::read_dir("/proc") else {
            let (ticks, _) = parse_proc_stat_ticks_and_pgrp(root_pid)?;
            return Some((ticks * 10) as f64);
        };

        let mut total_ticks: u64 = 0;
        let mut matched_any = false;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid_str) = name.to_str() else {
                continue;
            };
            let Ok(pid) = pid_str.parse::<u32>() else {
                continue;
            };
            let Some((ticks, pgrp)) = parse_proc_stat_ticks_and_pgrp(pid) else {
                continue;
            };
            let belongs = pid == root_pid
                || pgrp == root_pgrp
                || tag_bytes
                    .map(|needle| proc_has_worker_tag(pid, needle))
                    .unwrap_or(false);
            if belongs {
                total_ticks = total_ticks.saturating_add(ticks);
                matched_any = true;
            }
        }

        if matched_any {
            // Standard-USER_HZ unter Linux ist 100 Ticks/Sekunde (10 ms pro Tick)
            Some((total_ticks * 10) as f64)
        } else {
            None
        }
    }

    /// Beendet die Prozessgruppe sowie ggf. den isolierten `wineserver` des Wine-Workers unter Linux.
    pub fn kill_process_tree(child: &mut Child, wine_prefix: Option<&Path>) {
        let pid = child.id();
        let _ = Command::new("kill")
            .args(["-9", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
        if let Some(pfx) = wine_prefix {
            let _ = Command::new("wineserver")
                .arg("-k9")
                .env("WINEPREFIX", pfx)
                .env("WINEDEBUG", "-all")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

// ============================================================================
// 3. Automatisierter Multi-Core MOD4WIN- & FFmpeg-Batch-Diff (Mod_Batch.ps1 in Rust)
// ============================================================================

/// Konfigurationsparameter für den automatisierten Komplett-Batch-Lauf (`--batch-diff`).
#[derive(Clone, Debug)]
pub struct BatchM4wDiffOptions {
    /// Quellordner (oder Einzeldatei) mit den zu testenden Tracker-Modulen.
    pub mod_source: PathBuf,
    /// Zielordner für `pass/`, `fail/`, `timeout/`, `logs/`, `wav/`, `mods/`.
    pub result_base: PathBuf,
    /// Ordner mit dem originalen `MOD4WIN.EXE` & `PLAYER32.DLL` (wird automatisch zu `MOD4WIN_IDO0..3.EXE` gepatcht).
    pub m4w_dir: Option<PathBuf>,
    /// Pfad zu `otvdmw.exe` (unter Windows) bzw. `wine` (unter Linux).
    pub otvdm_or_wine: Option<String>,
    /// Optionaler Pfad zu `ffmpeg` (für automatischen Modus-Wechsel bei `.IT`-/.`UMX`-Modulen).
    pub ffmpeg_bin: Option<String>,
    /// Kurzes Arbeitsverzeichnis für temporäre `W<id>.MOD` und `W<id>.WAV` (z. B. `R:\` oder `/dev/shm/m4w_work`).
    pub work_dir: Option<PathBuf>,
    /// Anzahl paralleler Worker (Default: logische CPU-Kerne).
    pub workers: Option<usize>,
    /// Zu testende IDO-Modi (Default: `[0, 1, 2, 3]`).
    pub idos: Vec<u8>,
    /// Watchdog: Sekunden mit 0% CPU bis zum Kill („Defekte Datei"-Dialog, Default: `6`).
    pub max_idle_sec: u64,
    /// Watchdog: Maximale Sekunden in der 100%-CPU PeakSearch ohne WAV-Schreiben (Default: `600`).
    pub max_active_peak_sec: u64,
    /// Watchdog: Maximale Sekunden ohne WAV-Größenwachstum während des Schreibens (Default: `25`).
    pub max_stall_sec: u64,
    /// Watchdog: Maximale WAV-Dateigröße in MB vor Endlos-Loop-Abbruch (Default: `350`).
    pub max_wav_mb: u64,
    /// Wiederholt gezielt bisherige `fail/*.TXT`-Einträge im Ergebnisordner.
    pub retry_fails: bool,
    /// Wiederholt gezielt bisherige `timeout/*.TXT`-Einträge im Ergebnisordner.
    pub retry_timeouts: bool,
    /// Erlaubt 1-LSB-Rundungsabweichungen als `PASS` (Default: `false` = streng 0 LSB).
    pub allow_1lsb: bool,
}

impl Default for BatchM4wDiffOptions {
    fn default() -> Self {
        Self {
            mod_source: PathBuf::new(),
            result_base: PathBuf::new(),
            m4w_dir: None,
            otvdm_or_wine: None,
            ffmpeg_bin: None,
            work_dir: None,
            workers: None,
            idos: vec![0, 1, 2, 3],
            max_idle_sec: 6,
            max_active_peak_sec: 600,
            max_stall_sec: 25,
            max_wav_mb: 350,
            retry_fails: false,
            retry_timeouts: false,
            allow_1lsb: false,
        }
    }
}

/// Sucht automatisch nach dem Ordner mit den originalen `MOD4WIN.EXE`- und `PLAYER32.DLL`-Dateien.
pub fn resolve_mod4win_dir(user_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(d) = user_dir {
        if d.exists() {
            return Some(d.to_path_buf());
        }
    }
    let candidates = [
        PathBuf::from(r"F:\ModBatchWork\Mod4Win"),
        PathBuf::from("orig_bin"),
        PathBuf::from(r"..\orig_bin"),
    ];
    for c in candidates {
        if c.exists() && (c.join("MOD4WIN.EXE").exists() || c.join("MOD4WIN.EXE.bak").exists() || c.join("MOD4WIN_IDO0.EXE").exists()) {
            return Some(c);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for rel in ["orig_bin", "../orig_bin", "../../orig_bin"] {
                let p = parent.join(rel);
                if p.exists() && (p.join("MOD4WIN.EXE").exists() || p.join("MOD4WIN.EXE.bak").exists()) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Sucht unter Windows nach `otvdmw.exe` (oder unter Linux nach `wine`).
pub fn resolve_otvdm_or_wine_runner(user_arg: Option<&str>) -> Option<String> {
    if let Some(s) = user_arg {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"F:\ModBatchWork\otvdm\otvdmw.exe",
            r"C:\otvdm\otvdmw.exe",
            r"otvdm\otvdmw.exe",
            "otvdmw.exe",
        ];
        for c in candidates {
            let p = Path::new(c);
            if p.exists() && p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
        if Command::new("otvdmw.exe")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Some("otvdmw.exe".to_string());
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        if Command::new("wine")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Some("wine".to_string());
        }
        None
    }
}

/// Prüft, ob `ffmpeg` ausführbar ist, und gibt den Binärpfad zurück.
pub fn resolve_ffmpeg_available(user_arg: Option<&str>) -> Option<String> {
    let bin = resolve_ffmpeg_binary(user_arg);
    if Command::new(&bin)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Some(bin)
    } else {
        None
    }
}

/// Ermittelt ein kurzes, 16-Bit-kompatibles Arbeitsverzeichnis (bevorzugt RAM-Disk `R:\` bzw. `/dev/shm`).
pub fn resolve_work_dir(user_dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(d) = user_dir {
        fs::create_dir_all(d).map_err(|e| {
            format!(
                "Konnte Arbeitsverzeichnis '{}' nicht anlegen: {}",
                d.display(),
                e
            )
        })?;
        return Ok(d.to_path_buf());
    }
    #[cfg(target_os = "linux")]
    {
        let shm = PathBuf::from("/dev/shm/m4w_work");
        if Path::new("/dev/shm").exists() && fs::create_dir_all(&shm).is_ok() {
            return Ok(shm);
        }
        let tmp = PathBuf::from("/tmp/m4w_work");
        fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        Ok(tmp)
    }
    #[cfg(target_os = "windows")]
    {
        let r_drive = PathBuf::from(r"R:\");
        if r_drive.exists() {
            return Ok(r_drive);
        }
        let c_temp = PathBuf::from(r"C:\Temp\m4w_work");
        if fs::create_dir_all(&c_temp).is_ok() {
            return Ok(c_temp);
        }
        let fallback = std::env::temp_dir().join("m4w_work");
        fs::create_dir_all(&fallback).map_err(|e| e.to_string())?;
        Ok(fallback)
    }
}

/// Wandelt unter Linux einen Unix-Pfad (`/dev/shm/m4w_work/W0.MOD`) in einen Wine-DOS-Pfad
/// (`Z:\dev\shm\m4w_work\W0.MOD`) um; unter Windows bleibt der Pfad unverändert.
fn to_win16_runner_path(p: &Path) -> String {
    #[cfg(target_os = "linux")]
    {
        let abs = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = abs.to_string_lossy();
        format!("Z:{}", s.replace('/', "\\"))
    }
    #[cfg(target_os = "windows")]
    {
        p.to_string_lossy().into_owned()
    }
}

/// Erkennt sowohl klassische PC-Dateiendungen (`.mod`, `.nst`, `.xm`, `.s3m`, …) als auch
/// Amiga-/Modland-/Aminet-Präfixnamen (`mod.space debris`, `okt.song`, `xm.track`, …).
/// Gibt `(logische_ext_klein, dos_8_3_endung)` zurück, z. B. `("mod", ".MOD")`.
fn detect_batch_mod_format(path: &Path) -> Option<(&'static str, &'static str)> {
    fn map_token(tok: &str) -> Option<(&'static str, &'static str)> {
        match tok {
            "mod" | "nst" => Some(("mod", ".MOD")),
            "xm" => Some(("xm", ".XM")),
            "s3m" => Some(("s3m", ".S3M")),
            "mtm" => Some(("mtm", ".MTM")),
            "669" => Some(("669", ".669")),
            "ult" => Some(("ult", ".ULT")),
            "far" => Some(("far", ".FAR")),
            "stm" => Some(("stm", ".STM")),
            "okt" | "okta" => Some(("okt", ".OKT")),
            "wow" => Some(("wow", ".WOW")),
            "it" => Some(("it", ".IT")),
            "umx" => Some(("umx", ".UMX")),
            _ => None,
        }
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if let Some(mapped) = map_token(&ext_lower) {
            return Some(mapped);
        }
        // Nicht-Modul-Artefakte (z. B. `mod.song.0.TXT`, `.wav`, `.zip`) niemals als Amiga-Präfix werten
        if matches!(
            ext_lower.as_str(),
            "txt"
                | "wav"
                | "log"
                | "bak"
                | "ini"
                | "pks"
                | "exe"
                | "dll"
                | "bmp"
                | "hlp"
                | "mol"
                | "zip"
                | "7z"
                | "rar"
                | "lha"
                | "lzh"
                | "gz"
                | "xz"
                | "bz2"
                | "md"
                | "json"
                | "csv"
        ) {
            return None;
        }
    }

    let file_name = path.file_name().and_then(|n| n.to_str())?;
    if file_name.starts_with('.') {
        return None;
    }
    let (prefix, rest) = file_name.split_once('.')?;
    if rest.trim().is_empty() {
        return None;
    }
    map_token(&prefix.to_ascii_lowercase())
}

fn collect_mod_files_recursive(dir_or_file: &Path, out: &mut Vec<PathBuf>) {
    if dir_or_file.is_file() {
        if detect_batch_mod_format(dir_or_file).is_some() {
            out.push(dir_or_file.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(dir_or_file) else {
        return;
    };
    let mut sub_paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|d| d.path())).collect();
    sub_paths.sort();
    for p in sub_paths {
        if p.is_dir() {
            collect_mod_files_recursive(&p, out);
        } else if p.is_file() && detect_batch_mod_format(&p).is_some() {
            out.push(p);
        }
    }
}

/// Findet die von 16-Bit `MOD4WIN.EXE` geschriebene Worker-WAV-Datei unabhängig von der
/// Groß-/Kleinschreibung auf Linux-Dateisystemen (`tmpfs` schreibt z. B. `w0.WAV` statt `W0.WAV`).
fn find_worker_wav_variant(work_dir: &Path, prefix: &str) -> Option<PathBuf> {
    let lower = prefix.to_ascii_lowercase();
    let upper = prefix.to_ascii_uppercase();
    let candidates = [
        format!("{}.WAV", lower),
        format!("{}.WAV", upper),
        format!("{}.wav", lower),
        format!("{}.wav", upper),
    ];
    for c in candidates {
        let p = work_dir.join(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn remove_worker_wav_variants(work_dir: &Path, prefix: &str) {
    let lower = prefix.to_ascii_lowercase();
    let upper = prefix.to_ascii_uppercase();
    for c in [
        format!("{}.WAV", lower),
        format!("{}.WAV", upper),
        format!("{}.wav", lower),
        format!("{}.wav", upper),
    ] {
        let _ = fs::remove_file(work_dir.join(c));
    }
}

#[cfg(target_os = "linux")]
mod linux_wine {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::build_headless_mod4win_ini;

    pub const WINE_DLL_OVERRIDES: &str = "mscoree,mshtml,winemenubuilder.exe,winedbg.exe=d";

    pub fn is_wine_runner(runner_bin: &str) -> bool {
        let name = Path::new(runner_bin)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(runner_bin)
            .to_ascii_lowercase();
        name == "wine" || name.starts_with("wine32") || name.starts_with("wine64")
    }

    fn is_readonly_wine_system_asset(path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "dll"
                | "exe"
                | "exe16"
                | "dll16"
                | "drv"
                | "drv16"
                | "sys"
                | "ocx"
                | "cpl"
                | "acm"
                | "ax"
                | "tlb"
                | "nls"
                | "ttf"
                | "fon"
                | "icm"
        )
    }

    fn copy_tree_preserving_symlinks(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let meta = fs::symlink_metadata(&src_path)?;
            let ft = meta.file_type();
            if ft.is_symlink() {
                let target = fs::read_link(&src_path)?;
                let _ = fs::remove_file(&dst_path);
                symlink(&target, &dst_path)?;
            } else if ft.is_dir() {
                copy_tree_preserving_symlinks(&src_path, &dst_path)?;
            } else if ft.is_file() {
                // Ein 32-Bit Wine-Prefix belegt ~587 MB an unveränderlichen System-DLLs/-EXEs.
                // Per Hardlink auf /dev/shm teilen sich alle 16 Worker dieselben Inodes & Page-Cache-Seiten
                // (~590 MB gesamt statt ~10 GB), während .reg- und .ini-Dateien pro Worker echt kopiert werden.
                if is_readonly_wine_system_asset(&src_path) {
                    if fs::hard_link(&src_path, &dst_path).is_err() {
                        fs::copy(&src_path, &dst_path)?;
                    }
                } else {
                    fs::copy(&src_path, &dst_path)?;
                }
            }
        }
        Ok(())
    }

    /// Initialisiert einmalig vor dem Start der Worker-Threads ein sauberes 32/16-Bit `_pfx_template`
    /// in `work_dir` (inkl. `WINEARCH=win32`, `Managed=N` für Xvfb ohne Window-Manager und
    /// `[Player32] UseHardware=0` in `win.ini`).
    pub fn ensure_wine_template_prefix(work_dir: &Path) -> Result<PathBuf, String> {
        let template_dir = work_dir.join("_pfx_template");
        if !template_dir.join("system.reg").exists() {
            println!(
                "[Wine-Init] Initialisiere isoliertes 16/32-Bit Wine-Template in '{}' ...",
                template_dir.display()
            );
            let _ = fs::remove_dir_all(&template_dir);
            fs::create_dir_all(&template_dir).map_err(|e| {
                format!(
                    "Konnte Wine-Template '{}' nicht anlegen: {}",
                    template_dir.display(),
                    e
                )
            })?;

            let status = Command::new("wineboot")
                .arg("--init")
                .env("WINEPREFIX", &template_dir)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .env("WINEDLLOVERRIDES", WINE_DLL_OVERRIDES)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("Konnte `wineboot --init` nicht ausführen: {}", e))?;
            if !status.success() {
                return Err(format!(
                    "`wineboot --init` fehlgeschlagen (Exit-Code: {:?})",
                    status.code()
                ));
            }

            // Deaktiviere X11-Window-Manager-Synchronisation für Headless-Xvfb (vermeidet 2s Withdraw-Timeout)
            let _ = Command::new("wine")
                .args([
                    "reg",
                    "add",
                    r"HKCU\Software\Wine\X11 Driver",
                    "/v",
                    "Managed",
                    "/t",
                    "REG_SZ",
                    "/d",
                    "N",
                    "/f",
                ])
                .env("WINEPREFIX", &template_dir)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .env("WINEDLLOVERRIDES", WINE_DLL_OVERRIDES)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            // Unterdrücke modale Wine-Crash-Dialoge (`winedbg --auto`), falls 16-Bit `winevdm.exe`
            // beim DPMI-Exit nach geschriebener WAV oder bei korrupten Modland-Dateien faultet.
            let _ = Command::new("wine")
                .args([
                    "reg",
                    "add",
                    r"HKCU\Software\Wine\WineDbg",
                    "/v",
                    "ShowCrashDialog",
                    "/t",
                    "REG_DWORD",
                    "/d",
                    "0",
                    "/f",
                ])
                .env("WINEPREFIX", &template_dir)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .env("WINEDLLOVERRIDES", WINE_DLL_OVERRIDES)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            let _ = Command::new("wine")
                .args([
                    "reg",
                    "delete",
                    r"HKLM\Software\Microsoft\Windows NT\CurrentVersion\AeDebug",
                    "/f",
                ])
                .env("WINEPREFIX", &template_dir)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .env("WINEDLLOVERRIDES", WINE_DLL_OVERRIDES)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            let _ = Command::new("wineserver")
                .arg("-w")
                .env("WINEPREFIX", &template_dir)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }

        let win_dir = template_dir.join("drive_c").join("windows");
        let _ = fs::create_dir_all(&win_dir);

        // Stelle sicher, dass PLAYER32.DLL keinen ISA-Port-Scan (GUS/EWS64) im DPMI-Modus ausführt
        let win_ini_path = win_dir.join("win.ini");
        let existing_win_ini = fs::read_to_string(&win_ini_path).unwrap_or_default();
        if !existing_win_ini.to_ascii_lowercase().contains("[player32]") {
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&win_ini_path)
            {
                let _ = write!(
                    f,
                    "\r\n[Player32]\r\nUseHardware=0\r\nPALTiming=0\r\nPeakSearch=1\r\nShowPeakSearch=0\r\nExactPeakSearch=1\r\n"
                );
            }
        }

        let headless_ini = build_headless_mod4win_ini("R:\\");
        let _ = fs::write(win_dir.join("MOD4WIN.INI"), headless_ini.as_bytes());

        Ok(template_dir)
    }

    /// Isolierte pro-Worker-Umgebung (`WINEPREFIX` + eigenes `m4w_W<id>` + persistenter `wineserver -p3`),
    /// damit 16+ parallele 16-Bit-`MOD4WIN.EXE`-/DPMI-`PLAYER32.DLL`-Instanzen völlig lockfrei skalieren.
    pub struct LinuxWineWorkerGuard {
        pub worker_tag: String,
        pub wine_prefix: PathBuf,
        pub worker_m4w_dir: PathBuf,
        ini_bytes: Vec<u8>,
    }

    impl LinuxWineWorkerGuard {
        pub fn setup(
            worker_id: usize,
            work_dir: &Path,
            m4w_dir: &Path,
            template_dir: &Path,
        ) -> Result<Self, String> {
            let worker_tag = format!("W{}", worker_id);
            let wine_prefix = work_dir.join(format!("pfx_{}", worker_tag));
            let worker_m4w_dir = work_dir.join(format!("m4w_{}", worker_tag));

            let _ = Command::new("wineserver")
                .arg("-k9")
                .env("WINEPREFIX", &wine_prefix)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = fs::remove_dir_all(&wine_prefix);
            let _ = fs::remove_dir_all(&worker_m4w_dir);

            fs::create_dir_all(&worker_m4w_dir)
                .map_err(|e| format!("m4w-Worker-Dir Fehler ({}): {}", worker_tag, e))?;

            let abs_m4w_src = fs::canonicalize(m4w_dir).unwrap_or_else(|_| m4w_dir.to_path_buf());
            let entries = fs::read_dir(&abs_m4w_src)
                .map_err(|e| format!("Konnte '{}' nicht lesen: {}", abs_m4w_src.display(), e))?;
            for entry in entries.flatten() {
                let src_p = entry.path();
                let fname = entry.file_name();
                let fname_upper = fname.to_string_lossy().to_ascii_uppercase();
                // MOD4WIN.INI und MOD4WIN.PKS müssen pro Worker privat sein; alte Desktop-Playlists (.STA) ausblenden!
                if matches!(
                    fname_upper.as_str(),
                    "MOD4WIN.INI" | "MOD4WIN.PKS" | "MOD4WIN.STA"
                ) {
                    continue;
                }
                let dst_p = worker_m4w_dir.join(&fname);
                if symlink(&src_p, &dst_p).is_err() && src_p.is_file() {
                    let _ = fs::copy(&src_p, &dst_p);
                }
            }

            let ini_bytes = build_headless_mod4win_ini("R:\\").into_bytes();
            let _ = fs::write(worker_m4w_dir.join("MOD4WIN.INI"), &ini_bytes);

            copy_tree_preserving_symlinks(template_dir, &wine_prefix)
                .map_err(|e| format!("Wine-Prefix-Klon fehlgeschlagen ({}): {}", worker_tag, e))?;

            let win_dir = wine_prefix.join("drive_c").join("windows");
            let _ = fs::create_dir_all(&win_dir);
            let _ = fs::write(win_dir.join("MOD4WIN.INI"), &ini_bytes);

            // Kurze 8.3 DOS-Laufwerke `M:` (Worker-M4W) und `R:` (RAM-Work-Dir) eintragen
            let dosdevices = wine_prefix.join("dosdevices");
            let _ = fs::create_dir_all(&dosdevices);
            let abs_worker_m4w =
                fs::canonicalize(&worker_m4w_dir).unwrap_or_else(|_| worker_m4w_dir.clone());
            let abs_work_dir = fs::canonicalize(work_dir).unwrap_or_else(|_| work_dir.to_path_buf());
            let m_link = dosdevices.join("m:");
            let r_link = dosdevices.join("r:");
            let _ = fs::remove_file(&m_link);
            let _ = fs::remove_file(&r_link);
            let _ = symlink(&abs_worker_m4w, &m_link);
            let _ = symlink(&abs_work_dir, &r_link);

            let guard = Self {
                worker_tag,
                wine_prefix,
                worker_m4w_dir,
                ini_bytes,
            };
            guard.start_warm_wineserver();
            Ok(guard)
        }

        pub fn start_warm_wineserver(&self) {
            let _ = Command::new("wineserver")
                .arg("-p")
                .env("WINEPREFIX", &self.wine_prefix)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .env("M4W_WTAG", &self.worker_tag)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }

        /// Bereinigt vor jedem 16-Bit-Aufruf etwaige State-/PeakSearch-Reste (`MOD4WIN.sta`,
        /// `MOD4WIN.PKS`) im RAM-Verzeichnis und stellt eine frische Headless-`MOD4WIN.INI` her.
        pub fn prepare_before_run(&self) {
            for stale in ["MOD4WIN.PKS", "mod4win.pks", "MOD4WIN.STA", "MOD4WIN.sta", "mod4win.sta"] {
                let _ = fs::remove_file(self.worker_m4w_dir.join(stale));
            }
            let _ = fs::write(self.worker_m4w_dir.join("MOD4WIN.INI"), &self.ini_bytes);
        }

        /// Stellt nach einem harten Timeout-Kill (`wineserver -k9`) `MOD4WIN.INI` und den warmen
        /// `wineserver` für das nächste Modul sauber wieder her.
        pub fn reset_after_abort(&self) {
            self.prepare_before_run();
            let _ = fs::write(
                self.wine_prefix
                    .join("drive_c")
                    .join("windows")
                    .join("MOD4WIN.INI"),
                &self.ini_bytes,
            );
            self.start_warm_wineserver();
        }
    }

    impl Drop for LinuxWineWorkerGuard {
        fn drop(&mut self) {
            let _ = Command::new("wineserver")
                .arg("-k")
                .env("WINEPREFIX", &self.wine_prefix)
                .env("WINEARCH", "win32")
                .env("WINEDEBUG", "-all")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = fs::remove_dir_all(&self.wine_prefix);
            let _ = fs::remove_dir_all(&self.worker_m4w_dir);
        }
    }
}

fn compute_safe_base_name(file_path: &Path, duplicate_names: &HashSet<String>) -> String {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("UNKNOWN.MOD");
    if duplicate_names.contains(&file_name.to_ascii_lowercase()) {
        let lower_full = file_path.to_string_lossy().to_lowercase();
        let short_hash = md5_short_hex_6(lower_full.as_bytes());
        let parent_name = file_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("root");
        let sanitized_artist: String = parent_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}_{}_{}", sanitized_artist, short_hash, file_name)
    } else {
        file_name.to_string()
    }
}

fn append_line_utf8(path: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = write!(f, "{}\r\n", line);
    }
}

/// Startet den vollautomatischen Multi-Core Batch-Diff-Lauf (kompletter Rust-Ersatz für `Mod_Batch.ps1`).
pub fn run_batch_m4w_diff(opts: &BatchM4wDiffOptions) -> Result<(), Box<dyn std::error::Error>> {
    if !opts.mod_source.exists() {
        return Err(format!(
            "Quellpfad '{}' existiert nicht.",
            opts.mod_source.display()
        )
        .into());
    }

    let pass_path = opts.result_base.join("pass");
    let fail_path = opts.result_base.join("fail");
    let timeout_path = opts.result_base.join("timeout");
    let log_path = opts.result_base.join("logs");
    let wav_dest = opts.result_base.join("wav");
    let mod_dest = opts.result_base.join("mods");

    for dir in [
        &pass_path,
        &fail_path,
        &timeout_path,
        &log_path,
        &wav_dest,
        &mod_dest,
    ] {
        fs::create_dir_all(dir)?;
    }

    let mut all_mod_files = Vec::new();
    collect_mod_files_recursive(&opts.mod_source, &mut all_mod_files);
    if all_mod_files.is_empty() {
        return Err(format!(
            "Keine unterstützten Tracker-Module in '{}' gefunden.",
            opts.mod_source.display()
        )
        .into());
    }

    let has_classic_mods = all_mod_files.iter().any(|p| {
        let fmt = detect_batch_mod_format(p).map(|(ext_l, _)| ext_l).unwrap_or("mod");
        !matches!(fmt, "it" | "umx")
    });

    let ffmpeg_opt = resolve_ffmpeg_available(opts.ffmpeg_bin.as_deref());
    let runner_opt = resolve_otvdm_or_wine_runner(opts.otvdm_or_wine.as_deref());
    let m4w_dir_opt = resolve_mod4win_dir(opts.m4w_dir.as_deref());

    if has_classic_mods {
        if let Some(ref m4w_dir) = m4w_dir_opt {
            ensure_mod4win_test_environment(m4w_dir)?;
        } else if ffmpeg_opt.is_none() {
            return Err(
                "Kein MOD4WIN-Verzeichnis (--m4w-dir) und kein FFmpeg (--ffmpeg) gefunden."
                    .into(),
            );
        }
        if runner_opt.is_none() && ffmpeg_opt.is_none() {
            return Err(
                "Kein 16-Bit-Runner (--otvdm / --wine) und kein FFmpeg (--ffmpeg) gefunden."
                    .into(),
            );
        }
    }

    let work_dir = resolve_work_dir(opts.work_dir.as_deref())?;

    #[cfg(target_os = "linux")]
    let wine_template_opt: Option<PathBuf> = if has_classic_mods {
        if let (Some(_m4w_dir), Some(runner_bin)) = (m4w_dir_opt.as_deref(), runner_opt.as_deref())
        {
            if linux_wine::is_wine_runner(runner_bin) {
                Some(linux_wine::ensure_wine_template_prefix(&work_dir)?)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Duplikat-Dateinamen (Case-Insensitive) wie in Mod_Batch.ps1 bestimmen
    let mut seen_names = HashSet::new();
    let mut duplicate_names = HashSet::new();
    for f in &all_mod_files {
        if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
            let lower = name.to_ascii_lowercase();
            if !seen_names.insert(lower.clone()) {
                duplicate_names.insert(lower);
            }
        }
    }

    let default_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let worker_count = opts
        .workers
        .filter(|&w| w > 0)
        .unwrap_or(default_workers)
        .min(all_mod_files.len().max(1));

    let idos: Vec<u8> = if opts.idos.is_empty() {
        vec![0, 1, 2, 3]
    } else {
        opts.idos.iter().map(|&i| i.min(3)).collect()
    };
    let max_lsb_tolerance = if opts.allow_1lsb { 1 } else { 0 };
    let max_wav_size_bytes = opts.max_wav_mb.saturating_mul(1024 * 1024);

    println!("\n======================================================================");
    println!("MOD4WIN 3.0 Automatisierte Multi-Core Diff-Suite (--batch-diff)");
    println!("======================================================================");
    println!(
        "Quellpfad:    {} ({} Module entdeckt)",
        opts.mod_source.display(),
        all_mod_files.len()
    );
    println!("Ergebnisse:   {}", opts.result_base.display());
    println!(
        "16-Bit M4W:   {} | Runner: {}",
        m4w_dir_opt
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<Kein M4W-Ordner>".to_string()),
        runner_opt.as_deref().unwrap_or("<Kein Runner>")
    );
    println!(
        "FFmpeg (.IT): {}",
        ffmpeg_opt
            .as_deref()
            .unwrap_or("<Nicht gefunden -> .IT wird übersprungen>")
    );
    println!(
        "Work-Dir:     {} | Worker: {} | IDOs: {:?}",
        work_dir.display(),
        worker_count,
        idos
    );
    println!(
        "Watchdog:     Idle={}s | Peak={}s | Stall={}s | MaxWAV={}MB | Tol={} LSB",
        opts.max_idle_sec,
        opts.max_active_peak_sec,
        opts.max_stall_sec,
        opts.max_wav_mb,
        max_lsb_tolerance
    );
    println!("----------------------------------------------------------------------");

    let job_guard = os_proc::WorkerJobGuard::new();
    let total_files = all_mod_files.len();
    let next_file_idx = AtomicUsize::new(0);
    let pass_counter = AtomicUsize::new(0);
    let fail_counter = AtomicUsize::new(0);
    let timeout_counter = AtomicUsize::new(0);
    let skip_counter = AtomicUsize::new(0);
    let done_files_counter = AtomicUsize::new(0);
    let t_start = Instant::now();

    std::thread::scope(|s| {
        for worker_id in 0..worker_count {
            let all_files_ref = &all_mod_files;
            let dup_ref = &duplicate_names;
            let pass_ref = &pass_path;
            let fail_ref = &fail_path;
            let timeout_ref = &timeout_path;
            let log_ref = &log_path;
            let wav_ref = &wav_dest;
            let mod_ref = &mod_dest;
            let work_ref = &work_dir;
            let m4w_dir_ref = m4w_dir_opt.as_deref();
            let runner_ref = runner_opt.as_deref();
            let ffmpeg_ref = ffmpeg_opt.as_deref();
            let idos_ref = &idos;
            let job_ref = &job_guard;
            let next_ref = &next_file_idx;
            let pass_cnt = &pass_counter;
            let fail_cnt = &fail_counter;
            let timeout_cnt = &timeout_counter;
            let skip_cnt = &skip_counter;
            let done_cnt = &done_files_counter;
            #[cfg(target_os = "linux")]
            let wine_tpl_ref = wine_template_opt.as_deref();

            s.spawn(move || {
                // Gestaffelter Start wie in Mod_Batch.ps1 (verhindert I/O-Spike beim Start)
                if worker_id > 0 {
                    std::thread::sleep(Duration::from_millis((worker_id as u64 * 120).min(3000)));
                }

                let prefix = format!("W{}", worker_id);
                let mismatch_log = log_ref.join(format!("_MISMATCHES_{}.log", prefix));
                let debug_log = log_ref.join(format!("_WORKER_DEBUG_{}.log", prefix));

                #[cfg(target_os = "linux")]
                let wine_worker_guard = match (m4w_dir_ref, wine_tpl_ref) {
                    (Some(m4w_d), Some(tpl_d)) => {
                        match linux_wine::LinuxWineWorkerGuard::setup(
                            worker_id, work_ref, m4w_d, tpl_d,
                        ) {
                            Ok(g) => Some(g),
                            Err(e) => {
                                append_line_utf8(
                                    &debug_log,
                                    &format!("WINE SETUP ERROR ({}): {}", prefix, e),
                                );
                                None
                            }
                        }
                    }
                    _ => None,
                };

                loop {
                    let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                    if idx >= total_files {
                        break;
                    }
                    let file_path = &all_files_ref[idx];
                    let safe_base_name = compute_safe_base_name(file_path, dup_ref);
                    let (ext_lower_str, ext_dos_str) =
                        detect_batch_mod_format(file_path).unwrap_or(("mod", ".MOD"));
                    let ext_lower = ext_lower_str.to_string();

                    // Lade Modul einmalig in Rust im RAM
                    let raw_bytes = match fs::read(file_path) {
                        Ok(b) => b,
                        Err(e) => {
                            append_line_utf8(
                                &debug_log,
                                &format!("READ ERROR ({}): {}", file_path.display(), e),
                            );
                            done_cnt.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let loaded_res = catch_unwind(AssertUnwindSafe(|| {
                        load_module_with_ext(&raw_bytes, Some(&ext_lower))
                    }));

                    let loaded_opt = match loaded_res {
                        Ok(Ok(m)) => Some(m),
                        _ => None,
                    };

                    let is_it_mod = ext_lower == "it"
                        || ext_lower == "umx"
                        || loaded_opt
                            .as_ref()
                            .map(|m| m.state.mi.file_type == ModType::IT)
                            .unwrap_or(false);

                    // === PFAD A: Automatischer FFmpeg-Modus für .IT / .UMX (oder wenn kein 16-Bit-M4W vorhanden) ===
                    if is_it_mod || (m4w_dir_ref.is_none() && ffmpeg_ref.is_some()) {
                        let Some(ff_bin) = ffmpeg_ref else {
                            skip_cnt.fetch_add(1, Ordering::Relaxed);
                            done_cnt.fetch_add(1, Ordering::Relaxed);
                            continue;
                        };
                        let txt_name = format!("{}.IT.TXT", safe_base_name);
                        let pass_file = pass_ref.join(&txt_name);
                        let fail_file = fail_ref.join(&txt_name);
                        let timeout_file = timeout_ref.join(&txt_name);

                        if opts.retry_fails && fail_file.exists() {
                            let _ = fs::remove_file(&fail_file);
                        }
                        if opts.retry_timeouts && timeout_file.exists() {
                            let _ = fs::remove_file(&timeout_file);
                        }
                        if pass_file.exists() || fail_file.exists() || timeout_file.exists() {
                            skip_cnt.fetch_add(1, Ordering::Relaxed);
                            done_cnt.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }

                        let Some(loaded) = loaded_opt else {
                            let msg = format!(
                                "RUST_LOAD_ERROR | [{}] | {} | IT",
                                prefix,
                                file_path.display()
                            );
                            let _ = fs::write(&timeout_file, &msg);
                            append_line_utf8(&mismatch_log, &msg);
                            timeout_cnt.fetch_add(1, Ordering::Relaxed);
                            done_cnt.fetch_add(1, Ordering::Relaxed);
                            continue;
                        };

                        match render_ffmpeg_reference_pcm(ff_bin, file_path, 44100, None, None) {
                            Ok(samples) if !samples.is_empty() => {
                                let ref_wav = WavData {
                                    path: format!("ffmpeg:libopenmpt({})", file_path.display()),
                                    sample_rate: 44100,
                                    channels: 2,
                                    bits_per_sample: 16,
                                    is_ffmpeg: true,
                                    samples,
                                };
                                let oversamp = if is_it_mod { 6 } else { 2 };
                                let mut full_rep =
                                    format_module_header(&loaded, &file_path.display().to_string());
                                let (passed, stage_rep) = run_ffmpeg_stage_comparison_to_string(
                                    &loaded,
                                    &ref_wav,
                                    oversamp,
                                    0x80 | 20,
                                    true,
                                );
                                full_rep.push_str(&stage_rep);
                                if passed {
                                    let _ = fs::write(&pass_file, full_rep);
                                    pass_cnt.fetch_add(1, Ordering::Relaxed);
                                } else {
                                    let _ = fs::write(&fail_file, full_rep);
                                    let saved_mod = mod_ref.join(&safe_base_name);
                                    if !saved_mod.exists() {
                                        let _ = fs::copy(file_path, &saved_mod);
                                    }
                                    append_line_utf8(
                                        &mismatch_log,
                                        &format!(
                                            "MISMATCH (FFmpeg-IT) | {} | MOD: {}",
                                            file_path.display(),
                                            saved_mod.display()
                                        ),
                                    );
                                    fail_cnt.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(e) => {
                                let msg = format!(
                                    "FFMPEG_ERROR ({}) | [{}] | {}",
                                    e,
                                    prefix,
                                    file_path.display()
                                );
                                let _ = fs::write(&timeout_file, &msg);
                                append_line_utf8(&mismatch_log, &msg);
                                timeout_cnt.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {}
                        }

                        done_cnt.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    // === PFAD B: Klassisches 16-Bit MOD4WIN_IDO0..3.EXE über otvdmw.exe / wine ===
                    let (Some(m4w_dir), Some(runner_bin)) = (m4w_dir_ref, runner_ref) else {
                        skip_cnt.fetch_add(1, Ordering::Relaxed);
                        done_cnt.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };

                    let ext_dos = ext_dos_str;
                    let temp_mod = work_ref.join(format!("{}{}", prefix, ext_dos));
                    let temp_wav = work_ref.join(format!("{}.WAV", prefix));
                    let mut mod_copied = false;

                    for &ido in idos_ref {
                        let txt_name = format!("{}.{}.TXT", safe_base_name, ido);
                        let pass_file = pass_ref.join(&txt_name);
                        let fail_file = fail_ref.join(&txt_name);
                        let timeout_file = timeout_ref.join(&txt_name);

                        if opts.retry_fails && fail_file.exists() {
                            let _ = fs::remove_file(&fail_file);
                        }
                        if opts.retry_timeouts && timeout_file.exists() {
                            let _ = fs::remove_file(&timeout_file);
                        }

                        if pass_file.exists() || fail_file.exists() || timeout_file.exists() {
                            skip_cnt.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }

                        if !mod_copied {
                            let _ = fs::remove_file(&temp_mod);
                            remove_worker_wav_variants(work_ref, &prefix);
                            if let Err(e) = fs::write(&temp_mod, &raw_bytes) {
                                append_line_utf8(
                                    &debug_log,
                                    &format!("TEMP WRITE ERROR ({}): {}", temp_mod.display(), e),
                                );
                                break;
                            }
                            mod_copied = true;
                        } else {
                            remove_worker_wav_variants(work_ref, &prefix);
                        }

                        let mut cmd = Command::new(runner_bin);
                        #[cfg(target_os = "linux")]
                        let (worker_tag_opt, worker_pfx_opt): (Option<&str>, Option<&Path>) =
                            if let Some(ref wg) = wine_worker_guard {
                                use std::os::unix::process::CommandExt;
                                wg.prepare_before_run();
                                cmd.arg(format!("M:\\MOD4WIN_IDO{}.EXE", ido))
                                    .arg(format!("R:\\{}{}", prefix, ext_dos))
                                    .current_dir(&wg.worker_m4w_dir)
                                    .env("WINEPREFIX", &wg.wine_prefix)
                                    .env("WINEARCH", "win32")
                                    .env("WINEDEBUG", "-all")
                                    .env("WINEDLLOVERRIDES", linux_wine::WINE_DLL_OVERRIDES)
                                    .env("M4W_WTAG", &wg.worker_tag)
                                    .process_group(0)
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null());
                                (Some(wg.worker_tag.as_str()), Some(wg.wine_prefix.as_path()))
                            } else {
                                use std::os::unix::process::CommandExt;
                                let m4w_exe = m4w_dir.join(format!("MOD4WIN_IDO{}.EXE", ido));
                                cmd.arg(to_win16_runner_path(&m4w_exe))
                                    .arg(to_win16_runner_path(&temp_mod))
                                    .current_dir(m4w_dir)
                                    .env("WINEDEBUG", "-all")
                                    .process_group(0)
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null());
                                (None, None)
                            };

                        #[cfg(target_os = "windows")]
                        let (worker_tag_opt, worker_pfx_opt): (Option<&str>, Option<&Path>) = {
                            let m4w_exe = m4w_dir.join(format!("MOD4WIN_IDO{}.EXE", ido));
                            cmd.arg(to_win16_runner_path(&m4w_exe))
                                .arg(to_win16_runner_path(&temp_mod))
                                .current_dir(m4w_dir)
                                .stdout(Stdio::null())
                                .stderr(Stdio::null());
                            (None, None)
                        };

                        let mut child = match cmd.spawn() {
                            Ok(c) => c,
                            Err(e) => {
                                append_line_utf8(
                                    &debug_log,
                                    &format!("SPAWN ERROR ({}): {}", runner_bin, e),
                                );
                                break;
                            }
                        };
                        job_ref.assign(&child);

                        // Intelligenter CPU- und WAV-Watchdog (1:1 wie in Mod_Batch.ps1)
                        let timer = Instant::now();
                        let mut last_size: u64 = 0;
                        let mut last_growth_sec: f64 = 0.0;
                        let mut last_cpu_ms: f64 = 0.0;
                        let mut idle_seconds: u64 = 0;
                        let mut abort_reason: Option<String> = None;

                        'watch: loop {
                            if !matches!(child.try_wait(), Ok(None)) {
                                break 'watch;
                            }

                            // 50-ms Fast-Exit-Polling (spart bis zu ~950 ms Leerlauf pro IDO-Lauf),
                            // während der CPU-/WAV-Watchdog weiterhin exakt im 1,0-s-Takt prüft.
                            for _ in 0..20 {
                                std::thread::sleep(Duration::from_millis(50));
                                if !matches!(child.try_wait(), Ok(None)) {
                                    break 'watch;
                                }
                            }

                            let elapsed_sec = timer.elapsed().as_secs_f64();
                            let cur_cpu_ms = os_proc::get_process_cpu_ms(&child, worker_tag_opt)
                                .unwrap_or(last_cpu_ms);
                            let cpu_delta = cur_cpu_ms - last_cpu_ms;
                            last_cpu_ms = cur_cpu_ms;

                            if cpu_delta < 15.0 {
                                idle_seconds += 1;
                            } else {
                                idle_seconds = 0;
                            }

                            if let Some(found_wav) = find_worker_wav_variant(work_ref, &prefix) {
                                if let Ok(meta) = fs::metadata(&found_wav) {
                                    let cur_size = meta.len();
                                    if cur_size > last_size {
                                        last_size = cur_size;
                                        last_growth_sec = elapsed_sec;
                                        idle_seconds = 0;
                                    }
                                    if cur_size > max_wav_size_bytes {
                                        abort_reason = Some(format!(
                                            "ENDLESS_LOOP_WAV_SIZE (greater than {} MB)",
                                            opts.max_wav_mb
                                        ));
                                        break;
                                    }
                                    if last_size > 44
                                        && (elapsed_sec - last_growth_sec)
                                            > (opts.max_stall_sec as f64)
                                    {
                                        abort_reason = Some(format!(
                                            "STALLED_DURING_WRITE (No WAV growth for {}s)",
                                            opts.max_stall_sec
                                        ));
                                        break;
                                    }
                                }
                            }

                            if idle_seconds >= opts.max_idle_sec {
                                abort_reason = Some(format!(
                                    "PROCESS_IDLE_DEFECTIVE_FILE (0% CPU for {}s after {:.1}s)",
                                    opts.max_idle_sec, elapsed_sec
                                ));
                                break;
                            }

                            if last_size <= 44 && elapsed_sec > (opts.max_active_peak_sec as f64) {
                                abort_reason = Some(format!(
                                    "ENDLESS_LOOP_PEAK_SEARCH (100% CPU for {}s without WAV write)",
                                    opts.max_active_peak_sec
                                ));
                                break;
                            }
                        }

                        if let Some(reason) = abort_reason {
                            os_proc::kill_process_tree(&mut child, worker_pfx_opt);
                            #[cfg(target_os = "linux")]
                            if let Some(ref wg) = wine_worker_guard {
                                wg.reset_after_abort();
                            }
                            let msg = format!(
                                "{} | [{}] | {} | IDO {}",
                                reason,
                                prefix,
                                file_path.display(),
                                ido
                            );
                            let _ = fs::write(&timeout_file, &msg);
                            append_line_utf8(&mismatch_log, &msg);

                            let saved_mod = mod_ref.join(&safe_base_name);
                            if !saved_mod.exists() {
                                let _ = fs::copy(file_path, &saved_mod);
                            }
                            remove_worker_wav_variants(work_ref, &prefix);
                            timeout_cnt.fetch_add(1, Ordering::Relaxed);
                            break; // Restliche IDOs für dieses Modul überspringen
                        }

                        // WAV sofort (Case-Insensitive für Linux tmpfs `w0.WAV`) in den RAM einlesen und löschen!
                        let actual_wav_path =
                            find_worker_wav_variant(work_ref, &prefix).unwrap_or_else(|| temp_wav.clone());
                        let wav_bytes_opt = fs::read(&actual_wav_path).ok().filter(|b| b.len() > 44);
                        remove_worker_wav_variants(work_ref, &prefix);

                        if let Some(wav_bytes) = wav_bytes_opt {
                            let Some(ref loaded) = loaded_opt else {
                                let err_entry = format!(
                                    "RUST_LOAD_ERROR | [{}] | {} | IDO {}",
                                    prefix,
                                    file_path.display(),
                                    ido
                                );
                                let _ = fs::write(&fail_file, &err_entry);
                                append_line_utf8(&mismatch_log, &err_entry);
                                fail_cnt.fetch_add(1, Ordering::Relaxed);
                                continue;
                            };

                            let parsed = match parse_wav_bytes_i16(&wav_bytes) {
                                Ok(p) => p,
                                Err(e) => {
                                    let err_entry = format!(
                                        "WAV_PARSE_ERROR ({}) | [{}] | {} | IDO {}",
                                        e,
                                        prefix,
                                        file_path.display(),
                                        ido
                                    );
                                    let _ = fs::write(&timeout_file, &err_entry);
                                    append_line_utf8(&mismatch_log, &err_entry);
                                    timeout_cnt.fetch_add(1, Ordering::Relaxed);
                                    continue;
                                }
                            };

                            let ref_wav = WavData::from_parsed(
                                actual_wav_path.display().to_string(),
                                parsed,
                            );
                            let (passed, max_diff, diff_out) = run_single_ido_diff_in_memory(
                                loaded,
                                &temp_mod.display().to_string(),
                                &ref_wav,
                                ido,
                                max_lsb_tolerance,
                            );

                            if passed {
                                let _ = fs::write(&pass_file, diff_out);
                                // Falls von einem früheren Lauf noch eine WAV in wav/ lag, aufräumen
                                let old_wav =
                                    wav_ref.join(format!("{}.{}.wav", safe_base_name, ido));
                                if old_wav.exists() {
                                    let _ = fs::remove_file(&old_wav);
                                }
                                pass_cnt.fetch_add(1, Ordering::Relaxed);
                            } else {
                                let _ = fs::write(&fail_file, diff_out);
                                let saved_wav =
                                    wav_ref.join(format!("{}.{}.wav", safe_base_name, ido));
                                let _ = fs::write(&saved_wav, &wav_bytes);
                                let saved_mod = mod_ref.join(&safe_base_name);
                                if !saved_mod.exists() {
                                    let _ = fs::copy(file_path, &saved_mod);
                                }
                                let mismatch_entry = format!(
                                    "MISMATCH (MaxDiff={} LSB) | {} | IDO {} | WAV: {} | MOD: {}",
                                    max_diff,
                                    file_path.display(),
                                    ido,
                                    saved_wav.display(),
                                    saved_mod.display()
                                );
                                append_line_utf8(&mismatch_log, &mismatch_entry);
                                fail_cnt.fetch_add(1, Ordering::Relaxed);
                            }
                        } else {
                            let exit_code = child
                                .wait()
                                .ok()
                                .and_then(|s| s.code())
                                .unwrap_or(-1);
                            let err_entry = format!(
                                "NO_WAV_ERROR | [{}] | {} | IDO {} (Mod4Win ExitCode: {})",
                                prefix,
                                file_path.display(),
                                ido,
                                exit_code
                            );
                            let _ = fs::write(&timeout_file, &err_entry);
                            append_line_utf8(&mismatch_log, &err_entry);
                            let saved_mod = mod_ref.join(&safe_base_name);
                            if !saved_mod.exists() {
                                let _ = fs::copy(file_path, &saved_mod);
                            }
                            timeout_cnt.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    let _ = fs::remove_file(&temp_mod);
                    remove_worker_wav_variants(work_ref, &prefix);
                    let done = done_cnt.fetch_add(1, Ordering::Relaxed) + 1;
                    let p = pass_cnt.load(Ordering::Relaxed);
                    let f = fail_cnt.load(Ordering::Relaxed);
                    let t = timeout_cnt.load(Ordering::Relaxed);
                    let sk = skip_cnt.load(Ordering::Relaxed);
                    let elapsed = t_start.elapsed().as_secs();
                    println!(
                        "[{:04}/{:04}] PASS: {} | FAIL: {} | TIMEOUT: {} | SKIP: {} | {:02}:{:02}:{:02} | {}",
                        done,
                        total_files,
                        p,
                        f,
                        t,
                        sk,
                        elapsed / 3600,
                        (elapsed % 3600) / 60,
                        elapsed % 60,
                        safe_base_name
                    );
                }
            });
        }
    });

    // Alle Worker-Mismatch-Logs zu _ALL_MISMATCHES.log zusammenführen
    let mut all_mismatches = String::new();
    if let Ok(entries) = fs::read_dir(&log_path) {
        let mut log_files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|d| d.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("_MISMATCHES_W") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();
        log_files.sort();
        for lf in log_files {
            if let Ok(txt) = fs::read_to_string(&lf) {
                all_mismatches.push_str(&txt);
            }
        }
    }
    let _ = fs::write(log_path.join("_ALL_MISMATCHES.log"), &all_mismatches);

    let total_mins = t_start.elapsed().as_secs_f64() / 60.0;
    println!("======================================================================");
    println!(
        "Alle {} Worker fertig in {:.1} Minuten! (PASS: {} | FAIL: {} | TIMEOUT: {} | SKIP: {})",
        worker_count,
        total_mins,
        pass_counter.load(Ordering::Relaxed),
        fail_counter.load(Ordering::Relaxed),
        timeout_counter.load(Ordering::Relaxed),
        skip_counter.load(Ordering::Relaxed)
    );
    println!("======================================================================");

    Ok(())
}

// ============================================================================
// 4. Bestehende Batch-Verifier (--batch-verify) und FFmpeg-Diff (--batch-ffmpeg-diff)
// ============================================================================

#[derive(Clone, Debug)]
pub struct BatchVerifyFileEntry {
    pub display_path: String,
    pub format_ext: String,
    pub detected_type: String,
    pub size_bytes: usize,
    pub load_time_us: u64,
    pub render_time_ms: u64,
    pub peak_pre_amp: u8,
    pub rendered_frames: usize,
    pub max_abs_sample: i32,
    pub has_nan_or_panic: bool,
    pub status: String,
    pub error_msg: String,
}

#[derive(Clone, Debug)]
pub struct BatchVerifyReport {
    pub root_folder: String,
    pub total_files: usize,
    pub passed_files: usize,
    pub failed_files: usize,
    pub silent_files: usize,
    pub total_load_ms: u64,
    pub total_render_ms: u64,
    pub total_audio_frames: u64,
    pub format_counts: BTreeMap<String, usize>,
    pub entries: Vec<BatchVerifyFileEntry>,
}

#[derive(Clone)]
pub(crate) enum BatchCandidate {
    DiskFile(PathBuf),
    ZipMember {
        zip_path: PathBuf,
        entry_name: String,
    },
}

pub(crate) fn collect_batch_candidates(dir: &Path, out: &mut Vec<BatchCandidate>) {
    if dir.is_file() {
        let ext = dir
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if is_supported_archive_ext(&ext) {
            if let Ok(zip_bytes) = fs::read(dir) {
                for e in parse_zip_entries(&zip_bytes) {
                    if UiSharedState::is_tracker_file(Path::new(&e.file_name)) {
                        out.push(BatchCandidate::ZipMember {
                            zip_path: dir.to_path_buf(),
                            entry_name: e.full_name,
                        });
                    }
                }
            }
        } else if UiSharedState::is_tracker_file(dir) {
            out.push(BatchCandidate::DiskFile(dir.to_path_buf()));
        }
        return;
    }

    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read_dir.filter_map(|e| e.ok().map(|d| d.path())).collect();
    paths.sort();

    for p in paths {
        if p.is_dir() {
            collect_batch_candidates(&p, out);
        } else if p.is_file() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if is_supported_archive_ext(&ext) {
                if let Ok(zip_bytes) = fs::read(&p) {
                    for e in parse_zip_entries(&zip_bytes) {
                        if UiSharedState::is_tracker_file(Path::new(&e.file_name)) {
                            out.push(BatchCandidate::ZipMember {
                                zip_path: p.clone(),
                                entry_name: e.full_name,
                            });
                        }
                    }
                }
            } else if UiSharedState::is_tracker_file(&p) {
                out.push(BatchCandidate::DiskFile(p));
            }
        }
    }
}

fn verify_single_module_bytes(
    display_path: String,
    raw_bytes: &[u8],
    ext_opt: Option<&str>,
) -> BatchVerifyFileEntry {
    let ext_str = ext_opt.unwrap_or("unknown").to_ascii_uppercase();
    let size_bytes = raw_bytes.len();
    let t_load = Instant::now();

    let load_res = catch_unwind(AssertUnwindSafe(|| load_module_with_ext(raw_bytes, ext_opt)));
    let load_time_us = t_load.elapsed().as_micros() as u64;

    let loaded = match load_res {
        Ok(Ok(m)) => m,
        Ok(Err(code)) => {
            return BatchVerifyFileEntry {
                display_path,
                format_ext: ext_str,
                detected_type: "LOAD_ERR".to_string(),
                size_bytes,
                load_time_us,
                render_time_ms: 0,
                peak_pre_amp: 0,
                rendered_frames: 0,
                max_abs_sample: 0,
                has_nan_or_panic: false,
                status: "FAIL".to_string(),
                error_msg: format!("Loader Error Code {}", code),
            };
        }
        Err(_) => {
            return BatchVerifyFileEntry {
                display_path,
                format_ext: ext_str,
                detected_type: "PANIC_LOAD".to_string(),
                size_bytes,
                load_time_us,
                render_time_ms: 0,
                peak_pre_amp: 0,
                rendered_frames: 0,
                max_abs_sample: 0,
                has_nan_or_panic: true,
                status: "PANIC".to_string(),
                error_msg: "Panic in load_module_with_ext".to_string(),
            };
        }
    };

    let detected_type = format!("{:?}", loaded.state.mi.file_type);
    let t_render = Instant::now();

    let render_res = catch_unwind(AssertUnwindSafe(|| {
        let mut settings = SoundSettings::default();
        settings.device_id = DEV_HDD;
        settings.samplerate = 44100;
        settings.stereo = 2;
        settings.bits = 16;
        settings.oversamp = 2;
        settings.panning = true;
        settings.surround = false;
        settings.pre_amp = 0x80 | 30;

        let peak_pa = run_peak_search_isolated(&loaded, settings);
        settings.pre_amp = 0x80 | (peak_pa & 0x7F);

        let mut seq = Sequencer::new(loaded, settings);
        seq.module.state.repeat_song = false;

        let max_frames_target = 44100 * 30;
        let mut rendered_frames = 0usize;
        let mut max_abs_sample = 0i32;
        let mut ticks = 0usize;

        while !seq.module.state.player_repeats
            && rendered_frames < max_frames_target
            && ticks < 15_000
        {
            if let Some(mix_out) = seq.render_it_tick(None) {
                if let MixOutputBuffer::Pcm16(pcm) = mix_out {
                    rendered_frames += pcm.len() / 2;
                    for &s in &pcm {
                        let a = (s as i32).abs();
                        if a > max_abs_sample {
                            max_abs_sample = a;
                        }
                    }
                }
                if seq.module.state.player_repeats {
                    break;
                }
                ticks += 1;
                continue;
            }
            if seq.module.state.curr_vblanc == 0 {
                seq.process_row();
                if seq.module.state.player_repeats {
                    break;
                }
            }
            seq.update_channels_for_mixing();
            if seq.module.state.player_repeats {
                break;
            }
            seq.module.state.buf_samples = seq.module.state.num_samples.max(1);
            let am_repeating = seq.module.state.player_repeats && !seq.module.state.repeat_song;
            let mix_out = process_sample_block(
                &mut seq.module.state,
                &mut seq.mix_mem,
                &seq.module.sample_mem,
                am_repeating,
            );
            if let MixOutputBuffer::Pcm16(pcm) = mix_out {
                rendered_frames += pcm.len() / 2;
                for &s in &pcm {
                    let a = (s as i32).abs();
                    if a > max_abs_sample {
                        max_abs_sample = a;
                    }
                }
            }
            seq.advance_tick();
            ticks += 1;
        }

        (peak_pa & 0x7F, rendered_frames, max_abs_sample)
    }));

    let render_time_ms = t_render.elapsed().as_millis() as u64;

    match render_res {
        Ok((peak_pre_amp, rendered_frames, max_abs_sample)) => {
            let status = if rendered_frames == 0 || max_abs_sample == 0 {
                "SILENT".to_string()
            } else {
                "PASS".to_string()
            };
            BatchVerifyFileEntry {
                display_path,
                format_ext: ext_str,
                detected_type,
                size_bytes,
                load_time_us,
                render_time_ms,
                peak_pre_amp,
                rendered_frames,
                max_abs_sample,
                has_nan_or_panic: false,
                status,
                error_msg: String::new(),
            }
        }
        Err(_) => BatchVerifyFileEntry {
            display_path,
            format_ext: ext_str,
            detected_type,
            size_bytes,
            load_time_us,
            render_time_ms,
            peak_pre_amp: 0,
            rendered_frames: 0,
            max_abs_sample: 0,
            has_nan_or_panic: true,
            status: "PANIC".to_string(),
            error_msg: "Panic in Sequencer/Mixer render loop".to_string(),
        },
    }
}

pub fn run_batch_verify(
    folder_path: &Path,
    report_json_path: Option<&Path>,
) -> Result<BatchVerifyReport, Box<dyn std::error::Error>> {
    if !folder_path.exists() {
        return Err(format!("Ordner '{}' existiert nicht.", folder_path.display()).into());
    }

    let t_total = Instant::now();
    let mut candidates = Vec::new();
    collect_batch_candidates(folder_path, &mut candidates);

    println!("\n======================================================================");
    println!("MOD4WIN 2.40 Batch-Ordner-Verifier (--batch-verify)");
    println!("======================================================================");
    println!(
        "Scanne Pfad: {} ({} Module inkl. ZIP-Einträgen gefunden)",
        folder_path.display(),
        candidates.len()
    );
    println!("----------------------------------------------------------------------");

    let mut entries = Vec::with_capacity(candidates.len());
    let mut format_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut passed_files = 0usize;
    let mut failed_files = 0usize;
    let mut silent_files = 0usize;
    let mut total_load_ms = 0u64;
    let mut total_render_ms = 0u64;
    let mut total_audio_frames = 0u64;

    for (idx, cand) in candidates.iter().enumerate() {
        let entry = match cand {
            BatchCandidate::DiskFile(p) => {
                let disp = p.display().to_string();
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase());
                match fs::read(p) {
                    Ok(raw) => verify_single_module_bytes(disp, &raw, ext.as_deref()),
                    Err(e) => BatchVerifyFileEntry {
                        display_path: disp,
                        format_ext: ext.unwrap_or_default().to_ascii_uppercase(),
                        detected_type: "IO_ERR".to_string(),
                        size_bytes: 0,
                        load_time_us: 0,
                        render_time_ms: 0,
                        peak_pre_amp: 0,
                        rendered_frames: 0,
                        max_abs_sample: 0,
                        has_nan_or_panic: false,
                        status: "FAIL".to_string(),
                        error_msg: e.to_string(),
                    },
                }
            }
            BatchCandidate::ZipMember {
                zip_path,
                entry_name,
            } => {
                let disp = format!("{}::{}", zip_path.display(), entry_name);
                let ext = Path::new(entry_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase());
                match fs::read(zip_path) {
                    Ok(zip_bytes) => {
                        let list = parse_zip_entries(&zip_bytes);
                        if let Some(ze) = list.iter().find(|e| e.full_name == *entry_name) {
                            if let Some(raw) = extract_zip_entry_bytes(&zip_bytes, ze) {
                                verify_single_module_bytes(disp, &raw, ext.as_deref())
                            } else {
                                BatchVerifyFileEntry {
                                    display_path: disp,
                                    format_ext: ext.unwrap_or_default().to_ascii_uppercase(),
                                    detected_type: "ZIP_ERR".to_string(),
                                    size_bytes: 0,
                                    load_time_us: 0,
                                    render_time_ms: 0,
                                    peak_pre_amp: 0,
                                    rendered_frames: 0,
                                    max_abs_sample: 0,
                                    has_nan_or_panic: false,
                                    status: "FAIL".to_string(),
                                    error_msg: "Deflate unpack failed".to_string(),
                                }
                            }
                        } else {
                            continue;
                        }
                    }
                    Err(e) => BatchVerifyFileEntry {
                        display_path: disp,
                        format_ext: ext.unwrap_or_default().to_ascii_uppercase(),
                        detected_type: "IO_ERR".to_string(),
                        size_bytes: 0,
                        load_time_us: 0,
                        render_time_ms: 0,
                        peak_pre_amp: 0,
                        rendered_frames: 0,
                        max_abs_sample: 0,
                        has_nan_or_panic: false,
                        status: "FAIL".to_string(),
                        error_msg: e.to_string(),
                    },
                }
            }
        };

        *format_counts.entry(entry.format_ext.clone()).or_insert(0) += 1;
        total_load_ms += entry.load_time_us / 1000;
        total_render_ms += entry.render_time_ms;
        total_audio_frames += entry.rendered_frames as u64;

        match entry.status.as_str() {
            "PASS" => passed_files += 1,
            "SILENT" => {
                passed_files += 1;
                silent_files += 1;
            }
            _ => failed_files += 1,
        }

        println!(
            "[{:04}/{:04}] {:<6} | {:<7} | {:>8} B | PreAmp:{:>2} | Peak:{:>5} | {:>4} ms | {}",
            idx + 1,
            candidates.len(),
            entry.status,
            entry.detected_type,
            entry.size_bytes,
            entry.peak_pre_amp,
            entry.max_abs_sample,
            entry.render_time_ms,
            entry.display_path
        );
        entries.push(entry);
    }

    let elapsed_total_s = t_total.elapsed().as_secs_f64().max(0.001);
    let total_audio_s = total_audio_frames as f64 / 44100.0;

    println!("----------------------------------------------------------------------");
    println!(
        "ZUSAMMENFASSUNG: {} Dateien geprüft ({} PASS, {} SILENT, {} FAIL) in {:.2}s ({:.1}x Echtzeit)",
        entries.len(),
        passed_files,
        silent_files,
        failed_files,
        elapsed_total_s,
        total_audio_s / elapsed_total_s
    );
    print!("Formate: ");
    for (k, v) in &format_counts {
        print!("{}:{} ", k, v);
    }
    println!("\n======================================================================");

    let report = BatchVerifyReport {
        root_folder: folder_path.display().to_string(),
        total_files: entries.len(),
        passed_files,
        failed_files,
        silent_files,
        total_load_ms,
        total_render_ms,
        total_audio_frames,
        format_counts,
        entries,
    };

    let json_target = report_json_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("batch_verify_report.json"));
    write_batch_report_json(&report, &json_target)?;
    println!("JSON-Report geschrieben nach: {}", json_target.display());

    Ok(report)
}

pub(crate) fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn write_batch_report_json(
    report: &BatchVerifyReport,
    out_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let mut json = String::with_capacity(4096 + report.entries.len() * 256);
    json.push_str("{\n");
    json.push_str(&format!(
        "  \"root_folder\": \"{}\",\n",
        escape_json_str(&report.root_folder)
    ));
    json.push_str(&format!("  \"total_files\": {},\n", report.total_files));
    json.push_str(&format!("  \"passed_files\": {},\n", report.passed_files));
    json.push_str(&format!("  \"silent_files\": {},\n", report.silent_files));
    json.push_str(&format!("  \"failed_files\": {},\n", report.failed_files));
    json.push_str(&format!("  \"total_load_ms\": {},\n", report.total_load_ms));
    json.push_str(&format!(
        "  \"total_render_ms\": {},\n",
        report.total_render_ms
    ));
    json.push_str(&format!(
        "  \"total_audio_frames\": {},\n",
        report.total_audio_frames
    ));
    json.push_str("  \"format_counts\": {");
    for (idx, (k, v)) in report.format_counts.iter().enumerate() {
        if idx > 0 {
            json.push(',');
        }
        json.push_str(&format!(" \"{}\": {}", escape_json_str(k), v));
    }
    json.push_str(" },\n");
    json.push_str("  \"files\": [\n");
    for (idx, e) in report.entries.iter().enumerate() {
        let comma = if idx + 1 < report.entries.len() {
            ","
        } else {
            ""
        };
        json.push_str(&format!(
            "    {{ \"path\": \"{}\", \"format\": \"{}\", \"detected_type\": \"{}\", \"size_bytes\": {}, \"load_time_us\": {}, \"render_time_ms\": {}, \"peak_pre_amp\": {}, \"rendered_frames\": {}, \"max_abs_sample\": {}, \"status\": \"{}\", \"error\": \"{}\" }}{}\n",
            escape_json_str(&e.display_path),
            escape_json_str(&e.format_ext),
            escape_json_str(&e.detected_type),
            e.size_bytes,
            e.load_time_us,
            e.render_time_ms,
            e.peak_pre_amp,
            e.rendered_frames,
            e.max_abs_sample,
            escape_json_str(&e.status),
            escape_json_str(&e.error_msg),
            comma
        ));
    }
    json.push_str("  ]\n}\n");
    fs::write(out_path, json)?;
    Ok(())
}

/// Sucht die `ffmpeg`-Binärdatei (entweder explizit übergeben, im Arbeitsverzeichnis oder über `PATH`).
pub fn resolve_ffmpeg_binary(custom_bin: Option<&str>) -> String {
    if let Some(b) = custom_bin {
        if !b.trim().is_empty() {
            return b.to_string();
        }
    }
    if let Ok(env_bin) = std::env::var("FFMPEG_BIN") {
        if !env_bin.trim().is_empty() {
            return env_bin;
        }
    }
    for candidate in [
        "ffmpeg.exe",
        "ffmpeg",
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    ] {
        let p = Path::new(candidate);
        if p.is_file() {
            return candidate.to_string();
        }
    }
    "ffmpeg".to_string()
}

/// Rendert ein Tracker-Modul über `ffmpeg` (`libopenmpt`) direkt in einen 16-Bit Stereo PCM-Vektor.
pub fn render_ffmpeg_reference_pcm(
    ffmpeg_bin: &str,
    mod_path: &Path,
    sample_rate: u32,
    max_secs: Option<u32>,
    cache_wav_path: Option<&Path>,
) -> Result<Vec<i16>, String> {
    if let Some(wav_p) = cache_wav_path {
        if wav_p.exists() && wav_p.is_file() {
            if let Ok(raw) = fs::read(wav_p) {
                if let Ok(parsed) = parse_wav_bytes_i16(&raw) {
                    if !parsed.samples.is_empty() {
                        return Ok(parsed.samples);
                    }
                }
            }
        }
        if let Some(parent) = wav_p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = fs::create_dir_all(parent);
            }
        }
        let mut cmd = Command::new(ffmpeg_bin);
        cmd.arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(mod_path);
        if let Some(secs) = max_secs.filter(|&s| s > 0) {
            cmd.arg("-t").arg(secs.to_string());
        }
        cmd.arg("-ar")
            .arg(sample_rate.to_string())
            .arg("-ac")
            .arg("2")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(wav_p)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let output = cmd
            .output()
            .map_err(|e| format!("Konnte '{}' nicht starten: {}", ffmpeg_bin, e))?;
        if !output.status.success() {
            let err_txt = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("FFmpeg-Fehler: {}", err_txt));
        }
        let raw = fs::read(wav_p).map_err(|e| e.to_string())?;
        let parsed = parse_wav_bytes_i16(&raw)?;
        return Ok(parsed.samples);
    }

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(mod_path);
    if let Some(secs) = max_secs.filter(|&s| s > 0) {
        cmd.arg("-t").arg(secs.to_string());
    }
    cmd.arg("-f")
        .arg("s16le")
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg("-ar")
        .arg(sample_rate.to_string())
        .arg("-ac")
        .arg("2")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Konnte '{}' nicht starten: {}", ffmpeg_bin, e))?;
    if !output.status.success() && output.stdout.is_empty() {
        let err_txt = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("FFmpeg-Fehler: {}", err_txt));
    }

    let sample_count = output.stdout.len() / 2;
    let mut samples = Vec::with_capacity(sample_count);
    for chunk in output.stdout.chunks_exact(2) {
        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(samples)
}

#[derive(Clone, Debug, Default)]
pub struct BatchFfmpegDiffOptions {
    pub folder_path: PathBuf,
    pub ref_dir: Option<PathBuf>,
    pub ffmpeg_bin: Option<String>,
    pub threads: Option<usize>,
    pub max_secs: Option<u32>,
    pub report_json: Option<PathBuf>,
    pub ido: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct BatchFfmpegDiffEntry {
    pub display_path: String,
    pub format_ext: String,
    pub detected_type: String,
    pub status: String,
    pub ref_secs: f64,
    pub m4w_secs: f64,
    pub duration_ratio: f64,
    pub raw_rms_ratio: f64,
    pub orders_total: usize,
    pub orders_ok: usize,
    pub worst_order: usize,
    pub worst_order_pat: usize,
    pub worst_order_norm_ratio: f64,
    pub worst_order_pan_delta: f64,
    pub wall_time_ms: u64,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct BatchFfmpegDiffReport {
    pub root_folder: String,
    pub total_files: usize,
    pub pass_count: usize,
    pub warn_count: usize,
    pub fail_count: usize,
    pub threads_used: usize,
    pub total_wall_time_ms: u64,
    pub entries: Vec<BatchFfmpegDiffEntry>,
}

fn evaluate_single_module_ffmpeg_diff(
    cand: &BatchCandidate,
    root_folder: &Path,
    ref_dir: Option<&Path>,
    ffmpeg_bin: &str,
    max_secs: Option<u32>,
    ido_override: Option<u8>,
) -> BatchFfmpegDiffEntry {
    let t0 = Instant::now();
    let (display_path, ext_str, raw_res, disk_path_opt): (
        String,
        String,
        Result<Vec<u8>, String>,
        Option<PathBuf>,
    ) = match cand {
        BatchCandidate::DiskFile(p) => {
            let disp = p.display().to_string();
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_ascii_uppercase();
            let r = fs::read(p).map_err(|e| e.to_string());
            (disp, ext, r, Some(p.clone()))
        }
        BatchCandidate::ZipMember {
            zip_path,
            entry_name,
        } => {
            let disp = format!("{}::{}", zip_path.display(), entry_name);
            let ext = Path::new(entry_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_ascii_uppercase();
            let r = fs::read(zip_path).map_err(|e| e.to_string()).and_then(|zb| {
                let list = parse_zip_entries(&zb);
                let ze = list
                    .iter()
                    .find(|e| e.full_name == *entry_name)
                    .ok_or_else(|| "ZIP entry missing".to_string())?;
                extract_zip_entry_bytes(&zb, ze).ok_or_else(|| "ZIP unpack error".to_string())
            });
            (disp, ext, r, None)
        }
    };

    let raw_bytes = match raw_res {
        Ok(b) => b,
        Err(e) => {
            return BatchFfmpegDiffEntry {
                display_path,
                format_ext: ext_str,
                detected_type: "IO_ERR".to_string(),
                status: "FAIL".to_string(),
                ref_secs: 0.0,
                m4w_secs: 0.0,
                duration_ratio: 0.0,
                raw_rms_ratio: 0.0,
                orders_total: 0,
                orders_ok: 0,
                worst_order: 0,
                worst_order_pat: 0,
                worst_order_norm_ratio: 0.0,
                worst_order_pan_delta: 0.0,
                wall_time_ms: t0.elapsed().as_millis() as u64,
                message: e,
            };
        }
    };

    let ext_lower = ext_str.to_ascii_lowercase();
    let ext_opt = Some(ext_lower.as_str());

    let eval_res = catch_unwind(AssertUnwindSafe(|| -> Result<BatchFfmpegDiffEntry, String> {
        let loaded = load_module_with_ext(&raw_bytes, ext_opt)
            .map_err(|c| format!("Mod4Win load error {}", c))?;
        let detected_type = format!("{:?}", loaded.state.mi.file_type);

        let cache_wav_path: Option<PathBuf> = ref_dir.map(|rdir| {
            if let Some(ref dp) = disk_path_opt {
                if let Ok(rel) = dp.strip_prefix(root_folder) {
                    let mut out = rdir.join(rel);
                    out.set_extension("wav");
                    return out;
                }
            }
            let safe_name: String = display_path
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            rdir.join(format!("{}.wav", safe_name))
        });

        let mut temp_mod_path: Option<PathBuf> = None;
        let mod_file_for_ffmpeg: PathBuf = if let Some(dp) = disk_path_opt {
            dp
        } else {
            let tid = std::thread::current().id();
            let tmp = std::env::temp_dir().join(format!(
                "m4w_ff_{:?}_{}.{}",
                tid,
                t0.elapsed().as_nanos(),
                ext_str.to_ascii_lowercase()
            ));
            fs::write(&tmp, &raw_bytes).map_err(|e| e.to_string())?;
            temp_mod_path = Some(tmp.clone());
            tmp
        };

        let ref_samples_res = render_ffmpeg_reference_pcm(
            ffmpeg_bin,
            &mod_file_for_ffmpeg,
            44100,
            max_secs,
            cache_wav_path.as_deref(),
        );
        if let Some(tmp) = temp_mod_path {
            let _ = fs::remove_file(tmp);
        }
        let ref_samples = ref_samples_res?;
        let ref_frames = ref_samples.len() / 2;
        if ref_frames == 0 {
            return Err("FFmpeg lieferte 0 Audio-Frames".to_string());
        }

        let is_it = loaded.state.mi.file_type == ModType::IT;
        let mut settings = SoundSettings::default();
        settings.device_id = DEV_HDD;
        settings.samplerate = 44100;
        settings.stereo = 2;
        settings.bits = 16;
        settings.oversamp = ido_override.unwrap_or(if is_it { 6 } else { 2 });
        settings.panning = true;
        settings.surround = false;
        settings.pre_amp = 0x80 | 20;

        let mut seq = Sequencer::new(loaded, settings);
        seq.module.state.repeat_song = false;

        let max_m4w_frames = match max_secs.filter(|&s| s > 0) {
            Some(s) => (s as usize) * 44100,
            None => (ref_frames * 2).max(44100 * 600),
        };

        let mut m4w_samples: Vec<i16> = Vec::with_capacity(ref_samples.len().min(max_m4w_frames * 2));
        let mut order_starts: Vec<(usize, usize, usize)> = Vec::new();
        let mut last_ord = usize::MAX;
        let mut ticks = 0usize;

        while m4w_samples.len() / 2 < max_m4w_frames && ticks < 300_000 {
            if seq.is_it_module() {
                let (cur_ord, cur_pat) = {
                    let eng = seq.it_engine.as_ref().unwrap();
                    let ord = eng.curr_order;
                    let pat = seq
                        .module
                        .it_module
                        .as_ref()
                        .and_then(|it| it.orders.get(ord).copied())
                        .unwrap_or(255) as usize;
                    (ord, pat)
                };
                let Some(out) = seq.render_it_tick(None) else {
                    break;
                };
                if cur_pat < 254 && cur_ord != last_ord {
                    order_starts.push((cur_ord, cur_pat, m4w_samples.len() / 2));
                    last_ord = cur_ord;
                }
                if let MixOutputBuffer::Pcm16(pcm) = out {
                    m4w_samples.extend_from_slice(&pcm);
                }
                if seq.it_engine.as_ref().unwrap().song_finished || seq.module.state.player_repeats {
                    break;
                }
            } else {
                if seq.module.state.curr_vblanc == 0 {
                    seq.process_row();
                    if seq.module.state.player_repeats {
                        break;
                    }
                }
                seq.update_channels_for_mixing();
                if seq.module.state.player_repeats {
                    break;
                }
                let cur_ord = seq.module.state.curr_pattern as usize;
                let cur_pat = seq.module.state.p_arrangement[cur_ord.min(255)] as usize;
                if cur_pat < 254 && cur_ord != last_ord {
                    order_starts.push((cur_ord, cur_pat, m4w_samples.len() / 2));
                    last_ord = cur_ord;
                }
                seq.module.state.buf_samples = seq.module.state.num_samples.max(1);
                let am_repeating = seq.module.state.player_repeats && !seq.module.state.repeat_song;
                let out = process_sample_block(
                    &mut seq.module.state,
                    &mut seq.mix_mem,
                    &seq.module.sample_mem,
                    am_repeating,
                );
                if let MixOutputBuffer::Pcm16(pcm) = out {
                    m4w_samples.extend_from_slice(&pcm);
                }
                seq.advance_tick();
                if seq.module.state.player_repeats {
                    break;
                }
            }
            ticks += 1;
        }

        let m4w_frames = m4w_samples.len() / 2;
        let common_frames = ref_frames.min(m4w_frames);
        let ref_secs = ref_frames as f64 / 44100.0;
        let m4w_secs = m4w_frames as f64 / 44100.0;
        let duration_ratio = if ref_secs > 0.05 {
            m4w_secs / ref_secs
        } else {
            1.0
        };

        if common_frames == 0 {
            return Err("Mod4Win lieferte 0 Audio-Frames".to_string());
        }

        let mut tot_ref_sq = 0.0f64;
        let mut tot_m4w_sq = 0.0f64;
        for i in 0..common_frames {
            let rl = ref_samples[i * 2] as f64;
            let rr = ref_samples[i * 2 + 1] as f64;
            let ml = m4w_samples[i * 2] as f64;
            let mr = m4w_samples[i * 2 + 1] as f64;
            tot_ref_sq += rl * rl + rr * rr;
            tot_m4w_sq += ml * ml + mr * mr;
        }
        let rms_ref_all = (tot_ref_sq / (2.0 * common_frames as f64)).sqrt();
        let rms_m4w_all = (tot_m4w_sq / (2.0 * common_frames as f64)).sqrt();
        let raw_rms_ratio = if rms_ref_all > 1.0 {
            rms_m4w_all / rms_ref_all
        } else {
            1.0
        };
        let norm_scale = if raw_rms_ratio > 0.05 {
            1.0 / raw_rms_ratio
        } else {
            1.0
        };

        let mut orders_total = 0usize;
        let mut orders_ok = 0usize;
        let mut worst_order = 0usize;
        let mut worst_order_pat = 0usize;
        let mut worst_order_norm_ratio = 1.0f64;
        let mut worst_order_pan_delta = 0.0f64;
        let mut max_dev = -1.0f64;

        for w in 0..order_starts.len() {
            let (ord, pat, s_f) = order_starts[w];
            let e_f = if w + 1 < order_starts.len() {
                order_starts[w + 1].2
            } else {
                m4w_frames
            };
            let e_clamped = e_f.min(common_frames);
            if e_clamped <= s_f {
                continue;
            }
            let n = (e_clamped - s_f) as f64;
            let mut s_rl = 0.0f64;
            let mut s_rr = 0.0f64;
            let mut s_ml = 0.0f64;
            let mut s_mr = 0.0f64;
            for i in s_f..e_clamped {
                let rl = ref_samples[i * 2] as f64;
                let rr = ref_samples[i * 2 + 1] as f64;
                let ml = m4w_samples[i * 2] as f64;
                let mr = m4w_samples[i * 2 + 1] as f64;
                s_rl += rl * rl;
                s_rr += rr * rr;
                s_ml += ml * ml;
                s_mr += mr * mr;
            }
            let rl_rms = (s_rl / n).sqrt();
            let rr_rms = (s_rr / n).sqrt();
            let ml_rms = (s_ml / n).sqrt();
            let mr_rms = (s_mr / n).sqrt();
            let r_tot = ((s_rl + s_rr) / (2.0 * n)).sqrt();
            let m_tot = ((s_ml + s_mr) / (2.0 * n)).sqrt();
            if r_tot < 50.0 && m_tot < 50.0 {
                continue;
            }
            orders_total += 1;
            let raw_rat = if r_tot > 1.0 { m_tot / r_tot } else { 1.0 };
            let norm_rat = raw_rat * norm_scale;

            let ref_pan = if rl_rms + rr_rms > 10.0 {
                rr_rms / (rl_rms + rr_rms)
            } else {
                0.5
            };
            let m4w_pan = if ml_rms + mr_rms > 10.0 {
                mr_rms / (ml_rms + mr_rms)
            } else {
                0.5
            };
            let pan_diff = (m4w_pan - ref_pan).abs();

            let dev = (norm_rat - 1.0).abs() + pan_diff * 0.5;
            if dev > max_dev {
                max_dev = dev;
                worst_order = ord;
                worst_order_pat = pat;
                worst_order_norm_ratio = norm_rat;
                worst_order_pan_delta = pan_diff;
            }

            if (0.72..=1.35).contains(&norm_rat) && pan_diff <= 0.25 {
                orders_ok += 1;
            }
        }

        let dur_ok = max_secs.is_some()
            || (ref_secs - m4w_secs).abs() <= 2.5
            || (0.90..=1.10).contains(&duration_ratio);
        let rms_ok = rms_m4w_all > 10.0 || rms_ref_all <= 10.0;
        let ord_ratio_ok = orders_total == 0 || (orders_ok * 10 >= orders_total * 8);

        let (status, msg) = if !rms_ok {
            (
                "FAIL".to_string(),
                format!(
                    "Silent output (M4W RMS={:.1}, Ref RMS={:.1})",
                    rms_m4w_all, rms_ref_all
                ),
            )
        } else if dur_ok && ord_ratio_ok {
            ("PASS".to_string(), "OK".to_string())
        } else {
            (
                "WARN".to_string(),
                format!(
                    "Dur={:.2}x, Orders={}/{}, WorstOrd={} (Pat {}, NormRat={:.2}x, PanDiff={:.2})",
                    duration_ratio,
                    orders_ok,
                    orders_total,
                    worst_order,
                    worst_order_pat,
                    worst_order_norm_ratio,
                    worst_order_pan_delta
                ),
            )
        };

        Ok(BatchFfmpegDiffEntry {
            display_path: display_path.clone(),
            format_ext: ext_str.clone(),
            detected_type,
            status,
            ref_secs,
            m4w_secs,
            duration_ratio,
            raw_rms_ratio,
            orders_total,
            orders_ok,
            worst_order,
            worst_order_pat,
            worst_order_norm_ratio,
            worst_order_pan_delta,
            wall_time_ms: t0.elapsed().as_millis() as u64,
            message: msg,
        })
    }));

    match eval_res {
        Ok(Ok(entry)) => entry,
        Ok(Err(e)) => BatchFfmpegDiffEntry {
            display_path,
            format_ext: ext_str,
            detected_type: "ERR".to_string(),
            status: "FAIL".to_string(),
            ref_secs: 0.0,
            m4w_secs: 0.0,
            duration_ratio: 0.0,
            raw_rms_ratio: 0.0,
            orders_total: 0,
            orders_ok: 0,
            worst_order: 0,
            worst_order_pat: 0,
            worst_order_norm_ratio: 0.0,
            worst_order_pan_delta: 0.0,
            wall_time_ms: t0.elapsed().as_millis() as u64,
            message: e,
        },
        Err(_) => BatchFfmpegDiffEntry {
            display_path,
            format_ext: ext_str,
            detected_type: "PANIC".to_string(),
            status: "FAIL".to_string(),
            ref_secs: 0.0,
            m4w_secs: 0.0,
            duration_ratio: 0.0,
            raw_rms_ratio: 0.0,
            orders_total: 0,
            orders_ok: 0,
            worst_order: 0,
            worst_order_pat: 0,
            worst_order_norm_ratio: 0.0,
            worst_order_pan_delta: 0.0,
            wall_time_ms: t0.elapsed().as_millis() as u64,
            message: "Panic during evaluation".to_string(),
        },
    }
}

pub fn run_batch_ffmpeg_diff(
    opts: &BatchFfmpegDiffOptions,
) -> Result<BatchFfmpegDiffReport, Box<dyn std::error::Error>> {
    if !opts.folder_path.exists() {
        return Err(format!(
            "Ordner/Datei '{}' existiert nicht.",
            opts.folder_path.display()
        )
        .into());
    }

    let t_start = Instant::now();
    let ffmpeg_bin = resolve_ffmpeg_binary(opts.ffmpeg_bin.as_deref());
    let mut candidates = Vec::new();
    collect_batch_candidates(&opts.folder_path, &mut candidates);

    let default_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let num_threads = opts
        .threads
        .filter(|&t| t > 0)
        .unwrap_or(default_threads)
        .min(candidates.len().max(1));

    println!("\n======================================================================");
    println!("MOD4WIN 3.0 Multi-Core FFmpeg (libopenmpt) Diff-Suite (--batch-ffmpeg-diff)");
    println!("======================================================================");
    println!(
        "Zielpfad:    {} ({} Module entdeckt)",
        opts.folder_path.display(),
        candidates.len()
    );
    println!("FFmpeg-Bin:  {}", ffmpeg_bin);
    println!(
        "WAV-Cache:   {}",
        opts.ref_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<RAM-Direct-Pipe (0 Byte SSD-Write)>".to_string())
    );
    println!(
        "Parallelität: {} Worker-Threads | Max-Dauer/Song: {}",
        num_threads,
        opts.max_secs
            .filter(|&s| s > 0)
            .map(|s| format!("{}s", s))
            .unwrap_or_else(|| "Kompletter Song".to_string())
    );
    println!("----------------------------------------------------------------------");

    let total = candidates.len();
    let next_idx = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let results_mutex = Mutex::new(Vec::with_capacity(total));

    std::thread::scope(|s| {
        for _ in 0..num_threads {
            s.spawn(|| loop {
                let i = next_idx.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let entry = evaluate_single_module_ffmpeg_diff(
                    &candidates[i],
                    &opts.folder_path,
                    opts.ref_dir.as_deref(),
                    &ffmpeg_bin,
                    opts.max_secs,
                    opts.ido,
                );
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                let short_path = if entry.display_path.len() > 38 {
                    format!(
                        "...{}",
                        &entry.display_path[entry.display_path.len().saturating_sub(35)..]
                    )
                } else {
                    entry.display_path.clone()
                };
                println!(
                    "[{:04}/{:04}] [{:<4}] {:<3} | {:<38} | Dur:{:5.1}s/{:5.1}s | RMS:{:5.2}x | Ord:{:>2}/{:<2} | Worst:O{:02}({:4.2}x)",
                    done,
                    total,
                    entry.status,
                    entry.detected_type,
                    short_path,
                    entry.m4w_secs,
                    entry.ref_secs,
                    entry.raw_rms_ratio,
                    entry.orders_ok,
                    entry.orders_total,
                    entry.worst_order,
                    entry.worst_order_norm_ratio
                );
                if let Ok(mut guard) = results_mutex.lock() {
                    guard.push((i, entry));
                }
            });
        }
    });

    let mut indexed_entries = results_mutex.into_inner().unwrap_or_default();
    indexed_entries.sort_by_key(|(i, _)| *i);
    let entries: Vec<BatchFfmpegDiffEntry> = indexed_entries.into_iter().map(|(_, e)| e).collect();

    let pass_count = entries.iter().filter(|e| e.status == "PASS").count();
    let warn_count = entries.iter().filter(|e| e.status == "WARN").count();
    let fail_count = entries.iter().filter(|e| e.status == "FAIL").count();
    let total_wall_time_ms = t_start.elapsed().as_millis() as u64;

    println!("----------------------------------------------------------------------");
    println!(
        "ERGEBNIS: {} geprüft | {} PASS | {} WARN | {} FAIL | Wall-Time: {:.2}s ({:.1} Songs/s)",
        total,
        pass_count,
        warn_count,
        fail_count,
        total_wall_time_ms as f64 / 1000.0,
        if total_wall_time_ms > 0 {
            (total as f64) / (total_wall_time_ms as f64 / 1000.0)
        } else {
            0.0
        }
    );

    let mut anomalies: Vec<&BatchFfmpegDiffEntry> =
        entries.iter().filter(|e| e.status != "PASS").collect();
    anomalies.sort_by(|a, b| {
        let da = (a.worst_order_norm_ratio - 1.0).abs();
        let db = (b.worst_order_norm_ratio - 1.0).abs();
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    if !anomalies.is_empty() {
        println!("\nAUFFÄLLIGE DATEIEN (WARN/FAIL, Top 25 nach Abweichung):");
        for e in anomalies.iter().take(25) {
            println!("  [{}] {} -> {}", e.status, e.display_path, e.message);
        }
    }

    let report = BatchFfmpegDiffReport {
        root_folder: opts.folder_path.display().to_string(),
        total_files: total,
        pass_count,
        warn_count,
        fail_count,
        threads_used: num_threads,
        total_wall_time_ms,
        entries,
    };

    let json_target = opts
        .report_json
        .clone()
        .unwrap_or_else(|| PathBuf::from("batch_ffmpeg_diff_report.json"));
    write_batch_ffmpeg_diff_json(&report, &json_target)?;
    println!("JSON-Report gespeichert: {}", json_target.display());
    println!("======================================================================\n");

    Ok(report)
}

fn write_batch_ffmpeg_diff_json(
    report: &BatchFfmpegDiffReport,
    out_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let mut json = String::with_capacity(4096 + report.entries.len() * 320);
    json.push_str("{\n");
    json.push_str(&format!(
        "  \"root_folder\": \"{}\",\n",
        escape_json_str(&report.root_folder)
    ));
    json.push_str(&format!("  \"total_files\": {},\n", report.total_files));
    json.push_str(&format!("  \"pass_count\": {},\n", report.pass_count));
    json.push_str(&format!("  \"warn_count\": {},\n", report.warn_count));
    json.push_str(&format!("  \"fail_count\": {},\n", report.fail_count));
    json.push_str(&format!("  \"threads_used\": {},\n", report.threads_used));
    json.push_str(&format!(
        "  \"total_wall_time_ms\": {},\n",
        report.total_wall_time_ms
    ));
    json.push_str("  \"files\": [\n");
    let n = report.entries.len();
    for (idx, e) in report.entries.iter().enumerate() {
        let comma = if idx + 1 < n { "," } else { "" };
        let _ = write!(
            json,
            "    {{ \"path\": \"{}\", \"format\": \"{}\", \"detected_type\": \"{}\", \"status\": \"{}\", \"ref_secs\": {:.2}, \"m4w_secs\": {:.2}, \"duration_ratio\": {:.4}, \"raw_rms_ratio\": {:.4}, \"orders_total\": {}, \"orders_ok\": {}, \"worst_order\": {}, \"worst_order_pat\": {}, \"worst_order_norm_ratio\": {:.4}, \"worst_order_pan_delta\": {:.4}, \"wall_time_ms\": {}, \"message\": \"{}\" }}{}\n",
            escape_json_str(&e.display_path),
            escape_json_str(&e.format_ext),
            escape_json_str(&e.detected_type),
            escape_json_str(&e.status),
            e.ref_secs,
            e.m4w_secs,
            e.duration_ratio,
            e.raw_rms_ratio,
            e.orders_total,
            e.orders_ok,
            e.worst_order,
            e.worst_order_pat,
            e.worst_order_norm_ratio,
            e.worst_order_pan_delta,
            e.wall_time_ms,
            escape_json_str(&e.message),
            comma
        );
    }
    json.push_str("  ]\n}\n");
    fs::write(out_path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_short_hex_6_matches_dotnet_md5() {
        // "" -> d41d8cd98f00b204e9800998ecf8427e -> "D41D8C"
        assert_eq!(md5_short_hex_6(b""), "D41D8C");
        // "abc" -> 900150983cd24fb0d6963f7d28e17f72 -> "900150"
        assert_eq!(md5_short_hex_6(b"abc"), "900150");
    }

    #[test]
    fn test_detect_batch_mod_format_pc_and_amiga_modland() {
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/SPACE_DEBRIS.MOD")),
            Some(("mod", ".MOD"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/track.nst")),
            Some(("mod", ".MOD"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/song.okta")),
            Some(("okt", ".OKT"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/mod.space debris")),
            Some(("mod", ".MOD"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/mod.stardust.memories")),
            Some(("mod", ".MOD"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/corpus/okt.cyberpunk")),
            Some(("okt", ".OKT"))
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/results/pass/mod.space debris.0.TXT")),
            None
        );
        assert_eq!(
            detect_batch_mod_format(Path::new("/results/wav/mod.space debris.0.wav")),
            None
        );
    }
}

