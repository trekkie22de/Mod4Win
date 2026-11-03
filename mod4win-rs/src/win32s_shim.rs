//! 486 / Windows 3.11 (Win32s 1.30c) / Windows 95 Runtime- & API-Shim (`retro-win32`).
//!
//! Erfüllt alle drei Low-Level-Anforderungen für ein CRT-freies (`/NODEFAULTLIB`) 32-Bit-Binary
//! (`i686-win7-windows-msvc` mit `-C target-cpu=i486 -C target-feature=+x87,-mmx,-sse,-sse2`):
//! 1. **FPU-Intrinsics (`sin`, `cos`, `powf`, `fmod`, `sqrt`, `round`, `floor`, `ceil`):**
//!    Direkt über Inline-x87-Assembler (`fsin`, `fcos`, `fyl2x`, `f2xm1`, `fscale`, `fprem`, `fsqrt`, `frndint`)
//!    ohne Importe aus `MSVCRT.DLL` oder `VCRUNTIME140.DLL`.
//! 2. **Stack-Probe (`__chkstk` / `_alloca_probe`) & Speicher-Intrinsics (`memcpy`, `memset`, `memmove`, `memcmp`):**
//!    Direkt im eigenen Binary bereitgestellt (inkl. 64-Bit-Integer-Helfer `_allmul`, `_aulldiv`, `_allshr` etc.).
//! 3. **Win32s 1.30c / Windows 95 ANSI (`*A`) Polyfill-Importtabelle (`__imp__...`):**
//!    Fängt alle modernen NT-/Unicode-Aufrufe der Rust-Standardbibliothek (`NtReadFile`, `NtWriteFile`,
//!    `CreateFileW`, `FindFirstFileExW`, `GetFileAttributesW`, `GetEnvironmentVariableW`, `SystemFunction036` etc.)
//!    ab und übersetzt sie auf authentische 1993er `KERNEL32.DLL` ANSI-APIs (`CreateFileA`, `FindFirstFileA`,
//!    `ReadFile`, `WriteFile`, `GetTickCount`), inklusive `FILE_SHARE_DELETE`-Maskierung und virtuellem
//!    Verzeichnis-Metadaten-Handle für `std::fs::metadata` / `Path::is_dir`.

#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    suspicious_runtime_symbol_definitions,
    clippy::missing_safety_doc
)]

#[cfg(target_arch = "x86")]
mod x86_retro_impl {
    use core::arch::{asm, global_asm};
    use core::ffi::c_void;
    use core::ptr;
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    type HANDLE = isize;
    type HMODULE = isize;
    type BOOL = i32;
    type DWORD = u32;
    type WORD = u16;
    type LONG = i32;
    type ULONG = u32;
    type NTSTATUS = i32;

    const INVALID_HANDLE_VALUE: HANDLE = -1;
    const INVALID_FILE_ATTRIBUTES: DWORD = 0xFFFF_FFFF;
    const FILE_ATTRIBUTE_DIRECTORY: DWORD = 0x0000_0010;
    const FILE_ATTRIBUTE_NORMAL: DWORD = 0x0000_0080;
    const ERROR_FILE_NOT_FOUND: DWORD = 2;
    const ERROR_NO_MORE_FILES: DWORD = 18;
    const ERROR_CALL_NOT_IMPLEMENTED: DWORD = 120;

    // =========================================================================
    // AUTHENTISCHE 1993 WIN32S 1.30C / WINDOWS 95 KERNEL32.DLL ANSI IMPORTE
    // =========================================================================
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessHeap() -> HANDLE;
        fn HeapAlloc(h_heap: HANDLE, dw_flags: DWORD, dw_bytes: usize) -> *mut c_void;
        fn HeapReAlloc(
            h_heap: HANDLE,
            dw_flags: DWORD,
            lp_mem: *mut c_void,
            dw_bytes: usize,
        ) -> *mut c_void;
        fn HeapFree(h_heap: HANDLE, dw_flags: DWORD, lp_mem: *mut c_void) -> BOOL;

        fn ExitProcess(u_exit_code: u32) -> !;
        fn GetLastError() -> DWORD;
        fn SetLastError(dw_err_code: DWORD);
        fn GetTickCount() -> DWORD;
        fn Sleep(dw_milliseconds: DWORD);
        fn GetCurrentProcessId() -> DWORD;
        fn GetCurrentThreadId() -> DWORD;

        fn GetCommandLineA() -> *const u8;
        fn GetEnvironmentVariableA(
            lp_name: *const u8,
            lp_buffer: *mut u8,
            n_size: DWORD,
        ) -> DWORD;
        fn SetEnvironmentVariableA(lp_name: *const u8, lp_value: *const u8) -> BOOL;
        fn GetEnvironmentStrings() -> *mut u8;

        fn GetModuleHandleA(lp_module_name: *const u8) -> HMODULE;
        fn GetModuleFileNameA(h_module: HMODULE, lp_filename: *mut u8, n_size: DWORD) -> DWORD;
        fn LoadLibraryA(lp_lib_file_name: *const u8) -> HMODULE;
        fn FreeLibrary(h_lib_module: HMODULE) -> BOOL;
        fn GetProcAddress(h_module: HMODULE, lp_proc_name: *const u8) -> *mut c_void;

        fn CreateFileA(
            lp_file_name: *const u8,
            dw_desired_access: DWORD,
            dw_share_mode: DWORD,
            lp_security_attributes: *mut c_void,
            dw_creation_disposition: DWORD,
            dw_flags_and_attributes: DWORD,
            h_template_file: HANDLE,
        ) -> HANDLE;
        fn ReadFile(
            h_file: HANDLE,
            lp_buffer: *mut c_void,
            n_number_of_bytes_to_read: DWORD,
            lp_number_of_bytes_read: *mut DWORD,
            lp_overlapped: *mut c_void,
        ) -> BOOL;
        fn WriteFile(
            h_file: HANDLE,
            lp_buffer: *const c_void,
            n_number_of_bytes_to_write: DWORD,
            lp_number_of_bytes_written: *mut DWORD,
            lp_overlapped: *mut c_void,
        ) -> BOOL;
        fn GetFileSize(h_file: HANDLE, lp_file_size_high: *mut DWORD) -> DWORD;
        fn SetFilePointer(
            h_file: HANDLE,
            l_distance_to_move: LONG,
            lp_distance_to_move_high: *mut LONG,
            dw_move_method: DWORD,
        ) -> DWORD;
        fn SetEndOfFile(h_file: HANDLE) -> BOOL;
        fn FlushFileBuffers(h_file: HANDLE) -> BOOL;
        fn GetFileType(h_file: HANDLE) -> DWORD;

        fn GetFileAttributesA(lp_file_name: *const u8) -> DWORD;
        fn SetFileAttributesA(lp_file_name: *const u8, dw_file_attributes: DWORD) -> BOOL;
        fn DeleteFileA(lp_file_name: *const u8) -> BOOL;
        fn MoveFileA(lp_existing_file_name: *const u8, lp_new_file_name: *const u8) -> BOOL;
        fn CopyFileA(
            lp_existing_file_name: *const u8,
            lp_new_file_name: *const u8,
            b_fail_if_exists: BOOL,
        ) -> BOOL;
        fn CreateDirectoryA(
            lp_path_name: *const u8,
            lp_security_attributes: *mut c_void,
        ) -> BOOL;
        fn RemoveDirectoryA(lp_path_name: *const u8) -> BOOL;
        fn GetCurrentDirectoryA(n_buffer_length: DWORD, lp_buffer: *mut u8) -> DWORD;
        fn SetCurrentDirectoryA(lp_path_name: *const u8) -> BOOL;
        fn GetFullPathNameA(
            lp_file_name: *const u8,
            n_buffer_length: DWORD,
            lp_buffer: *mut u8,
            lp_file_part: *mut *mut u8,
        ) -> DWORD;
        fn GetTempPathA(n_buffer_length: DWORD, lp_buffer: *mut u8) -> DWORD;

