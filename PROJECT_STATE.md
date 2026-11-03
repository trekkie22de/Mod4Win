# MOD4WIN [3.0 Beta] – Project State

**100 % bit-exakter Rust-Port der historischen Tracker-Audio-Engine & Multi-Window-Oberfläche (MOD4WIN 2.30 / 2.40 Last Beta)**

---

## 1. Architektur-Überblick

Das Projekt portiert den klassischen Windows-3.1/95-Tracker-Player **MOD4WIN** (Original: Kay Bruns, 1994–1996, Borland Pascal / 16-Bit & 32-Bit x86-Assembler in `PLAYER32.DLL`) vollständig nach sicherem, plattformübergreifendem **Rust** (`mod4win-rs`).

```
Mod4Win/
├── help/                       # 7-sprachige HTML-3.2-Hilfe (DE, EN, FR, ES, IT, NL, GRK) + Hotspot-Grafiken
├── legacy_src/                 # Historische Original-Quellen (Pascal/ASM, nur Referenz)
├── mod4win-rs/                 # Haupt-Rust-Crate
│   ├── assets/                 # Eingebettete 4bpp-Original-Bitmaps,MOD4WIN.ico & hlp_extracted/ (bm0..bm15)
│   ├── help/                   # Synchronisierte HTML-Hilfedateien für build.rs (RFC-1951 Deflate)
│   ├── lang/                   # Lokalisierungskataloge (de.ini, en.ini)
│   ├── scripts/                # ALSA-Stub & Cross-Compile-Hilfen
│   ├── src/
│   │   ├── engine/             # Bit-exakte DSP-, Sequencer- & Format-Engine
│   │   │   ├── formats/        # 13 Modul-Loader (MOD, S3M, XM, IT/UMX, MTM, 669, STM, FAR, ULT, DMF, OKT, WOW, NST)
│   │   │   ├── it_engine.rs    # Impulse Tracker 2.14p5 Engine (NNA/DCT/DCA, Pitch/Pan/Filter-Envelopes, Resonanzfilter)
│   │   │   ├── mixer.rs        # Integer-/x87-FPU-Mixer (IDO 0..6, Surround, Panning, PreAmp, Clipping-LUT & CL+DC De-Click)
│   │   │   ├── sequencer.rs    # Tick-/Row-/Pattern-Sequencer, Loop-Erkennung, GetTotalTime & PeakSearch
│   │   │   ├── tables.rs       # Originale Sinus-, Vibrato-, Amiga-Perioden- & Cubic-Spline-Tabellen
│   │   │   └── types.rs        # Kernstrukturen (ModuleState, TrackState, InstrumentHeader, SoundSettings)
│   │   ├── ui/                 # Multi-Window-GUI (winit + pixels / GDI-Shim für Retro-Win32s)
│   │   │   ├── app.rs          # Fensterverwaltung, Event-Loop, Drag & Drop, Magnetisches Window-Snapping
│   │   │   ├── main_win.rs     # Hauptfenster, Mini-Status & About-Dialog (Classic 2.40)
│   │   │   ├── modern_skin.rs  # Modern Skin (MOD4WIN 3.0 Beta Hi-Fi-Rack-Design & nativer rfd-Dialog)
│   │   │   ├── open_dlg.rs     # Dual-Pane Commander Open-Dialog, Quick-Selection, .ZIP & .MOL-Support
│   │   │   ├── setup_dlg.rs    # Setup-Dialog, Direct-to-Disk WAV-Prompt & Verzeichnis-Browser
│   │   │   ├── info_win.rs     # Module-Info (bm10) & Instrument-Info (bm11)
│   │   │   ├── effects_win.rs  # 32-Kanal Echtzeit-Effekt- & Pegel-Matrix (bm12)
│   │   │   ├── file_mgr.rs     # Datei-Manager (F5 Copy / F6 Move) & Archivierer-Setup (ARJ/ZIP/LHA)
│   │   │   ├── hotkeys_dlg.rs  # Frei belegbarer Hotkey-Editor (43 Aktionen)
│   │   │   ├── winhelp.rs      # Integrierter WinHelp-Viewer mit klickbaren bm0..bm15-Hotspots
│   │   │   ├── live_audio.rs   # Echtzeit-Audio-Streaming (cpal), PeakSearch-Worker & WAV-Exporter
│   │   │   ├── cpu_sim.rs      # Historische CPU- & DMA-Puffer-Simulation (386SX-16 bis K6-2 450)
│   │   │   ├── ini_config.rs   # MOD4WIN.INI & binäre MOD4WIN.PKS Peak-Datenbank
│   │   │   └── i18n.rs         # Mehrsprachigkeit (Deutsch / Englisch)
│   │   ├── win32s_shim.rs      # Reiner Win32-GDI/WaveOut-Backend-Pfad für Windows 3.1x (Win32s), 95 & NT 3.51
│   │   └── main.rs             # CLI-Einstiegspunkt, Headless WAV-Renderer & automatische Diff-Suite
│   ├── build.rs                # Deflate-Kompression der 7 Hilfe-HTMLs & Windows-Icon-Ressource
│   └── Cargo.toml
└── build-universal.ps1         # Universal-Build-Skript
```

