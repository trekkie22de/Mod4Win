//! Unterstützung für originale MOD4WIN `.MOL`-Abspiellisten (`newopen.pas`)
//! sowie direktes Browsen und Entpacken von `.ZIP`-Archiven ohne externe Crate-Abhängigkeiten.

use std::fs;
use std::path::{Path, PathBuf};

use super::app::UiSharedState;

/// Konstanten aus `newopen.pas` für das `.MOL`-Dateiformat
const INDEX_OFFSET: u8 = 31;
const NO_ARC_CONST: u8 = 255;

/// Originale Dateityp-Tabelle `filetypesasstring[1..20]` aus `newopen.pas` (Zeile 64).
pub const FILETYPES_AS_STRING: [&str; 20] = [
    "arj", "zip", "lzh", "mol", "mod", "nst", "okt", "669", "wow", "stm", "mtm", "far", "ult",
    "s3m", "dmf", "xm", "wav", "mid", "ams", "it",
];

/// Prüft case-insensitiv, ob eine Dateiendung zu einem unterstützten Archivformat (`.ZIP`, `.ARJ`, `.LHA`, `.LZH`) gehört.
pub fn is_supported_archive_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "zip" | "arj" | "lha" | "lzh"
    )
}

/// Prüft, ob ein Dateipfad auf ein unterstütztes Archiv (`.ZIP`, `.ARJ`, `.LHA`, `.LZH`) zeigt.
pub fn is_supported_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(is_supported_archive_ext)
        .unwrap_or(false)
}

/// Ermittelt den 1-basierten `filetype`-Index (1..=20) aus der Dateiendung.
pub fn filetype_index_from_ext(ext: &str) -> Option<u8> {
    let lower = ext.to_ascii_lowercase();
    if lower == "lha" {
        return Some(3); // In newopen.pas entspricht Typ 3 ("lzh") sowohl .LZH als auch .LHA
    }
    FILETYPES_AS_STRING
        .iter()
        .position(|&e| e == lower)
        .map(|i| (i + 1) as u8)
}

/// Normalisiert Pfadtrenner aus alten DOS/Windows-`.MOL`-Listen (ersetzt `\` unter Linux/Unix durch `/`).
pub fn normalize_mol_path_separators(raw: &str) -> String {
    #[cfg(not(target_os = "windows"))]
    {
        raw.replace('\\', "/")
    }
    #[cfg(target_os = "windows")]
    {
        raw.to_string()
    }
}

/// Löst einen Dateipfad bei Bedarf case-insensitiv auf:
/// Wenn `path.exists()` fehlschlägt (z. B. `GIANA_S.MOD` vs. `giana_s.mod` auf einem
/// case-sensitiven Linux-Dateisystem), wird im Elternverzeichnis nach einer Datei
/// mit exakt demselben Namen (`eq_ignore_ascii_case`) gesucht.
pub fn resolve_path_case_insensitive(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let norm_str = normalize_mol_path_separators(&path.to_string_lossy());
    let norm_path = PathBuf::from(&norm_str);
    if norm_path.exists() {
        return norm_path;
    }
    if let Some(file_name) = norm_path.file_name().and_then(|n| n.to_str()) {
        let parent = norm_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let resolved_parent = if parent.exists() {
            parent.to_path_buf()
        } else if let (Some(grand), Some(par_name)) =
            (parent.parent(), parent.file_name().and_then(|n| n.to_str()))
        {
            let gp = if grand.as_os_str().is_empty() {
                Path::new(".")
            } else {
                grand
            };
            find_file_case_insensitive(gp, par_name).unwrap_or_else(|| parent.to_path_buf())
        } else {
            parent.to_path_buf()
        };
        if let Some(found) = find_file_case_insensitive(&resolved_parent, file_name) {
            return found;
        }
    }
    norm_path
}