        fn FindFirstFileA(
            lp_file_name: *const u8,
            lp_find_file_data: *mut Win32FindDataA,
        ) -> HANDLE;
        fn FindNextFileA(
            h_find_file: HANDLE,
            lp_find_file_data: *mut Win32FindDataA,
        ) -> BOOL;
        fn FindClose(h_find_file: HANDLE) -> BOOL;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxA(
            h_wnd: HANDLE,
            lp_text: *const u8,
            lp_caption: *const u8,
            u_type: u32,
        ) -> i32;
    }

    pub fn show_retro_panic_and_exit(msg: &str) -> ! {
        let mut buf = [0u8; 512];
        let bytes = msg.as_bytes();
        let copy_len = bytes.len().min(buf.len() - 1);
        for i in 0..copy_len {
            buf[i] = if bytes[i] == 0 { b' ' } else { bytes[i] };
        }
        buf[copy_len] = 0;
        unsafe {
            MessageBoxA(
                0,
                buf.as_ptr(),
                b"MOD4WIN32 Laufzeitfehler\0".as_ptr(),
                0x0000_0010, // MB_OK | MB_ICONHAND
            );
            ExitProcess(1);
        }
    }

    // =========================================================================
    // 1. REQUIREMENT #1: 486 x87 FPU INTRINSICS (OHNE MSVCRT.DLL!)
    // =========================================================================

    #[unsafe(no_mangle)]
    pub static _fltused: i32 = 1;

    #[inline]
    unsafe fn x87_sin(x: f64) -> f64 {
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fsin",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                res = in(reg) &mut res,
                options(nostack)
            );
        }
        res
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sin(x: f64) -> f64 {
        unsafe { x87_sin(x) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sinf(x: f32) -> f32 {
        unsafe { x87_sin(x as f64) as f32 }
    }

    #[inline]
    unsafe fn x87_cos(x: f64) -> f64 {
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fcos",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                res = in(reg) &mut res,
                options(nostack)
            );
        }
        res
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cos(x: f64) -> f64 {
        unsafe { x87_cos(x) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cosf(x: f32) -> f32 {
        unsafe { x87_cos(x as f64) as f32 }
    }

    #[inline]
    unsafe fn x87_sqrt(x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fsqrt",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                res = in(reg) &mut res,
                options(nostack)
            );
        }
        res
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sqrt(x: f64) -> f64 {
        unsafe { x87_sqrt(x) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sqrtf(x: f32) -> f32 {
        unsafe { x87_sqrt(x as f64) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn fabs(x: f64) -> f64 {
        f64::from_bits(x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn fabsf(x: f32) -> f32 {
        f32::from_bits(x.to_bits() & 0x7FFF_FFFF)
    }

    /// x87 `2^x` Helfer via `frndint`, `f2xm1` und `fscale`
    #[inline]
    unsafe fn x87_exp2(x: f64) -> f64 {
        if x.is_nan() {
            return x;
        }
        if x >= 1024.0 {
            return f64::INFINITY;
        }
        if x <= -1075.0 {
            return 0.0;
        }
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{x}]",
                "fld st(0)",
                "frndint",
                "fsub st(1), st(0)",
                "fxch st(1)",
                "f2xm1",
                "fld1",
                "faddp st(1), st(0)",
                "fscale",
                "fstp st(1)",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                res = in(reg) &mut res,
                options(nostack)
            );
        }
        res
    }

    /// x87 `y * log2(x)` Helfer via `fyl2x` (`x > 0`)
    #[inline]
    unsafe fn x87_yl2x(x: f64, y: f64) -> f64 {
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{y}]",
                "fld qword ptr [{x}]",
                "fyl2x",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                y = in(reg) &y,
                res = in(reg) &mut res,
                options(nostack)
            );
        }
        res
    }

    #[inline]
    unsafe fn x87_pow(base: f64, exp: f64) -> f64 {
        if exp == 0.0 {
            return 1.0;
        }
        if base == 1.0 {
            return 1.0;
        }
        if base.is_nan() || exp.is_nan() {
            return f64::NAN;
        }
        if base == 0.0 {
            return if exp > 0.0 { 0.0 } else { f64::INFINITY };
        }
        if base > 0.0 {
            unsafe {
                let p = x87_yl2x(base, exp);
                return x87_exp2(p);
            }
        }
        // Negative Basis: nur für ganzzahlige Exponenten definiert
        let exp_trunc = unsafe { x87_round_with_cw(exp, 0x0C00) };
        if exp_trunc != exp {
            return f64::NAN;
        }
        let abs_base = f64::from_bits(base.to_bits() & 0x7FFF_FFFF_FFFF_FFFF);
        let mag = unsafe { x87_exp2(x87_yl2x(abs_base, exp)) };
        let is_odd = ((exp_trunc as i64) & 1) != 0;
        if is_odd { -mag } else { mag }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn pow(base: f64, exp: f64) -> f64 {
        unsafe { x87_pow(base, exp) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn powf(base: f32, exp: f32) -> f32 {
        unsafe { x87_pow(base as f64, exp as f64) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn exp2(x: f64) -> f64 {
        unsafe { x87_exp2(x) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn exp2f(x: f32) -> f32 {
        unsafe { x87_exp2(x as f64) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn exp(x: f64) -> f64 {
        const LOG2_E: f64 = core::f64::consts::LOG2_E;
        unsafe { x87_exp2(x * LOG2_E) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn expf(x: f32) -> f32 {
        const LOG2_E: f64 = core::f64::consts::LOG2_E;
        unsafe { x87_exp2((x as f64) * LOG2_E) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn log2(x: f64) -> f64 {
        if x <= 0.0 {
            return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
        }
        unsafe { x87_yl2x(x, 1.0) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn log2f(x: f32) -> f32 {
        let xd = x as f64;
        if xd <= 0.0 {
            return if xd == 0.0 { f32::NEG_INFINITY } else { f32::NAN };
        }
        unsafe { x87_yl2x(xd, 1.0) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn log(x: f64) -> f64 {
        if x <= 0.0 {
            return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
        }
        const LN_2: f64 = core::f64::consts::LN_2;
        unsafe { x87_yl2x(x, LN_2) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn logf(x: f32) -> f32 {
        let xd = x as f64;
        if xd <= 0.0 {
            return if xd == 0.0 { f32::NEG_INFINITY } else { f32::NAN };
        }
        const LN_2: f64 = core::f64::consts::LN_2;
        unsafe { x87_yl2x(xd, LN_2) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn log10(x: f64) -> f64 {
        if x <= 0.0 {
            return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
        }
        const LOG10_2: f64 = core::f64::consts::LOG10_2;
        unsafe { x87_yl2x(x, LOG10_2) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn log10f(x: f32) -> f32 {
        let xd = x as f64;
        if xd <= 0.0 {
            return if xd == 0.0 { f32::NEG_INFINITY } else { f32::NAN };
        }
        const LOG10_2: f64 = core::f64::consts::LOG10_2;
        unsafe { x87_yl2x(xd, LOG10_2) as f32 }
    }

    #[inline]
    unsafe fn x87_fmod(x: f64, y: f64) -> f64 {
        if y == 0.0 || x.is_nan() || y.is_nan() || x.is_infinite() {
            return f64::NAN;
        }
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fld qword ptr [{y}]",
                "fld qword ptr [{x}]",
                "2:",
                "fprem",
                "fnstsw ax",
                "test ah, 4",
                "jnz 2b",
                "fstp st(1)",
                "fstp qword ptr [{res}]",
                x = in(reg) &x,
                y = in(reg) &y,
                res = in(reg) &mut res,
                out("ax") _,
                options(nostack)
            );
        }
        res
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn fmod(x: f64, y: f64) -> f64 {
        unsafe { x87_fmod(x, y) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn fmodf(x: f32, y: f32) -> f32 {
        unsafe { x87_fmod(x as f64, y as f64) as f32 }
    }

    /// x87 Rundung mit spezifischem Control-Word-Rounding-Mode (`rc_bits`: `0x0400`=floor, `0x0800`=ceil, `0x0C00`=trunc)
    #[inline]
    unsafe fn x87_round_with_cw(x: f64, rc_bits: u16) -> f64 {
        let mut old_cw: u16 = 0;
        let mut new_cw: u16 = 0;
        let mut res: f64 = 0.0;
        unsafe {
            asm!(
                "fnstcw word ptr [{old_cw}]",
                "mov ax, word ptr [{old_cw}]",
                "and ax, 0xF3FF",
                "or ax, {rc:x}",
                "mov word ptr [{new_cw}], ax",
                "fldcw word ptr [{new_cw}]",
                "fld qword ptr [{x}]",
                "frndint",
                "fstp qword ptr [{res}]",
                "fldcw word ptr [{old_cw}]",
                old_cw = in(reg) &mut old_cw,
                new_cw = in(reg) &mut new_cw,
                x = in(reg) &x,
                res = in(reg) &mut res,
                rc = in(reg) rc_bits,
                out("ax") _,
                options(nostack)
            );
        }
        res
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn floor(x: f64) -> f64 {
        unsafe { x87_round_with_cw(x, 0x0400) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn floorf(x: f32) -> f32 {
        unsafe { x87_round_with_cw(x as f64, 0x0400) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn ceil(x: f64) -> f64 {
        unsafe { x87_round_with_cw(x, 0x0800) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn ceilf(x: f32) -> f32 {
        unsafe { x87_round_with_cw(x as f64, 0x0800) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trunc(x: f64) -> f64 {
        unsafe { x87_round_with_cw(x, 0x0C00) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn truncf(x: f32) -> f32 {
        unsafe { x87_round_with_cw(x as f64, 0x0C00) as f32 }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn round(x: f64) -> f64 {
        if x >= 0.0 {
            unsafe { x87_round_with_cw(x + 0.5, 0x0400) }
        } else {
            unsafe { x87_round_with_cw(x - 0.5, 0x0800) }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn roundf(x: f32) -> f32 {
        let xd = x as f64;
        if xd >= 0.0 {
            unsafe { x87_round_with_cw(xd + 0.5, 0x0400) as f32 }
        } else {
            unsafe { x87_round_with_cw(xd - 0.5, 0x0800) as f32 }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rint(x: f64) -> f64 {
        unsafe { x87_round_with_cw(x, 0x0000) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rintf(x: f32) -> f32 {
        unsafe { x87_round_with_cw(x as f64, 0x0000) as f32 }
    }

    // MSVC x87 Register-Intrinsics (`_CIsin`, `_CIcos`, `_CIsqrt`, `_CIfmod`, `_CIpow`, `_CIlog`, `_CIlog10`, `_CIexp`)
    // Argumente liegen bereits in `st(0)` (bzw. `st(1)` = Basis, `st(0)` = Exponent bei `_CIpow`).
    global_asm!(
        ".global __CIsin",
        "__CIsin:",
        "fsin",
        "ret",

        ".global __CIcos",
        "__CIcos:",
        "fcos",
        "ret",

        ".global __CIsqrt",
        "__CIsqrt:",
        "fsqrt",
        "ret",

        ".global __CIfmod",
        "__CIfmod:",
        "2:",
        "fprem",
        "fnstsw ax",
        "test ah, 4",
        "jnz 2b",
        "fstp st(1)",
        "ret",

        ".global __CIpow",
        "__CIpow:",
        "sub esp, 16",
        "fstp qword ptr [esp + 8]",
        "fstp qword ptr [esp]",
        "call _pow",
        "add esp, 16",
        "ret",

        ".global __CIlog",
        "__CIlog:",
        "sub esp, 8",
        "fstp qword ptr [esp]",
        "call _log",
        "add esp, 8",
        "ret",

        ".global __CIlog10",
        "__CIlog10:",
        "sub esp, 8",
        "fstp qword ptr [esp]",
        "call _log10",
        "add esp, 8",
        "ret",

        ".global __CIexp",
        "__CIexp:",
        "sub esp, 8",
        "fstp qword ptr [esp]",
        "call _exp",
        "add esp, 8",
        "ret",
    );

    // =========================================================================
    // 2. REQUIREMENT #2: STACK PROBE (`__chkstk` / `_alloca_probe`) & MEMORY INTRINSICS
    // =========================================================================

    global_asm!(
        ".global __chkstk",
        ".global __alloca_probe",
        ".global __alloca_probe_16",
        "__chkstk:",
        "__alloca_probe:",
        "push ecx",
        "lea ecx, [esp + 8]",
        "cmp eax, 4096",
        "jb 3f",
        "2:",
        "sub ecx, 4096",
        "test dword ptr [ecx], eax",
        "sub eax, 4096",
        "cmp eax, 4096",
        "jae 2b",
        "3:",
        "sub ecx, eax",
        "test dword ptr [ecx], eax",
        "mov eax, esp",
        "mov esp, ecx",
        "mov ecx, dword ptr [eax]",
        "mov eax, dword ptr [eax + 4]",
        "push eax",
        "ret",
        "__alloca_probe_16:",
        "push ecx",
        "lea ecx, [esp + 8]",
        "sub ecx, eax",
        "and ecx, 15",
        "add eax, ecx",
        "pop ecx",
        "jmp __chkstk",
    );

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
        let mut i = 0usize;
        while i + 4 <= n {
            unsafe {
                ptr::write_unaligned(
                    dest.add(i) as *mut u32,
                    ptr::read_unaligned(src.add(i) as *const u32),
                );
            }
            i += 4;
        }
        while i < n {
            unsafe {
                *dest.add(i) = *src.add(i);
            }
            i += 1;
        }
        dest
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
        if (dest as usize) <= (src as usize) || (dest as usize) >= (src as usize).wrapping_add(n) {
            unsafe { memcpy(dest, src, n) }
        } else {
            let mut i = n;
            while i > 0 {
                i -= 1;
                unsafe {
                    *dest.add(i) = *src.add(i);
                }
            }
            dest
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
        let b = c as u8;
        let b4 = (b as u32) * 0x0101_0101;
        let mut i = 0usize;
        while i + 4 <= n {
            unsafe {
                ptr::write_unaligned(dest.add(i) as *mut u32, b4);
            }
            i += 4;
        }
        while i < n {
            unsafe {
                *dest.add(i) = b;
            }
            i += 1;
        }
        dest
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
        let mut i = 0usize;
        while i < n {
            let a = unsafe { *s1.add(i) };
            let b = unsafe { *s2.add(i) };
            if a != b {
                return (a as i32) - (b as i32);
            }
            i += 1;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
        if s.is_null() {
            return 0;
        }
        let mut len = 0usize;
        while unsafe { *s.add(len) } != 0 {
            len += 1;
        }
        len
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wcslen(s: *const u16) -> usize {
        if s.is_null() {
            return 0;
        }
        let mut len = 0usize;
        while unsafe { *s.add(len) } != 0 {
            len += 1;
        }
        len
    }

    // 32-Bit x86 MSVC 64-Bit-Integer-Helfer (`_allmul`, `_aulldiv`, `_aullrem`, `_alldiv`, `_allrem`, `_allshl`, `_allshr`, `_aullshr`)
    global_asm!(
        ".global __allshl",
        "__allshl:",
        "cmp cl, 64",
        "jae 2f",
        "cmp cl, 32",
        "jae 1f",
        "shld edx, eax, cl",
        "shl eax, cl",
        "ret",
        "1:",
        "mov edx, eax",
        "xor eax, eax",
        "and cl, 31",
        "shl edx, cl",
        "ret",
        "2:",
        "xor eax, eax",
        "xor edx, edx",
        "ret",

        ".global __allshr",
        "__allshr:",
        "cmp cl, 64",
        "jae 2f",
        "cmp cl, 32",
        "jae 1f",
        "shrd eax, edx, cl",
        "sar edx, cl",
        "ret",
        "1:",
        "mov eax, edx",
        "sar edx, 31",
        "and cl, 31",
        "sar eax, cl",
        "ret",
        "2:",
        "sar edx, 31",
        "mov eax, edx",
        "ret",

        ".global __aullshr",
        "__aullshr:",
        "cmp cl, 64",
        "jae 2f",
        "cmp cl, 32",
        "jae 1f",
        "shrd eax, edx, cl",
        "shr edx, cl",
        "ret",
        "1:",
        "mov eax, edx",
        "xor edx, edx",
        "and cl, 31",
        "shr eax, cl",
        "ret",
        "2:",
        "xor eax, eax",
        "xor edx, edx",
        "ret",

        ".global __allmul",
        "__allmul:",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "call _rust_allmul",
        "add esp, 16",
        "ret 16",

        ".global __aulldiv",
        "__aulldiv:",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "call _rust_aulldiv",
        "add esp, 16",
        "ret 16",

        ".global __aullrem",
        "__aullrem:",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "call _rust_aullrem",
        "add esp, 16",
        "ret 16",

        ".global __alldiv",
        "__alldiv:",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "call _rust_alldiv",
        "add esp, 16",
        "ret 16",

        ".global __allrem",
        "__allrem:",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "call _rust_allrem",
        "add esp, 16",
        "ret 16",
    );

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rust_allmul(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
        let a = ((a_hi as u64) << 32) | (a_lo as u64);
        let b = ((b_hi as u64) << 32) | (b_lo as u64);
        a.wrapping_mul(b)
    }

    fn udivrem64(mut num: u64, den: u64) -> (u64, u64) {
        if den == 0 {
            return (u64::MAX, 0);
        }
        if den > num {
            return (0, num);
        }
        let mut quot = 0u64;
        let mut bit = 1u64;
        let mut d = den;
        while (d >> 63) == 0 && d <= (num >> 1) {
            d <<= 1;
            bit <<= 1;
        }
        while bit != 0 {
            if num >= d {
                num -= d;
                quot |= bit;
            }
            d >>= 1;
            bit >>= 1;
        }
        (quot, num)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rust_aulldiv(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
        let a = ((a_hi as u64) << 32) | (a_lo as u64);
        let b = ((b_hi as u64) << 32) | (b_lo as u64);
        udivrem64(a, b).0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rust_aullrem(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
        let a = ((a_hi as u64) << 32) | (a_lo as u64);
        let b = ((b_hi as u64) << 32) | (b_lo as u64);
        udivrem64(a, b).1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rust_alldiv(a_lo: u32, a_hi: i32, b_lo: u32, b_hi: i32) -> i64 {
        let a = ((a_hi as i64) << 32) | (a_lo as u64 as i64);
        let b = ((b_hi as i64) << 32) | (b_lo as u64 as i64);
        let neg = (a < 0) ^ (b < 0);
        let ua = a.unsigned_abs();
        let ub = b.unsigned_abs();
        let q = udivrem64(ua, ub).0 as i64;
        if neg { q.wrapping_neg() } else { q }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rust_allrem(a_lo: u32, a_hi: i32, b_lo: u32, b_hi: i32) -> i64 {
        let a = ((a_hi as i64) << 32) | (a_lo as u64 as i64);
        let b = ((b_hi as i64) << 32) | (b_lo as u64 as i64);
        let neg = a < 0;
        let ua = a.unsigned_abs();
        let ub = b.unsigned_abs();
        let r = udivrem64(ua, ub).1 as i64;
        if neg { r.wrapping_neg() } else { r }
    }

    // 80486 64-Bit-Atomics-Ersatzfunktionen (da der 486er kein Pentium `CMPXCHG8B` besitzt!)
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __atomic_load_8(src: *const u64, _ordering: i32) -> u64 {
        unsafe { ptr::read_volatile(src) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __atomic_store_8(dst: *mut u64, val: u64, _ordering: i32) {
        unsafe { ptr::write_volatile(dst, val) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __atomic_exchange_8(dst: *mut u64, val: u64, _ordering: i32) -> u64 {
        unsafe {
            let old = ptr::read_volatile(dst);
            ptr::write_volatile(dst, val);
            old
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __atomic_compare_exchange_8(
        dst: *mut u64,
        expected: *mut u64,
        desired: u64,
        _weak: bool,
        _success_order: i32,
        _failure_order: i32,
    ) -> bool {
        unsafe {
            let cur = ptr::read_volatile(dst);
            let exp = ptr::read_volatile(expected);
            if cur == exp {
                ptr::write_volatile(dst, desired);
                true
            } else {
                ptr::write_volatile(expected, cur);
                false
            }
        }
    }

    // =========================================================================
    // CRT STARTUP & MINIMALE RUNTIME-SYMBOLE FÜR `/NODEFAULTLIB`
    // =========================================================================

    unsafe extern "C" {
        fn main(argc: i32, argv: *const *const u8) -> i32;
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn WinMainCRTStartup() -> ! {
        // Initialisiert die 80486 x87 FPU wie MSVC `_fpmath` (53-Bit Double-Präzision,
        // Round-to-Nearest, alle FPU-Exceptions maskiert: Control Word = 0x027F),
        // damit unsaubere 16-Bit-Nachbarprogramme unter Windows 3.11 (Win32s) keine
        // FPU-Exceptions oder abweichende Rundungsmodi hinterlassen.
        let cw: u16 = 0x027F;
        unsafe {
            asm!(
                "fninit",
                "fldcw word ptr [{cw}]",
                cw = in(reg) &cw,
                options(nostack)
            );
        }
        let dummy_arg = b"MOD4WIN32.EXE\0".as_ptr();
        let argv = [dummy_arg, ptr::null()];
        let code = unsafe { main(1, argv.as_ptr()) };
        unsafe { ExitProcess(code as u32) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn mainCRTStartup() -> ! {
        unsafe { WinMainCRTStartup() }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn atexit(_func: Option<unsafe extern "C" fn()>) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn _atexit(_func: Option<unsafe extern "C" fn()>) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __CxxFrameHandler3() -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "stdcall" fn _CxxThrowException(_p_exc: *mut c_void, _p_info: *mut c_void) -> ! {
        unsafe { ExitProcess(3) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __std_terminate() -> ! {
        unsafe { ExitProcess(3) }
    }

    // =========================================================================
    // 3. WIN32S 1.30C & WINDOWS 95 ANSI (*A) POLYFILL SHIMS
    // =========================================================================

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct FileTime {
        pub dw_low_date_time: DWORD,
        pub dw_high_date_time: DWORD,
    }

    #[repr(C)]
    pub struct Win32FindDataA {
        pub dw_file_attributes: DWORD,
        pub ft_creation_time: FileTime,
        pub ft_last_access_time: FileTime,
        pub ft_last_write_time: FileTime,
        pub n_file_size_high: DWORD,
        pub n_file_size_low: DWORD,
        pub dw_reserved0: DWORD,
        pub dw_reserved1: DWORD,
        pub c_file_name: [u8; 260],
        pub c_alternate_file_name: [u8; 14],
    }

    #[repr(C)]
    pub struct Win32FindDataW {
        pub dw_file_attributes: DWORD,
        pub ft_creation_time: FileTime,
        pub ft_last_access_time: FileTime,
        pub ft_last_write_time: FileTime,
        pub n_file_size_high: DWORD,
        pub n_file_size_low: DWORD,
        pub dw_reserved0: DWORD,
        pub dw_reserved1: DWORD,
        pub c_file_name: [u16; 260],
        pub c_alternate_file_name: [u16; 14],
    }

    #[repr(C)]
    pub struct ByHandleFileInformation {
        pub dw_file_attributes: DWORD,
        pub ft_creation_time: FileTime,
        pub ft_last_access_time: FileTime,
        pub ft_last_write_time: FileTime,
        pub dw_volume_serial_number: DWORD,
        pub n_file_size_high: DWORD,
        pub n_file_size_low: DWORD,
        pub n_number_of_links: DWORD,
        pub n_file_index_high: DWORD,
        pub n_file_index_low: DWORD,
    }

    #[repr(C)]
    pub struct IoStatusBlock {
        pub status_or_ptr: isize,
        pub information: usize,
    }

    /// Konvertiert einen UTF-16 Pfad (`\\?\C:\...` oder `C:\...`) in einen nullterminierten ANSI-Pfad.
    unsafe fn wide_path_to_ansi_buf(mut wptr: *const u16, out: &mut [u8; 520]) -> usize {
        if wptr.is_null() {
            out[0] = 0;
            return 0;
        }
        // "\\?\" Prefix von Rust std::fs abstreifen (Win95 & Win32s unterstützen kein \\?\!)
        unsafe {
            if *wptr == b'\\' as u16
                && *wptr.add(1) == b'\\' as u16
                && *wptr.add(2) == b'?' as u16
                && *wptr.add(3) == b'\\' as u16
            {
                wptr = wptr.add(4);
            }
        }
        let mut idx = 0usize;
        while idx + 1 < out.len() {
            let ch = unsafe { *wptr.add(idx) };
            if ch == 0 {
                break;
            }
            out[idx] = if ch <= 0xFF { ch as u8 } else { b'?' };
            idx += 1;
        }
        out[idx] = 0;
        idx
    }

    unsafe fn ansi_to_wide_buf(src: *const u8, dst: *mut u16, max_wide: usize) -> usize {
        if src.is_null() || dst.is_null() || max_wide == 0 {
            return 0;
        }
        let mut i = 0usize;
        while i + 1 < max_wide {
            let b = unsafe { *src.add(i) };
            if b == 0 {
                break;
            }
            unsafe {
                *dst.add(i) = b as u16;
            }
            i += 1;
        }
        unsafe {
            *dst.add(i) = 0;
        }
        i
    }

    unsafe fn copy_find_data_a_to_w(src: &Win32FindDataA, dst: &mut Win32FindDataW) {
        dst.dw_file_attributes = src.dw_file_attributes;
        dst.ft_creation_time = src.ft_creation_time;
        dst.ft_last_access_time = src.ft_last_access_time;
        dst.ft_last_write_time = src.ft_last_write_time;
        dst.n_file_size_high = src.n_file_size_high;
        dst.n_file_size_low = src.n_file_size_low;
        dst.dw_reserved0 = src.dw_reserved0;
        dst.dw_reserved1 = src.dw_reserved1;
        for i in 0..260 {
            let b = src.c_file_name[i];
            dst.c_file_name[i] = b as u16;
            if b == 0 {
                for j in (i + 1)..260 {
                    dst.c_file_name[j] = 0;
                }
                break;
            }
        }
        for i in 0..14 {
            let b = src.c_alternate_file_name[i];
            dst.c_alternate_file_name[i] = b as u16;
            if b == 0 {
                for j in (i + 1)..14 {
                    dst.c_alternate_file_name[j] = 0;
                }
                break;
            }
        }
    }

    // Virtuelle Metadaten-Handle-Tabelle für `CreateFileW(..., dwDesiredAccess = 0, ...)`
    // (Unter Win95/Win32s kann `CreateFileA` keine Verzeichnisse öffnen; `std::fs::metadata` / `is_dir`
    //  öffnet Pfade mit `dwDesiredAccess = 0` und fragt danach `GetFileInformationByHandle` ab.)
    const VIRT_META_BASE: HANDLE = 0x7FFE_0000;
    const VIRT_META_SLOTS: usize = 32;

    #[derive(Clone, Copy)]
    struct VirtMetaSlot {
        in_use: bool,
        attr: DWORD,
        size_low: DWORD,
        size_high: DWORD,
    }

    static mut VIRT_META_TABLE: [VirtMetaSlot; VIRT_META_SLOTS] = [VirtMetaSlot {
        in_use: false,
        attr: 0,
        size_low: 0,
        size_high: 0,
    }; VIRT_META_SLOTS];

    unsafe fn alloc_virt_meta_handle(attr: DWORD, size_low: DWORD, size_high: DWORD) -> HANDLE {
        unsafe {
            for i in 0..VIRT_META_SLOTS {
                if !VIRT_META_TABLE[i].in_use {
                    VIRT_META_TABLE[i] = VirtMetaSlot {
                        in_use: true,
                        attr,
                        size_low,
                        size_high,
                    };
                    return VIRT_META_BASE | (i as HANDLE);
                }
            }
            VIRT_META_TABLE[0] = VirtMetaSlot {
                in_use: true,
                attr,
                size_low,
                size_high,
            };
            VIRT_META_BASE
        }
    }

    unsafe extern "system" fn shim_CreateFileW(
        lp_file_name: *const u16,
        dw_desired_access: DWORD,
        dw_share_mode: DWORD,
        lp_security_attributes: *mut c_void,
        dw_creation_disposition: DWORD,
        dw_flags_and_attributes: DWORD,
        h_template_file: HANDLE,
    ) -> HANDLE {
        let mut ansi = [0u8; 520];
        let len = unsafe { wide_path_to_ansi_buf(lp_file_name, &mut ansi) };
        if len == 0 {
            unsafe { SetLastError(ERROR_FILE_NOT_FOUND) };
            return INVALID_HANDLE_VALUE;
        }

        // Wichtig für Win95 & Win32s:
        // 1) FILE_SHARE_DELETE (0x04) existiert unter Win95/Win32s nicht -> führt zu ERROR_INVALID_PARAMETER!
        // 2) FILE_FLAG_BACKUP_SEMANTICS (0x0200_0000) für Verzeichnisse funktioniert unter Win95/Win32s nicht.
        let clean_share = dw_share_mode & 0x03;
        let clean_flags = dw_flags_and_attributes & !0x0200_0000;

        // Wenn dwDesiredAccess == 0 (Metadaten-Abfrage via std::fs::metadata / is_dir / exists):
        if dw_desired_access == 0 {
            let mut attr = unsafe { GetFileAttributesA(ansi.as_ptr()) };
            let mut size_low = 0u32;
            let mut size_high = 0u32;
            if attr == INVALID_FILE_ATTRIBUTES {
                // Fallback für Pfade mit abschließendem Backslash (z.B. "C:\MODS\")
                if len > 3 && (ansi[len - 1] == b'\\' || ansi[len - 1] == b'/') {
                    ansi[len - 1] = 0;
                    attr = unsafe { GetFileAttributesA(ansi.as_ptr()) };
                }
            }
            if attr == INVALID_FILE_ATTRIBUTES {
                unsafe { SetLastError(ERROR_FILE_NOT_FOUND) };
                return INVALID_HANDLE_VALUE;
            }
            if (attr & FILE_ATTRIBUTE_DIRECTORY) == 0 {
                let mut fdata: Win32FindDataA = unsafe { core::mem::zeroed() };
                let hfind = unsafe { FindFirstFileA(ansi.as_ptr(), &mut fdata) };
                if hfind != INVALID_HANDLE_VALUE {
                    size_low = fdata.n_file_size_low;
                    size_high = fdata.n_file_size_high;
                    unsafe {
                        FindClose(hfind);
                    }
                }
            }
            return unsafe { alloc_virt_meta_handle(attr, size_low, size_high) };
        }

        unsafe {
            CreateFileA(
                ansi.as_ptr(),
                dw_desired_access,
                clean_share,
                lp_security_attributes,
                dw_creation_disposition,
                if clean_flags == 0 {
                    FILE_ATTRIBUTE_NORMAL
                } else {
                    clean_flags
                },
                h_template_file,
            )
        }
    }

    static REAL_CLOSE_HANDLE: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "system" fn shim_CloseHandle(h_object: HANDLE) -> BOOL {
        if (h_object & 0xFFFF_FFE0u32 as isize) == VIRT_META_BASE {
            let slot = (h_object & 0x1F) as usize;
            unsafe {
                VIRT_META_TABLE[slot].in_use = false;
            }
            return 1;
        }
        let mut p = REAL_CLOSE_HANDLE.load(Ordering::Relaxed);
        if p == 0 {
            let h_k32 = unsafe { GetModuleHandleA(b"KERNEL32.DLL\0".as_ptr()) };
            if h_k32 != 0 {
                p = unsafe { GetProcAddress(h_k32, b"CloseHandle\0".as_ptr()) } as usize;
                if p != 0 {
                    REAL_CLOSE_HANDLE.store(p, Ordering::Relaxed);
                }
            }
        }
        if p != 0 {
            let func: unsafe extern "system" fn(HANDLE) -> BOOL =
                unsafe { core::mem::transmute(p) };
            unsafe { func(h_object) }
        } else {
            1
        }
    }

    unsafe extern "system" fn shim_GetFileInformationByHandle(
        h_file: HANDLE,
        lp_file_information: *mut ByHandleFileInformation,
    ) -> BOOL {
        if lp_file_information.is_null() {
            return 0;
        }
        unsafe {
            ptr::write_bytes(lp_file_information, 0, 1);
            if (h_file & 0xFFFF_FFE0u32 as isize) == VIRT_META_BASE {
                let slot = (h_file & 0x1F) as usize;
                let entry = VIRT_META_TABLE[slot];
                if !entry.in_use {
                    return 0;
                }
                (*lp_file_information).dw_file_attributes = entry.attr;
                (*lp_file_information).n_file_size_low = entry.size_low;
                (*lp_file_information).n_file_size_high = entry.size_high;
                (*lp_file_information).n_number_of_links = 1;
                return 1;
            }
            let mut high: DWORD = 0;
            let low = GetFileSize(h_file, &mut high);
            (*lp_file_information).dw_file_attributes = FILE_ATTRIBUTE_NORMAL;
            (*lp_file_information).n_file_size_low = if low == 0xFFFF_FFFF { 0 } else { low };
            (*lp_file_information).n_file_size_high = high;
            (*lp_file_information).n_number_of_links = 1;
        }
        1
    }

    unsafe extern "system" fn shim_GetFileInformationByHandleEx(
        _h_file: HANDLE,
        _file_information_class: i32,
        _lp_file_information: *mut c_void,
        _dw_buffer_size: DWORD,
    ) -> BOOL {
        unsafe { SetLastError(ERROR_CALL_NOT_IMPLEMENTED) };
        0
    }

    unsafe extern "system" fn shim_SetFileInformationByHandle(
        h_file: HANDLE,
        file_information_class: i32,
        lp_file_information: *const c_void,
        _dw_buffer_size: DWORD,
    ) -> BOOL {
        // FileEndOfFileInfo (6) / FileAllocationInfo (5) -> SetFilePointer + SetEndOfFile
        if (file_information_class == 5 || file_information_class == 6)
            && !lp_file_information.is_null()
        {
            unsafe {
                let pos = *(lp_file_information as *const i64);
                let low = (pos & 0xFFFF_FFFF) as i32;
                let mut high = (pos >> 32) as i32;
                let saved_high = 0i32;
                let _ = SetFilePointer(h_file, low, &mut high, 0); // FILE_BEGIN
                let ok = SetEndOfFile(h_file);
                let _ = saved_high;
                return ok;
            }
        }
        1
    }

    unsafe extern "system" fn shim_GetFileSizeEx(h_file: HANDLE, lp_file_size: *mut i64) -> BOOL {
        if lp_file_size.is_null() {
            return 0;
        }
        unsafe {
            let mut high: DWORD = 0;
            let low = GetFileSize(h_file, &mut high);
            if low == 0xFFFF_FFFF && GetLastError() != 0 {
                return 0;
            }
            *lp_file_size = ((high as i64) << 32) | (low as u64 as i64);
        }
        1
    }

    unsafe extern "system" fn shim_SetFilePointerEx(
        h_file: HANDLE,
        li_distance_to_move_lo: u32,
        li_distance_to_move_hi: i32,
        lp_new_file_pointer: *mut i64,
        dw_move_method: DWORD,
    ) -> BOOL {
        unsafe {
            let mut high: LONG = li_distance_to_move_hi;
            let low = SetFilePointer(
                h_file,
                li_distance_to_move_lo as LONG,
                &mut high,
                dw_move_method,
            );
            if low == 0xFFFF_FFFF && GetLastError() != 0 {
                return 0;
            }
            if !lp_new_file_pointer.is_null() {
                *lp_new_file_pointer = ((high as i64) << 32) | (low as u64 as i64);
            }
        }
        1
    }

    // NTDLL.DLL `NtReadFile` / `NtWriteFile` -> direkt auf `KERNEL32.DLL` `ReadFile` / `WriteFile`!
    unsafe extern "system" fn shim_NtReadFile(
        file_handle: HANDLE,
        _event: HANDLE,
        _apc_routine: *mut c_void,
        _apc_context: *mut c_void,
        io_status_block: *mut IoStatusBlock,
        buffer: *mut c_void,
        length: ULONG,
        byte_offset: *const i64,
        _key: *const ULONG,
    ) -> NTSTATUS {
        unsafe {
            if !byte_offset.is_null() {
                let off = *byte_offset;
                if off >= 0 {
                    let mut hi = (off >> 32) as LONG;
                    SetFilePointer(file_handle, (off & 0xFFFF_FFFF) as LONG, &mut hi, 0);
                }
            }
            let mut bytes_read: DWORD = 0;
            let ok = ReadFile(file_handle, buffer, length, &mut bytes_read, ptr::null_mut());
            if !io_status_block.is_null() {
                (*io_status_block).status_or_ptr = if ok != 0 { 0 } else { -1 };
                (*io_status_block).information = bytes_read as usize;
            }
            if ok != 0 {
                0 // STATUS_SUCCESS
            } else {
                -1073741823 // STATUS_UNSUCCESSFUL (0xC0000001)
            }
        }
    }

    unsafe extern "system" fn shim_NtWriteFile(
        file_handle: HANDLE,
        _event: HANDLE,
        _apc_routine: *mut c_void,
        _apc_context: *mut c_void,
        io_status_block: *mut IoStatusBlock,
        buffer: *const c_void,
        length: ULONG,
        byte_offset: *const i64,
        _key: *const ULONG,
    ) -> NTSTATUS {
        unsafe {
            if !byte_offset.is_null() {
                let off = *byte_offset;
                if off >= 0 {
                    let mut hi = (off >> 32) as LONG;
                    SetFilePointer(file_handle, (off & 0xFFFF_FFFF) as LONG, &mut hi, 0);
                }
            }
            let mut bytes_written: DWORD = 0;
            let ok = WriteFile(
                file_handle,
                buffer,
                length,
                &mut bytes_written,
                ptr::null_mut(),
            );
            if ok != 0 {
                if !io_status_block.is_null() {
                    (*io_status_block).status_or_ptr = 0;
                    (*io_status_block).information = bytes_written as usize;
                }
                return 0; // STATUS_SUCCESS
            }
            // Wichtig für Windows 95 & Windows 3.11 (Win32s):
            // In einem `/SUBSYSTEM:WINDOWS` GUI-Prozess liefert `GetStdHandle(STD_OUTPUT_HANDLE)`
            // kein `0` (wie unter Windows NT/10/11), sondern `INVALID_HANDLE_VALUE` (`-1` mit `LastError=0`)
            // oder ein geerbtes DOS-Handle (z.B. `1` oder `7`), auf dem `WriteFile` mit `FALSE` fehlschlägt!
            // Damit `println!` / `eprintln!` niemals mit `"failed printing to stdout"` (`stdio.rs:1168`)
            // in einen Panic läuft, behandeln wir fehlgeschlagene Writes auf Stdout/Stderr als Null-Sink.
            if file_handle == 0
                || file_handle == INVALID_HANDLE_VALUE
                || (file_handle as usize) <= 0x20
            {
                if !io_status_block.is_null() {
                    (*io_status_block).status_or_ptr = 0;
                    (*io_status_block).information = length as usize;
                }
                return 0; // STATUS_SUCCESS
            }
            if !io_status_block.is_null() {
                (*io_status_block).status_or_ptr = -1;
                (*io_status_block).information = 0;
            }
            -1073741823 // STATUS_UNSUCCESSFUL
        }
    }

    // Für ein `/SUBSYSTEM:WINDOWS`-Binary liefert `GetStdHandle` unter Windows NT/10/11 `0` (`NULL`),
    // wodurch `std::io::stdio` automatisch `ERROR_INVALID_HANDLE` (`6`) erkennt und `stdout`/`stderr`
    // als Null-Sink ignoriert. Unter Windows 95 und Win32s 1.30c liefert `KERNEL32.DLL!GetStdHandle`
    // stattdessen `0xFFFFFFFF` (`INVALID_HANDLE_VALUE`) mit `GetLastError() == 0` (oder ein DOS-Handle `1`),
    // was `std::io::stdio::print_to` sofort mit `"failed printing to stdout"` in einen Panic treibt!
    // Durch Rückgabe von `0` verhält sich `std::io::stdio` unter Win32s & Win95 exakt wie unter WinNT+.
    unsafe extern "system" fn shim_GetStdHandle(_n_std_handle: DWORD) -> HANDLE {
        0
    }

    unsafe extern "system" fn shim_RtlNtStatusToDosError(_status: NTSTATUS) -> ULONG {
        unsafe { GetLastError() }
    }

    unsafe extern "system" fn shim_FindFirstFileExW(
        lp_file_name: *const u16,
        _f_info_level_id: i32,
        lp_find_file_data: *mut Win32FindDataW,
        _f_search_op: i32,
        _lp_search_filter: *mut c_void,
        _dw_additional_flags: DWORD,
    ) -> HANDLE {
        if lp_find_file_data.is_null() {
            return INVALID_HANDLE_VALUE;
        }
        let mut ansi = [0u8; 520];
        let len = unsafe { wide_path_to_ansi_buf(lp_file_name, &mut ansi) };
        if len == 0 {
            return INVALID_HANDLE_VALUE;
        }
        let mut data_a: Win32FindDataA = unsafe { core::mem::zeroed() };
        let h = unsafe { FindFirstFileA(ansi.as_ptr(), &mut data_a) };
        if h != INVALID_HANDLE_VALUE {
            unsafe {
                copy_find_data_a_to_w(&data_a, &mut *lp_find_file_data);
            }
        }
        h
    }

    unsafe extern "system" fn shim_FindFirstFileW(
        lp_file_name: *const u16,
        lp_find_file_data: *mut Win32FindDataW,
    ) -> HANDLE {
        unsafe {
            shim_FindFirstFileExW(
                lp_file_name,
                0,
                lp_find_file_data,
                0,
                ptr::null_mut(),
                0,
            )
        }
    }

    unsafe extern "system" fn shim_FindNextFileW(
        h_find_file: HANDLE,
        lp_find_file_data: *mut Win32FindDataW,
    ) -> BOOL {
        if lp_find_file_data.is_null() {
            return 0;
        }
        let mut data_a: Win32FindDataA = unsafe { core::mem::zeroed() };
        let ok = unsafe { FindNextFileA(h_find_file, &mut data_a) };
        if ok != 0 {
            unsafe {
                copy_find_data_a_to_w(&data_a, &mut *lp_find_file_data);
            }
        } else {
            unsafe { SetLastError(ERROR_NO_MORE_FILES) };
        }
        ok
    }

    unsafe extern "system" fn shim_GetFileAttributesW(lp_file_name: *const u16) -> DWORD {
        let mut ansi = [0u8; 520];
        let len = unsafe { wide_path_to_ansi_buf(lp_file_name, &mut ansi) };
        if len == 0 {
            return INVALID_FILE_ATTRIBUTES;
        }
        unsafe { GetFileAttributesA(ansi.as_ptr()) }
    }

    unsafe extern "system" fn shim_SetFileAttributesW(
        lp_file_name: *const u16,
        dw_file_attributes: DWORD,
    ) -> BOOL {
        let mut ansi = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_file_name, &mut ansi);
            SetFileAttributesA(ansi.as_ptr(), dw_file_attributes)
        }
    }

    #[repr(C)]
    struct Win32FileAttributeData {
        dw_file_attributes: DWORD,
        ft_creation_time: FileTime,
        ft_last_access_time: FileTime,
        ft_last_write_time: FileTime,
        n_file_size_high: DWORD,
        n_file_size_low: DWORD,
    }

    unsafe extern "system" fn shim_GetFileAttributesExW(
        lp_file_name: *const u16,
        _f_info_level_id: i32,
        lp_file_information: *mut Win32FileAttributeData,
    ) -> BOOL {
        if lp_file_information.is_null() {
            return 0;
        }
        let mut ansi = [0u8; 520];
        let len = unsafe { wide_path_to_ansi_buf(lp_file_name, &mut ansi) };
        if len == 0 {
            return 0;
        }
        unsafe {
            let attr = GetFileAttributesA(ansi.as_ptr());
            if attr == INVALID_FILE_ATTRIBUTES {
                return 0;
            }
            ptr::write_bytes(lp_file_information, 0, 1);
            (*lp_file_information).dw_file_attributes = attr;
            if (attr & FILE_ATTRIBUTE_DIRECTORY) == 0 {
                let mut fdata: Win32FindDataA = core::mem::zeroed();
                let h = FindFirstFileA(ansi.as_ptr(), &mut fdata);
                if h != INVALID_HANDLE_VALUE {
                    (*lp_file_information).ft_creation_time = fdata.ft_creation_time;
                    (*lp_file_information).ft_last_access_time = fdata.ft_last_access_time;
                    (*lp_file_information).ft_last_write_time = fdata.ft_last_write_time;
                    (*lp_file_information).n_file_size_high = fdata.n_file_size_high;
                    (*lp_file_information).n_file_size_low = fdata.n_file_size_low;
                    FindClose(h);
                }
            }
        }
        1
    }

    unsafe extern "system" fn shim_DeleteFileW(lp_file_name: *const u16) -> BOOL {
        let mut ansi = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_file_name, &mut ansi);
            DeleteFileA(ansi.as_ptr())
        }
    }

    unsafe extern "system" fn shim_MoveFileExW(
        lp_existing: *const u16,
        lp_new: *const u16,
        dw_flags: DWORD,
    ) -> BOOL {
        let mut a_old = [0u8; 520];
        let mut a_new = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_existing, &mut a_old);
            wide_path_to_ansi_buf(lp_new, &mut a_new);
            if (dw_flags & 0x01) != 0 {
                // MOVEFILE_REPLACE_EXISTING
                let _ = DeleteFileA(a_new.as_ptr());
            }
            MoveFileA(a_old.as_ptr(), a_new.as_ptr())
        }
    }

    unsafe extern "system" fn shim_CopyFileExW(
        lp_existing: *const u16,
        lp_new: *const u16,
        _lp_progress: *mut c_void,
        _lp_data: *mut c_void,
        _pb_cancel: *mut BOOL,
        dw_copy_flags: DWORD,
    ) -> BOOL {
        let mut a_old = [0u8; 520];
        let mut a_new = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_existing, &mut a_old);
            wide_path_to_ansi_buf(lp_new, &mut a_new);
            let fail_if_exists = if (dw_copy_flags & 0x01) != 0 { 1 } else { 0 };
            CopyFileA(a_old.as_ptr(), a_new.as_ptr(), fail_if_exists)
        }
    }

    unsafe extern "system" fn shim_CreateDirectoryW(
        lp_path_name: *const u16,
        lp_security_attributes: *mut c_void,
    ) -> BOOL {
        let mut ansi = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_path_name, &mut ansi);
            CreateDirectoryA(ansi.as_ptr(), lp_security_attributes)
        }
    }

    unsafe extern "system" fn shim_RemoveDirectoryW(lp_path_name: *const u16) -> BOOL {
        let mut ansi = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_path_name, &mut ansi);
            RemoveDirectoryA(ansi.as_ptr())
        }
    }

    unsafe extern "system" fn shim_GetCurrentDirectoryW(
        n_buffer_length: DWORD,
        lp_buffer: *mut u16,
    ) -> DWORD {
        let mut ansi = [0u8; 520];
        let len = unsafe { GetCurrentDirectoryA(ansi.len() as DWORD, ansi.as_mut_ptr()) } as usize;
        if len == 0 {
            return 0;
        }
        if lp_buffer.is_null() || n_buffer_length == 0 {
            return (len + 1) as DWORD;
        }
        unsafe { ansi_to_wide_buf(ansi.as_ptr(), lp_buffer, n_buffer_length as usize) as DWORD }
    }

    unsafe extern "system" fn shim_SetCurrentDirectoryW(lp_path_name: *const u16) -> BOOL {
        let mut ansi = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_path_name, &mut ansi);
            SetCurrentDirectoryA(ansi.as_ptr())
        }
    }

    unsafe extern "system" fn shim_GetFullPathNameW(
        lp_file_name: *const u16,
        n_buffer_length: DWORD,
        lp_buffer: *mut u16,
        lp_file_part: *mut *mut u16,
    ) -> DWORD {
        let mut ansi_in = [0u8; 520];
        let mut ansi_out = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_file_name, &mut ansi_in);
            let mut file_part_a: *mut u8 = ptr::null_mut();
            let len = GetFullPathNameA(
                ansi_in.as_ptr(),
                ansi_out.len() as DWORD,
                ansi_out.as_mut_ptr(),
                &mut file_part_a,
            ) as usize;
            if len == 0 {
                return 0;
            }
            if lp_buffer.is_null() || (n_buffer_length as usize) <= len {
                return (len + 1) as DWORD;
            }
            let written = ansi_to_wide_buf(ansi_out.as_ptr(), lp_buffer, n_buffer_length as usize);
            if !lp_file_part.is_null() {
                *lp_file_part = ptr::null_mut();
            }
            written as DWORD
        }
    }

    unsafe extern "system" fn shim_GetTempPathW(
        n_buffer_length: DWORD,
        lp_buffer: *mut u16,
    ) -> DWORD {
        let mut ansi = [0u8; 520];
        let len = unsafe { GetTempPathA(ansi.len() as DWORD, ansi.as_mut_ptr()) } as usize;
        if len == 0 {
            return 0;
        }
        if lp_buffer.is_null() || n_buffer_length == 0 {
            return (len + 1) as DWORD;
        }
        unsafe { ansi_to_wide_buf(ansi.as_ptr(), lp_buffer, n_buffer_length as usize) as DWORD }
    }

    unsafe extern "system" fn shim_GetModuleFileNameW(
        h_module: HMODULE,
        lp_filename: *mut u16,
        n_size: DWORD,
    ) -> DWORD {
        let mut ansi = [0u8; 520];
        let len = unsafe { GetModuleFileNameA(h_module, ansi.as_mut_ptr(), ansi.len() as DWORD) };
        if len == 0 {
            return 0;
        }
        unsafe { ansi_to_wide_buf(ansi.as_ptr(), lp_filename, n_size as usize) as DWORD }
    }

    unsafe extern "system" fn shim_GetModuleHandleW(lp_module_name: *const u16) -> HMODULE {
        if lp_module_name.is_null() {
            return unsafe { GetModuleHandleA(ptr::null()) };
        }
        let mut ansi = [0u8; 260];
        let mut i = 0usize;
        while i + 1 < ansi.len() {
            let ch = unsafe { *lp_module_name.add(i) };
            if ch == 0 {
                break;
            }
            ansi[i] = ch as u8;
            i += 1;
        }
        ansi[i] = 0;
        unsafe { GetModuleHandleA(ansi.as_ptr()) }
    }

    unsafe extern "system" fn shim_LoadLibraryExW(
        lp_lib_file_name: *const u16,
        _h_file: HANDLE,
        _dw_flags: DWORD,
    ) -> HMODULE {
        if lp_lib_file_name.is_null() {
            return 0;
        }
        let mut ansi = [0u8; 260];
        let mut i = 0usize;
        while i + 1 < ansi.len() {
            let ch = unsafe { *lp_lib_file_name.add(i) };
            if ch == 0 {
                break;
            }
            ansi[i] = ch as u8;
            i += 1;
        }
        ansi[i] = 0;
        unsafe { LoadLibraryA(ansi.as_ptr()) }
    }

    static mut CACHED_CMDLINE_W: [u16; 1024] = [0; 1024];

    unsafe extern "system" fn shim_GetCommandLineW() -> *const u16 {
        unsafe {
            let cmd_a = GetCommandLineA();
            let dst = ptr::addr_of_mut!(CACHED_CMDLINE_W) as *mut u16;
            if !cmd_a.is_null() {
                ansi_to_wide_buf(cmd_a, dst, 1024);
            } else {
                *dst = 0;
            }
            dst
        }
    }

    unsafe extern "system" fn shim_GetEnvironmentVariableW(
        lp_name: *const u16,
        lp_buffer: *mut u16,
        n_size: DWORD,
    ) -> DWORD {
        let mut name_a = [0u8; 260];
        let mut val_a = [0u8; 1024];
        unsafe {
            wide_path_to_ansi_buf(lp_name, &mut [0u8; 520]);
            let mut i = 0usize;
            if !lp_name.is_null() {
                while i + 1 < name_a.len() {
                    let ch = *lp_name.add(i);
                    if ch == 0 {
                        break;
                    }
                    name_a[i] = ch as u8;
                    i += 1;
                }
            }
            name_a[i] = 0;
            let len = GetEnvironmentVariableA(name_a.as_ptr(), val_a.as_mut_ptr(), val_a.len() as DWORD);
            if len == 0 {
                return 0;
            }
            if lp_buffer.is_null() || n_size == 0 {
                return len + 1;
            }
            ansi_to_wide_buf(val_a.as_ptr(), lp_buffer, n_size as usize) as DWORD
        }
    }

    unsafe extern "system" fn shim_SetEnvironmentVariableW(
        lp_name: *const u16,
        lp_value: *const u16,
    ) -> BOOL {
        let mut name_a = [0u8; 520];
        let mut val_a = [0u8; 520];
        unsafe {
            wide_path_to_ansi_buf(lp_name, &mut name_a);
            if lp_value.is_null() {
                SetEnvironmentVariableA(name_a.as_ptr(), ptr::null())
            } else {
                wide_path_to_ansi_buf(lp_value, &mut val_a);
                SetEnvironmentVariableA(name_a.as_ptr(), val_a.as_ptr())
            }
        }
    }

    unsafe extern "system" fn shim_GetEnvironmentStringsW() -> *mut u16 {
        unsafe {
            let env_a = GetEnvironmentStrings();
            if env_a.is_null() {
                return ptr::null_mut();
            }
            let mut len = 0usize;
            while len < 32766 && (*env_a.add(len) != 0 || *env_a.add(len + 1) != 0) {
                len += 1;
            }
            len += 2;
            let hheap = GetProcessHeap();
            let buf_w = HeapAlloc(hheap, 0, len * 2) as *mut u16;
            if !buf_w.is_null() {
                for i in 0..len {
                    *buf_w.add(i) = (*env_a.add(i)) as u16;
                }
            }
            // Unter Win32s 1.30c zeigt `GetEnvironmentStrings()` direkt auf den statischen
            // DOS-Umgebungsblock und darf nicht per `FreeEnvironmentStringsA` freigegeben werden.
            buf_w
        }
    }

    unsafe extern "system" fn shim_FreeEnvironmentStringsW(lpsz_env_block: *mut u16) -> BOOL {
        if !lpsz_env_block.is_null() {
            unsafe {
                let hheap = GetProcessHeap();
                HeapFree(hheap, 0, lpsz_env_block as *mut c_void);
            }
        }
        1
    }

    // Zeitgeber & Zufallsgenerator (für HashMap & Instant::now() auf 486 / Win32s / Win95)
    static PRNG_STATE: AtomicU32 = AtomicU32::new(0x9E37_79B9);

    unsafe extern "system" fn shim_SystemFunction036(buf: *mut u8, len: ULONG) -> u8 {
        if buf.is_null() {
            return 1;
        }
        let ticks = unsafe { GetTickCount() };
        let mut s = PRNG_STATE.load(Ordering::Relaxed) ^ ticks.wrapping_mul(0x85EB_CA6B);
        if s == 0 {
            s = 0xA5A5_5A5A;
        }
        for i in 0..(len as usize) {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            unsafe {
                *buf.add(i) = (s & 0xFF) as u8;
            }
        }
        PRNG_STATE.store(s, Ordering::Relaxed);
        1
    }

    unsafe extern "system" fn shim_QueryPerformanceFrequency(lp_frequency: *mut i64) -> BOOL {
        if !lp_frequency.is_null() {
            unsafe {
                *lp_frequency = 1000; // 1 ms Auflösung via GetTickCount()
            }
        }
        1
    }

    static LAST_TICK: AtomicU32 = AtomicU32::new(0);
    static TICK_EPOCH_HI: AtomicU32 = AtomicU32::new(0);

    unsafe fn monotonic_ms_u64() -> u64 {
        let cur = unsafe { GetTickCount() };
        let prev = LAST_TICK.swap(cur, Ordering::Relaxed);
        if cur < prev {
            TICK_EPOCH_HI.fetch_add(1, Ordering::Relaxed);
        }
        let hi = TICK_EPOCH_HI.load(Ordering::Relaxed) as u64;
        (hi << 32) | (cur as u64)
    }

    unsafe extern "system" fn shim_QueryPerformanceCounter(lp_performance_count: *mut i64) -> BOOL {
        if !lp_performance_count.is_null() {
            unsafe {
                *lp_performance_count = monotonic_ms_u64() as i64;
            }
        }
        1
    }

    unsafe extern "system" fn shim_GetTickCount64() -> u64 {
        unsafe { monotonic_ms_u64() }
    }

    unsafe extern "system" fn shim_GetSystemTimeAsFileTime(lp_file_time: *mut FileTime) {
        if !lp_file_time.is_null() {
            let t = unsafe { monotonic_ms_u64() } * 10_000 + 116_444_736_000_000_000u64;
            unsafe {
                (*lp_file_time).dw_low_date_time = (t & 0xFFFF_FFFF) as DWORD;
                (*lp_file_time).dw_high_date_time = (t >> 32) as DWORD;
            }
        }
    }

    // Synchronisation (100% konsistente Single-Thread-Implementierung für Win32s & Win95:
    // verhindert jeglichen `LeaveCriticalSection`-Underflow/Deadlock nach `TryEnterCriticalSection`!)
    unsafe extern "system" fn shim_InitializeCriticalSection(_lp_cs: *mut c_void) {}

    unsafe extern "system" fn shim_InitializeCriticalSectionAndSpinCount(
        _lp_cs: *mut c_void,
        _dw_spin_count: DWORD,
    ) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_EnterCriticalSection(_lp_cs: *mut c_void) {}

    unsafe extern "system" fn shim_TryEnterCriticalSection(_lp_cs: *mut c_void) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_LeaveCriticalSection(_lp_cs: *mut c_void) {}

    unsafe extern "system" fn shim_DeleteCriticalSection(_lp_cs: *mut c_void) {}

    unsafe extern "system" fn shim_AcquireSRWLockExclusive(_lock: *mut usize) {}
    unsafe extern "system" fn shim_ReleaseSRWLockExclusive(_lock: *mut usize) {}
    unsafe extern "system" fn shim_AcquireSRWLockShared(_lock: *mut usize) {}
    unsafe extern "system" fn shim_ReleaseSRWLockShared(_lock: *mut usize) {}
    unsafe extern "system" fn shim_TryAcquireSRWLockExclusive(_lock: *mut usize) -> u8 {
        1
    }
    unsafe extern "system" fn shim_TryAcquireSRWLockShared(_lock: *mut usize) -> u8 {
        1
    }

    unsafe extern "system" fn shim_SleepConditionVariableSRW(
        _cv: *mut usize,
        _lock: *mut usize,
        dw_milliseconds: DWORD,
        _flags: ULONG,
    ) -> BOOL {
        unsafe {
            Sleep(dw_milliseconds.min(10));
        }
        1
    }

    unsafe extern "system" fn shim_WakeConditionVariable(_cv: *mut usize) {}
    unsafe extern "system" fn shim_WakeAllConditionVariable(_cv: *mut usize) {}

    unsafe extern "system" fn shim_WaitOnAddress(
        address: *const c_void,
        compare_address: *const c_void,
        address_size: usize,
        dw_milliseconds: DWORD,
    ) -> BOOL {
        if !address.is_null() && !compare_address.is_null() {
            let same = unsafe {
                memcmp(address as *const u8, compare_address as *const u8, address_size) == 0
            };
            if same && dw_milliseconds > 0 {
                unsafe {
                    Sleep(dw_milliseconds.min(5));
                }
            }
        }
        1
    }

    unsafe extern "system" fn shim_WakeByAddressSingle(_address: *const c_void) {}
    unsafe extern "system" fn shim_WakeByAddressAll(_address: *const c_void) {}

    unsafe extern "system" fn shim_InitOnceBeginInitialize(
        lp_init_once: *mut usize,
        _dw_flags: DWORD,
        f_pending: *mut BOOL,
        _lp_context: *mut *mut c_void,
    ) -> BOOL {
        if lp_init_once.is_null() || f_pending.is_null() {
            return 0;
        }
        unsafe {
            if *lp_init_once == 2 {
                *f_pending = 0;
            } else {
                *lp_init_once = 2;
                *f_pending = 1;
            }
        }
        1
    }

    unsafe extern "system" fn shim_InitOnceComplete(
        lp_init_once: *mut usize,
        _dw_flags: DWORD,
        _lp_context: *mut c_void,
    ) -> BOOL {
        if !lp_init_once.is_null() {
            unsafe {
                *lp_init_once = 2;
            }
        }
        1
    }

    unsafe extern "system" fn shim_SwitchToThread() -> BOOL {
        unsafe {
            Sleep(0);
        }
        1
    }

    unsafe extern "system" fn shim_IsDebuggerPresent() -> BOOL {
        0
    }

    unsafe extern "system" fn shim_AddVectoredExceptionHandler(
        _first: ULONG,
        _handler: *mut c_void,
    ) -> *mut c_void {
        1 as *mut c_void
    }

    unsafe extern "system" fn shim_SetThreadStackGuarantee(_stack_size_in_bytes: *mut ULONG) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_GetConsoleMode(_h_console_handle: HANDLE, _lp_mode: *mut DWORD) -> BOOL {
        0
    }

    unsafe extern "system" fn shim_WriteConsoleW(
        h_console_output: HANDLE,
        lp_buffer: *const u16,
        n_number_of_chars_to_write: DWORD,
        lp_number_of_chars_written: *mut DWORD,
        _lp_reserved: *mut c_void,
    ) -> BOOL {
        if !lp_number_of_chars_written.is_null() {
            unsafe {
                *lp_number_of_chars_written = n_number_of_chars_to_write;
            }
        }
        let _ = (h_console_output, lp_buffer);
        1
    }

    unsafe extern "system" fn shim_ReadConsoleW(
        _h_console_input: HANDLE,
        _lp_buffer: *mut c_void,
        _n_number_of_chars_to_read: DWORD,
        lp_number_of_chars_read: *mut DWORD,
        _p_input_control: *mut c_void,
    ) -> BOOL {
        if !lp_number_of_chars_read.is_null() {
            unsafe {
                *lp_number_of_chars_read = 0;
            }
        }
        1
    }

    unsafe extern "system" fn shim_FormatMessageW(
        _dw_flags: DWORD,
        _lp_source: *const c_void,
        _dw_message_id: DWORD,
        _dw_language_id: DWORD,
        lp_buffer: *mut u16,
        n_size: DWORD,
        _arguments: *mut c_void,
    ) -> DWORD {
        if !lp_buffer.is_null() && n_size > 0 {
            unsafe {
                *lp_buffer = 0;
            }
        }
        0
    }

    unsafe extern "system" fn shim_MultiByteToWideChar(
        _code_page: u32,
        _dw_flags: DWORD,
        lp_multi_byte_str: *const u8,
        cb_multi_byte: i32,
        lp_wide_char_str: *mut u16,
        cch_wide_char: i32,
    ) -> i32 {
        if lp_multi_byte_str.is_null() {
            return 0;
        }
        let len = if cb_multi_byte < 0 {
            unsafe { strlen(lp_multi_byte_str) + 1 }
        } else {
            cb_multi_byte as usize
        };
        if cch_wide_char == 0 || lp_wide_char_str.is_null() {
            return len as i32;
        }
        let copy_len = len.min(cch_wide_char as usize);
        for i in 0..copy_len {
            unsafe {
                *lp_wide_char_str.add(i) = (*lp_multi_byte_str.add(i)) as u16;
            }
        }
        copy_len as i32
    }

    unsafe extern "system" fn shim_WideCharToMultiByte(
        _code_page: u32,
        _dw_flags: DWORD,
        lp_wide_char_str: *const u16,
        cch_wide_char: i32,
        lp_multi_byte_str: *mut u8,
        cb_multi_byte: i32,
        _lp_default_char: *const u8,
        _lp_used_default_char: *mut BOOL,
    ) -> i32 {
        if lp_wide_char_str.is_null() {
            return 0;
        }
        let len = if cch_wide_char < 0 {
            unsafe { wcslen(lp_wide_char_str) + 1 }
        } else {
            cch_wide_char as usize
        };
        if cb_multi_byte == 0 || lp_multi_byte_str.is_null() {
            return len as i32;
        }
        let copy_len = len.min(cb_multi_byte as usize);
        for i in 0..copy_len {
            let ch = unsafe { *lp_wide_char_str.add(i) };
            unsafe {
                *lp_multi_byte_str.add(i) = if ch <= 0xFF { ch as u8 } else { b'?' };
            }
        }
        copy_len as i32
    }

    unsafe extern "system" fn shim_GetWindowsDirectoryW(
        lp_buffer: *mut u16,
        u_size: u32,
    ) -> u32 {
        unsafe { ansi_to_wide_buf(b"C:\\WINDOWS\0".as_ptr(), lp_buffer, u_size as usize) as u32 }
    }

    unsafe extern "system" fn shim_GetUserProfileDirectoryW(
        _h_token: HANDLE,
        _lp_profile_dir: *mut u16,
        _lpcch_size: *mut DWORD,
    ) -> BOOL {
        0
    }

    unsafe extern "system" fn shim_OpenProcessToken(
        _process_handle: HANDLE,
        _desired_access: DWORD,
        _token_handle: *mut HANDLE,
    ) -> BOOL {
        0
    }

    unsafe extern "system" fn shim_CommandLineToArgvW(
        _lp_cmd_line: *const u16,
        p_num_args: *mut i32,
    ) -> *mut *mut u16 {
        // Wenn wir NULL zurückgeben, fällt Rusts `std::env::args()` automatisch auf seinen
        // eingebauten Pure-Rust Kommandozeilen-Parser (`parse_lp_cmd_line`) zurück!
        if !p_num_args.is_null() {
            unsafe {
                *p_num_args = 0;
            }
        }
        ptr::null_mut()
    }

    unsafe extern "system" fn shim_GetCurrentProcess() -> HANDLE {
        -1
    }

    unsafe extern "system" fn shim_DuplicateHandle(
        _h_source_process_handle: HANDLE,
        h_source_handle: HANDLE,
        _h_target_process_handle: HANDLE,
        lp_target_handle: *mut HANDLE,
        _dw_desired_access: DWORD,
        _b_inherit_handle: BOOL,
        _dw_options: DWORD,
    ) -> BOOL {
        if !lp_target_handle.is_null() {
            unsafe {
                *lp_target_handle = h_source_handle;
            }
        }
        1
    }

    unsafe extern "system" fn shim_CancelIo(_h_file: HANDLE) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_GetOverlappedResult(
        _h_file: HANDLE,
        _lp_overlapped: *mut c_void,
        lp_number_of_bytes_transferred: *mut DWORD,
        _b_wait: BOOL,
    ) -> BOOL {
        if !lp_number_of_bytes_transferred.is_null() {
            unsafe {
                *lp_number_of_bytes_transferred = 0;
            }
        }
        0
    }

    unsafe extern "system" fn shim_CreateEventW(
        _lp_event_attributes: *mut c_void,
        _b_manual_reset: BOOL,
        _b_initial_state: BOOL,
        _lp_name: *const u16,
    ) -> HANDLE {
        1
    }

    unsafe extern "system" fn shim_SetEvent(_h_event: HANDLE) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_WaitForSingleObject(
        _h_handle: HANDLE,
        _dw_milliseconds: DWORD,
    ) -> DWORD {
        0
    }

    unsafe extern "system" fn shim_WaitForMultipleObjects(
        _n_count: DWORD,
        _lp_handles: *const HANDLE,
        _b_wait_all: BOOL,
        _dw_milliseconds: DWORD,
    ) -> DWORD {
        0
    }

    unsafe extern "system" fn shim_CreateThread(
        _lp_thread_attributes: *mut c_void,
        _dw_stack_size: usize,
        _lp_start_address: *mut c_void,
        _lp_parameter: *mut c_void,
        _dw_creation_flags: DWORD,
        _lp_thread_id: *mut DWORD,
    ) -> HANDLE {
        unsafe { SetLastError(ERROR_CALL_NOT_IMPLEMENTED) };
        0
    }

    unsafe extern "system" fn shim_GetExitCodeThread(
        _h_thread: HANDLE,
        lp_exit_code: *mut DWORD,
    ) -> BOOL {
        if !lp_exit_code.is_null() {
            unsafe {
                *lp_exit_code = 0;
            }
        }
        1
    }

    // Statisches TLS-Array für Single-Thread Win32s / Win95
    static mut TLS_VALUES: [*mut c_void; 64] = [ptr::null_mut(); 64];
    static NEXT_TLS_IDX: AtomicUsize = AtomicUsize::new(1);

    unsafe extern "system" fn shim_TlsAlloc() -> DWORD {
        let idx = NEXT_TLS_IDX.fetch_add(1, Ordering::Relaxed);
        if idx < 64 {
            idx as DWORD
        } else {
            0xFFFF_FFFF
        }
    }

    unsafe extern "system" fn shim_TlsGetValue(dw_tls_index: DWORD) -> *mut c_void {
        if (dw_tls_index as usize) < 64 {
            unsafe {
                SetLastError(0);
                TLS_VALUES[dw_tls_index as usize]
            }
        } else {
            ptr::null_mut()
        }
    }

    unsafe extern "system" fn shim_TlsSetValue(
        dw_tls_index: DWORD,
        lp_tls_value: *mut c_void,
    ) -> BOOL {
        if (dw_tls_index as usize) < 64 {
            unsafe {
                TLS_VALUES[dw_tls_index as usize] = lp_tls_value;
            }
            1
        } else {
            0
        }
    }

    unsafe extern "system" fn shim_TlsFree(_dw_tls_index: DWORD) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_FlsAlloc(_lp_callback: *mut c_void) -> DWORD {
        unsafe { shim_TlsAlloc() }
    }

    unsafe extern "system" fn shim_FlsFree(dw_fls_index: DWORD) -> BOOL {
        unsafe { shim_TlsFree(dw_fls_index) }
    }

    unsafe extern "system" fn shim_FlsSetValue(
        dw_fls_index: DWORD,
        lp_fls_data: *mut c_void,
    ) -> BOOL {
        unsafe { shim_TlsSetValue(dw_fls_index, lp_fls_data) }
    }

    unsafe extern "system" fn shim_IsThreadAFiber() -> BOOL {
        0
    }

    unsafe extern "system" fn shim_GetModuleHandleExW(
        _dw_flags: DWORD,
        _lp_module_name: *const u16,
        ph_module: *mut HMODULE,
    ) -> BOOL {
        if !ph_module.is_null() {
            unsafe {
                *ph_module = GetModuleHandleA(ptr::null());
            }
        }
        1
    }

    unsafe extern "system" fn shim_GetSystemDirectoryW(
        lp_buffer: *mut u16,
        u_size: u32,
    ) -> u32 {
        unsafe {
            ansi_to_wide_buf(
                b"C:\\WINDOWS\\SYSTEM\0".as_ptr(),
                lp_buffer,
                u_size as usize,
            ) as u32
        }
    }

    unsafe extern "system" fn shim_GetCurrentThread() -> HANDLE {
        -2
    }

    unsafe extern "system" fn shim_CompareStringOrdinal(
        lp_string1: *const u16,
        cch_count1: i32,
        lp_string2: *const u16,
        cch_count2: i32,
        b_ignore_case: BOOL,
    ) -> i32 {
        // CSTR_LESS_THAN = 1, CSTR_EQUAL = 2, CSTR_GREATER_THAN = 3
        let len1 = if cch_count1 < 0 {
            unsafe { wcslen(lp_string1) }
        } else {
            cch_count1 as usize
        };
        let len2 = if cch_count2 < 0 {
            unsafe { wcslen(lp_string2) }
        } else {
            cch_count2 as usize
        };
        let min_len = len1.min(len2);
        for i in 0..min_len {
            let mut c1 = unsafe { *lp_string1.add(i) };
            let mut c2 = unsafe { *lp_string2.add(i) };
            if b_ignore_case != 0 {
                if (b'a' as u16..=b'z' as u16).contains(&c1) {
                    c1 -= 32;
                }
                if (b'a' as u16..=b'z' as u16).contains(&c2) {
                    c2 -= 32;
                }
            }
            if c1 < c2 {
                return 1;
            }
            if c1 > c2 {
                return 3;
            }
        }
        if len1 < len2 {
            1
        } else if len1 > len2 {
            3
        } else {
            2
        }
    }

    unsafe extern "system" fn shim_lstrlenW(lp_string: *const u16) -> i32 {
        unsafe { wcslen(lp_string) as i32 }
    }

    unsafe extern "system" fn shim_WaitForSingleObjectEx(
        _h_handle: HANDLE,
        _dw_milliseconds: DWORD,
        _b_alertable: BOOL,
    ) -> DWORD {
        0
    }

    unsafe extern "system" fn shim_CreateMutexA(
        _lp_mutex_attributes: *mut c_void,
        _b_initial_owner: BOOL,
        _lp_name: *const u8,
    ) -> HANDLE {
        1
    }

    unsafe extern "system" fn shim_ReleaseMutex(_h_mutex: HANDLE) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_ReadFileEx(
        _h_file: HANDLE,
        _lp_buffer: *mut c_void,
        _n_number_of_bytes_to_read: DWORD,
        _lp_overlapped: *mut c_void,
        _lp_completion_routine: *mut c_void,
    ) -> BOOL {
        0
    }

    unsafe extern "system" fn shim_WriteFileEx(
        _h_file: HANDLE,
        _lp_buffer: *const c_void,
        _n_number_of_bytes_to_write: DWORD,
        _lp_overlapped: *mut c_void,
        _lp_completion_routine: *mut c_void,
    ) -> BOOL {
        0
    }

    unsafe extern "system" fn shim_SleepEx(dw_milliseconds: DWORD, _b_alertable: BOOL) -> DWORD {
        unsafe {
            Sleep(dw_milliseconds);
        }
        0
    }

    unsafe extern "system" fn shim_SetFileTime(
        _h_file: HANDLE,
        _lp_creation_time: *const FileTime,
        _lp_last_access_time: *const FileTime,
        _lp_last_write_time: *const FileTime,
    ) -> BOOL {
        1
    }

    unsafe extern "system" fn shim_CreateProcessW(
        _lp_application_name: *const u16,
        _lp_command_line: *mut u16,
        _lp_process_attributes: *mut c_void,
        _lp_thread_attributes: *mut c_void,
        _b_inherit_handles: BOOL,
        _dw_creation_flags: DWORD,
        _lp_environment: *mut c_void,
        _lp_current_directory: *const u16,
        _lp_startup_info: *mut c_void,
        _lp_process_information: *mut c_void,
    ) -> BOOL {
        unsafe { SetLastError(ERROR_CALL_NOT_IMPLEMENTED) };
        0
    }

    unsafe extern "system" fn shim_NtOpenFile(
        _file_handle: *mut HANDLE,
        _desired_access: DWORD,
        _object_attributes: *mut c_void,
        _io_status_block: *mut IoStatusBlock,
        _share_access: DWORD,
        _open_options: DWORD,
    ) -> NTSTATUS {
        unsafe { SetLastError(ERROR_CALL_NOT_IMPLEMENTED) };
        -1073741822 // STATUS_NOT_IMPLEMENTED
    }

    unsafe extern "system" fn shim_RtlCaptureContext(_context_record: *mut c_void) {}

    unsafe extern "system" fn shim_NtCreateNamedPipeFile(
        _file_handle: *mut HANDLE,
        _desired_access: ULONG,
        _object_attributes: *mut c_void,
        _io_status_block: *mut IoStatusBlock,
        _share_access: ULONG,
        _create_disposition: ULONG,
        _create_options: ULONG,
        _named_pipe_type: ULONG,
        _read_mode: ULONG,
        _completion_mode: ULONG,
        _maximum_instances: ULONG,
        _inbound_quota: ULONG,
        _outbound_quota: ULONG,
        _default_timeout: *const i64,
    ) -> NTSTATUS {
        unsafe { SetLastError(ERROR_CALL_NOT_IMPLEMENTED) };
        -1073741822 // STATUS_NOT_IMPLEMENTED
    }

    // =========================================================================
    // `__imp__...` IMPORT-POINTER-TABELLE FÜR RUST `std` (`i686-win7-windows-msvc`)
    // =========================================================================

    #[repr(transparent)]
    pub struct ImpFnPtr(pub *const ());
    unsafe impl Sync for ImpFnPtr {}

    macro_rules! export_imp {
        ($sym:literal, $ident:ident, $fn_ident:ident) => {
            #[unsafe(export_name = $sym)]
            pub static $ident: ImpFnPtr = ImpFnPtr($fn_ident as *const ());
        };
    }

    export_imp!("_imp__CreateFileW@28", IMP_CreateFileW, shim_CreateFileW);
    export_imp!("_imp__CloseHandle@4", IMP_CloseHandle, shim_CloseHandle);
    export_imp!(
        "_imp__GetFileInformationByHandle@8",
        IMP_GetFileInformationByHandle,
        shim_GetFileInformationByHandle
    );
    export_imp!(
        "_imp__GetFileInformationByHandleEx@16",
        IMP_GetFileInformationByHandleEx,
        shim_GetFileInformationByHandleEx
    );
    export_imp!(
        "_imp__SetFileInformationByHandle@16",
        IMP_SetFileInformationByHandle,
        shim_SetFileInformationByHandle
    );
    export_imp!("_imp__GetFileSizeEx@8", IMP_GetFileSizeEx, shim_GetFileSizeEx);
    export_imp!(
        "_imp__SetFilePointerEx@20",
        IMP_SetFilePointerEx,
        shim_SetFilePointerEx
    );

    export_imp!("_imp__NtReadFile@36", IMP_NtReadFile, shim_NtReadFile);
    export_imp!("_imp__NtWriteFile@36", IMP_NtWriteFile, shim_NtWriteFile);
    export_imp!(
        "_imp__RtlNtStatusToDosError@4",
        IMP_RtlNtStatusToDosError,
        shim_RtlNtStatusToDosError
    );

    export_imp!(
        "_imp__FindFirstFileExW@24",
        IMP_FindFirstFileExW,
        shim_FindFirstFileExW
    );
    export_imp!(
        "_imp__FindFirstFileW@8",
        IMP_FindFirstFileW,
        shim_FindFirstFileW
    );
    export_imp!(
        "_imp__FindNextFileW@8",
        IMP_FindNextFileW,
        shim_FindNextFileW
    );
    export_imp!(
        "_imp__GetFileAttributesW@4",
        IMP_GetFileAttributesW,
        shim_GetFileAttributesW
    );
    export_imp!(
        "_imp__SetFileAttributesW@8",
        IMP_SetFileAttributesW,
        shim_SetFileAttributesW
    );
    export_imp!(
        "_imp__GetFileAttributesExW@12",
        IMP_GetFileAttributesExW,
        shim_GetFileAttributesExW
    );
    export_imp!("_imp__DeleteFileW@4", IMP_DeleteFileW, shim_DeleteFileW);
    export_imp!("_imp__MoveFileExW@12", IMP_MoveFileExW, shim_MoveFileExW);
    export_imp!("_imp__CopyFileExW@24", IMP_CopyFileExW, shim_CopyFileExW);
    export_imp!(
        "_imp__CreateDirectoryW@8",
        IMP_CreateDirectoryW,
        shim_CreateDirectoryW
    );
    export_imp!(
        "_imp__RemoveDirectoryW@4",
        IMP_RemoveDirectoryW,
        shim_RemoveDirectoryW
    );
    export_imp!(
        "_imp__GetCurrentDirectoryW@8",
        IMP_GetCurrentDirectoryW,
        shim_GetCurrentDirectoryW
    );
    export_imp!(
        "_imp__SetCurrentDirectoryW@4",
        IMP_SetCurrentDirectoryW,
        shim_SetCurrentDirectoryW
    );
    export_imp!(
        "_imp__GetFullPathNameW@16",
        IMP_GetFullPathNameW,
        shim_GetFullPathNameW
    );
    export_imp!("_imp__GetTempPathW@8", IMP_GetTempPathW, shim_GetTempPathW);
    export_imp!(
        "_imp__GetModuleFileNameW@12",
        IMP_GetModuleFileNameW,
        shim_GetModuleFileNameW
    );
    export_imp!(
        "_imp__GetModuleHandleW@4",
        IMP_GetModuleHandleW,
        shim_GetModuleHandleW
    );
    export_imp!(
        "_imp__LoadLibraryExW@12",
        IMP_LoadLibraryExW,
        shim_LoadLibraryExW
    );

    export_imp!(
        "_imp__GetCommandLineW@0",
        IMP_GetCommandLineW,
        shim_GetCommandLineW
    );
    export_imp!(
        "_imp__GetEnvironmentVariableW@12",
        IMP_GetEnvironmentVariableW,
        shim_GetEnvironmentVariableW
    );
    export_imp!(
        "_imp__SetEnvironmentVariableW@8",
        IMP_SetEnvironmentVariableW,
        shim_SetEnvironmentVariableW
    );
    export_imp!(
        "_imp__GetEnvironmentStringsW@0",
        IMP_GetEnvironmentStringsW,
        shim_GetEnvironmentStringsW
    );
    export_imp!(
        "_imp__FreeEnvironmentStringsW@4",
        IMP_FreeEnvironmentStringsW,
        shim_FreeEnvironmentStringsW
    );

    export_imp!(
        "_imp__SystemFunction036@8",
        IMP_SystemFunction036,
        shim_SystemFunction036
    );
    export_imp!(
        "_imp__QueryPerformanceFrequency@4",
        IMP_QueryPerformanceFrequency,
        shim_QueryPerformanceFrequency
    );
    export_imp!(
        "_imp__QueryPerformanceCounter@4",
        IMP_QueryPerformanceCounter,
        shim_QueryPerformanceCounter
    );
    export_imp!(
        "_imp__GetTickCount64@0",
        IMP_GetTickCount64,
        shim_GetTickCount64
    );
    export_imp!(
        "_imp__GetSystemTimeAsFileTime@4",
        IMP_GetSystemTimeAsFileTime,
        shim_GetSystemTimeAsFileTime
    );

    export_imp!(
        "_imp__InitializeCriticalSection@4",
        IMP_InitializeCriticalSection,
        shim_InitializeCriticalSection
    );
    export_imp!(
        "_imp__InitializeCriticalSectionAndSpinCount@8",
        IMP_InitializeCriticalSectionAndSpinCount,
        shim_InitializeCriticalSectionAndSpinCount
    );
    export_imp!(
        "_imp__EnterCriticalSection@4",
        IMP_EnterCriticalSection,
        shim_EnterCriticalSection
    );
    export_imp!(
        "_imp__TryEnterCriticalSection@4",
        IMP_TryEnterCriticalSection,
        shim_TryEnterCriticalSection
    );
    export_imp!(
        "_imp__LeaveCriticalSection@4",
        IMP_LeaveCriticalSection,
        shim_LeaveCriticalSection
    );
    export_imp!(
        "_imp__DeleteCriticalSection@4",
        IMP_DeleteCriticalSection,
        shim_DeleteCriticalSection
    );
    export_imp!(
        "_imp__AcquireSRWLockExclusive@4",
        IMP_AcquireSRWLockExclusive,
        shim_AcquireSRWLockExclusive
    );
    export_imp!(
        "_imp__ReleaseSRWLockExclusive@4",
        IMP_ReleaseSRWLockExclusive,
        shim_ReleaseSRWLockExclusive
    );
    export_imp!(
        "_imp__AcquireSRWLockShared@4",
        IMP_AcquireSRWLockShared,
        shim_AcquireSRWLockShared
    );
    export_imp!(
        "_imp__ReleaseSRWLockShared@4",
        IMP_ReleaseSRWLockShared,
        shim_ReleaseSRWLockShared
    );
    export_imp!(
        "_imp__TryAcquireSRWLockExclusive@4",
        IMP_TryAcquireSRWLockExclusive,
        shim_TryAcquireSRWLockExclusive
    );
    export_imp!(
        "_imp__TryAcquireSRWLockShared@4",
        IMP_TryAcquireSRWLockShared,
        shim_TryAcquireSRWLockShared
    );
    export_imp!(
        "_imp__SleepConditionVariableSRW@16",
        IMP_SleepConditionVariableSRW,
        shim_SleepConditionVariableSRW
    );
    export_imp!(
        "_imp__WakeConditionVariable@4",
        IMP_WakeConditionVariable,
        shim_WakeConditionVariable
    );
    export_imp!(
        "_imp__WakeAllConditionVariable@4",
        IMP_WakeAllConditionVariable,
        shim_WakeAllConditionVariable
    );
    export_imp!(
        "_imp__WaitOnAddress@16",
        IMP_WaitOnAddress,
        shim_WaitOnAddress
    );
    export_imp!(
        "_imp__WakeByAddressSingle@4",
        IMP_WakeByAddressSingle,
        shim_WakeByAddressSingle
    );
    export_imp!(
        "_imp__WakeByAddressAll@4",
        IMP_WakeByAddressAll,
        shim_WakeByAddressAll
    );
    export_imp!(
        "_imp__InitOnceBeginInitialize@16",
        IMP_InitOnceBeginInitialize,
        shim_InitOnceBeginInitialize
    );
    export_imp!(
        "_imp__InitOnceComplete@12",
        IMP_InitOnceComplete,
        shim_InitOnceComplete
    );

    export_imp!(
        "_imp__SwitchToThread@0",
        IMP_SwitchToThread,
        shim_SwitchToThread
    );
    export_imp!(
        "_imp__IsDebuggerPresent@0",
        IMP_IsDebuggerPresent,
        shim_IsDebuggerPresent
    );
    export_imp!(
        "_imp__AddVectoredExceptionHandler@8",
        IMP_AddVectoredExceptionHandler,
        shim_AddVectoredExceptionHandler
    );
    export_imp!(
        "_imp__SetThreadStackGuarantee@4",
        IMP_SetThreadStackGuarantee,
        shim_SetThreadStackGuarantee
    );
    export_imp!(
        "_imp__GetStdHandle@4",
        IMP_GetStdHandle,
        shim_GetStdHandle
    );
    export_imp!(
        "_imp__GetConsoleMode@8",
        IMP_GetConsoleMode,
        shim_GetConsoleMode
    );
    export_imp!(
        "_imp__WriteConsoleW@20",
        IMP_WriteConsoleW,
        shim_WriteConsoleW
    );
    export_imp!(
        "_imp__ReadConsoleW@20",
        IMP_ReadConsoleW,
        shim_ReadConsoleW
    );
    export_imp!(
        "_imp__FormatMessageW@28",
        IMP_FormatMessageW,
        shim_FormatMessageW
    );
    export_imp!(
        "_imp__MultiByteToWideChar@24",
        IMP_MultiByteToWideChar,
        shim_MultiByteToWideChar
    );
    export_imp!(
        "_imp__WideCharToMultiByte@32",
        IMP_WideCharToMultiByte,
        shim_WideCharToMultiByte
    );
    export_imp!(
        "_imp__GetWindowsDirectoryW@8",
        IMP_GetWindowsDirectoryW,
        shim_GetWindowsDirectoryW
    );
    export_imp!(
        "_imp__GetUserProfileDirectoryW@12",
        IMP_GetUserProfileDirectoryW,
        shim_GetUserProfileDirectoryW
    );
    export_imp!(
        "_imp__OpenProcessToken@12",
        IMP_OpenProcessToken,
        shim_OpenProcessToken
    );
    export_imp!(
        "_imp__CommandLineToArgvW@8",
        IMP_CommandLineToArgvW,
        shim_CommandLineToArgvW
    );
    export_imp!(
        "_imp__GetCurrentProcess@0",
        IMP_GetCurrentProcess,
        shim_GetCurrentProcess
    );
    export_imp!(
        "_imp__DuplicateHandle@28",
        IMP_DuplicateHandle,
        shim_DuplicateHandle
    );
    export_imp!("_imp__CancelIo@4", IMP_CancelIo, shim_CancelIo);
    export_imp!(
        "_imp__GetOverlappedResult@16",
        IMP_GetOverlappedResult,
        shim_GetOverlappedResult
    );
    export_imp!("_imp__CreateEventW@16", IMP_CreateEventW, shim_CreateEventW);
    export_imp!("_imp__SetEvent@4", IMP_SetEvent, shim_SetEvent);
    export_imp!(
        "_imp__WaitForSingleObject@8",
        IMP_WaitForSingleObject,
        shim_WaitForSingleObject
    );
    export_imp!(
        "_imp__WaitForMultipleObjects@16",
        IMP_WaitForMultipleObjects,
        shim_WaitForMultipleObjects
    );
    export_imp!("_imp__CreateThread@24", IMP_CreateThread, shim_CreateThread);
    export_imp!(
        "_imp__GetExitCodeThread@8",
        IMP_GetExitCodeThread,
        shim_GetExitCodeThread
    );
    export_imp!("_imp__TlsAlloc@0", IMP_TlsAlloc, shim_TlsAlloc);
    export_imp!("_imp__TlsGetValue@4", IMP_TlsGetValue, shim_TlsGetValue);
    export_imp!("_imp__TlsSetValue@8", IMP_TlsSetValue, shim_TlsSetValue);
    export_imp!("_imp__TlsFree@4", IMP_TlsFree, shim_TlsFree);
    export_imp!("_imp__FlsAlloc@4", IMP_FlsAlloc, shim_FlsAlloc);
    export_imp!("_imp__FlsFree@4", IMP_FlsFree, shim_FlsFree);
    export_imp!("_imp__FlsSetValue@8", IMP_FlsSetValue, shim_FlsSetValue);
    export_imp!("_imp__IsThreadAFiber@0", IMP_IsThreadAFiber, shim_IsThreadAFiber);
    export_imp!(
        "_imp__GetModuleHandleExW@12",
        IMP_GetModuleHandleExW,
        shim_GetModuleHandleExW
    );
    export_imp!(
        "_imp__GetSystemDirectoryW@8",
        IMP_GetSystemDirectoryW,
        shim_GetSystemDirectoryW
    );
    export_imp!(
        "_imp__GetCurrentThread@0",
        IMP_GetCurrentThread,
        shim_GetCurrentThread
    );
    export_imp!(
        "_imp__CompareStringOrdinal@20",
        IMP_CompareStringOrdinal,
        shim_CompareStringOrdinal
    );
    export_imp!("_imp__lstrlenW@4", IMP_lstrlenW, shim_lstrlenW);
    export_imp!(
        "_imp__WaitForSingleObjectEx@12",
        IMP_WaitForSingleObjectEx,
        shim_WaitForSingleObjectEx
    );
    export_imp!("_imp__CreateMutexA@12", IMP_CreateMutexA, shim_CreateMutexA);
    export_imp!("_imp__ReleaseMutex@4", IMP_ReleaseMutex, shim_ReleaseMutex);
    export_imp!("_imp__ReadFileEx@20", IMP_ReadFileEx, shim_ReadFileEx);
    export_imp!("_imp__WriteFileEx@20", IMP_WriteFileEx, shim_WriteFileEx);
    export_imp!("_imp__SleepEx@8", IMP_SleepEx, shim_SleepEx);
    export_imp!("_imp__SetFileTime@16", IMP_SetFileTime, shim_SetFileTime);
    export_imp!(
        "_imp__CreateProcessW@40",
        IMP_CreateProcessW,
        shim_CreateProcessW
    );
    export_imp!("_imp__NtOpenFile@24", IMP_NtOpenFile, shim_NtOpenFile);
    export_imp!(
        "_imp__RtlCaptureContext@4",
        IMP_RtlCaptureContext,
        shim_RtlCaptureContext
    );
    export_imp!(
        "_imp__NtCreateNamedPipeFile@56",
        IMP_NtCreateNamedPipeFile,
        shim_NtCreateNamedPipeFile
    );
}

#[cfg(target_arch = "x86")]
pub use x86_retro_impl::show_retro_panic_and_exit;