---

## 2. Kernfunktionen & Status

### Audio- & DSP-Engine (`src/engine/`)
* **13 unterstützte Formate:** `.MOD` (4–32 Chn), `.NST`, `.WOW`, `.OCT`/`.OKT`, `.S3M`, `.STM`, `.669`, `.FAR`, `.MTM`, `.ULT` (v1–v4), `.XM`, `.DMF` (X-Tracker v5–v8) sowie `.IT` (Impulse Tracker 1.00–2.14p5) und `.UMX` (Unreal Music Package).
* **Oversampling / Interpolation (`IDO 0..6`):**
  * `0`: Off (Zero-Order Hold)
  * `1`: Linear (`fOverSamp`)
  * `2`: CPU Cubic (`fCPUCubic`, 16-Bit Integer-Spline-LUT)
  * `3`: FPU Cubic (`fFPUCubic`, exakte 80-Bit x87 Extended-Precision-Emulation)
  * `4..6`: IT2 Resonanzfilter (127-Tap Sinc / Linear / Cubic für `.IT`/`.UMX`)
* **Engine-Modi:** Umschaltbar zwischen **Full Emulation** (100 % bit-identisch zu `PLAYER32.DLL` inkl. historischer Eigenheiten) und **Clean Mode** (artefaktfreie Wiedergabe).
* **3-stufiger Clip / CL+DC (De-Click)-Filter (`F9`):** Umschaltbar zwischen `Clip Off`, `Clip` und `CL+DC` (kanalweises 1,5-ms-Transienten-Crossfade bei Lautstärke-/Notensprüngen + automatische Sample-Tail-Entknacksung).
* **Auto-PreAmp PeakSearch:** Asynchroner Vorab-Durchlauf im Hintergrund mit Fortschrittsanzeige (`PEAK SEARCH ... XX%`) und persistentem Cache in `MOD4WIN.PKS`.

### Benutzeroberfläche & Skins (`src/ui/`)
* **Zwei finale Skins (per Setup, `F11` oder `--skin <2.40|modern>` live umschaltbar):**
  1. **2.40 Classic:** Originalgetreues Layout und Pixel-Rendering der MOD4WIN 2.40 Last Beta mit allen 15 klassischen Dialogfenstern.
  2. **Modern:** Modernes Hi-Fi-Rack-Interface mit hochauflösenden Segment- und Dot-Matrix-Anzeigen, integriertem Playlist-Dock und nativem Betriebssystem-Dateidialog (`rfd`) für unbegrenzte Mehrfachauswahl.
* **Integriertes Hilfesystem (`F1`):** 7-sprachige HTML-Hilfe (`DE`, `EN`, `FR`, `ES`, `IT`, `NL`, `GRK`) direkt im eingebetteten Viewer (inkl. aller 266 interaktiven `bm0`..`bm15`-Bild-Hotspots) oder per Knopfdruck im System-Webbrowser.

---

## 3. Build & Tests

Alle Befehle werden im Verzeichnis `mod4win-rs/` ausgeführt:

```powershell
# Compiler-Check & vollständige Unit-Test-Suite
cargo check
cargo test

# Standard-Release-Build (Windows / Linux 64-Bit)
cargo build --release

# Headless Direct-to-Disk WAV-Export
cargo run --release -- --render-wav input.mod output.wav --ido 3 --rate 44100 --bits 16 --channels stereo

# Retro-Win32s-Build (Windows 3.1x / Windows 95 / Windows NT 3.51, i586)
.\build_retro_win32s.ps1
```

