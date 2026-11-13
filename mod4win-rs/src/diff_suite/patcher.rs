//! Automatischer Binär-Patcher für die originalen 16-Bit-Referenzdateien
//! `MOD4WIN.EXE` (`MOD4WIN_IDO0..3.EXE`) und `PLAYER32.DLL`.
//!
//! Erzeugt aus einer originalen `MOD4WIN.EXE` (677.888 Bytes) und `PLAYER32.DLL` (134.144 Bytes)
//! vollautomatisch die vier Headless-Test-Binaries `MOD4WIN_IDO0.EXE` bis `MOD4WIN_IDO3.EXE`
//! sowie die gepatchte `PLAYER32.DLL` (inklusive `.bak`-Sicherheitskopien und passender `MOD4WIN.INI`).

use std::fs;
use std::path::Path;

/// Erwartete Dateigröße der originalen 16-Bit `MOD4WIN.EXE` (v2.40 Beta).
pub const MOD4WIN_EXE_SIZE: usize = 677_888;

/// Erwartete Dateigröße der originalen 16-Bit DPMI `PLAYER32.DLL` (v2.40 Beta).
pub const PLAYER32_DLL_SIZE: usize = 134_144;

/// Patcht die Rohbytes der `MOD4WIN.EXE` (677.888 Bytes) für den angegebenen `ido`-Modus (`0..=3`).
///
/// Akzeptiert sowohl eine völlig ungepatchte `MOD4WIN.EXE` (`MOD4WIN.EXE.bak`) als auch eine
/// bereits vorab gepatchte Variante.
pub fn patch_mod4win_exe_bytes(base: &[u8], ido: u8) -> Result<Vec<u8>, String> {
    if base.len() != MOD4WIN_EXE_SIZE {
        return Err(format!(
            "Ungültige Dateigröße für MOD4WIN.EXE: {} Bytes (erwartet: {} Bytes)",
            base.len(),
            MOD4WIN_EXE_SIZE
        ));
    }
    if &base[0..2] != b"MZ" {
        return Err("Ungültiger MZ-Header in MOD4WIN.EXE".to_string());
    }
    let ido_clamped = ido.min(3);
    let mut out = base.to_vec();

    // 1. Gemeinsame Headless-/Auto-Exit-Patches
    out[0x0029A9] = 0x4B;
    out[0x002B11] = 0xC6;
    out[0x002B14] = ido_clamped;
    out[0x002B15..=0x002B20].fill(0x90);
    out[0x002CEC..=0x002CED].copy_from_slice(&[0xEB, 0x2E]);

    out[0x006111..=0x00611F].copy_from_slice(&[
        0x26,
        0xC6,
        0x45,
        0x36,
        ido_clamped,
        0xFF,
        0x76,
        0x0A,
        0x31,
        0xC0,
        0x26,
        0x80,
        0x7D,
        0xEC,
        0x02,
    ]);

    out[0x0077A7..=0x0077A8].copy_from_slice(&[0x90, 0x90]);
    out[0x0077B4] = 0x64;
    out[0x00781A..=0x007823].copy_from_slice(&[
        0xC4, 0x7E, 0x06, 0x26, 0xFF, 0xB5, 0x73, 0x17, 0x6A, 0x10,
    ]);
    out[0x00782F..=0x007830].copy_from_slice(&[0xEB, 0x7C]);
    out[0x011E89] = 0x6E;
    out[0x04DCAA..=0x04DCAD].copy_from_slice(&[0xCB, 0x90, 0x90, 0x90]);

    // 2. IDO-spezifische Checksummen-/Konfigurations-Offsets
    out[0x059EC0] = 0x2F + ido_clamped * 4;
    out[0x059EC1] = 0x97;
    out[0x05A02E] = ido_clamped;
    out[0x05A06A] = ido_clamped;

    Ok(out)
}

/// Patcht die Rohbytes der `PLAYER32.DLL` (134.144 Bytes), sodass das in der EXE gesetzte
/// `OverSampling` (`IDO 0..3`) beim Initialisieren nicht mit `3` überschrieben wird.
pub fn patch_player32_dll_bytes(base: &[u8]) -> Result<Vec<u8>, String> {
    if base.len() != PLAYER32_DLL_SIZE {
        return Err(format!(
            "Ungültige Dateigröße für PLAYER32.DLL: {} Bytes (erwartet: {} Bytes)",
            base.len(),
            PLAYER32_DLL_SIZE
        ));
    }
    if &base[0..2] != b"MZ" {
        return Err("Ungültiger MZ-Header in PLAYER32.DLL".to_string());
    }
    let orig_slice = &base[0x00815B..=0x00815F];
    if orig_slice != [0xC6, 0x06, 0x94, 0x69, 0x03] && orig_slice != [0x90, 0x90, 0x90, 0x90, 0x90]
    {
        return Err(format!(
            "Unerwartete Bytes in PLAYER32.DLL bei 0x00815B: {:02X?}",
            orig_slice
        ));
    }
    let mut out = base.to_vec();
    out[0x00815B..=0x00815F].fill(0x90);
    Ok(out)
}

