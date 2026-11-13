//! Persistenz für `MOD4WIN.INI` und die PeakSearch-Datenbank (`MOD4WIN.PKS` / `[Peaks]`).
//! Spiegelt die originalen INI-Sektionen und Schlüssel aus `mod4win.pas`, `dsetup.pas` und `player32.pas`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Geräte-ID für Direct-to-Disk WAV-Export (`dev_HDD = -1` in `mod4win.pas`).
pub const DEV_HDD: i32 = -1;
/// Standard-Soundkarten-Geräte-ID (`0` = CPAL Default Output Device).
pub const DEV_SOUNDCARD: i32 = 0;

/// Eine einzelne Tastenkombination im Hotkey-Editor (`KEYSEL.PAS` / `modc.pas`).
/// `state`: Bit 0 (`1`) = Control, Bit 1 (`2`) = Shift, Bit 2 (`4`) = Alt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub action_id: u8,
    pub label: &'static str,
    pub key_name: String,
    pub state: u8,
}

/// Alle 43 steuerbaren Original-Events aus `modc.pas` (`defkeys[1..43]`) und `M4W_USA.RES` (`2100..2142`),
/// sortiert nach ihrer Funktionsnummer `1..43` (wie `lbs_sort` in `KEYSEL.PAS`).
pub fn default_hotkey_bindings() -> Vec<HotkeyBinding> {
    let defs: &[(u8, &'static str, &'static str, u8)] = &[
        (1, " 1. Stop playback", "ESCAPE", 0),
        (2, " 2. Start playback", "SPACE", 0),
        (3, " 3. Pause playback", "P", 0),
        (4, " 4. Previous song", "CURSOR UP", 0),
        (5, " 5. Rewind", "CURSOR LEFT", 0),
        (6, " 6. Fast forward", "CURSOR RIGHT", 0),
        (7, " 7. Next song", "CURSOR DOWN", 0),
        (8, " 8. First song", "HOME", 0),
        (9, " 9. Last song", "END", 0),
        (10, "10. Open dialog", "O", 0),
        (11, "11. Show Quick Select Box", "L", 0),
        (12, "12. File menu", "F", 0),
        (13, "13. Info dialog", "I", 0),
        (14, "14. Effect panel", "E", 0),
        (15, "15. Elapsed/Rest time display mode", "T", 0),
        (16, "16. File name display", "PAGE UP", 0),
        (17, "17. Song name display", "PAGE DOWN", 0),
        (18, "18. External mixer", "M", 0),
        (19, "19. Setup dialog", "S", 0),
        (20, "20. Open Help System", "F1", 0),
        (21, "21. Shuffle mode", "F2", 0),
        (22, "22. Repeat mode", "F3", 0),
        (23, "23. Introscan mode", "F4", 0),
        (24, "24. Copy file", "F5", 0),
        (25, "25. Move file", "F6", 0),
        (26, "26. Remove File From Playlist", "F7", 0),
        (27, "27. Delete file", "F8", 0),
        (28, "28. Toggle Mixer Clipping", "F9", 0),
        (29, "29. Toggle BPM-Speeds enabled", "F10", 0),
        (30, "30. Toggle Surround Sound", "F11", 0),
        (31, "31. Toggle IDO-Filter", "F12", 0),
        (32, "32. Toggle Panning", "INSERT", 0),
        (33, "33. Decrease Pre-Amplification", "NUMPAD DIVIDE", 0),
        (34, "34. Increase Pre-Amplification", "NUMPAD MULTIPLY", 0),
        (35, "35. Next Instrument (Info dialog)", "NUMPAD PLUS", 0),
        (36, "36. Prev Instrument (Info dialog)", "NUMPAD MINUS", 0),
        (37, "37. Song Info up    (Info dialog)", "CURSOR UP", 2),
        (38, "38. Song Info down  (Info dialog)", "CURSOR DOWN", 2),
        (39, "39. Song Info left  (Info dialog)", "CURSOR LEFT", 2),
        (40, "40. Song Info right (Info dialog)", "CURSOR RIGHT", 2),
        (41, "41. Next Instrument (Detail dialog)", "NUMPAD PLUS", 1),
        (42, "42. Prev Instrument (Detail dialog)", "NUMPAD MINUS", 1),
        (43, "43. About this program", "A", 0),
    ];
    defs.iter()
        .map(|&(action_id, label, key_name, state)| HotkeyBinding {
            action_id,
            label,
            key_name: key_name.to_string(),
            state,
        })
        .collect()
}

/// Vollständige Setup- und Status-Konfiguration aus `MOD4WIN.INI`.
#[derive(Clone, Debug)]
pub struct Mod4WinIniConfig {
    // [Device]
    pub device_id: i32,
    pub device_name: String,

    // [Sampling]
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
    pub surround: bool,
    pub output_buffers: u8,
    pub hdd_rec_buffers: u8,
    /// Visueller Hauptfenster-Timer-Modus: `0` = MM Task, `1` = Win Timer, `2` = Interrupt (MMTimer).
    pub timer_mode: u8,
    /// Engine-Emulationsmodus im Setup: `false` = Full Emulation (inkl. 1995er Stack-/Speicher-Quirks),
    /// `true` = Clean Mode (bereinigte Hüllkurven/Perioden).
    pub clean_mode: bool,
    pub clipping: bool,
    /// Optionaler Per-Kanal-De-Click-Filter (`DeClick`).
    pub declick: bool,
    /// Authentischer Amiga-Paula-Tiefpassfilter (`Amiga`: 4.42 kHz RC + ~3.275 kHz Sallen-Key LED-Filter).
    pub amiga_filter: bool,
    /// Moderner adaptiver 5-Band-Wiener-/Spektral-Denoiser (`DeHiss`).
    pub dehiss: bool,
    /// OverSampling (IDO): `0` = Off, `1` = Linear, `2` = Kubisch (FPU).
    pub over_sampling: u8,
    pub enable_panning: bool,
    pub pre_amplification: u8,
    pub auto_preamp_peaksearch: bool,

    // [State]
    pub override_rate_change: bool,
    pub pause_while_setup: bool,
    pub play_on_load: bool,
    pub move_on_click: bool,
    pub full_window_move: bool,
    pub ask_hd: bool,
    pub state_saver: bool,
    pub force_filename_display: bool,
    pub mini_on_top: bool,
    pub mini_state: bool,
    pub x_main: i32,
    pub y_main: i32,
    pub x_mini: i32,
    pub y_mini: i32,
    pub mixer_path: String,
    pub hd_path: String,
    pub last_dir: String,
    pub last_fm_dir: String,

    // [State] / StateSaver-Wiedergabezustand (wie `MOD4WIN.STA`)
    pub saved_playlist: Vec<PathBuf>,
    pub saved_playlist_idx: usize,
    pub saved_order_pos: usize,
    pub saved_playing: bool,
    pub saved_paused: bool,
    pub saved_repeat_mode: u8,
    pub saved_shuffle: bool,
    pub saved_intro_scan: bool,
    pub saved_minus_mode: bool,