/// Liest eine `.MOL`-Datei (`sauglist` in `newopen.pas` Zeilen 7410-7578) und gibt alle
/// darin referenzierten Tracker-Dateipfade zurück.
/// Unterstützt sowohl das originale binär-indizierte `[DIRECTORIES]`/`[FILES]`-Format
/// aus MOD4WIN 2.40 als auch einfache Textzeilen-Listen.
pub fn load_mol_file(mol_path: &Path) -> Vec<PathBuf> {
    let resolved_mol = resolve_path_case_insensitive(mol_path);
    let Ok(bytes) = fs::read(&resolved_mol) else {
        return Vec::new();
    };
    let content = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = content.lines().map(|l| l.trim_end_matches('\r')).collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let base_dir = resolved_mol
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // Prüfen, ob es eine originale MOD4WIN `[DIRECTORIES]`-Datei ist
    if lines[0].trim() == "[DIRECTORIES]" {
        let mut dirs: Vec<(usize, PathBuf)> = Vec::new();
        let mut arcs: Vec<(usize, u8, String)> = Vec::new();
        let mut arc_dirs: Vec<String> = vec![String::new(), String::new()]; // 1-basiert, Index 1 = Root
        let mut result = Vec::new();

        #[derive(PartialEq)]
        enum Section {
            Directories,
            Archives,
            ArchivDirectories,
            Files,
        }
        let mut sec = Section::Directories;

        for &raw_line in &lines[1..] {
            let trimmed = raw_line.trim();
            if trimmed == "[ARCHIVES]" {
                sec = Section::Archives;
                continue;
            } else if trimmed == "[ARCHIVDIRECTORIES]" {
                sec = Section::ArchivDirectories;
                continue;
            } else if trimmed == "[FILES]" {
                sec = Section::Files;
                continue;
            }
            if raw_line.is_empty() {
                continue;
            }
            let lb = raw_line.as_bytes();
            match sec {
                Section::Directories => {
                    if lb.len() >= 2 {
                        let idx = lb[0].wrapping_sub(INDEX_OFFSET) as usize;
                        let dir_str = normalize_mol_path_separators(raw_line[1..].trim());
                        if !dir_str.is_empty() {
                            dirs.push((idx, PathBuf::from(dir_str)));
                        }
                    }
                }
                Section::Archives => {
                    if lb.len() >= 3 {
                        let dir_idx = lb[0].wrapping_sub(INDEX_OFFSET) as usize;
                        let arc_type = lb[1].wrapping_sub(INDEX_OFFSET);
                        let arc_name = normalize_mol_path_separators(raw_line[2..].trim());
                        arcs.push((dir_idx, arc_type, arc_name));
                    }
                }
                Section::ArchivDirectories => {
                    if lb.len() >= 2 {
                        let sub = normalize_mol_path_separators(raw_line[1..].trim());
                        arc_dirs.push(sub);
                    }
                }
                Section::Files => {
                    // Format in savelist:
                    // s[1] = filetype + 31
                    // s[2] = dir_idx + 31
                    // s[3] = arc_idx + 31 (oder 255 + 31 = 30 für noarcconst)
                    // s[4] = arcdir_idx + 31
                    // s[5..] = Dateiname ohne Endung (8 Zeichen oder Langname)
                    if lb.len() >= 5 {
                        let ft = lb[0].wrapping_sub(INDEX_OFFSET) as usize;
                        let dir_idx = lb[1].wrapping_sub(INDEX_OFFSET) as usize;
                        let arc_idx = lb[2].wrapping_sub(INDEX_OFFSET);
                        let _arcdir_idx = lb[3].wrapping_sub(INDEX_OFFSET) as usize;
                        let stem = raw_line[4..].trim();
                        if ft >= 1 && ft <= FILETYPES_AS_STRING.len() && !stem.is_empty() {
                            let ext = FILETYPES_AS_STRING[ft - 1];
                            let file_name = format!("{}.{}", stem, ext);
                            let dir_path = dirs
                                .iter()
                                .find(|(i, _)| *i == dir_idx)
                                .map(|(_, p)| p.clone())
                                .unwrap_or_else(|| base_dir.clone());

                            if arc_idx == NO_ARC_CONST || arc_idx == 0 {
                                let candidate = resolve_path_case_insensitive(&dir_path.join(&file_name));
                                if candidate.exists() {
                                    result.push(candidate);
                                } else if let Some(found) = find_file_case_insensitive(&base_dir, &file_name) {
                                    result.push(found);
                                } else {
                                    result.push(candidate);
                                }
                            } else if let Some((a_dir_idx, _, arc_name)) = arcs.get((arc_idx - 1) as usize) {
                                let a_dir = dirs
                                    .iter()
                                    .find(|(i, _)| *i == *a_dir_idx)
                                    .map(|(_, p)| p.clone())
                                    .unwrap_or_else(|| dir_path.clone());
                                let mut zip_path = resolve_path_case_insensitive(&a_dir.join(arc_name));
                                if !zip_path.exists() {
                                    if let Some(found_zip) = find_file_case_insensitive(&base_dir, arc_name) {
                                        zip_path = found_zip;
                                    }
                                }
                                if zip_path.exists() {
                                    if let Some(extracted) = extract_zip_entry_by_name(&zip_path, &file_name) {
                                        result.push(extracted);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return result;
    }

    // Fallback: Einfache Textzeilen-Playlist (.MOL / .M3U)
    let mut result = Vec::new();
    for line in lines.drain(..) {
        let raw_trimmed = line.trim();
        if raw_trimmed.is_empty() || raw_trimmed.starts_with('#') || raw_trimmed.starts_with(';') {
            continue;
        }
        let norm_line = normalize_mol_path_separators(raw_trimmed);
        let p = Path::new(&norm_line);
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            base_dir.join(p)
        };
        let full = resolve_path_case_insensitive(&candidate);
        #[cfg(not(target_os = "windows"))]
        let full = if !full.exists() {
            // Falls ein alter DOS/Windows-Laufwerkspfad (z. B. `C:/MODS/SONG.MOD`) unter Linux geladen wird,
            // versuche auch den Dateinamen direkt relativ zu `base_dir` aufzulösen.
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|fname| find_file_case_insensitive(&base_dir, fname))
                .unwrap_or(full)
        } else {
            full
        };
        if UiSharedState::is_tracker_file(&full) {
            result.push(full);
        }
    }
    result
}

/// Speichert eine Playlist im originalen MOD4WIN 2.40 `.MOL`-Format (`tmydirs.savelist` in `newopen.pas` Zeilen 3516-3615).
pub fn save_mol_file(mol_path: &Path, playlist: &[PathBuf]) -> std::io::Result<()> {
    let mut dirs: Vec<String> = Vec::new();
    let mut file_lines: Vec<Vec<u8>> = Vec::new();

    for path in playlist {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(ft_idx) = filetype_index_from_ext(ext) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let parent_str = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let clean_dir = parent_str.trim_end_matches(['\\', '/']).to_string();

        let dir_idx = if let Some(pos) = dirs
            .iter()
            .position(|d| d.eq_ignore_ascii_case(&clean_dir))
        {
            (pos + 1) as u8
        } else {
            if dirs.len() >= 200 {
                continue;
            }
            dirs.push(clean_dir);
            dirs.len() as u8
        };

        // 4 Status-Bytes + Dateiname (ohne Endung)
        let mut line = Vec::with_capacity(4 + stem.len());
        line.push(ft_idx.wrapping_add(INDEX_OFFSET));
        line.push(dir_idx.wrapping_add(INDEX_OFFSET));
        line.push(NO_ARC_CONST.wrapping_add(INDEX_OFFSET)); // #255 + 31 = #30
        line.push(1u8.wrapping_add(INDEX_OFFSET)); // arcdir = 1
        line.extend_from_slice(stem.as_bytes());
        file_lines.push(line);
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"[DIRECTORIES]\r\n");
    for (i, d) in dirs.iter().enumerate() {
        let idx_byte = ((i + 1) as u8).wrapping_add(INDEX_OFFSET);
        out.push(idx_byte);
        out.extend_from_slice(d.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"[ARCHIVES]\r\n");
    out.extend_from_slice(b"[ARCHIVDIRECTORIES]\r\n");
    out.extend_from_slice(b"[FILES]\r\n");
    for fl in file_lines {
        out.extend_from_slice(&fl);
        out.extend_from_slice(b"\r\n");
    }

    fs::write(mol_path, out)
}

fn find_file_case_insensitive(dir: &Path, target_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case(target_name) {
                return Some(p);
            }
        }
    }
    None
}

// =============================================================================
// Reiner Rust .ZIP-, .ARJ- & .LHA/.LZH-Archiv-Browser & Entpacker
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Arj,
    Lha,
}

#[derive(Clone, Debug)]
pub struct ZipEntryInfo {
    pub format: ArchiveFormat,
    pub full_name: String,
    pub dir_part: String,
    pub file_name: String,
    pub compression_method: u16,
    pub compressed_size: usize,
    pub uncompressed_size: usize,
    pub local_header_offset: usize,
}

fn split_archive_path(raw_name: &str) -> (String, String, String) {
    let norm = raw_name.replace('/', "\\").trim_matches('\\').to_string();
    let (dir_part, file_name) = match norm.rfind('\\') {
        Some(slash) => (
            norm[..slash].trim_matches('\\').to_string(),
            norm[slash + 1..].to_string(),
        ),
        None => (String::new(), norm.clone()),
    };
    (norm, dir_part, file_name)
}

/// Liest das Inhaltsverzeichnis eines `.ZIP`-, `.ARJ`-, `.LHA`- oder `.LZH`-Archivs
/// und gibt alle darin enthaltenen Dateien zurück.
pub fn list_zip_entries(zip_path: &Path) -> Vec<ZipEntryInfo> {
    let Ok(data) = fs::read(zip_path) else {
        return Vec::new();
    };
    parse_zip_entries(&data)
}

pub fn parse_zip_entries(data: &[u8]) -> Vec<ZipEntryInfo> {
    // 1. Prüfe auf .ARJ-Signatur (0x60 0xEA)
    if data.len() >= 4 && data[0] == 0x60 && data[1] == 0xEA {
        let arj = parse_arj_entries(data);
        if !arj.is_empty() {
            return arj;
        }
    }
    // 2. Prüfe auf .LHA / .LZH-Header ("-lhX-" oder "-lzX-" ab Offset 2)
    if data.len() >= 21 && data[2] == b'-' && data[6] == b'-' && (data[3] == b'l' || data[3] == b'L') {
        let lha = parse_lha_entries(data);
        if !lha.is_empty() {
            return lha;
        }
    }

    let mut entries = Vec::new();
    // 3. Suche End of Central Directory (PK\x05\x06) von hinten (.ZIP)
    let min_eocd = 22usize;
    if data.len() < min_eocd {
        return entries;
    }
    let search_start = data.len().saturating_sub(65557);
    let mut eocd_pos = None;
    for i in (search_start..=data.len() - min_eocd).rev() {
        if &data[i..i + 4] == b"PK\x05\x06" {
            eocd_pos = Some(i);
            break;
        }
    }

    if let Some(eocd) = eocd_pos {
        let total_entries = u16::from_le_bytes([data[eocd + 10], data[eocd + 11]]) as usize;
        let cd_offset = u32::from_le_bytes([
            data[eocd + 16],
            data[eocd + 17],
            data[eocd + 18],
            data[eocd + 19],
        ]) as usize;

        let mut pos = cd_offset;
        for _ in 0..total_entries {
            if pos + 46 > data.len() || &data[pos..pos + 4] != b"PK\x01\x02" {
                break;
            }
            let method = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
            let comp_size = u32::from_le_bytes([
                data[pos + 20],
                data[pos + 21],
                data[pos + 22],
                data[pos + 23],
            ]) as usize;
            let uncomp_size = u32::from_le_bytes([
                data[pos + 24],
                data[pos + 25],
                data[pos + 26],
                data[pos + 27],
            ]) as usize;
            let name_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
            let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
            let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
            let lho = u32::from_le_bytes([
                data[pos + 42],
                data[pos + 43],
                data[pos + 44],
                data[pos + 45],
            ]) as usize;

            let name_start = pos + 46;
            let name_end = (name_start + name_len).min(data.len());
            let raw_name = String::from_utf8_lossy(&data[name_start..name_end])
                .replace('/', "\\");

            if !raw_name.ends_with('\\') {
                let (full_name, dir_part, file_name) = split_archive_path(&raw_name);
                if !file_name.is_empty() {
                    entries.push(ZipEntryInfo {
                        format: ArchiveFormat::Zip,
                        full_name,
                        dir_part,
                        file_name,
                        compression_method: method,
                        compressed_size: comp_size,
                        uncompressed_size: uncomp_size,
                        local_header_offset: lho,
                    });
                }
            }

            pos = name_start + name_len + extra_len + comment_len;
        }
    }
    entries
}

/// Parst das Inhaltsverzeichnis eines `.ARJ`-Archivs (`0x60 0xEA` Header).
pub fn parse_arj_entries(data: &[u8]) -> Vec<ZipEntryInfo> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    let mut is_first_header = true;

    while pos + 4 <= data.len() {
        if data[pos] != 0x60 || data[pos + 1] != 0xEA {
            break;
        }
        let basic_hdr_size = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if basic_hdr_size == 0 {
            // End-of-Archive-Marker
            break;
        }
        if basic_hdr_size > 2600 || pos + 4 + basic_hdr_size + 4 > data.len() {
            break;
        }
        let basic_hdr = &data[pos + 4..pos + 4 + basic_hdr_size];
        if basic_hdr.len() < 30 {
            break;
        }
        let first_hdr_size = basic_hdr[0] as usize;
        let arj_flags = basic_hdr[4];
        let method = basic_hdr[5] as u16;
        let file_type = basic_hdr[6];
        let comp_size = u32::from_le_bytes([
            basic_hdr[12],
            basic_hdr[13],
            basic_hdr[14],
            basic_hdr[15],
        ]) as usize;
        let uncomp_size = u32::from_le_bytes([
            basic_hdr[16],
            basic_hdr[17],
            basic_hdr[18],
            basic_hdr[19],
        ]) as usize;

        // Überspringe Basic Header + 4 Bytes CRC32 + alle Extended Headers (bis ext_size == 0)
        let mut ext_pos = pos + 4 + basic_hdr_size + 4;
        loop {
            if ext_pos + 2 > data.len() {
                return entries;
            }
            let ext_size = u16::from_le_bytes([data[ext_pos], data[ext_pos + 1]]) as usize;
            ext_pos += 2;
            if ext_size == 0 {
                break;
            }
            if ext_pos + ext_size + 4 > data.len() {
                return entries;
            }
            ext_pos += ext_size + 4;
        }

        let payload_offset = ext_pos;
        // Der erste Header (oder file_type == 2) ist der Archive Main Header ohne Datei-Payload
        if is_first_header || file_type == 2 {
            is_first_header = false;
            pos = payload_offset;
            continue;
        }

        // Dateiname ab first_hdr_size (NUL-terminiert)
        if first_hdr_size < basic_hdr.len() && (file_type == 0 || file_type == 1) {
            let name_slice = &basic_hdr[first_hdr_size..];
            let nul_pos = name_slice
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(name_slice.len());
            let raw_name = String::from_utf8_lossy(&name_slice[..nul_pos]).to_string();
            // Flag 0x01 = GARBLED (verschlüsselt) ignorieren wir, unverschlüsselte Dateien übernehmen wir
            if (arj_flags & 0x01) == 0 && !raw_name.is_empty() {
                let (full_name, dir_part, file_name) = split_archive_path(&raw_name);
                if !file_name.is_empty() && payload_offset + comp_size <= data.len() {
                    entries.push(ZipEntryInfo {
                        format: ArchiveFormat::Arj,
                        full_name,
                        dir_part,
                        file_name,
                        compression_method: method,
                        compressed_size: comp_size,
                        uncompressed_size: uncomp_size,
                        local_header_offset: payload_offset,
                    });
                }
            }
        }

        let Some(next_pos) = payload_offset.checked_add(comp_size) else {
            break;
        };
        if next_pos > data.len() {
            break;
        }
        pos = next_pos;
    }

    entries
}

/// Codiert die 5-Byte-Methodenkennung eines `.LHA`/`.LZH`-Headers als `u16`.
fn lha_method_id(method: &[u8]) -> Option<u16> {
    if method.eq_ignore_ascii_case(b"-lh0-") || method.eq_ignore_ascii_case(b"-lz4-") {
        Some(0)
    } else if method.eq_ignore_ascii_case(b"-lh1-") {
        Some(1)
    } else if method.eq_ignore_ascii_case(b"-lh4-") {
        Some(4)
    } else if method.eq_ignore_ascii_case(b"-lh5-") {
        Some(5)
    } else if method.eq_ignore_ascii_case(b"-lh6-") {
        Some(6)
    } else if method.eq_ignore_ascii_case(b"-lh7-") {
        Some(7)
    } else if method.eq_ignore_ascii_case(b"-lz5-") {
        Some(8)
    } else if method.eq_ignore_ascii_case(b"-lhd-") {
        Some(0xFF) // Directory
    } else {
        None
    }
}

/// Parst das Inhaltsverzeichnis eines `.LHA`/`.LZH`-Archivs (Header Level 0, 1 und 2).
pub fn parse_lha_entries(data: &[u8]) -> Vec<ZipEntryInfo> {
    let mut entries = Vec::new();
    let mut pos = 0usize;

    while pos + 21 <= data.len() {
        if data[pos] == 0 {
            break;
        }
        let method_bytes = &data[pos + 2..pos + 7];
        if method_bytes[0] != b'-' || method_bytes[4] != b'-' {
            break;
        }
        let Some(method_code) = lha_method_id(method_bytes) else {
            break;
        };

        let comp_or_skip = u32::from_le_bytes([
            data[pos + 7],
            data[pos + 8],
            data[pos + 9],
            data[pos + 10],
        ]) as usize;
        let uncomp_size = u32::from_le_bytes([
            data[pos + 11],
            data[pos + 12],
            data[pos + 13],
            data[pos + 14],
        ]) as usize;
        let header_level = data[pos + 20];

        let (raw_path, payload_offset, comp_size, next_pos) = match header_level {
            0 => {
                let hdr_size = data[pos] as usize;
                let payload_off = pos + hdr_size + 2;
                if pos + 22 > data.len() || payload_off > data.len() {
                    break;
                }
                let name_len = data[pos + 21] as usize;
                let name_end = (pos + 22 + name_len).min(payload_off);
                let mapped: Vec<u8> = data[pos + 22..name_end]
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| if b == 0xFF { b'\\' } else { b })
                    .collect();
                let name = String::from_utf8_lossy(&mapped).to_string();
                let Some(n_pos) = payload_off.checked_add(comp_or_skip) else {
                    break;
                };
                (name, payload_off, comp_or_skip, n_pos)
            }
            1 => {
                let hdr_size = data[pos] as usize;
                let base_end = pos + hdr_size + 2;
                if pos + 22 > data.len() || base_end > data.len() {
                    break;
                }
                let name_len = data[pos + 21] as usize;
                let name_end = (pos + 22 + name_len).min(base_end.saturating_sub(2));
                let mapped: Vec<u8> = data[pos + 22..name_end]
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| if b == 0xFF { b'\\' } else { b })
                    .collect();
                let mut file_name = String::from_utf8_lossy(&mapped).to_string();
                let mut dir_name = String::new();

                // Extended Headers ab base_end - 2
                let mut ext_pos = base_end - 2;
                let mut total_ext = 0usize;
                loop {
                    if ext_pos + 2 > data.len() {
                        return entries;
                    }
                    let ext_size = u16::from_le_bytes([data[ext_pos], data[ext_pos + 1]]) as usize;
                    ext_pos += 2;
                    if ext_size == 0 {
                        break;
                    }
                    total_ext += ext_size;
                    if ext_size < 3 || ext_pos + ext_size - 2 > data.len() {
                        return entries;
                    }
                    let ext_type = data[ext_pos];
                    let ext_body = &data[ext_pos + 1..ext_pos + ext_size - 2];
                    match ext_type {
                        0x01 => {
                            file_name = String::from_utf8_lossy(ext_body)
                                .trim_end_matches('\0')
                                .to_string();
                        }
                        0x02 => {
                            let d_mapped: Vec<u8> = ext_body
                                .iter()
                                .take_while(|&&b| b != 0)
                                .map(|&b| if b == 0xFF { b'\\' } else { b })
                                .collect();
                            dir_name = String::from_utf8_lossy(&d_mapped).to_string();
                        }
                        _ => {}
                    }
                    ext_pos += ext_size - 2;
                }
                let payload_off = ext_pos;
                let actual_comp = comp_or_skip.saturating_sub(total_ext);
                let Some(n_pos) = base_end.checked_add(comp_or_skip) else {
                    break;
                };
                let combined = if dir_name.is_empty() {
                    file_name
                } else {
                    format!("{}\\{}", dir_name.trim_matches('\\'), file_name)
                };
                (combined, payload_off, actual_comp, n_pos)
            }
            2 => {
                let total_hdr_size = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                let payload_off = pos + total_hdr_size;
                if pos + 26 > data.len() || payload_off > data.len() {
                    break;
                }
                let mut file_name = String::new();
                let mut dir_name = String::new();
                let mut ext_pos = pos + 24;
                loop {
                    if ext_pos + 2 > payload_off {
                        break;
                    }
                    let ext_size = u16::from_le_bytes([data[ext_pos], data[ext_pos + 1]]) as usize;
                    ext_pos += 2;
                    if ext_size == 0 {
                        break;
                    }
                    if ext_size < 3 || ext_pos + ext_size - 2 > payload_off {
                        break;
                    }
                    let ext_type = data[ext_pos];
                    let ext_body = &data[ext_pos + 1..ext_pos + ext_size - 2];
                    match ext_type {
                        0x01 => {
                            file_name = String::from_utf8_lossy(ext_body)
                                .trim_end_matches('\0')
                                .to_string();
                        }
                        0x02 => {
                            let d_mapped: Vec<u8> = ext_body
                                .iter()
                                .take_while(|&&b| b != 0)
                                .map(|&b| if b == 0xFF { b'\\' } else { b })
                                .collect();
                            dir_name = String::from_utf8_lossy(&d_mapped).to_string();
                        }
                        _ => {}
                    }
                    ext_pos += ext_size - 2;
                }
                let Some(n_pos) = payload_off.checked_add(comp_or_skip) else {
                    break;
                };
                let combined = if dir_name.is_empty() {
                    file_name
                } else {
                    format!("{}\\{}", dir_name.trim_matches('\\'), file_name)
                };
                (combined, payload_off, comp_or_skip, n_pos)
            }
            _ => break,
        };

        if method_code != 0xFF && !raw_path.is_empty() && payload_offset + comp_size <= data.len() {
            let (full_name, dir_part, file_name) = split_archive_path(&raw_path);
            if !file_name.is_empty() {
                entries.push(ZipEntryInfo {
                    format: ArchiveFormat::Lha,
                    full_name,
                    dir_part,
                    file_name,
                    compression_method: method_code,
                    compressed_size: comp_size,
                    uncompressed_size: uncomp_size,
                    local_header_offset: payload_offset,
                });
            }
        }

        if next_pos <= pos || next_pos > data.len() {
            break;
        }
        pos = next_pos;
    }

    entries
}

/// Entpackt einen einzelnen Eintrag aus einem bereits im Speicher liegenden `.ZIP`-, `.ARJ`- oder `.LHA`/`.LZH`-Archiv direkt in ein `Vec<u8>`.
pub fn extract_zip_entry_bytes(data: &[u8], entry: &ZipEntryInfo) -> Option<Vec<u8>> {
    match entry.format {
        ArchiveFormat::Zip => {
            let lho = entry.local_header_offset;
            if lho + 30 > data.len() || &data[lho..lho + 4] != b"PK\x03\x04" {
                return None;
            }
            let name_len = u16::from_le_bytes([data[lho + 26], data[lho + 27]]) as usize;
            let extra_len = u16::from_le_bytes([data[lho + 28], data[lho + 29]]) as usize;
            let payload_start = lho + 30 + name_len + extra_len;
            let payload_end = payload_start.checked_add(entry.compressed_size)?;
            if payload_end > data.len() {
                return None;
            }
            let payload = &data[payload_start..payload_end];
            match entry.compression_method {
                0 => Some(payload.to_vec()),
                8 => inflate_raw_deflate(payload, entry.uncompressed_size),
                _ => None,
            }
        }
        ArchiveFormat::Arj => {
            let payload_start = entry.local_header_offset;
            let payload_end = payload_start.checked_add(entry.compressed_size)?;
            if payload_end > data.len() {
                return None;
            }
            let payload = &data[payload_start..payload_end];
            match entry.compression_method {
                0 => Some(payload.to_vec()),
                1..=3 => decode_lzh_huffman(payload, entry.uncompressed_size, 5, 17),
                4 => decode_arj_method4(payload, entry.uncompressed_size),
                _ => None,
            }
        }
        ArchiveFormat::Lha => {
            let payload_start = entry.local_header_offset;
            let payload_end = payload_start.checked_add(entry.compressed_size)?;
            if payload_end > data.len() {
                return None;
            }
            let payload = &data[payload_start..payload_end];
            match entry.compression_method {
                0 => Some(payload.to_vec()),
                1 => decode_lha_lh1(payload, entry.uncompressed_size),
                4 | 5 => decode_lzh_huffman(payload, entry.uncompressed_size, 4, 14),
                6 => decode_lzh_huffman(payload, entry.uncompressed_size, 5, 16),
                7 => decode_lzh_huffman(payload, entry.uncompressed_size, 5, 17),
                8 => decode_lha_lz5(payload, entry.uncompressed_size),
                _ => None,
            }
        }
    }
}

/// Entpackt einen einzelnen Eintrag aus einem `.ZIP`-, `.ARJ`- oder `.LHA`/`.LZH`-Archiv in das temporäre
/// MOD4WIN-Archivverzeichnis und gibt den Pfad zur entpackten Datei zurück.
pub fn extract_zip_entry(zip_path: &Path, entry: &ZipEntryInfo) -> Option<PathBuf> {
    let zip_stem = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");
    let cache_dir = std::env::temp_dir()
        .join("mod4win_zip_cache")
        .join(zip_stem);
    let _ = fs::create_dir_all(&cache_dir);
    let out_path = cache_dir.join(&entry.file_name);

    // Wenn bereits mit passender Größe entpackt, direkt wiederverwenden
    if let Ok(meta) = fs::metadata(&out_path) {
        if meta.len() as usize == entry.uncompressed_size && entry.uncompressed_size > 0 {
            return Some(out_path);
        }
    }

    let data = fs::read(zip_path).ok()?;
    let decoded = extract_zip_entry_bytes(&data, entry)?;
    fs::write(&out_path, &decoded).ok()?;
    Some(out_path)
}

pub fn extract_zip_entry_by_name(zip_path: &Path, file_name: &str) -> Option<PathBuf> {
    let entries = list_zip_entries(zip_path);
    let found = entries
        .iter()
        .find(|e| e.file_name.eq_ignore_ascii_case(file_name))?;
    extract_zip_entry(zip_path, found)
}

/// Entpackt alle Tracker-Module aus einem `.ZIP`-, `.ARJ`- oder `.LHA`/`.LZH`-Archiv (z.B. bei Drag & Drop).
pub fn extract_all_tracker_files_from_zip(zip_path: &Path) -> Vec<PathBuf> {
    let entries = list_zip_entries(zip_path);
    let mut out = Vec::new();
    for entry in &entries {
        if UiSharedState::is_tracker_file(Path::new(&entry.file_name)) {
            if let Some(p) = extract_zip_entry(zip_path, entry) {
                out.push(p);
            }
        }
    }
    out
}

// =============================================================================
// MSB-First Bit-Reader & LZH/ARJ Huffman- + LZSS-Entpacker (ar002 / unarj)
// =============================================================================

struct MsbBitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u64,
    bit_cnt: u8,
}