/// Erzeugt eine garantiert Headless-taugliche `MOD4WIN.INI` (CRLF) für Batch-Regressionstests.
///
/// Verhindert zuverlässig modale Blocker-Dialoge (`AskHD=0`, `AskExtensions=0`, `ForceExtensions=0`),
/// aktiviert den Sofortstart (`PlayOnLoad=1`) sowie `Direct To Disk Recording` (`DeviceID=-2`)
/// und setzt `HDPath` auf das jeweilige RAM-/Temp-Arbeitsverzeichnis (z. B. `R:\`).
pub fn build_headless_mod4win_ini(hd_path_dos: &str) -> String {
    let mut hd = hd_path_dos.trim().replace('/', "\\");
    if !hd.ends_with('\\') {
        hd.push('\\');
    }
    format!(
        "[State]\r\n\
OverrideRateChange=0\r\n\
PauseWhileSetup=0\r\n\
PlayOnLoad=1\r\n\
StateSaver=0\r\n\
AskHD=0\r\n\
HDPath={hd}\r\n\
AskExtensions=0\r\n\
ForceExtensions=0\r\n\
LOG_ERRORS=0\r\n\
\r\n\
[Playing Device]\r\n\
DeviceID=-2\r\n\
DeviceName=Direct To Disk Recording\r\n\
\r\n\
[Sampling]\r\n\
SampleRate=44100\r\n\
BitsPerSample=16\r\n\
Channels=2\r\n\
OutputBuffers=99\r\n\
TimerMode=0\r\n\
Surround=0\r\n\
PreAmplification=30\r\n\
OverSampling=3\r\n\
EnablePanning=1\r\n\
Clipping=1\r\n\
HDDRecBuffers=16\r\n\
\r\n\
[Language]\r\n\
Help=M4W_GER.HLP\r\n\
Resource=RES_GER.DLL\r\n\
Language=1\r\n"
    )
}