    // [Packer]
    pub show_pack_window: bool,
    pub arj_pack: String,
    pub arj_unpack: String,
    pub zip_pack: String,
    pub zip_unpack: String,
    pub lha_pack: String,
    pub lha_unpack: String,
    pub arj_cmd_extract: String,
    pub arj_cmd_pack: String,
    pub arj_cmd_del: String,
    pub zip_cmd_extract: String,
    pub zip_cmd_pack: String,
    pub zip_cmd_del: String,
    pub lha_cmd_extract: String,
    pub lha_cmd_pack: String,
    pub lha_cmd_del: String,

    // [Hotkeys] (43 konfigurierbare Hotkeys aus `KEYSEL.PAS`)
    pub hotkeys: Vec<HotkeyBinding>,

    // [Language]
    /// `0` = ENGLISH, `1` = GERMAN
    pub language: u8,

    // [State] / Historische CPU- & Puffer-Simulation (Paket 6)
    /// `0` = Native, `1..=10` = 386 SX/16 .. AMD K6-2 450 (`CpuProfile`)
    pub cpu_sim_profile: u8,
    /// `false` = Visual Only (Standard), `true` = Authentic 1995 DMA-Underrun Stutter (`--authentic-stutter`)
    pub authentic_stutter: bool,
    /// `0` = 2.40 Classic, `1` = 2.30 Classic, `2` = Dark Classic (`ModernDarkHiFi`), `3` = Modern (`SkinMode`)
    pub skin_mode: u8,
    /// Gespeicherter GUI-Skalierungsfaktor (`0.5 .. 4.0`, Standard `1.0` = 1:1 Originalgröße aus `mod4win.pas`)
    pub gui_scale: f64,
    /// Anzahl der sichtbaren Zeilen im Playlist-Dock des Modern Skins (`5..=36`, Standard `9` = `225 px` Höhe)
    pub modern_playlist_rows: u8,
    /// Beim Beenden geöffnete Unterfenster (`"INFO"`, `"INSTRUMENTS"`, `"EFFECTS"`, `"QUICK_SELECT"`, `"OPEN_DIALOG"`, `"SETUP"`, `"HELP"`, ...)
    pub saved_open_windows: Vec<String>,
    /// Gespeicherte Fensterpositionen (`"MAIN"`, `"MINI"`, `"EFFECTS"`, `"INFO"`, `"INSTRUMENTS"`, `"SETUP"`, ...)
    pub window_positions: HashMap<String, (i32, i32)>,

    // [Peaks] (Zusätzlich zur binären `MOD4WIN.PKS`: Schlüssel `"<fsize>_<crc16>"` -> PreAmp 0..40)
    pub peaks: HashMap<String, u8>,
}

impl Default for Mod4WinIniConfig {
    fn default() -> Self {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned();
        #[cfg(target_os = "linux")]
        let default_mixer = "alsamixer".to_string();
        #[cfg(not(target_os = "linux"))]
        let default_mixer = "sndvol.exe".to_string();

        #[cfg(target_os = "linux")]
        let default_hd_path = "./WAV_OUT/".to_string();
        #[cfg(not(target_os = "linux"))]
        let default_hd_path = cwd.clone();

        Self {
            device_id: DEV_SOUNDCARD,
            device_name: "Wave Mapper (CPAL Default)".to_string(),
            sample_rate: 44100,
            bits_per_sample: 16,
            channels: 2,
            surround: false,
            output_buffers: 18,
            hdd_rec_buffers: 4,
            timer_mode: 2,
            clean_mode: false,
            clipping: true,
            declick: false,
            amiga_filter: false,
            dehiss: false,
            over_sampling: 2, // Kubisch (FPU) als Standard
            enable_panning: true,
            pre_amplification: 20,
            auto_preamp_peaksearch: true,
            override_rate_change: true,
            pause_while_setup: false,
            play_on_load: true,
            move_on_click: true,
            full_window_move: false,
            ask_hd: true,
            state_saver: true,
            force_filename_display: false,
            mini_on_top: true,
            mini_state: false,
            x_main: 90,
            y_main: 100,
            x_mini: 90,
            y_mini: 100,
            mixer_path: default_mixer,
            hd_path: default_hd_path,
            last_dir: cwd.clone(),
            last_fm_dir: cwd,
            saved_playlist: Vec::new(),
            saved_playlist_idx: 0,
            saved_order_pos: 0,
            saved_playing: false,
            saved_paused: false,
            saved_repeat_mode: 0,
            saved_shuffle: false,
            saved_intro_scan: false,
            saved_minus_mode: false,
            show_pack_window: false,
            arj_pack: "arj.exe".to_string(),
            arj_unpack: "arj.exe".to_string(),
            zip_pack: "pkzip.exe".to_string(),
            zip_unpack: "pkunzip.exe".to_string(),
            lha_pack: "lha.exe".to_string(),
            lha_unpack: "lha.exe".to_string(),
            arj_cmd_extract: "e -y -v".to_string(),
            arj_cmd_pack: "a -y".to_string(),
            arj_cmd_del: "d -y".to_string(),
            zip_cmd_extract: "-o".to_string(),
            zip_cmd_pack: "-a".to_string(),
            zip_cmd_del: "-d".to_string(),
            lha_cmd_extract: "e -m -c".to_string(),
            lha_cmd_pack: "a -m".to_string(),
            lha_cmd_del: "d -m".to_string(),
            hotkeys: default_hotkey_bindings(),
            language: 0,
            cpu_sim_profile: 0,
            authentic_stutter: false,
            skin_mode: 0,
            gui_scale: 1.0,
            modern_playlist_rows: 9,
            saved_open_windows: Vec::new(),
            window_positions: HashMap::new(),
            peaks: HashMap::new(),
        }
    }
}

