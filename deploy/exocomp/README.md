# Exocomp – 16-Bit MOD4WIN Regression-Test-Worker (`root@192.168.10.64`)

Hochdurchsatz-Regression-Test-Umgebung auf **Exocomp** (Rocky Linux 10.1, Kernel `6.12`, 16 vCPUs, 15 GiB RAM, 3,6 TB NAS unter `/mnt/nas`), um das originale **16-Bit `MOD4WIN.EXE` v2.40 Beta** (`MOD4WIN_IDO0..3.EXE` + DPMI `PLAYER32.DLL`) im großen Maßstab (z. B. komplette Modland-/Aminet-Sammlungen) Sample-exakt (`0 LSB`) gegen den 64-Bit-Rust-Port zu prüfen.

## Architektur & Ressourcen-Koexistenz

1. **Sauberer Host durch Rootful Podman-Container (`localhost/mod4win-reg-worker:latest`):**
   - Da RHEL/Rocky Linux 10 keine `.i686`-Multilib-Pakete mehr mitliefert, laufen `wine32` (`winevdm.exe.so`), ein einzelner Headless-`Xvfb :99`-X-Server, `ffmpeg` (für `.IT`/`.UMX`) und das statisch gegen glibc 2.28 gebaute `mod4win-diff` gemeinsam in **einem** langlebigen Debian-Bookworm-Container.
   - Kein Container-Start-Overhead pro Datei: `mod4win-diff` spawnt alle 16 Worker direkt innerhalb des Containers (`fork+exec < 2 ms`).