/// Prüft im angegebenen `m4w_dir`, ob `MOD4WIN_IDO0..3.EXE` und die gepatchte `PLAYER32.DLL`
/// vorhanden und auf dem korrekten Patch-Stand sind. Falls nicht, werden automatisch `.bak`-Backups
/// angelegt und die vier `MOD4WIN_IDO0..3.EXE` sowie `PLAYER32.DLL` gepatcht.
pub fn ensure_mod4win_test_environment(m4w_dir: &Path) -> Result<(), String> {
    if !m4w_dir.exists() {
        return Err(format!(
            "Das angegebene MOD4WIN-Verzeichnis '{}' existiert nicht.",
            m4w_dir.display()
        ));
    }

    // 1. PLAYER32.DLL prüfen / patchen
    let dll_path = m4w_dir.join("PLAYER32.DLL");
    let dll_bak_path = m4w_dir.join("PLAYER32.DLL.bak");
    let base_dll_bytes = if dll_bak_path.exists() {
        fs::read(&dll_bak_path)
            .map_err(|e| format!("Konnte '{}' nicht lesen: {}", dll_bak_path.display(), e))?
    } else if dll_path.exists() {
        let raw = fs::read(&dll_path)
            .map_err(|e| format!("Konnte '{}' nicht lesen: {}", dll_path.display(), e))?;
        // Falls noch ungepatcht, als .bak sichern
        if raw.len() == PLAYER32_DLL_SIZE
            && raw[0x00815B..=0x00815F] == [0xC6, 0x06, 0x94, 0x69, 0x03]
        {
            let _ = fs::write(&dll_bak_path, &raw);
        }
        raw
    } else {
        return Err(format!(
            "Weder PLAYER32.DLL noch PLAYER32.DLL.bak in '{}' gefunden.",
            m4w_dir.display()
        ));
    };

    let expected_dll = patch_player32_dll_bytes(&base_dll_bytes)?;
    let dll_needs_write = match fs::read(&dll_path) {
        Ok(cur) => cur != expected_dll,
        Err(_) => true,
    };
    if dll_needs_write {
        fs::write(&dll_path, &expected_dll)
            .map_err(|e| format!("Konnte gepatchte '{}' nicht schreiben: {}", dll_path.display(), e))?;
        println!(
            "[Auto-Patcher] PLAYER32.DLL in '{}' erfolgreich gepatcht.",
            m4w_dir.display()
        );
    }

    // 2. Basis-MOD4WIN.EXE ermitteln und ggf. .bak sichern
    let exe_path = m4w_dir.join("MOD4WIN.EXE");
    let exe_bak_path = m4w_dir.join("MOD4WIN.EXE.bak");
    let base_exe_bytes = if exe_bak_path.exists() {
        fs::read(&exe_bak_path)
            .map_err(|e| format!("Konnte '{}' nicht lesen: {}", exe_bak_path.display(), e))?
    } else if exe_path.exists() {
        let raw = fs::read(&exe_path)
            .map_err(|e| format!("Konnte '{}' nicht lesen: {}", exe_path.display(), e))?;
        if raw.len() == MOD4WIN_EXE_SIZE && raw[0x0029A9] == 0x9B {
            let _ = fs::write(&exe_bak_path, &raw);
        }
        raw
    } else {
        // Fallback: Prüfe, ob bereits eine der MOD4WIN_IDO0..3.EXE existiert
        let mut found = None;
        for ido in 0..=3 {
            let candidate = m4w_dir.join(format!("MOD4WIN_IDO{}.EXE", ido));
            if let Ok(raw) = fs::read(&candidate) {
                if raw.len() == MOD4WIN_EXE_SIZE {
                    found = Some(raw);
                    break;
                }
            }
        }
        found.ok_or_else(|| {
            format!(
                "Keine MOD4WIN.EXE oder MOD4WIN.EXE.bak in '{}' gefunden.",
                m4w_dir.display()
            )
        })?
    };

    // 3. Alle vier MOD4WIN_IDO0..3.EXE prüfen / erzeugen
    for ido in 0..=3u8 {
        let target_exe = m4w_dir.join(format!("MOD4WIN_IDO{}.EXE", ido));
        let expected_exe = patch_mod4win_exe_bytes(&base_exe_bytes, ido)?;
        let needs_write = match fs::read(&target_exe) {
            Ok(cur) => cur != expected_exe,
            Err(_) => true,
        };
        if needs_write {
            fs::write(&target_exe, &expected_exe).map_err(|e| {
                format!(
                    "Konnte '{}' nicht schreiben: {}",
                    target_exe.display(),
                    e
                )
            })?;
            println!(
                "[Auto-Patcher] {} erfolgreich erzeugt.",
                target_exe.display()
            );
        }
    }

    // 4. Prüfe, ob eine passende MOD4WIN.INI für Direct-To-Disk existiert
    let ini_path = m4w_dir.join("MOD4WIN.INI");
    if !ini_path.exists() {
        let _ = fs::write(&ini_path, build_headless_mod4win_ini("R:\\"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_patcher_matches_orig_bin_bit_exact() {
        let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Workspace root");
        let orig_bin = root_dir.join("orig_bin");
        let exe_bak = orig_bin.join("MOD4WIN.EXE.bak");
        let dll_bak = orig_bin.join("PLAYER32.DLL.bak");
        if !exe_bak.exists() || !dll_bak.exists() {
            return;
        }

        let base_exe = fs::read(&exe_bak).expect("MOD4WIN.EXE.bak lesen");
        for ido in 0..=3u8 {
            let patched = patch_mod4win_exe_bytes(&base_exe, ido).expect("MOD4WIN patchen");
            let ref_exe_path = orig_bin.join(format!("MOD4WIN_IDO{}.EXE", ido));
            if ref_exe_path.exists() {
                let ref_bytes = fs::read(&ref_exe_path).expect("Referenz-EXE lesen");
                assert_eq!(
                    patched, ref_bytes,
                    "Gepatchte MOD4WIN_IDO{}.EXE muss Byte-für-Byte mit orig_bin übereinstimmen",
                    ido
                );
            }
        }

        let base_dll = fs::read(&dll_bak).expect("PLAYER32.DLL.bak lesen");
        let patched_dll = patch_player32_dll_bytes(&base_dll).expect("PLAYER32.DLL patchen");
        let ref_dll_path = orig_bin.join("PLAYER32.DLL");
        if ref_dll_path.exists() {
            let ref_dll = fs::read(&ref_dll_path).expect("Referenz-DLL lesen");
            assert_eq!(
                patched_dll, ref_dll,
                "Gepatchte PLAYER32.DLL muss Byte-für-Byte mit orig_bin übereinstimmen"
            );
        }
    }
}