impl<'a> MsbBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    #[inline]
    fn ensure_bits(&mut self, n: u8) {
        while self.bit_cnt < n {
            let b = if self.byte_pos < self.data.len() {
                let v = self.data[self.byte_pos];
                self.byte_pos += 1;
                v as u64
            } else {
                0
            };
            self.bit_buf = (self.bit_buf << 8) | b;
            self.bit_cnt += 8;
        }
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> u32 {
        if n == 0 {
            return 0;
        }
        self.ensure_bits(n);
        self.bit_cnt -= n;
        ((self.bit_buf >> self.bit_cnt) & ((1u64 << n) - 1)) as u32
    }
}

enum MsbHuffDecoder {
    Single(u16),
    Tree {
        counts: [u16; 17],
        symbols: Vec<u16>,
    },
}

impl MsbHuffDecoder {
    fn build(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; 17];
        for &l in lengths {
            if l as usize > 16 {
                return None;
            }
            if l > 0 {
                counts[l as usize] += 1;
            }
        }
        let mut offsets = [0u16; 17];
        let mut total = 0u16;
        for bits in 1..=16 {
            offsets[bits] = total;
            total += counts[bits];
        }
        if total == 0 {
            return Some(Self::Single(0));
        }
        let mut symbols = vec![0u16; total as usize];
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let idx = offsets[l as usize] as usize;
                offsets[l as usize] += 1;
                symbols[idx] = sym as u16;
            }
        }
        Some(Self::Tree { counts, symbols })
    }

    #[inline]
    fn decode(&self, br: &mut MsbBitReader) -> Option<u16> {
        match self {
            Self::Single(sym) => Some(*sym),
            Self::Tree { counts, symbols } => {
                let mut code = 0i32;
                let mut first = 0i32;
                let mut index = 0i32;
                for len in 1..=16 {
                    code |= br.read_bits(1) as i32;
                    let count = counts[len] as i32;
                    if code - count < first {
                        let sym_idx = (index + (code - first)) as usize;
                        return symbols.get(sym_idx).copied();
                    }
                    index += count;
                    first += count;
                    first <<= 1;
                    code <<= 1;
                }
                None
            }
        }
    }
}