impl Mod4WinIniConfig {
    /// Ermittelt den Standardpfad für `MOD4WIN.INI` (im aktuellen Arbeitsverzeichnis oder neben der `.exe`).
    pub fn default_ini_path() -> PathBuf {
        let local = PathBuf::from("MOD4WIN.INI");
        if local.exists() {
            return local;
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent() {
                let candidate = dir.join("MOD4WIN.INI");
                if candidate.exists() {
                    return candidate;
                }
            }
        local
    }

    /// Ermittelt den Standardpfad für `MOD4WIN.PKS` (binäre PeakSearch-Datenbank wie in `player32.pas`,
    /// im selben Verzeichnis wie `MOD4WIN.INI` bzw. neben der `.exe`).
    pub fn default_pks_path() -> PathBuf {
        let local = PathBuf::from("MOD4WIN.PKS");
        if local.exists() {
            return local;
        }
        Self::default_ini_path().with_file_name("MOD4WIN.PKS")
    }

    /// Berechnet den Speicherbedarf (`min. Memory` in KB) exakt nach der Formel aus `dsetup.pas`:
    /// - Bei `DEV_HDD`: `hdd_rec_buffers * 32` KB
    /// - Sonst: `(output_buffers * channels * (bits/8) * 200 * (sample_rate / 1000)) / 1024` KB
    pub fn calc_min_memory_kb(&self) -> u32 {
        if self.device_id == DEV_HDD {
            (self.hdd_rec_buffers.max(1) as u32) * 32
        } else {
            let n_buf = self.output_buffers.max(2) as u32;
            let ch = self.channels.clamp(1, 2) as u32;
            let bytes_per_smp = (self.bits_per_sample / 8).clamp(1, 2) as u32;
            let khz = (self.sample_rate / 1000).clamp(1, 96);
            (n_buf * ch * bytes_per_smp * 200 * khz) / 1024
        }
    }

    /// Lädt `MOD4WIN.INI` von der Festplatte (oder gibt `Default` zurück, falls nicht vorhanden).
    /// Unterstützt sowohl UTF-8 (mit/ohne BOM) als auch historische Windows-1252/Latin-1 Dateien aus MOD4WIN 2.40.
    pub fn load_from_file(path: &Path) -> Self {
        let mut cfg = Self::default();
        let Ok(raw_bytes) = fs::read(path) else {
            return cfg;
        };
        let stripped = raw_bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw_bytes);
        let content: String = match std::str::from_utf8(stripped) {
            Ok(s) => s.to_string(),
            Err(_) => stripped.iter().map(|&b| b as char).collect(),
        };

        let mut current_section = String::new();
        let mut playlist_entries: Vec<(usize, PathBuf)> = Vec::new();

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim().to_ascii_uppercase();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim();

            match current_section.as_str() {
                "DEVICE" => match key.to_ascii_uppercase().as_str() {
                    "DEVICEID" => {
                        if let Ok(id) = val.parse::<i32>() {
                            cfg.device_id = if id == DEV_HDD { DEV_HDD } else { DEV_SOUNDCARD };
                        }
                    }
                    "DEVICENAME"
                        if !val.is_empty() => {
                            cfg.device_name = val.to_string();
                        }
                    _ => {}
                },
                "SAMPLING" => match key.to_ascii_uppercase().as_str() {
                    "SAMPLERATE" => {
                        if let Ok(sr) = val.parse::<u32>()
                            && (4000..=96000).contains(&sr) {
                                cfg.sample_rate = sr;
                            }
                    }
                    "BITSPERSAMPLE" => {
                        if let Ok(b) = val.parse::<u8>() {
                            cfg.bits_per_sample = if b == 8 { 8 } else { 16 };
                        }
                    }
                    "CHANNELS" => {
                        if let Ok(c) = val.parse::<u8>() {
                            cfg.channels = c.clamp(1, 2);
                        }
                    }
                    "SURROUND" => {
                        cfg.surround = val != "0";
                    }
                    "OUTPUTBUFFERS" => {
                        if let Ok(nb) = val.parse::<u8>() {
                            cfg.output_buffers = nb.clamp(2, 99);
                        }
                    }
                    "HDDRECBUFFERS" => {
                        if let Ok(nb) = val.parse::<u8>() {
                            cfg.hdd_rec_buffers = nb.clamp(1, 99);
                        }
                    }
                    "TIMERMODE" => {
                        if let Ok(tm) = val.parse::<u8>() {
                            cfg.timer_mode = tm.min(2);
                        }
                    }
                    "CLEANMODE" => {
                        cfg.clean_mode = val != "0" && !val.eq_ignore_ascii_case("false");
                    }
                    "CLIPPING" => {
                        cfg.clipping = val != "0";
                    }
                    "DECLICK" => {
                        cfg.declick = val != "0" && !val.eq_ignore_ascii_case("false");
                    }
                    "AMIGAFILTER" | "AMIGA" => {
                        cfg.amiga_filter = val != "0" && !val.eq_ignore_ascii_case("false");
                    }
                    "DEHISS" => {
                        cfg.dehiss = val != "0" && !val.eq_ignore_ascii_case("false");
                    }
                    "OVERSAMPLING" => {
                        if let Ok(os) = val.parse::<u8>() {
                            // 0=IDO OFF, 1=IDO Linear, 2=IDO CPU-Cubic, 3=IDO FPU-Cubic,
                            // 4=IT Off, 5=IT Linear, 6=IT Cubic Spline
                            cfg.over_sampling = os.min(6);
                        }
                    }
                    "ENABLEPANNING" => {
                        cfg.enable_panning = val != "0";
                    }
                    "PREAMPLIFICATION" => {
                        if let Ok(pa) = val.parse::<u8>() {
                            cfg.pre_amplification = pa.min(40);
                        }
                    }
                    "PEAKSEARCH" | "AUTOPREAMP" => {
                        cfg.auto_preamp_peaksearch = val != "0";
                    }
                    _ => {}
                },
                "STATE" => match key.to_ascii_uppercase().as_str() {
                    "OVERRIDERATECHANGE" => cfg.override_rate_change = val != "0",
                    "PAUSEWHILESETUP" => cfg.pause_while_setup = val != "0",
                    "PLAYONLOAD" => cfg.play_on_load = val != "0",
                    "MOVEONCLICK" => cfg.move_on_click = val != "0",
                    "FULLWINDOWMOVE" => cfg.full_window_move = val != "0",
                    "ASKHD" => cfg.ask_hd = val != "0",
                    "STATESAVER" => cfg.state_saver = val != "0",
                    "FORCEFILENAMEDISPLAY" => cfg.force_filename_display = val != "0",
                    "MINIONTOP" => cfg.mini_on_top = val != "0",
                    "MINISTATE" => cfg.mini_state = val != "0",
                    "X_MAIN" => {
                        if let Ok(v) = val.parse::<i32>() {
                            cfg.x_main = v.max(0);
                        }
                    }
                    "Y_MAIN" => {
                        if let Ok(v) = val.parse::<i32>() {
                            cfg.y_main = v.max(0);
                        }
                    }
                    "X_MINI" => {
                        if let Ok(v) = val.parse::<i32>() {
                            cfg.x_mini = v.max(0);
                        }
                    }
                    "Y_MINI" => {
                        if let Ok(v) = val.parse::<i32>() {
                            cfg.y_mini = v.max(0);
                        }
                    }
                    "MIXERPATH" => cfg.mixer_path = val.to_string(),
                    "HDPATH" => {
                        if !val.is_empty() {
                            #[cfg(not(target_os = "windows"))]
                            if val.len() >= 2 && val.as_bytes()[1] == b':' {
                                cfg.hd_path = "./WAV_OUT/".to_string();
                            } else {
                                cfg.hd_path = val.replace('\\', "/");
                            }
                            #[cfg(target_os = "windows")]
                            {
                                cfg.hd_path = val.to_string();
                            }
                        }
                    }
                    "LASTDIR" => {
                        if !val.is_empty() {
                            #[cfg(not(target_os = "windows"))]
                            if !(val.len() >= 2 && val.as_bytes()[1] == b':') {
                                cfg.last_dir = val.replace('\\', "/");
                            }
                            #[cfg(target_os = "windows")]
                            {
                                cfg.last_dir = val.to_string();
                            }
                        }
                    }
                    "LASTFMDIR" => {
                        if !val.is_empty() {
                            #[cfg(not(target_os = "windows"))]
                            if !(val.len() >= 2 && val.as_bytes()[1] == b':') {
                                cfg.last_fm_dir = val.replace('\\', "/");
                            }
                            #[cfg(target_os = "windows")]
                            {
                                cfg.last_fm_dir = val.to_string();
                            }
                        }
                    }
                    "SAVEDPLAYLISTINDEX" => {
                        if let Ok(idx) = val.parse::<usize>() {
                            cfg.saved_playlist_idx = idx;
                        }
                    }
                    "SAVEDORDERPOS" => {
                        if let Ok(ord) = val.parse::<usize>() {
                            cfg.saved_order_pos = ord;
                        }
                    }
                    "SAVEDPLAYING" => cfg.saved_playing = val != "0",
                    "SAVEDPAUSED" => cfg.saved_paused = val != "0",
                    "SAVEDREPEATMODE" => {
                        if let Ok(rm) = val.parse::<u8>() {
                            cfg.saved_repeat_mode = rm.min(2);
                        }
                    }
                    "SAVEDSHUFFLE" => cfg.saved_shuffle = val != "0",
                    "SAVEDINTROSCAN" => cfg.saved_intro_scan = val != "0",
                    "SAVEDMINUSMODE" => cfg.saved_minus_mode = val != "0",
                    "CPUSIMPROFILE" | "CPUSIM" => {
                        if let Ok(idx) = val.parse::<u8>() {
                            cfg.cpu_sim_profile = idx.min(10);
                        } else if let Some(prof) = super::cpu_sim::CpuProfile::parse_str(val) {
                            cfg.cpu_sim_profile = prof.as_index();
                        }
                    }
                    "AUTHENTICSTUTTER" => cfg.authentic_stutter = val != "0",
                    "SKINMODE" | "SKIN" => {
                        if let Ok(sm) = val.parse::<u8>() {
                            cfg.skin_mode = sm.min(3);
                        } else if let Some(sm) = super::theme_modern::SkinMode::parse_str(val) {
                            cfg.skin_mode = sm.to_u8();
                        }
                    }
                    "GUISCALE" | "SCALEFACTOR" => {
                        if let Ok(sc) = val.parse::<f64>()
                            && sc.is_finite() {
                                cfg.gui_scale = sc.clamp(0.5, 4.0);
                            }
                    }
                    "MODERNPLAYLISTROWS" => {
                        if let Ok(rows) = val.parse::<u8>() {
                            cfg.modern_playlist_rows = rows.clamp(5, 36);
                        }
                    }
                    "OPENWINDOWS" => {
                        cfg.saved_open_windows = val
                            .split(',')
                            .map(|s| s.trim().to_ascii_uppercase())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    _ => {}
                },
                "WINDOWS" => {
                    if let Some((xs, ys)) = val.split_once(',')
                        && let (Ok(x), Ok(y)) = (xs.trim().parse::<i32>(), ys.trim().parse::<i32>()) {
                            cfg.window_positions
                                .insert(key.to_ascii_uppercase(), (x.max(0), y.max(0)));
                        }
                }
                "PACKER" => match key.to_ascii_uppercase().as_str() {
                    "SHOWWINDOW" => cfg.show_pack_window = val != "0",
                    "ARJ_P" => cfg.arj_pack = val.to_string(),
                    "ARJ_UP" => cfg.arj_unpack = val.to_string(),
                    "ZIP_P" => cfg.zip_pack = val.to_string(),
                    "ZIP_UP" => cfg.zip_unpack = val.to_string(),
                    "LHA_P" => cfg.lha_pack = val.to_string(),
                    "LHA_UP" => cfg.lha_unpack = val.to_string(),
                    "ARJ_CMD_UP" => cfg.arj_cmd_extract = val.to_string(),
                    "ARJ_CMD_P" => cfg.arj_cmd_pack = val.to_string(),
                    "ARJ_CMD_DEL" => cfg.arj_cmd_del = val.to_string(),
                    "ZIP_CMD_UP" => cfg.zip_cmd_extract = val.to_string(),
                    "ZIP_CMD_P" => cfg.zip_cmd_pack = val.to_string(),
                    "ZIP_CMD_DEL" => cfg.zip_cmd_del = val.to_string(),
                    "LHA_CMD_UP" => cfg.lha_cmd_extract = val.to_string(),
                    "LHA_CMD_P" => cfg.lha_cmd_pack = val.to_string(),
                    "LHA_CMD_DEL" => cfg.lha_cmd_del = val.to_string(),
                    _ => {}
                },
                "HOTKEYS" => {
                    if let Some(id_str) = key.strip_prefix("KEY").or_else(|| key.strip_prefix("Key"))
                        && let Ok(act_id) = id_str.parse::<u8>()
                            && let Some((k_name, st_str)) = val.rsplit_once('|')
                                && let Ok(st_val) = st_str.trim().parse::<u8>()
                                    && let Some(hk) =
                                        cfg.hotkeys.iter_mut().find(|h| h.action_id == act_id)
                                    {
                                        hk.key_name = k_name.trim().to_string();
                                        hk.state = st_val & 0x07;
                                    }
                }
                "LANGUAGE" => {
                    if key.eq_ignore_ascii_case("LANGUAGE")
                        && let Ok(l) = val.parse::<u8>() {
                            cfg.language = l.min(6);
                        }
                }
                "PLAYLIST" => {
                    if let Some(num_str) = key.strip_prefix("FILE").or_else(|| key.strip_prefix("File"))
                        && let Ok(idx) = num_str.parse::<usize>()
                            && !val.is_empty() {
                                // Kein synchroner Disk-Syscall (`exists()` / `read_dir()`) pro Playlist-Zeile beim INI-Start!
                                // Bei 8.700+ oder 999.999 Einträgen würde `GetFileAttributesW` sonst Sekunden bis Minuten blockieren.
                                #[cfg(target_os = "windows")]
                                let p = PathBuf::from(val);
                                #[cfg(not(target_os = "windows"))]
                                let p = PathBuf::from(
                                    super::archive_mol::normalize_mol_path_separators(val),
                                );
                                playlist_entries.push((idx, p));
                            }
                }
                "PEAKS" => {
                    if let Ok(pa) = val.parse::<u8>() {
                        cfg.peaks.insert(key.to_string(), pa.min(40));
                    }
                }
                _ => {}
            }
        }

        if !playlist_entries.is_empty() {
            playlist_entries.sort_by_key(|(idx, _)| *idx);
            cfg.saved_playlist = playlist_entries.into_iter().map(|(_, p)| p).collect();
        }

        cfg
    }