2. **Lockfreie Multi-Worker-Skalierung unter Wine (Standard: `48` Worker auf 16 Kernen):**
   - Da 16-Bit `MOD4WIN.EXE` beim `Direct To Disk Recording` kooperativ über Win16-Timer (`WM_TIMER`) taktet (~8 % Last pro Worker auf einem Zen-4-Kern), skaliert eine 3-fache Überbuchung (**48 Worker** bei ~5,8 GB RAM) mehr als doppelt so schnell wie 16 Worker.
   - Jeder Worker `W0..W47` erhält in `/dev/shm/m4w_work` (4 GiB RAM-Disk) ein eigenes, per Hardlink/Symlink geklontes `WINEPREFIX` (`pfx_W<id>`), einen eigenen persistenten `wineserver -p`, eigene 8.3-DOS-Laufwerke (`M:\` & `R:\`) sowie eine private `MOD4WIN.INI` und `MOD4WIN.PKS` (mit deaktiviertem `StateSaver=0` und unterdrücktem `winedbg`-Crash-Dialog).
   - Dadurch entfällt jegliche `Win16Mutex`-, Registry- oder `MOD4WIN.PKS`-Serialisierung; `Xvfb` läuft mit `-maxclients 2048` und Podmans `--init` (`catatonit`) erntet verwaiste Wine-Subprozesse laufend ab.
   - Der Linux-CPU-Watchdog aggregiert die CPU-Ticks über Prozessgruppe **und** Worker-Tag (`M4W_WTAG=W<id>`), sodass auch vom `wineserver` entkoppelte `winevdm.exe`-Prozesse während langer PeakSearch-Läufe exakt erfasst werden.
3. **Nahtlose Koexistenz mit `vt-worker-daemon.service` (Video-Transcoding):**
   - `vt-worker-daemon.service` läuft dauerhaft mit `Nice=10`, `CPUWeight=25` und `IOSchedulingClass=best-effort` (`Priority=6`).
   - Der MOD4WIN-Batch-Container startet mit normaler Gewichtung (`--cpu-shares=1024` / `CPUWeight=100`). Der Linux cgroup-v2 EEVDF-Scheduler teilt dem Regression-Test während eines Laufs automatisch ~80 % der CPU-Zeit zu und gibt in der Millisekunde nach Ende des Laufs wieder 100 % aller 16 Kerne an `HandBrakeCLI` zurück – ohne Pause-Signale, ohne Service-Restarts und ohne abgebrochene Encodes.

## Erforderliche 16-Bit INI-Einstellungen (Referenz)

Auf Linux/Wine erzeugt `mod4win-diff --batch-diff` diese Konfiguration für jeden Worker (`m4w_W<id>/MOD4WIN.INI`, `drive_c/windows/MOD4WIN.INI` und `drive_c/windows/win.ini`) vollautomatisch. Bei manuellen Einzel-Referenzläufen (z. B. unter Windows/`otvdm`) müssen folgende Werte gesetzt sein, damit keine modalen Dialoge blockieren:

```ini
; --- MOD4WIN.INI ---
[State]
PlayOnLoad=1
StateSaver=0
AskHD=0
HDPath=R:\
AskExtensions=0
ForceExtensions=0
LOG_ERRORS=0

[Playing Device]
DeviceID=-2
DeviceName=Direct To Disk Recording

[Sampling]
SampleRate=44100
BitsPerSample=16
Channels=2
Surround=0
PreAmplification=30
OverSampling=3
EnablePanning=1
Clipping=1

; --- WIN.INI ---
[Player32]
UseHardware=0
PALTiming=0
PeakSearch=1
ShowPeakSearch=0
ExactPeakSearch=1
```

## Verzeichnislayout auf Exocomp

- `/opt/Mod4Win-Worker/bin/mod4win-diff` – 64-Bit Rust Batch-Diff-Engine (`--features diff-suite`)
- `/opt/Mod4Win-Worker/bin/Invoke-Mod4WinBatch.ps1` (Symlink `/usr/local/bin/m4w-batch`) – CLI-Steuerung
- `/opt/Mod4Win-Worker/orig_bin/` – Originale 16-Bit `MOD4WIN.EXE`, `MOD4WIN_IDO0..3.EXE`, `PLAYER32.DLL`, `MOD4WIN.INI`
- `/mnt/mods/` – Read-Only NFSv4.2-Mount von `voyager.berilia.net:/samba/Audio/MOD_MID_SID/MOD` (inkl. `Modland/`, `MOD`–`Mod6`, `XM`, `S3M`, `669`, `Stm`)
- `/mnt/nas/mod4win-regression/results/<lauf>/` – Ergebnisse (`pass/`, `fail/`, `timeout/`, `logs/_ALL_MISMATCHES.log`, `wav/`, `mods/`)

## Bedienung

### 1. Worker von Daystrom aus installieren oder aktualisieren

```powershell
pwsh -NoProfile -File /mnt/targets/Data/Mod4Win/deploy/exocomp/Install-ExocompWorker.ps1
```

### 2. Batch-Lauf starten (direkt auf Exocomp oder per SSH)

Synchron mit Live-Ausgabe:

```powershell
ssh root@192.168.10.64 "m4w-batch /mnt/mods/MOD"
```

Als abgekoppelter Hintergrund-Job (`-Detach`) für den kompletten Voyager-Korpus (~11.800 Dateien inkl. `Modland`):

```powershell
ssh root@192.168.10.64 "m4w-batch /mnt/mods -ResultPath /mnt/nas/mod4win-regression/results/voyager_full -Detach"
```

> **Hinweis zu Dateinamen:** Sowohl PC-Endungen (`.mod`, `.nst`, `.xm`, `.s3m`, `.mtm`, `.669`, `.ult`, `.far`, `.stm`, `.okt`, `.okta`, `.wow`, `.it`, `.umx`) als auch Amiga-/Modland-Präfixnamen (`mod.<titel>`, `okt.<titel>`, …) werden automatisch erkannt. Bereits geprüfte Module in `pass/`, `fail/` oder `timeout/` werden bei Fortsetzungsläufen automatisch übersprungen (`SKIP`), sofern nicht `-RetryFails` oder `-RetryTimeouts` angegeben wird.

### 3. Status prüfen / Logs verfolgen / Stoppen

```powershell
# Zählerstand (PASS / FAIL / TIMEOUT) + letzte Logzeilen anzeigen
ssh root@192.168.10.64 "m4w-batch -Status"

# Live-Stream eines laufenden Hintergrund-Batches
ssh root@192.168.10.64 "podman logs -f m4w-reg-batch"

# Geordneten Abbruch auslösen (bereits geschriebene Ergebnisse bleiben erhalten)
ssh root@192.168.10.64 "m4w-batch -Stop"
```