fn read_lzh_pt_table(
    br: &mut MsbBitReader,
    nn: usize,
    nbit: u8,
    i_special: Option<usize>,
) -> Option<MsbHuffDecoder> {
    let n = br.read_bits(nbit) as usize;
    if n == 0 {
        let c = br.read_bits(nbit) as u16;
        return Some(MsbHuffDecoder::Single(c));
    }
    let mut pt_len = vec![0u8; nn.max(n)];
    let mut i = 0usize;
    while i < n {
        let mut c = br.read_bits(3) as u8;
        if c == 7 {
            while br.read_bits(1) != 0 {
                c += 1;
                if c > 16 {
                    return None;
                }
            }
        }
        if i < pt_len.len() {
            pt_len[i] = c;
        }
        i += 1;
        if Some(i) == i_special {
            let zeros = br.read_bits(2) as usize;
            for _ in 0..zeros {
                if i < pt_len.len() {
                    pt_len[i] = 0;
                }
                i += 1;
            }
        }
    }
    MsbHuffDecoder::build(&pt_len)
}

fn read_lzh_c_table(br: &mut MsbBitReader, pt_table: &MsbHuffDecoder) -> Option<MsbHuffDecoder> {
    const NC: usize = 510;
    let n = br.read_bits(9) as usize;
    if n == 0 {
        let c = br.read_bits(9) as u16;
        return Some(MsbHuffDecoder::Single(c));
    }
    let mut c_len = vec![0u8; NC];
    let mut i = 0usize;
    while i < n && i < NC {
        let c = pt_table.decode(br)?;
        if c <= 2 {
            let zeros = match c {
                0 => 1,
                1 => br.read_bits(4) as usize + 3,
                _ => br.read_bits(9) as usize + 20,
            };
            i = (i + zeros).min(NC);
        } else {
            c_len[i] = (c - 2) as u8;
            i += 1;
        }
    }
    MsbHuffDecoder::build(&c_len)
}