    /// Speichert die komplette Konfiguration in `MOD4WIN.INI`.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(2048 + self.saved_playlist.len() * 64);

        out.push_str("[Device]\r\n");
        out.push_str(&format!("DeviceID={}\r\n", self.device_id));
        out.push_str(&format!("DeviceName={}\r\n\r\n", self.device_name));

        out.push_str("[Sampling]\r\n");
        out.push_str(&format!("SampleRate={}\r\n", self.sample_rate));
        out.push_str(&format!("BitsPerSample={}\r\n", self.bits_per_sample));
        out.push_str(&format!("Channels={}\r\n", self.channels));
        out.push_str(&format!("Surround={}\r\n", u8::from(self.surround)));
        out.push_str(&format!("OutputBuffers={}\r\n", self.output_buffers));
        out.push_str(&format!("HDDRecBuffers={}\r\n", self.hdd_rec_buffers));
        out.push_str(&format!("TimerMode={}\r\n", self.timer_mode));
        out.push_str(&format!("CleanMode={}\r\n", u8::from(self.clean_mode)));
        out.push_str(&format!("Clipping={}\r\n", u8::from(self.clipping)));
        out.push_str(&format!("DeClick={}\r\n", u8::from(self.declick)));
        out.push_str(&format!("AmigaFilter={}\r\n", u8::from(self.amiga_filter)));
        out.push_str(&format!("DeHiss={}\r\n", u8::from(self.dehiss)));
        // Im Original: 0=Off, 1=Linear, 3=Cub (bzw. 2 in unserer UI-Darstellung)
        out.push_str(&format!("OverSampling={}\r\n", self.over_sampling));
        out.push_str(&format!("EnablePanning={}\r\n", u8::from(self.enable_panning)));
        out.push_str(&format!("PreAmplification={}\r\n", self.pre_amplification));
        out.push_str(&format!(
            "PeakSearch={}\r\n\r\n",
            u8::from(self.auto_preamp_peaksearch)
        ));

