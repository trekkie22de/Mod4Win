use std::env;
use std::fs;
use std::path::PathBuf;

const HELP_LANGS: &[&str] = &["de", "en", "fr", "es", "it", "nl", "grk"];

struct BitWriter {
    out: Vec<u8>,
    bit_buf: u32,
    bit_cnt: u8,
}

impl BitWriter {
    fn new(cap: usize) -> Self {
        Self {
            out: Vec::with_capacity(cap),
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, val: u32, nbits: u8) {
        self.bit_buf |= (val & ((1u32 << nbits) - 1)) << self.bit_cnt;
        self.bit_cnt += nbits;
        while self.bit_cnt >= 8 {
            self.out.push((self.bit_buf & 0xFF) as u8);
            self.bit_buf >>= 8;
            self.bit_cnt -= 8;
        }
    }

    #[inline]
    fn write_huff(&mut self, code: u16, nbits: u8) {
        let mut rev = 0u32;
        for i in 0..nbits {
            if (code & (1 << (nbits - 1 - i))) != 0 {
                rev |= 1 << i;
            }
        }
        self.write_bits(rev, nbits);
    }

    fn write_fixed_sym(&mut self, sym: u16) {
        match sym {
            0..=143 => self.write_huff(0x30 + sym, 8),
            144..=255 => self.write_huff(0x190 + (sym - 144), 9),
            256..=279 => self.write_huff(sym - 256, 7),
            280..=287 => self.write_huff(0xC0 + (sym - 280), 8),
            _ => unreachable!(),
        }
    }

    fn write_fixed_len(&mut self, len: usize) {
        const BASE: [usize; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const EXTRA: [u8; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        let mut idx = 0;
        for i in (0..29).rev() {
            if len >= BASE[i] {
                idx = i;
                break;
            }
        }
        self.write_fixed_sym(257 + idx as u16);
        let eb = EXTRA[idx];
        if eb > 0 {
            self.write_bits((len - BASE[idx]) as u32, eb);
        }
    }

    fn write_fixed_dist(&mut self, dist: usize) {
        const BASE: [usize; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
            1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
        ];
        const EXTRA: [u8; 30] = [
            0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
            12, 13, 13,
        ];
        let mut idx = 0;
        for i in (0..30).rev() {
            if dist >= BASE[i] {
                idx = i;
                break;
            }
        }
        self.write_huff(idx as u16, 5);
        let eb = EXTRA[idx];
        if eb > 0 {
            self.write_bits((dist - BASE[idx]) as u32, eb);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_cnt > 0 {
            self.out.push((self.bit_buf & 0xFF) as u8);
        }
        self.out
    }
}

/// Compress `data` into a raw RFC-1951 Deflate stream using Fixed Huffman codes + LZ77 hash chains.
fn deflate_compress_fixed(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new(data.len() / 2 + 128);
    // 4-byte little-endian uncompressed length header expected by WinHelpBook::decompress_embedded_help_html
    bw.out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    // Single final block (BFINAL=1) with Fixed Huffman (BTYPE=01)
    bw.write_bits(1, 1);
    bw.write_bits(1, 2);

    const HASH_BITS: usize = 15;
    const HASH_SIZE: usize = 1 << HASH_BITS;
    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut prev = vec![usize::MAX; data.len()];

    let hash3 = |p: usize| -> usize {
        (((data[p] as usize) << 10) ^ ((data[p + 1] as usize) << 5) ^ (data[p + 2] as usize))
            & (HASH_SIZE - 1)
    };

    let mut pos = 0;
    while pos < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;

        if pos + 3 <= data.len() {
            let h = hash3(pos);
            let mut cur = head[h];
            prev[pos] = cur;
            head[h] = pos;

            let max_len = (data.len() - pos).min(258);
            let mut chain_steps = 0;
            while cur != usize::MAX && pos - cur <= 32768 && chain_steps < 96 {
                chain_steps += 1;
                if data[cur + best_len] == data[pos + best_len] {
                    let mut l = 0;
                    while l < max_len && data[cur + l] == data[pos + l] {
                        l += 1;
                    }
                    if l >= 3 && l > best_len {
                        best_len = l;
                        best_dist = pos - cur;
                        if l == max_len {
                            break;
                        }
                    }
                }
                cur = prev[cur];
            }
        }

        if best_len >= 3 {
            bw.write_fixed_len(best_len);
            bw.write_fixed_dist(best_dist);
            for step in 1..best_len {
                let p = pos + step;
                if p + 3 <= data.len() {
                    let h = hash3(p);
                    prev[p] = head[h];
                    head[h] = p;
                }
            }
            pos += best_len;
        } else {
            bw.write_fixed_sym(data[pos] as u16);
            pos += 1;
        }
    }

    // End of block (256)
    bw.write_fixed_sym(256);
    bw.finish()
}

fn push_res_record(
    out: &mut Vec<u8>,
    type_ordinal: u16,
    name_ordinal: u16,
    memory_flags: u16,
    lang_id: u16,
    data: &[u8],
) {
    let data_size = data.len() as u32;
    let header_size = 32u32;
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    // Type ordinal: 0xFFFF followed by u16 ordinal
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&type_ordinal.to_le_bytes());
    // Name ordinal: 0xFFFF followed by u16 ordinal
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name_ordinal.to_le_bytes());
    // DataVersion (u32), MemoryFlags (u16), LanguageId (u16), Version (u32), Characteristics (u32)
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&lang_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    // Resource payload padded to 4-byte DWORD alignment
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn main() {
    println!("cargo:rerun-if-changed=assets/MOD4WIN.ico");
    println!("cargo:rerun-if-changed=help");
    println!("cargo:rerun-if-changed=src");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // 1. Sync & compress all 7 language help HTML files into OUT_DIR/help_*.deflate
    let crate_help_dir = manifest_dir.join("help");
    let root_help_dir = manifest_dir.parent().map(|p| p.join("help"));

    for lang in HELP_LANGS {
        let filename = format!("help_{}.html", lang);
        let crate_path = crate_help_dir.join(&filename);
        println!("cargo:rerun-if-changed={}", crate_path.display());

        let mut src_path = crate_path.clone();
        if let Some(ref rdir) = root_help_dir {
            let root_path = rdir.join(&filename);
            println!("cargo:rerun-if-changed={}", root_path.display());
            if let (Ok(m_crate), Ok(m_root)) = (fs::metadata(&crate_path), fs::metadata(&root_path))
                && let (Ok(t_crate), Ok(t_root)) = (m_crate.modified(), m_root.modified()) {
                    if t_root > t_crate {
                        let _ = fs::copy(&root_path, &crate_path);
                        src_path = root_path;
                    } else if t_crate > t_root {
                        let _ = fs::copy(&crate_path, &root_path);
                    }
                }
        }

        let html_bytes = fs::read(&src_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", src_path.display(), e));
        let compressed = deflate_compress_fixed(&html_bytes);
        let out_path = out_dir.join(format!("help_{}.deflate", lang));
        fs::write(&out_path, &compressed)
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", out_path.display(), e));
    }

    // 2. Windows-only: stack size & MOD4WIN.ico application icon resource
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        println!("cargo:rustc-link-arg=/STACK:16777216");
    } else if target_env == "gnu" {
        println!("cargo:rustc-link-arg=-Wl,--stack,16777216");
    }

    let ico_path = manifest_dir.join("assets").join("MOD4WIN.ico");
    let Ok(ico_bytes) = fs::read(&ico_path) else {
        return;
    };
    if ico_bytes.len() < 22 {
        return;
    }

    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if count == 0 || ico_bytes.len() < 6 + count * 16 {
        return;
    }

    let mut res = Vec::with_capacity(1024);
    // 1. 32-byte empty resource file header marker
    push_res_record(&mut res, 0, 0, 0, 0, &[]);

    // 2. RT_ICON (Type 3) records + build RT_GROUP_ICON (Type 14) directory
    let mut grp_data = Vec::with_capacity(6 + count * 14);
    grp_data.extend_from_slice(&0u16.to_le_bytes()); // idReserved = 0
    grp_data.extend_from_slice(&1u16.to_le_bytes()); // idType = 1 (ICON)
    grp_data.extend_from_slice(&(count as u16).to_le_bytes());

    for idx in 0..count {
        let entry_off = 6 + idx * 16;
        let width = ico_bytes[entry_off];
        let height = ico_bytes[entry_off + 1];
        let color_count = ico_bytes[entry_off + 2];
        let bytes_in_res = u32::from_le_bytes([
            ico_bytes[entry_off + 8],
            ico_bytes[entry_off + 9],
            ico_bytes[entry_off + 10],
            ico_bytes[entry_off + 11],
        ]) as usize;
        let img_off = u32::from_le_bytes([
            ico_bytes[entry_off + 12],
            ico_bytes[entry_off + 13],
            ico_bytes[entry_off + 14],
            ico_bytes[entry_off + 15],
        ]) as usize;

        if img_off + bytes_in_res > ico_bytes.len() || bytes_in_res < 40 {
            continue;
        }
        let img_slice = &ico_bytes[img_off..img_off + bytes_in_res];
        let planes = u16::from_le_bytes([img_slice[12], img_slice[13]]).max(1);
        let bit_count = u16::from_le_bytes([img_slice[14], img_slice[15]]).max(4);
        let icon_id = (idx + 1) as u16;

        // RT_ICON = 3
        push_res_record(&mut res, 3, icon_id, 0x1010, 0x0409, img_slice);

        // GRPICONDIRENTRY (14 bytes)
        grp_data.push(width);
        grp_data.push(height);
        grp_data.push(color_count);
        grp_data.push(0); // bReserved = 0
        grp_data.extend_from_slice(&planes.to_le_bytes());
        grp_data.extend_from_slice(&bit_count.to_le_bytes());
        grp_data.extend_from_slice(&(bytes_in_res as u32).to_le_bytes());
        grp_data.extend_from_slice(&icon_id.to_le_bytes());
    }

    // 3. RT_GROUP_ICON = 14, Name Ordinal = 1 (Main EXE icon)
    push_res_record(&mut res, 14, 1, 0x1030, 0x0409, &grp_data);

    let res_path = out_dir.join("mod4win_icon.res");
    if fs::write(&res_path, &res).is_ok() {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
    }
}