/// Gemeinsamer Block-Huffman + LZSS-Entpacker für LHA (`-lh4-`, `-lh5-`, `-lh6-`, `-lh7-`)
/// und ARJ (`Method 1`, `Method 2`, `Method 3`).
pub fn decode_lzh_huffman(
    input: &[u8],
    expected_size: usize,
    pbit: u8,
    np: usize,
) -> Option<Vec<u8>> {
    let mut br = MsbBitReader::new(input);
    let mut out = Vec::with_capacity(expected_size);
    let mut block_rem = 0usize;
    let mut c_table = MsbHuffDecoder::Single(0);
    let mut p_table = MsbHuffDecoder::Single(0);

    while out.len() < expected_size {
        if block_rem == 0 {
            block_rem = br.read_bits(16) as usize;
            if block_rem == 0 {
                break;
            }
            let pt_for_c = read_lzh_pt_table(&mut br, 19, 5, Some(3))?;
            c_table = read_lzh_c_table(&mut br, &pt_for_c)?;
            p_table = read_lzh_pt_table(&mut br, np, pbit, None)?;
        }
        block_rem -= 1;

        let c = c_table.decode(&mut br)?;
        if c < 256 {
            out.push(c as u8);
        } else {
            let match_len = (c as usize) - 256 + 3;
            let p = p_table.decode(&mut br)? as usize;
            let dist_minus_1 = if p <= 1 {
                p
            } else {
                (1usize << (p - 1)) + (br.read_bits((p - 1) as u8) as usize)
            };
            let distance = dist_minus_1 + 1;
            for _ in 0..match_len {
                if out.len() >= expected_size {
                    break;
                }
                let b = if distance <= out.len() {
                    out[out.len() - distance]
                } else {
                    b' '
                };
                out.push(b);
            }
        }
    }

    Some(out)
}

/// ARJ Method 4 (`decode_f` in `unarj.c`): Variable-Length LZSS ohne Huffman-Bäume.
pub fn decode_arj_method4(input: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    let mut br = MsbBitReader::new(input);
    let mut out = Vec::with_capacity(expected_size);

    while out.len() < expected_size {
        // decode_len (STRTL = 0, STOPL = 7)
        let mut c = 0usize;
        let mut width = 0u8;
        while width < 7 {
            if br.read_bits(1) == 0 {
                break;
            }
            c += 1usize << width;
            width += 1;
        }
        if width > 0 {
            c += br.read_bits(width) as usize;
        }

        if c == 0 {
            out.push(br.read_bits(8) as u8);
        } else {
            let match_len = c + 2; // THRESHOLD = 3 -> c + (3 - 1)
            // decode_ptr (STRTP = 9, STOPP = 13)
            let mut pos = 0usize;
            let mut pwidth = 9u8;
            while pwidth < 13 {
                if br.read_bits(1) == 0 {
                    break;
                }
                pos += 1usize << pwidth;
                pwidth += 1;
            }
            pos += br.read_bits(pwidth) as usize;
            let distance = pos + 1;

            for _ in 0..match_len {
                if out.len() >= expected_size {
                    break;
                }
                let b = if distance <= out.len() {
                    out[out.len() - distance]
                } else {
                    0
                };
                out.push(b);
            }
        }
    }

    Some(out)
}

/// LArc `-lz5-` 4 KB Ringbuffer-LZSS-Entpacker.
pub fn decode_lha_lz5(input: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    let mut ring = [0u8; 4096];
    let mut idx = 0usize;
    for i in 0..256usize {
        for _ in 0..13 {
            ring[idx] = i as u8;
            idx += 1;
        }
    }
    for i in 0..256usize {
        ring[idx] = i as u8;
        idx += 1;
    }
    for i in (0..256usize).rev() {
        ring[idx] = i as u8;
        idx += 1;
    }
    for _ in 0..128 {
        ring[idx] = 0;
        idx += 1;
    }
    for _ in 0..110 {
        ring[idx] = b' ';
        idx += 1;
    }

    let mut r = 4096 - 18;
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(expected_size);

    while out.len() < expected_size && pos < input.len() {
        let flags = input[pos];
        pos += 1;
        for bit in 0..8 {
            if out.len() >= expected_size || pos >= input.len() {
                break;
            }
            if (flags & (1 << bit)) != 0 {
                let b = input[pos];
                pos += 1;
                out.push(b);
                ring[r] = b;
                r = (r + 1) & 4095;
            } else {
                if pos + 1 >= input.len() {
                    break;
                }
                let b1 = input[pos] as usize;
                let b2 = input[pos + 1] as usize;
                pos += 2;
                let offset = b1 | ((b2 & 0xF0) << 4);
                let match_len = (b2 & 0x0F) + 3;
                for k in 0..match_len {
                    if out.len() >= expected_size {
                        break;
                    }
                    let b = ring[(offset + k) & 4095];
                    out.push(b);
                    ring[r] = b;
                    r = (r + 1) & 4095;
                }
            }
        }
    }

    Some(out)
}