        out.push_str("[State]\r\n");
        out.push_str(&format!(
            "OverrideRateChange={}\r\n",
            u8::from(self.override_rate_change)
        ));
        out.push_str(&format!(
            "PauseWhileSetup={}\r\n",
            u8::from(self.pause_while_setup)
        ));
        out.push_str(&format!("PlayOnLoad={}\r\n", u8::from(self.play_on_load)));
        out.push_str(&format!("MoveOnClick={}\r\n", u8::from(self.move_on_click)));
        out.push_str(&format!(
            "FullWindowMove={}\r\n",
            u8::from(self.full_window_move)
        ));
        out.push_str(&format!("AskHD={}\r\n", u8::from(self.ask_hd)));
        out.push_str(&format!("StateSaver={}\r\n", u8::from(self.state_saver)));
        out.push_str(&format!(
            "ForceFileNameDisplay={}\r\n",
            u8::from(self.force_filename_display)
        ));
        out.push_str(&format!("MiniOnTop={}\r\n", u8::from(self.mini_on_top)));
        out.push_str(&format!("MiniState={}\r\n", u8::from(self.mini_state)));
        out.push_str(&format!("X_MAIN={}\r\n", self.x_main));
        out.push_str(&format!("Y_MAIN={}\r\n", self.y_main));
        out.push_str(&format!("X_MINI={}\r\n", self.x_mini));
        out.push_str(&format!("Y_MINI={}\r\n", self.y_mini));
        out.push_str(&format!("MixerPath={}\r\n", self.mixer_path));
        out.push_str(&format!("HDPath={}\r\n", self.hd_path));
        out.push_str(&format!("LastDir={}\r\n", self.last_dir));
        out.push_str(&format!("LastFMDir={}\r\n", self.last_fm_dir));
        out.push_str(&format!("SavedPlaylistIndex={}\r\n", self.saved_playlist_idx));
        out.push_str(&format!("SavedOrderPos={}\r\n", self.saved_order_pos));
        out.push_str(&format!("SavedPlaying={}\r\n", u8::from(self.saved_playing)));
        out.push_str(&format!("SavedPaused={}\r\n", u8::from(self.saved_paused)));
        out.push_str(&format!("SavedRepeatMode={}\r\n", self.saved_repeat_mode));
        out.push_str(&format!("SavedShuffle={}\r\n", u8::from(self.saved_shuffle)));
        out.push_str(&format!(
            "SavedIntroScan={}\r\n",
            u8::from(self.saved_intro_scan)
        ));
        out.push_str(&format!(
            "SavedMinusMode={}\r\n",
            u8::from(self.saved_minus_mode)
        ));
        out.push_str(&format!("CpuSimProfile={}\r\n", self.cpu_sim_profile));
        out.push_str(&format!(
            "AuthenticStutter={}\r\n",
            u8::from(self.authentic_stutter)
        ));
        out.push_str(&format!("SkinMode={}\r\n", self.skin_mode));
        out.push_str(&format!("GuiScale={:.3}\r\n", self.gui_scale.clamp(0.5, 4.0)));
        out.push_str(&format!(
            "ModernPlaylistRows={}\r\n",
            self.modern_playlist_rows.clamp(5, 36)
        ));
        out.push_str(&format!(
            "OpenWindows={}\r\n\r\n",
            self.saved_open_windows.join(",")
        ));

        out.push_str("[Windows]\r\n");
        let mut sorted_win: Vec<_> = self.window_positions.iter().collect();
        sorted_win.sort_by_key(|(k, _)| *k);
        for (k, (x, y)) in sorted_win {
            out.push_str(&format!("{}={},{}\r\n", k, x, y));
        }
        out.push_str("\r\n");

        out.push_str("[Packer]\r\n");
        out.push_str(&format!("ShowWindow={}\r\n", u8::from(self.show_pack_window)));
        out.push_str(&format!("ARJ_P={}\r\n", self.arj_pack));
        out.push_str(&format!("ARJ_UP={}\r\n", self.arj_unpack));
        out.push_str(&format!("ZIP_P={}\r\n", self.zip_pack));
        out.push_str(&format!("ZIP_UP={}\r\n", self.zip_unpack));
        out.push_str(&format!("LHA_P={}\r\n", self.lha_pack));
        out.push_str(&format!("LHA_UP={}\r\n", self.lha_unpack));
        out.push_str(&format!("ARJ_CMD_UP={}\r\n", self.arj_cmd_extract));
        out.push_str(&format!("ARJ_CMD_P={}\r\n", self.arj_cmd_pack));
        out.push_str(&format!("ARJ_CMD_DEL={}\r\n", self.arj_cmd_del));
        out.push_str(&format!("ZIP_CMD_UP={}\r\n", self.zip_cmd_extract));
        out.push_str(&format!("ZIP_CMD_P={}\r\n", self.zip_cmd_pack));
        out.push_str(&format!("ZIP_CMD_DEL={}\r\n", self.zip_cmd_del));
        out.push_str(&format!("LHA_CMD_UP={}\r\n", self.lha_cmd_extract));
        out.push_str(&format!("LHA_CMD_P={}\r\n", self.lha_cmd_pack));
        out.push_str(&format!("LHA_CMD_DEL={}\r\n\r\n", self.lha_cmd_del));