/// LHA `-lh1-` (LHArc 1.x / Amiga LHA): Adaptiver Huffman-Baum (314 Symbole) + 4 KB LZSS.
pub fn decode_lha_lh1(input: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    const N_CHAR: usize = 314;
    const T: usize = N_CHAR * 2 - 1; // 627
    const R: usize = T - 1; // 626
    const MAX_FREQ: u16 = 0x8000;

    // Statische Längen für die oberen 6 Bits der 12-Bit-Distanz (aus Haruhiko Okumura lhuf.c)
    let mut p_lengths = [0u8; 64];
    p_lengths[0] = 3;
    for l in &mut p_lengths[1..4] {
        *l = 4;
    }
    for l in &mut p_lengths[4..12] {
        *l = 5;
    }
    for l in &mut p_lengths[12..24] {
        *l = 6;
    }
    for l in &mut p_lengths[24..48] {
        *l = 7;
    }
    for l in &mut p_lengths[48..64] {
        *l = 8;
    }
    let p_table = MsbHuffDecoder::build(&p_lengths)?;

    let mut freq = [0u16; T + 1];
    let mut prnt = [0usize; T + N_CHAR];
    let mut son = [0usize; T];

    for i in 0..N_CHAR {
        freq[i] = 1;
        son[i] = i + T;
        prnt[i + T] = i;
    }
    let mut i = 0usize;
    let mut j = N_CHAR;
    while j <= R {
        freq[j] = freq[i] + freq[i + 1];
        son[j] = i;
        prnt[i] = j;
        prnt[i + 1] = j;
        i += 2;
        j += 1;
    }
    freq[T] = 0xFFFF;
    prnt[R] = 0;

    let mut ring = [b' '; 4096];
    let mut r = 4096 - 60;
    let mut br = MsbBitReader::new(input);
    let mut out = Vec::with_capacity(expected_size);

    while out.len() < expected_size {
        // DecodeChar
        let mut c = son[R];
        while c < T {
            c += br.read_bits(1) as usize;
            c = son[c];
        }
        let sym = c - T;

        // Update adaptive Huffman tree
        if freq[R] == MAX_FREQ {
            let mut leaf_idx = 0usize;
            for node in 0..T {
                if son[node] >= T {
                    freq[leaf_idx] = (freq[node] + 1) / 2;
                    son[leaf_idx] = son[node];
                    leaf_idx += 1;
                }
            }
            let mut ch_i = 0usize;
            for node in N_CHAR..T {
                let k = ch_i + 1;
                let f = freq[ch_i] + freq[k];
                freq[node] = f;
                let mut m = node - 1;
                while f < freq[m] {
                    if m == 0 {
                        break;
                    }
                    m -= 1;
                }
                if f >= freq[m] {
                    m += 1;
                }
                freq.copy_within(m..node, m + 1);
                freq[m] = f;
                son.copy_within(m..node, m + 1);
                son[m] = ch_i;
                ch_i += 2;
            }
            for node in 0..T {
                let k = son[node];
                prnt[k] = node;
                if k < T {
                    prnt[k + 1] = node;
                }
            }
        }

        let mut node = prnt[sym + T];
        loop {
            freq[node] += 1;
            let k = freq[node];
            let mut l = node + 1;
            if k > freq[l] {
                while k > freq[l + 1] {
                    l += 1;
                }
                freq[node] = freq[l];
                freq[l] = k;

                let ch_a = son[node];
                prnt[ch_a] = l;
                if ch_a < T {
                    prnt[ch_a + 1] = l;
                }
                let ch_b = son[l];
                son[l] = ch_a;
                prnt[ch_b] = node;
                if ch_b < T {
                    prnt[ch_b + 1] = node;
                }
                son[node] = ch_b;
                node = l;
            }
            node = prnt[node];
            if node == 0 {
                break;
            }
        }

        if sym < 256 {
            let b = sym as u8;
            out.push(b);
            ring[r] = b;
            r = (r + 1) & 4095;
        } else {
            let match_len = sym - 256 + 3;
            let upper6 = p_table.decode(&mut br)? as usize;
            let lower6 = br.read_bits(6) as usize;
            let dist = (upper6 << 6) | lower6;
            let start = (r + 4096 - dist - 1) & 4095;
            for k in 0..match_len {
                if out.len() >= expected_size {
                    break;
                }
                let b = ring[(start + k) & 4095];
                out.push(b);
                ring[r] = b;
                r = (r + 1) & 4095;
            }
        }
    }

    Some(out)
}

// =============================================================================
// RFC 1951 Raw Deflate Decoder (für ZIP compression_method == 8)
// =============================================================================

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u32,
    bit_cnt: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    #[inline]
    fn ensure_bits(&mut self, n: u8) {
        while self.bit_cnt < n {
            let b = if self.byte_pos < self.data.len() {
                let v = self.data[self.byte_pos];
                self.byte_pos += 1;
                v as u32
            } else {
                0
            };
            self.bit_buf |= b << self.bit_cnt;
            self.bit_cnt += 8;
        }
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> u32 {
        if n == 0 {
            return 0;
        }
        self.ensure_bits(n);
        let val = self.bit_buf & ((1u32 << n) - 1);
        self.bit_buf >>= n;
        self.bit_cnt -= n;
        val
    }

    #[inline]
    fn align_byte(&mut self) {
        let drop = self.bit_cnt & 7;
        self.bit_buf >>= drop;
        self.bit_cnt -= drop;
    }
}

#[derive(Clone)]
struct HuffmanTable {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl HuffmanTable {
    fn build(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l as usize >= 16 {
                return None;
            }
            if l > 0 {
                counts[l as usize] += 1;
            }
        }
        let mut offsets = [0u16; 16];
        let mut total = 0u16;
        for bits in 1..=15 {
            offsets[bits] = total;
            total += counts[bits];
        }
        let mut symbols = vec![0u16; total as usize];
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let idx = offsets[l as usize] as usize;
                offsets[l as usize] += 1;
                symbols[idx] = sym as u16;
            }
        }
        Some(Self { counts, symbols })
    }

    #[inline]
    fn decode(&self, br: &mut BitReader) -> Option<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=15 {
            code |= br.read_bits(1) as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                let sym_idx = (index + (code - first)) as usize;
                return self.symbols.get(sym_idx).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

pub fn inflate_raw_deflate(input: &[u8], expected_size: usize) -> Option<Vec<u8>> {
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    let mut br = BitReader::new(input);
    let mut out = Vec::with_capacity(expected_size.max(4096));

    loop {
        let bfinal = br.read_bits(1);
        let btype = br.read_bits(2);

        match btype {
            0 => {
                // Uncompressed block
                br.align_byte();
                let len = br.read_bits(16) as usize;
                let nlen = br.read_bits(16) as usize;
                if (len ^ 0xFFFF) != nlen {
                    return None;
                }
                for _ in 0..len {
                    out.push(br.read_bits(8) as u8);
                }
            }
            1 | 2 => {
                let (lit_table, dist_table) = if btype == 1 {
                    let mut lit_lengths = [0u8; 288];
                    for i in 0..=143 {
                        lit_lengths[i] = 8;
                    }
                    for i in 144..=255 {
                        lit_lengths[i] = 9;
                    }
                    for i in 256..=279 {
                        lit_lengths[i] = 7;
                    }
                    for i in 280..=287 {
                        lit_lengths[i] = 8;
                    }
                    let dist_lengths = [5u8; 32];
                    (
                        HuffmanTable::build(&lit_lengths)?,
                        HuffmanTable::build(&dist_lengths)?,
                    )
                } else {
                    let hlit = br.read_bits(5) as usize + 257;
                    let hdist = br.read_bits(5) as usize + 1;
                    let hclen = br.read_bits(4) as usize + 4;
                    let mut cl_lengths = [0u8; 19];
                    for i in 0..hclen {
                        cl_lengths[CL_ORDER[i]] = br.read_bits(3) as u8;
                    }
                    let cl_table = HuffmanTable::build(&cl_lengths)?;
                    let mut all_lengths = Vec::with_capacity(hlit + hdist);
                    while all_lengths.len() < hlit + hdist {
                        let sym = cl_table.decode(&mut br)?;
                        match sym {
                            0..=15 => all_lengths.push(sym as u8),
                            16 => {
                                let prev = *all_lengths.last()?;
                                let rep = br.read_bits(2) as usize + 3;
                                for _ in 0..rep {
                                    all_lengths.push(prev);
                                }
                            }
                            17 => {
                                let rep = br.read_bits(3) as usize + 3;
                                for _ in 0..rep {
                                    all_lengths.push(0);
                                }
                            }
                            18 => {
                                let rep = br.read_bits(7) as usize + 11;
                                for _ in 0..rep {
                                    all_lengths.push(0);
                                }
                            }
                            _ => return None,
                        }
                    }
                    if all_lengths.len() < hlit + hdist {
                        return None;
                    }
                    (
                        HuffmanTable::build(&all_lengths[..hlit])?,
                        HuffmanTable::build(&all_lengths[hlit..hlit + hdist])?,
                    )
                };

                loop {
                    let sym = lit_table.decode(&mut br)?;
                    if sym < 256 {
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let len_idx = (sym as usize).checked_sub(257)?;
                        if len_idx >= LEN_BASE.len() {
                            return None;
                        }
                        let length =
                            LEN_BASE[len_idx] as usize + br.read_bits(LEN_EXTRA[len_idx]) as usize;
                        let dist_sym = dist_table.decode(&mut br)? as usize;
                        if dist_sym >= DIST_BASE.len() {
                            return None;
                        }
                        let distance = DIST_BASE[dist_sym] as usize
                            + br.read_bits(DIST_EXTRA[dist_sym]) as usize;
                        if distance == 0 || distance > out.len() {
                            return None;
                        }
                        let start = out.len() - distance;
                        for i in 0..length {
                            let b = out[start + i];
                            out.push(b);
                        }
                    }
                }
            }
            _ => return None,
        }

        if bfinal != 0 {
            break;
        }
    }

    Some(out)
}

/// Prüft, ob ein Pfad auf einen virtuellen `.ZIP`-Eintrag verweist (`pfad.zip::eintrag` oder `pfad.zip\eintrag`).
pub fn parse_zip_virtual_path(path: &Path) -> Option<(PathBuf, String)> {
    let s = path.to_string_lossy();
    if let Some(idx) = s.find("::") {
        let zip_p = resolve_path_case_insensitive(Path::new(&s[..idx]));
        let entry_name = s[idx + 2..].trim_matches(['\\', '/']).to_string();
        if zip_p.is_file() && !entry_name.is_empty() {
            return Some((zip_p, entry_name));
        }
    }
    None
}

/// Liest die Rohbytes und Dateiendung eines Moduls (sowohl reguläre Dateien als auch `.ZIP`-Einträge).
pub fn read_module_bytes_universal(path: &Path) -> std::io::Result<(Vec<u8>, Option<String>)> {
    if let Some((zip_p, entry_name)) = parse_zip_virtual_path(path) {
        let data = fs::read(&zip_p)?;
        let entries = parse_zip_entries(&data);
        if let Some(found) = entries.iter().find(|e| {
            e.full_name.eq_ignore_ascii_case(&entry_name)
                || e.file_name.eq_ignore_ascii_case(&entry_name)
        }) {
            if let Some(bytes) = extract_zip_entry_bytes(&data, found) {
                let ext = Path::new(&found.file_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_string());
                return Ok((bytes, ext));
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "ZIP entry not found",
        ));
    }
    let resolved = resolve_path_case_insensitive(path);
    let bytes = fs::read(&resolved)?;
    let ext = resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_string());
    Ok((bytes, ext))
}