        out.push_str("[Hotkeys]\r\n");
        for hk in &self.hotkeys {
            out.push_str(&format!(
                "Key{}={}|{}\r\n",
                hk.action_id, hk.key_name, hk.state
            ));
        }
        out.push_str("\r\n");

        out.push_str("[Language]\r\n");
        out.push_str(&format!("Language={}\r\n\r\n", self.language));

        out.push_str("[Playlist]\r\n");
        for (i, p) in self.saved_playlist.iter().enumerate() {
            let _ = write!(out, "File{}={}\r\n", i, p.to_string_lossy());
        }

        let tmp_path = path.with_extension("ini.tmp");
        if fs::write(&tmp_path, out.as_bytes()).is_ok() {
            if fs::rename(&tmp_path, path).is_ok() {
                return Ok(());
            }
            let _ = fs::remove_file(&tmp_path);
        }
        fs::write(path, out)
    }

    /// Berechnet die 16-Bit CRC (`Check: Word` in `tPeakStruct` aus `player32.pas`) über den Modul-Titel.
    pub fn calc_modname_crc16(mod_title: &str) -> u16 {
        let mut crc: u16 = 0;
        for &b in mod_title.as_bytes() {
            crc ^= (b as u16) << 8;
            for _ in 0..8 {
                if (crc & 0x8000) != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    /// Erzeugt den Schlüssel `<fSize>_<CRC16>` für ein Modul.
    pub fn peak_key(file_size: u32, mod_title: &str) -> (String, u16) {
        let crc = Self::calc_modname_crc16(mod_title);
        (format!("{}_{:04X}", file_size, crc), crc)
    }

    /// Sucht einen gespeicherten PeakSearch-PreAmp-Wert (`0..40`) in `MOD4WIN.INI` oder `MOD4WIN.PKS`.
    pub fn find_peak(&self, file_size: u32, mod_title: &str, pks_path: &Path) -> Option<u8> {
        let (key, crc) = Self::peak_key(file_size, mod_title);
        if let Some(&pa) = self.peaks.get(&key) {
            return Some(pa.min(40));
        }
        // Fallback: Binäre `MOD4WIN.PKS` durchsuchen (8 Bytes pro Eintrag: fSize: u32, Check: u16, PreAmp: u16)
        if let Ok(bytes) = fs::read(pks_path) {
            for chunk in bytes.as_chunks::<8>().0 {
                let f_size = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let check = u16::from_le_bytes([chunk[4], chunk[5]]);
                let preamp = u16::from_le_bytes([chunk[6], chunk[7]]);
                if f_size == file_size && check == crc {
                    return Some((preamp as u8).min(40));
                }
            }
        }
        None
    }

    /// Speichert einen ermittelten PeakSearch-PreAmp-Wert (`0..40`) sowohl in `self.peaks` als auch in `MOD4WIN.PKS`.
    pub fn store_peak(&mut self, file_size: u32, mod_title: &str, preamp: u8, pks_path: &Path) {
        let preamp_clamped = preamp.min(40);
        let (key, crc) = Self::peak_key(file_size, mod_title);
        self.peaks.insert(key, preamp_clamped);

        // Aktualisiere oder hänge den 8-Byte `tPeakStruct`-Datensatz in `MOD4WIN.PKS` an
        let mut data = fs::read(pks_path).unwrap_or_default();
        data.truncate((data.len() / 8) * 8);
        let mut updated = false;
        for chunk in data.as_chunks_mut::<8>().0 {
            let f_size = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let check = u16::from_le_bytes([chunk[4], chunk[5]]);
            if f_size == file_size && check == crc {
                let pa_bytes = (preamp_clamped as u16).to_le_bytes();
                chunk[6] = pa_bytes[0];
                chunk[7] = pa_bytes[1];
                updated = true;
                break;
            }
        }
        if !updated {
            data.extend_from_slice(&file_size.to_le_bytes());
            data.extend_from_slice(&crc.to_le_bytes());
            data.extend_from_slice(&(preamp_clamped as u16).to_le_bytes());
        }
        let tmp_path = pks_path.with_extension("pks.tmp");
        if fs::write(&tmp_path, &data).is_ok() {
            if fs::rename(&tmp_path, pks_path).is_err() {
                let _ = fs::write(pks_path, &data);
                let _ = fs::remove_file(&tmp_path);
            }
        } else {
            let _ = fs::write(pks_path, &data);
        }
    }
}

/// Führt `PeakSearch` in einem isolierten Thread mit 8 MB Stack aus, damit `Sequencer` niemals den Aufrufer-Stack sprengt.
#[inline(never)]
pub fn run_peak_search_isolated(
    loaded_module: &crate::engine::loader::LoadedModule,
    base_settings: crate::engine::types::SoundSettings,
) -> u8 {
    let mod_clone = loaded_module.clone();
    let mut ps_settings = base_settings;
    // WICHTIG: PeakSearch muss auch bei gewähltem 8-Bit-Modus intern mit 16-Bit-Auflösung
    // (`vol_master` ohne `/ 256` und 16-Bit-Vollaussteuerung `32767`) gemessen werden,
    // damit bei 8-Bit derselbe korrekte dB-PreAmp-Wert ermittelt wird wie bei 16-Bit!
    ps_settings.bits = 16;
    ps_settings.pre_amp = 0x80 | 40;
    #[cfg(feature = "retro-win32")]
    {
        let mut ps_seq = Box::new(crate::engine::sequencer::Sequencer::new(
            mod_clone,
            ps_settings,
        ));
        return ps_seq.run_exact_peak_search();
    }
    #[cfg(not(feature = "retro-win32"))]
    {
        std::thread::Builder::new()
            .name("m4w-peak-search".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let mut ps_seq = crate::engine::sequencer::Sequencer::new(mod_clone, ps_settings);
                ps_seq.run_exact_peak_search()
            })
            .ok()
            .and_then(|h| h.join().ok())
            .unwrap_or(18)
    }
}

/// Führt den bit-exakten Direct-to-Disk WAV-Export (`DEV_HDD`) exakt wie `--diff-suite` aus:
/// 1. Übernimmt die aktuellen UI/Setup-Parameter (`OverSamp = 0, 1, 2`, `SampleRate`, `Bits`, `Stereo/Mono/Surround`, `Panning`, `PeakSearch/PreAmp`).
/// 2. Führt bei aktivem `PeakSearch` den exakten `DEV_HDD`-`run_exact_peak_search()`-Vorlauf mit dem gewählten `OverSamp` durch.
/// 3. Rendert Tick für Tick über `process_row` -> `update_channels_for_mixing` -> `process_sample_block` -> `advance_tick`.
/// 4. Schreibt den exakten 44-Byte RIFF-WAVE-Header samt PCM-Payload.
#[inline(never)]
pub fn export_module_to_wav_file(
    loaded_module: crate::engine::loader::LoadedModule,
    file_size_bytes: u32,
    ini_cfg: &mut Mod4WinIniConfig,
    wav_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    if let Some(parent) = wav_path.parent()
        && !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }

    let sr = ini_cfg.sample_rate.clamp(4000, 48000);
    let ch = ini_cfg.channels.clamp(1, 2);
    let bits = if ini_cfg.bits_per_sample == 8 { 8u8 } else { 16u8 };
    let oversamp = ini_cfg.over_sampling.min(6);

    let mut hdd_settings = crate::engine::types::SoundSettings::default();
    hdd_settings.device_id = crate::engine::types::DEV_HDD;
    hdd_settings.samplerate = sr as i32;
    hdd_settings.stereo = ch;
    hdd_settings.bits = bits;
    hdd_settings.oversamp = oversamp;
    hdd_settings.panning = ini_cfg.enable_panning;
    hdd_settings.surround = ini_cfg.surround && ch == 2;
    hdd_settings.set_clean_mode(ini_cfg.clean_mode);
    hdd_settings.set_declick(ini_cfg.declick);
    hdd_settings.set_amiga_filter(ini_cfg.amiga_filter);
    hdd_settings.set_dehiss(ini_cfg.dehiss && !loaded_module.state.mi.is_16bit_xm_or_it());
    hdd_settings.pre_amp = 0x80 + 40;

    // 1. Exakter PeakSearch-Vorlauf (wie im Original bei DEV_HDD und in --diff-suite)
    let found_pre_amp = if ini_cfg.auto_preamp_peaksearch {
        let pa = run_peak_search_isolated(&loaded_module, hdd_settings);
        let mod_name_len = loaded_module.state.mi.mod_name_len as usize;
        let mod_name =
            String::from_utf8_lossy(&loaded_module.state.mi.mod_name[..mod_name_len.min(40)])
                .trim()
                .to_string();
        let pks_path = Mod4WinIniConfig::default_pks_path();
        ini_cfg.store_peak(file_size_bytes, &mod_name, pa & 0x7F, &pks_path);
        pa
    } else {
        let clip_bit = if ini_cfg.clipping { 0x80 } else { 0x00 };
        clip_bit | ini_cfg.pre_amplification.min(40)
    };

    // 2. Exakter Tick-für-Tick Streaming-Render-Durchlauf direkt in einen `BufWriter<File>`
    //    (vermeidet OOM/190+ MB Heap-Spitzen auf 32-Bit/Retro-Systemen!)
    use std::io::{BufWriter, Seek, SeekFrom, Write as IoWrite};

    let clip_bit = if ini_cfg.clipping { 0x80 } else { 0x00 };
    hdd_settings.pre_amp = clip_bit | (found_pre_amp & 0x7F);
    let mut render_seq = crate::engine::sequencer::Sequencer::new(loaded_module, hdd_settings);
    render_seq.module.state.repeat_song = false;

    let out_file = fs::File::create(wav_path)?;
    let mut writer = BufWriter::with_capacity(64 * 1024, out_file);
    writer.write_all(&[0u8; 44])?;

    let samples_per_frame = (ch as usize).max(1);
    let bytes_per_sample = (bits / 8) as usize;
    let mut total_samples: usize = 0;
    let mut pcm16_scratch: Vec<u8> = Vec::with_capacity(8192);
    let mut tick_idx = 0usize;

    let mut write_mix_out = |mix_out: crate::engine::mixer::MixOutputBuffer,
                             writer: &mut BufWriter<fs::File>,
                             total_samples: &mut usize|
     -> std::io::Result<()> {
        match mix_out {
            crate::engine::mixer::MixOutputBuffer::Pcm16(s) if bits == 16 => {
                pcm16_scratch.clear();
                pcm16_scratch.reserve(s.len() * 2);
                for sample in s {
                    pcm16_scratch.extend_from_slice(&sample.to_le_bytes());
                }
                let next_bytes = total_samples
                    .saturating_add(pcm16_scratch.len() / 2)
                    .saturating_mul(2);
                if next_bytes > (u32::MAX as usize - 44) {
                    return Err(std::io::Error::other("WAV-Export überschreitet 4-GB-RIFF-Grenze"));
                }
                writer.write_all(&pcm16_scratch)?;
                *total_samples += pcm16_scratch.len() / 2;
            }
            crate::engine::mixer::MixOutputBuffer::Pcm8(b) if bits == 8 => {
                let next_bytes = total_samples.saturating_add(b.len());
                if next_bytes > (u32::MAX as usize - 44) {
                    return Err(std::io::Error::other("WAV-Export überschreitet 4-GB-RIFF-Grenze"));
                }
                writer.write_all(&b)?;
                *total_samples += b.len();
            }
            _ => {}
        }
        Ok(())
    };

    while !render_seq.module.state.player_repeats && tick_idx < 500_000 {
        if let Some(mix_out) = render_seq.render_it_tick(None) {
            if render_seq.module.state.player_repeats {
                break;
            }
            write_mix_out(mix_out, &mut writer, &mut total_samples)?;
            tick_idx += 1;
            continue;
        }
        if render_seq.module.state.curr_vblanc == 0 {
            render_seq.process_row();
            if render_seq.module.state.player_repeats {
                break;
            }
        }
        render_seq.update_channels_for_mixing();
        if render_seq.module.state.player_repeats {
            break;
        }
        render_seq.module.state.buf_samples = render_seq.module.state.num_samples.max(1);
        let am_repeating =
            render_seq.module.state.player_repeats && !render_seq.module.state.repeat_song;

        let mix_out = crate::engine::mixer::process_sample_block(
            &mut render_seq.module.state,
            &mut render_seq.mix_mem,
            &render_seq.module.sample_mem,
            am_repeating,
        );
        write_mix_out(mix_out, &mut writer, &mut total_samples)?;
        render_seq.advance_tick();
        tick_idx += 1;
    }

    let total_frames = total_samples / samples_per_frame;
    let data_bytes = (total_samples * bytes_per_sample) as u32;
    let pad_byte = if (data_bytes & 1) != 0 {
        writer.write_all(&[0u8])?;
        1u32
    } else {
        0u32
    };
    writer.flush()?;

    let byte_rate = sr * (ch as u32) * (bytes_per_sample as u32);
    let block_align = (ch as u16) * (bytes_per_sample as u16);
    let riff_size = 36u32.saturating_add(data_bytes).saturating_add(pad_byte);

    let mut hdr = [0u8; 44];
    hdr[0..4].copy_from_slice(b"RIFF");
    hdr[4..8].copy_from_slice(&riff_size.to_le_bytes());
    hdr[8..16].copy_from_slice(b"WAVEfmt ");
    hdr[16..20].copy_from_slice(&16u32.to_le_bytes());
    hdr[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    hdr[22..24].copy_from_slice(&(ch as u16).to_le_bytes());
    hdr[24..28].copy_from_slice(&sr.to_le_bytes());
    hdr[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    hdr[32..34].copy_from_slice(&block_align.to_le_bytes());
    hdr[34..36].copy_from_slice(&(bits as u16).to_le_bytes());
    hdr[36..40].copy_from_slice(b"data");
    hdr[40..44].copy_from_slice(&data_bytes.to_le_bytes());

    let mut file = writer.into_inner()?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&hdr)?;
    file.flush()?;

    Ok(total_frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ini_roundtrip_and_peaks() {
        let tmp_dir = std::env::temp_dir();
        let ini_path = tmp_dir.join("mod4win_test_paket3.ini");
        let pks_path = tmp_dir.join("mod4win_test_paket3.pks");
        let _ = fs::remove_file(&ini_path);
        let _ = fs::remove_file(&pks_path);

        let mut cfg = Mod4WinIniConfig::default();
        cfg.device_id = DEV_HDD;
        cfg.device_name = "Direct To Disk Recording".to_string();
        cfg.sample_rate = 48000;
        cfg.bits_per_sample = 8;
        cfg.channels = 1;
        cfg.surround = false;
        cfg.over_sampling = 1;
        cfg.clean_mode = true;
        cfg.saved_playlist = vec![PathBuf::from("C:\\MODS\\PAHGIANA.S3M")];
        cfg.saved_playlist_idx = 0;
        cfg.saved_order_pos = 3;
        cfg.modern_playlist_rows = 18;
        cfg.saved_open_windows = vec!["EFFECTS".to_string(), "QUICK_SELECT".to_string()];
        cfg.store_peak(123456, "Pahgiana", 27, &pks_path);
        cfg.save_to_file(&ini_path).unwrap();

        let loaded = Mod4WinIniConfig::load_from_file(&ini_path);
        assert_eq!(loaded.device_id, DEV_HDD);
        assert_eq!(loaded.sample_rate, 48000);
        assert_eq!(loaded.bits_per_sample, 8);
        assert_eq!(loaded.channels, 1);
        assert!(!loaded.surround);
        assert_eq!(loaded.over_sampling, 1);
        assert!(loaded.clean_mode);
        assert_eq!(loaded.saved_playlist.len(), 1);
        assert_eq!(loaded.saved_order_pos, 3);
        assert_eq!(loaded.modern_playlist_rows, 18);
        assert_eq!(
            loaded.saved_open_windows,
            vec!["EFFECTS".to_string(), "QUICK_SELECT".to_string()]
        );
        assert_eq!(loaded.find_peak(123456, "Pahgiana", &pks_path), Some(27));

        let _ = fs::remove_file(&ini_path);
        let _ = fs::remove_file(&pks_path);
    }

    #[test]
    fn test_peak_search_8bit_equals_16bit() {
        let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Workspace root");
        let xm_path = root_dir.join("PAHGIANA.XM");
        let Ok(xm_bytes) = fs::read(&xm_path) else {
            return;
        };
        let loaded_mod =
            crate::engine::loader::load_module_with_ext(&xm_bytes, Some("xm")).expect("XM laden");

        let mut s16 = crate::engine::types::SoundSettings::default();
        s16.samplerate = 44100;
        s16.stereo = 2;
        s16.bits = 16;
        s16.oversamp = 0;
        s16.panning = true;

        let mut s8 = s16;
        s8.bits = 8;

        let pa16 = run_peak_search_isolated(&loaded_mod, s16) & 0x7F;
        let pa8 = run_peak_search_isolated(&loaded_mod, s8) & 0x7F;
        assert_eq!(
            pa8, pa16,
            "8-Bit PeakSearch muss exakt denselben dB-PreAmp-Wert ermitteln wie 16-Bit (statt +20 dB = 40)"
        );
        assert!(pa8 < 40, "PAHGIANA.XM darf bei 8-Bit nicht am +20 dB Anschlag (40) landen");
    }

    #[test]
    fn test_gui_wav_export_matches_reference_wavs_bit_exact() {
        let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Workspace root");
        let xm_path = root_dir.join("PAHGIANA.XM");
        let Ok(xm_bytes) = fs::read(&xm_path) else {
            return;
        };
        let loaded_mod =
            crate::engine::loader::load_module_with_ext(&xm_bytes, Some("xm")).expect("XM laden");
        let file_size = xm_bytes.len() as u32;

        let cases = [
            (
                0u8,
                "diagnose/PAHGIANA_44_16_PAN_IDO-OFF.WAV",
                "diagnose/TEST_GUI_EXPORT_IDO-OFF.WAV",
            ),
            (
                1u8,
                "diagnose/PAHGIANA_44_16_PAN_IDO-LIN.WAV",
                "diagnose/TEST_GUI_EXPORT_IDO-LIN.WAV",
            ),
            (
                2u8,
                "diagnose/PAHGIANA_44_16_PAN_IDO-CUB.WAV",
                "diagnose/TEST_GUI_EXPORT_IDO-CUB.WAV",
            ),
        ];

        for (oversamp, ref_rel, out_rel) in cases {
            let mut cfg = Mod4WinIniConfig::default();
            cfg.device_id = DEV_HDD;
            cfg.sample_rate = 44100;
            cfg.bits_per_sample = 16;
            cfg.channels = 2;
            cfg.surround = false;
            cfg.enable_panning = true;
            cfg.clipping = true;
            cfg.auto_preamp_peaksearch = true;
            cfg.over_sampling = oversamp;

            let ref_path = root_dir.join(ref_rel);
            if !ref_path.exists() {
                continue;
            }
            let out_path = root_dir.join(out_rel);
            let frames = export_module_to_wav_file(
                loaded_mod.clone(),
                file_size,
                &mut cfg,
                &out_path,
            )
            .expect("WAV export");
            assert_eq!(frames, 14_717_120);

            let ref_bytes = fs::read(&ref_path).expect("Referenz-WAV lesen");
            let out_bytes = fs::read(&out_path).expect("Export-WAV lesen");
            assert_eq!(
                out_bytes.len(),
                ref_bytes.len(),
                "Dateigröße weicht für OverSamp={} ab",
                oversamp
            );
            assert!(
                out_bytes == ref_bytes,
                "WAV-Bytes weichen für OverSamp={} von {:?} ab",
                oversamp,
                ref_path
            );
        }
    }
}