/// Ermittelt die verfügbaren Laufwerksbuchstaben (`C:`, `D:`, `F:`, ...) auf dem System.
/// Nutzt unter Windows `GetLogicalDrives()` (Bitmaske ohne blockierenden Netzwerk-/Floppy-Disk-Zugriff).
pub fn list_available_drives() -> Vec<char> {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLogicalDrives() -> u32;
        }
        let mask = unsafe { GetLogicalDrives() };
        let mut drives = Vec::new();
        for i in 0..26u32 {
            if (mask & (1 << i)) != 0 {
                drives.push((b'A' + i as u8) as char);
            }
        }
        if drives.is_empty() {
            drives.push('C');
        }
        drives
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Prüft case-insensitiv, ob ein Dateiname `name` auf eine DOS-Wildcard-Maske `mask` (z.B. `*.*`, `*.XM`, `MOD*.*`) passt.
pub fn matches_wildcard(name: &str, mask: &str) -> bool {
    let m = mask.trim();
    if m.is_empty() || m == "*" || m == "*.*" {
        return true;
    }
    let s_chars: Vec<char> = name.to_ascii_uppercase().chars().collect();
    let p_chars: Vec<char> = m.to_ascii_uppercase().chars().collect();

    fn match_slice(s: &[char], p: &[char]) -> bool {
        if p.is_empty() {
            return s.is_empty();
        }
        if p[0] == '*' {
            for i in 0..=s.len() {
                if match_slice(&s[i..], &p[1..]) {
                    return true;
                }
            }
            false
        } else if p[0] == '?' {
            !s.is_empty() && match_slice(&s[1..], &p[1..])
        } else {
            !s.is_empty() && s[0] == p[0] && match_slice(&s[1..], &p[1..])
        }
    }

    match_slice(&s_chars, &p_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path_case_insensitive() {
        let tmp = std::env::temp_dir().join("m4w_case_test_dir");
        let _ = fs::create_dir_all(&tmp);
        let real_file = tmp.join("giana_s.mod");
        let _ = fs::write(&real_file, b"M.K.");

        let upper_query = tmp.join("GIANA_S.MOD");
        let resolved = resolve_path_case_insensitive(&upper_query);
        assert!(resolved.exists());
        assert!(
            resolved
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.eq_ignore_ascii_case("giana_s.mod"))
                .unwrap_or(false)
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    struct MsbBitWriter {
        bytes: Vec<u8>,
        cur_byte: u8,
        bit_cnt: u8,
    }

    impl MsbBitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                cur_byte: 0,
                bit_cnt: 0,
            }
        }

        fn write_bits(&mut self, val: u32, n: u8) {
            for i in (0..n).rev() {
                let bit = ((val >> i) & 1) as u8;
                self.cur_byte = (self.cur_byte << 1) | bit;
                self.bit_cnt += 1;
                if self.bit_cnt == 8 {
                    self.bytes.push(self.cur_byte);
                    self.cur_byte = 0;
                    self.bit_cnt = 0;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.bit_cnt > 0 {
                self.bytes.push(self.cur_byte << (8 - self.bit_cnt));
            }
            self.bytes
        }
    }

    fn build_arj_header(
        file_type: u8,
        method: u8,
        name: &str,
        comp_size: u32,
        uncomp_size: u32,
    ) -> Vec<u8> {
        let mut basic = vec![0u8; 30];
        basic[0] = 30; // first_hdr_size
        basic[1] = 6; // archiver_version
        basic[2] = 1; // min_version
        basic[3] = 0; // host_os (MS-DOS)
        basic[4] = 0x10; // arj_flags (PATHSYM)
        basic[5] = method;
        basic[6] = file_type;
        basic[12..16].copy_from_slice(&comp_size.to_le_bytes());
        basic[16..20].copy_from_slice(&uncomp_size.to_le_bytes());
        basic.extend_from_slice(name.as_bytes());
        basic.push(0); // NUL-terminated filename
        basic.push(0); // NUL-terminated comment

        let mut hdr = Vec::new();
        hdr.extend_from_slice(&[0x60, 0xEA]);
        hdr.extend_from_slice(&(basic.len() as u16).to_le_bytes());
        hdr.extend_from_slice(&basic);
        hdr.extend_from_slice(&0u32.to_le_bytes()); // CRC32
        hdr.extend_from_slice(&0u16.to_le_bytes()); // ext_size == 0
        hdr
    }

    #[test]
    fn test_arj_stored_and_method4_and_method1() {
        let orig_stored = b"M.K. ProTracker Module Test Payload!";
        // Baue einen ARJ Method-4-Stream für "ABCDABCDABCD" (4 Literale + 1 Match mit len=8, dist=4):
        // Literal 'A', 'B', 'C', 'D' -> jeweils 1 Bit 0 + 8 Bits Byte
        // Match len=8 -> c = 8 - 2 = 6 -> binär: width=2 (Bits "110"), extra 2 Bits = 6 - 3 = 3 ("11")
        // Match dist=4 -> pos = 4 - 1 = 3 -> pwidth=9 (Bit "0"), extra 9 Bits = 3
        let orig_m4 = b"ABCDABCDABCD";
        let mut bw4 = MsbBitWriter::new();
        for &b in b"ABCD" {
            bw4.write_bits(0, 1);
            bw4.write_bits(b as u32, 8);
        }
        // c = 6: width 0 (+1), width 1 (+2) -> sum=3, then 0 bit to stop at width=2, then 2 bits = 3 (3+3=6)
        bw4.write_bits(0b110, 3);
        bw4.write_bits(3, 2);
        // pos = 3: pwidth=9 -> 0 bit to stop at pwidth=9, then 9 bits = 3
        bw4.write_bits(0, 1);
        bw4.write_bits(3, 9);
        let comp_m4 = bw4.finish();

        // Baue einen ARJ Method-1 (LZH Block-Huffman) Stream für "AAAAAAA" (1 Literal 'A' + 1 Match len=6 (sym 259), dist=1 (p=0)):
        let orig_m1 = b"AAAAAAA";
        let mut bw1 = MsbBitWriter::new();
        bw1.write_bits(2, 16); // 2 Symbole im Block ('A' + Match len=6)
        // pt_for_c: n = 4 -> pt_len[0] = 1 (Bit 0), pt_len[1] = 0, pt_len[2] = 0, (i==3 -> 0 extra zeros), pt_len[3] = 1 (Bit 1)
        bw1.write_bits(4, 5);
        bw1.write_bits(1, 3); // pt_len[0] = 1
        bw1.write_bits(0, 3); // pt_len[1] = 0
        bw1.write_bits(0, 3); // pt_len[2] = 0
        bw1.write_bits(0, 2); // i==3 special: 0 extra zeros
        bw1.write_bits(1, 3); // pt_len[3] = 1
        // c_table: n = 260
        bw1.write_bits(260, 9);
        for sym in 0..260 {
            if sym == b'A' as usize || sym == 259 {
                bw1.write_bits(1, 1); // pt symbol 3 -> c_len = 1
            } else {
                bw1.write_bits(0, 1); // pt symbol 0 -> c_len = 0
            }
        }
        // p_table (pbit=5): n=0, single symbol = 0 (dist_minus_1 = 0 -> dist = 1)
        bw1.write_bits(0, 5);
        bw1.write_bits(0, 5);
        // Token 1: 'A' (erster Eintrag mit c_len=1 -> Code 0)
        bw1.write_bits(0, 1);
        // Token 2: 259 (zweiter Eintrag mit c_len=1 -> Code 1), p_table ist Single(0) (0 Bits)
        bw1.write_bits(1, 1);
        let comp_m1 = bw1.finish();

        let mut arj_bytes = Vec::new();
        // Main Archive Header (file_type = 2)
        arj_bytes.extend_from_slice(&build_arj_header(2, 0, "TEST.ARJ", 0, 0));
        // Entry 1: Stored
        arj_bytes.extend_from_slice(&build_arj_header(
            0,
            0,
            "SUBDIR/STORED.MOD",
            orig_stored.len() as u32,
            orig_stored.len() as u32,
        ));
        arj_bytes.extend_from_slice(orig_stored);
        // Entry 2: Method 4
        arj_bytes.extend_from_slice(&build_arj_header(
            0,
            4,
            "FAST.S3M",
            comp_m4.len() as u32,
            orig_m4.len() as u32,
        ));
        arj_bytes.extend_from_slice(&comp_m4);
        // Entry 3: Method 1
        arj_bytes.extend_from_slice(&build_arj_header(
            0,
            1,
            "HUFF.XM",
            comp_m1.len() as u32,
            orig_m1.len() as u32,
        ));
        arj_bytes.extend_from_slice(&comp_m1);
        // End of Archive marker
        arj_bytes.extend_from_slice(&[0x60, 0xEA, 0x00, 0x00]);

        let entries = parse_zip_entries(&arj_bytes);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].format, ArchiveFormat::Arj);
        assert_eq!(entries[0].dir_part, "SUBDIR");
        assert_eq!(entries[0].file_name, "STORED.MOD");
        assert_eq!(
            extract_zip_entry_bytes(&arj_bytes, &entries[0]).as_deref(),
            Some(&orig_stored[..])
        );
        assert_eq!(
            extract_zip_entry_bytes(&arj_bytes, &entries[1]).as_deref(),
            Some(&orig_m4[..])
        );
        assert_eq!(
            extract_zip_entry_bytes(&arj_bytes, &entries[2]).as_deref(),
            Some(&orig_m1[..])
        );
    }

    #[test]
    fn test_lha_level0_and_level1_lh5() {
        let payload_lh0 = b"Extended Module: LHA Level 0 Stored Test";
        let orig_lh5 = b"MMMMMMMM";
        let mut bw = MsbBitWriter::new();
        bw.write_bits(2, 16); // 2 Tokens
        bw.write_bits(4, 5); // pt_for_c: n = 4
        bw.write_bits(1, 3); // pt_len[0] = 1
        bw.write_bits(0, 3); // pt_len[1] = 0
        bw.write_bits(0, 3); // pt_len[2] = 0
        bw.write_bits(0, 2); // i==3 special: 0 extra zeros
        bw.write_bits(1, 3); // pt_len[3] = 1
        bw.write_bits(261, 9); // c_table: n = 261
        for sym in 0..261 {
            if sym == b'M' as usize || sym == 260 {
                bw.write_bits(1, 1); // c_len = 1
            } else {
                bw.write_bits(0, 1); // c_len = 0
            }
        }
        // p_table für -lh5- (pbit=4): n=0, single symbol = 0 (dist = 1)
        bw.write_bits(0, 4);
        bw.write_bits(0, 4);
        bw.write_bits(0, 1); // 'M'
        bw.write_bits(1, 1); // sym 260 (len = 260 - 256 + 3 = 7)
        let comp_lh5 = bw.finish();

        let mut lha = Vec::new();
        // 1. Level-0 Header (-lh0-) für "MODS\SONG1.MOD"
        let name0 = b"MODS\\SONG1.MOD";
        let hdr0_size = (20 + name0.len() + 2) as u8; // 22 Bytes Basis + name_len + 2 Bytes CRC = hdr_size + 2
        lha.push(hdr0_size);
        lha.push(0); // Checksum
        lha.extend_from_slice(b"-lh0-");
        lha.extend_from_slice(&(payload_lh0.len() as u32).to_le_bytes());
        lha.extend_from_slice(&(payload_lh0.len() as u32).to_le_bytes());
        lha.extend_from_slice(&0u32.to_le_bytes()); // Timestamp
        lha.push(0x20); // Attribute
        lha.push(0); // Level 0
        lha.push(name0.len() as u8);
        lha.extend_from_slice(name0);
        lha.extend_from_slice(&0u16.to_le_bytes()); // CRC16
        lha.extend_from_slice(payload_lh0);

        // 2. Level-1 Header (-lh5-) mit Extended Dir Header (0x02) für "AMIGA\KLISJE.MOD"
        let base_name1 = b"KLISJE.MOD";
        let ext_dir_body = b"\x02AMIGA\xff"; // 0x02 = Directory-Ext-Header, 0xFF = Separator
        let ext1_size = (ext_dir_body.len() + 2) as u16;
        let skip_size = (ext1_size as usize + comp_lh5.len()) as u32;
        let hdr1_size = (23 + base_name1.len() + 2) as u8; // base_end = hdr1_size + 2 = 22 + name_len + 2 (CRC) + 1 (OS) + 2 (first ext_size)
        lha.push(hdr1_size);
        lha.push(0);
        lha.extend_from_slice(b"-lh5-");
        lha.extend_from_slice(&skip_size.to_le_bytes());
        lha.extend_from_slice(&(orig_lh5.len() as u32).to_le_bytes());
        lha.extend_from_slice(&0u32.to_le_bytes());
        lha.push(0x20);
        lha.push(1); // Level 1
        lha.push(base_name1.len() as u8);
        lha.extend_from_slice(base_name1);
        lha.extend_from_slice(&0u16.to_le_bytes()); // CRC16
        lha.push(b'M'); // OS ID
        lha.extend_from_slice(&ext1_size.to_le_bytes()); // first ext_size
        lha.extend_from_slice(ext_dir_body);
        lha.extend_from_slice(&0u16.to_le_bytes()); // next ext_size = 0
        lha.extend_from_slice(&comp_lh5);

        // End of LHA archive marker
        lha.push(0);

        let entries = parse_zip_entries(&lha);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].format, ArchiveFormat::Lha);
        assert_eq!(entries[0].dir_part, "MODS");
        assert_eq!(entries[0].file_name, "SONG1.MOD");
        assert_eq!(
            extract_zip_entry_bytes(&lha, &entries[0]).as_deref(),
            Some(&payload_lh0[..])
        );

        assert_eq!(entries[1].format, ArchiveFormat::Lha);
        assert_eq!(entries[1].dir_part, "AMIGA");
        assert_eq!(entries[1].file_name, "KLISJE.MOD");
        assert_eq!(
            extract_zip_entry_bytes(&lha, &entries[1]).as_deref(),
            Some(&orig_lh5[..])
        );
    }
}


