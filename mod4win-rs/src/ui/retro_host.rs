//! Nativer Win32s (Windows 3.11) & Windows 95 GDI Multi-Window- & `waveOut`-Host (`retro-win32`).
//!
//! Stellt ein 100% kompatibles Drop-In-Ersatzmodul für `winit` und `pixels` auf Basis klassischer
//! 1993er Win32s-1.30c-/Win95-APIs (`USER32.DLL`, `GDI32.DLL`, `WINMM.DLL`, `SHELL32.DLL`) bereit:
//! - Alle 15 MOD4WIN-Fenster und alle 3 Skins (`2.30`, `2.40`, `Modern`) laufen unverändert.
//! - Rendering über kompatible Bottom-Up 24-Bit BGR DIBs (`BI_RGB`, positives `biHeight`),
//!   damit auch der 16-Bit-Thunking-Layer (`GDI.EXE`) unter Windows 3.11 + Win32s fehlerfrei zeichnet.
//! - Single-Thread `waveOutOpen(..., CALLBACK_WINDOW)` Audio-Engine:
//!   `WAVEHDR`-Strukturen und PCM-Puffer werden beim Öffnen des Audiogeräts **einmalig** im Heap
//!   alloziert und via `waveOutPrepareHeader` vorbereitet. Im `MM_WOM_DONE`-Handler wird der Puffer
//!   lediglich neu mit PCM-Samples befüllt und sofort wieder per `waveOutWrite` eingereiht
//!   (kein `waveOutUnprepareHeader`/`waveOutPrepareHeader` pro Block -> kein 16-Bit-Selector-Churn unter Win32s!).

#![allow(dead_code, non_snake_case)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::engine::sequencer::Sequencer;
use crate::engine::types::SoundSettings;
use crate::ui::app::{Mod4WinApp, UiSharedState};
use crate::ui::live_audio::{handle_audio_cycle_and_sync, LiveAudioProcessor};

// =============================================================================
// WIN32 API DEKLARATIONEN (100% Win32s 1.30c & Windows 95 kompatibel, ANSI *A)
// =============================================================================

type HWND = isize;
type HDC = isize;
type HINSTANCE = isize;
type HICON = isize;
type HCURSOR = isize;
type HBRUSH = isize;
type HMENU = isize;
type HDROP = isize;
type HWAVEOUT = isize;
type UINT = u32;
type DWORD = u32;
type WORD = u16;
type LONG = i32;
type BOOL = i32;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

const WS_POPUP: DWORD = 0x8000_0000;
const WS_VISIBLE: DWORD = 0x1000_0000;
const WS_CLIPSIBLINGS: DWORD = 0x0400_0000;
const WS_CLIPCHILDREN: DWORD = 0x0200_0000;

const CS_VREDRAW: UINT = 0x0001;
const CS_HREDRAW: UINT = 0x0002;

const PM_REMOVE: UINT = 0x0001;

const WM_DESTROY: UINT = 0x0002;
const WM_MOVE: UINT = 0x0003;
const WM_SIZE: UINT = 0x0005;
const WM_PAINT: UINT = 0x000F;
const WM_CLOSE: UINT = 0x0010;
const WM_QUIT: UINT = 0x0012;
const WM_ERASEBKGND: UINT = 0x0014;
const WM_SETCURSOR: UINT = 0x0020;
const WM_KEYDOWN: UINT = 0x0100;
const WM_KEYUP: UINT = 0x0101;
const WM_SYSKEYDOWN: UINT = 0x0104;
const WM_SYSKEYUP: UINT = 0x0105;
const WM_MOUSEMOVE: UINT = 0x0200;
const WM_LBUTTONDOWN: UINT = 0x0201;
const WM_LBUTTONUP: UINT = 0x0202;
const WM_RBUTTONDOWN: UINT = 0x0204;
const WM_RBUTTONUP: UINT = 0x0205;
const WM_MBUTTONDOWN: UINT = 0x0207;
const WM_MBUTTONUP: UINT = 0x0208;
const WM_MOUSEWHEEL: UINT = 0x020A;
const WM_DROPFILES: UINT = 0x0233;
const MM_WOM_DONE: UINT = 0x03BD;

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

const SWP_NOSIZE: UINT = 0x0001;
const SWP_NOMOVE: UINT = 0x0002;
const SWP_NOZORDER: UINT = 0x0004;
const SWP_NOACTIVATE: UINT = 0x0010;
const HWND_TOP: HWND = 0;

const IDC_ARROW: *const u8 = 32512 as *const u8;
const IDC_HAND: *const u8 = 32649 as *const u8;
const IDC_SIZEWE: *const u8 = 32644 as *const u8;
const IDC_SIZENS: *const u8 = 32645 as *const u8;
const IDC_SIZENWSE: *const u8 = 32642 as *const u8;
const IDC_SIZENESW: *const u8 = 32643 as *const u8;

const BI_RGB: DWORD = 0;
const DIB_RGB_COLORS: UINT = 0;
const SRCCOPY: DWORD = 0x00CC_0020;
const COLORONCOLOR: i32 = 3;

const WAVE_MAPPER: UINT = 0xFFFF_FFFF;
const WAVE_FORMAT_PCM: WORD = 1;
const CALLBACK_WINDOW: DWORD = 0x0001_0000;
const WHDR_DONE: DWORD = 0x0000_0001;
const WHDR_PREPARED: DWORD = 0x0000_0002;

#[repr(C)]
struct WndClassA {
    style: UINT,
    lpfn_wnd_proc: unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: HINSTANCE,
    h_icon: HICON,
    h_cursor: HCURSOR,
    hbr_background: HBRUSH,
    lpsz_menu_name: *const u8,
    lpsz_class_name: *const u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: LONG,
    y: LONG,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: LONG,
    top: LONG,
    right: LONG,
    bottom: LONG,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Msg {
    hwnd: HWND,
    message: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
    time: DWORD,
    pt: Point,
}

#[repr(C)]
struct PaintStruct {
    hdc: HDC,
    f_erase: BOOL,
    rc_paint: Rect,
    f_restore: BOOL,
    f_inc_update: BOOL,
    rgb_reserved: [u8; 32],
}

#[repr(C)]
struct BitmapInfoHeader {
    bi_size: DWORD,
    bi_width: LONG,
    bi_height: LONG,
    bi_planes: WORD,
    bi_bit_count: WORD,
    bi_compression: DWORD,
    bi_size_image: DWORD,
    bi_x_pels_per_meter: LONG,
    bi_y_pels_per_meter: LONG,
    bi_clr_used: DWORD,
    bi_clr_important: DWORD,
}

#[repr(C)]
struct BitmapInfo24 {
    bmi_header: BitmapInfoHeader,
    bmi_colors: [u32; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WaveFormatEx {
    w_format_tag: WORD,
    n_channels: WORD,
    n_samples_per_sec: DWORD,
    n_avg_bytes_per_sec: DWORD,
    n_block_align: WORD,
    w_bits_per_sample: WORD,
    cb_size: WORD,
}

#[repr(C)]
struct WaveHdr {
    lp_data: *mut u8,
    dw_buffer_length: DWORD,
    dw_bytes_recorded: DWORD,
    dw_user: usize,
    dw_flags: DWORD,
    dw_loops: DWORD,
    lp_next: *mut WaveHdr,
    reserved: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleA(lp_module_name: *const u8) -> HINSTANCE;
    fn LoadLibraryA(lp_lib_file_name: *const u8) -> HINSTANCE;
    fn GetProcAddress(h_module: HINSTANCE, lp_proc_name: *const u8) -> *mut c_void;
    fn GetVersion() -> DWORD;
    fn Sleep(dw_milliseconds: DWORD);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassA(lp_wnd_class: *const WndClassA) -> u16;
    fn CreateWindowExA(
        dw_ex_style: DWORD,
        lp_class_name: *const u8,
        lp_window_name: *const u8,
        dw_style: DWORD,
        x: i32,
        y: i32,
        n_width: i32,
        n_height: i32,
        h_wnd_parent: HWND,
        h_menu: HMENU,
        h_instance: HINSTANCE,
        lp_param: *mut c_void,
    ) -> HWND;
    fn DestroyWindow(h_wnd: HWND) -> BOOL;
    fn ShowWindow(h_wnd: HWND, n_cmd_show: i32) -> BOOL;
    fn UpdateWindow(h_wnd: HWND) -> BOOL;
    fn DefWindowProcA(h_wnd: HWND, msg: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
    fn PeekMessageA(
        lp_msg: *mut Msg,
        h_wnd: HWND,
        w_msg_filter_min: UINT,
        w_msg_filter_max: UINT,
        w_remove_msg: UINT,
    ) -> BOOL;
    fn TranslateMessage(lp_msg: *const Msg) -> BOOL;
    fn DispatchMessageA(lp_msg: *const Msg) -> LRESULT;
    fn BeginPaint(h_wnd: HWND, lp_paint: *mut PaintStruct) -> HDC;
    fn EndPaint(h_wnd: HWND, lp_paint: *const PaintStruct) -> BOOL;
    fn GetDC(h_wnd: HWND) -> HDC;
    fn ReleaseDC(h_wnd: HWND, h_dc: HDC) -> i32;
    fn InvalidateRect(h_wnd: HWND, lp_rect: *const Rect, b_erase: BOOL) -> BOOL;
    fn GetWindowRect(h_wnd: HWND, lp_rect: *mut Rect) -> BOOL;
    fn GetClientRect(h_wnd: HWND, lp_rect: *mut Rect) -> BOOL;
    fn SetWindowPos(
        h_wnd: HWND,
        h_wnd_insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        u_flags: UINT,
    ) -> BOOL;
    fn SetWindowTextA(h_wnd: HWND, lp_string: *const u8) -> BOOL;
    fn GetCursorPos(lp_point: *mut Point) -> BOOL;
    fn SetCapture(h_wnd: HWND) -> HWND;
    fn ReleaseCapture() -> BOOL;
    fn LoadIconA(h_instance: HINSTANCE, lp_icon_name: *const u8) -> HICON;
    fn LoadCursorA(h_instance: HINSTANCE, lp_cursor_name: *const u8) -> HCURSOR;
    fn SetCursor(h_cursor: HCURSOR) -> HCURSOR;
    fn GetSystemMetrics(n_index: i32) -> i32;
    fn GetKeyState(n_virt_key: i32) -> i16;
    fn GetKeyboardState(lp_key_state: *mut u8) -> BOOL;
    fn ToAscii(
        u_virt_key: UINT,
        u_scan_code: UINT,
        lp_key_state: *const u8,
        lp_char: *mut u16,
        u_flags: UINT,
    ) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn SetStretchBltMode(hdc: HDC, mode: i32) -> i32;
    fn SetDIBitsToDevice(
        hdc: HDC,
        x_dest: i32,
        y_dest: i32,
        w: DWORD,
        h: DWORD,
        x_src: i32,
        y_src: i32,
        start_scan: UINT,
        c_lines: UINT,
        lpv_bits: *const c_void,
        lpbmi: *const BitmapInfo24,
        color_use: UINT,
    ) -> i32;
    fn StretchDIBits(
        hdc: HDC,
        x_dest: i32,
        y_dest: i32,
        dest_width: i32,
        dest_height: i32,
        x_src: i32,
        y_src: i32,
        src_width: i32,
        src_height: i32,
        lp_bits: *const c_void,
        lpbmi: *const BitmapInfo24,
        i_usage: UINT,
        dw_rop: DWORD,
    ) -> i32;
}

/// Gibt `true` zurück, wenn das Binary unter Windows 3.11 mit Win32s läuft
/// (`GetVersion()`: Bit 31 gesetzt und Major-Version `< 4`).
#[inline]
fn is_running_on_win32s() -> bool {
    let ver = unsafe { GetVersion() };
    (ver & 0x8000_0000) != 0 && (ver & 0xFF) < 4
}

/// Lädt `SHELL32.DLL` ausschließlich dynamisch auf Windows 95 / NT (`!is_running_on_win32s()`),
/// damit die PE-Importtabelle von `MOD4WIN32.EXE` keinerlei statische Abhängigkeit auf `SHELL32.DLL`
/// besitzt (da `SHELL32.DLL` unter manchen Win32s-1.30-Installationen fehlt oder unvollständig ist).
unsafe fn dyn_drag_accept_files(h_wnd: HWND, f_accept: BOOL) {
    if is_running_on_win32s() {
        return;
    }
    unsafe {
        let mut h = GetModuleHandleA(b"SHELL32.DLL\0".as_ptr());
        if h == 0 {
            h = LoadLibraryA(b"SHELL32.DLL\0".as_ptr());
        }
        if h != 0 {
            let p = GetProcAddress(h, b"DragAcceptFiles\0".as_ptr());
            if !p.is_null() {
                let func: unsafe extern "system" fn(HWND, BOOL) = std::mem::transmute(p);
                func(h_wnd, f_accept);
            }
        }
    }
}

unsafe fn dyn_drag_query_file_a(
    h_drop: HDROP,
    i_file: UINT,
    lpsz_file: *mut u8,
    cch: UINT,
) -> UINT {
    unsafe {
        let h = GetModuleHandleA(b"SHELL32.DLL\0".as_ptr());
        if h != 0 {
            let p = GetProcAddress(h, b"DragQueryFileA\0".as_ptr());
            if !p.is_null() {
                let func: unsafe extern "system" fn(HDROP, UINT, *mut u8, UINT) -> UINT =
                    std::mem::transmute(p);
                return func(h_drop, i_file, lpsz_file, cch);
            }
        }
    }
    0
}

unsafe fn dyn_drag_finish(h_drop: HDROP) {
    unsafe {
        let h = GetModuleHandleA(b"SHELL32.DLL\0".as_ptr());
        if h != 0 {
            let p = GetProcAddress(h, b"DragFinish\0".as_ptr());
            if !p.is_null() {
                let func: unsafe extern "system" fn(HDROP) = std::mem::transmute(p);
                func(h_drop);
            }
        }
    }
}

#[link(name = "winmm")]
unsafe extern "system" {
    fn waveOutOpen(
        phwo: *mut HWAVEOUT,
        u_device_id: UINT,
        pwfx: *const WaveFormatEx,
        dw_callback: usize,
        dw_instance: usize,
        fdw_open: DWORD,
    ) -> UINT;
    fn waveOutClose(hwo: HWAVEOUT) -> UINT;
    fn waveOutPrepareHeader(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: UINT) -> UINT;
    fn waveOutUnprepareHeader(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: UINT) -> UINT;
    fn waveOutWrite(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: UINT) -> UINT;
    fn waveOutReset(hwo: HWAVEOUT) -> UINT;
}

// =============================================================================
// DROP-IN ERSATZMODULE FÜR `winit` UND `pixels`
// =============================================================================

pub mod dpi {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct PhysicalSize<T> {
        pub width: T,
        pub height: T,
    }
    impl<T> PhysicalSize<T> {
        pub const fn new(width: T, height: T) -> Self {
            Self { width, height }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct PhysicalPosition<T> {
        pub x: T,
        pub y: T,
    }
    impl<T> PhysicalPosition<T> {
        pub const fn new(x: T, y: T) -> Self {
            Self { x, y }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Default)]
    pub struct LogicalSize<T> {
        pub width: T,
        pub height: T,
    }
    impl<T> LogicalSize<T> {
        pub const fn new(width: T, height: T) -> Self {
            Self { width, height }
        }
    }
}

pub mod keyboard {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum NamedKey {
        Escape,
        Enter,
        Tab,
        Space,
        Backspace,
        Delete,
        Insert,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Control,
        Shift,
        Alt,
        Super,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    pub enum Key {
        Named(NamedKey),
        Character(String),
        Unidentified,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum KeyCode {
        Backquote,
        Backslash,
        BracketLeft,
        BracketRight,
        Comma,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Equal,
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Minus,
        Period,
        Quote,
        Semicolon,
        Slash,
        Space,
        Tab,
        Enter,
        Backspace,
        Delete,
        Insert,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Escape,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        NumpadDecimal,
        NumpadEnter,
        Unidentified,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum PhysicalKey {
        Code(KeyCode),
        Unidentified(u32),
    }
}

pub mod event {
    use super::dpi::{PhysicalPosition, PhysicalSize};
    use super::keyboard::{Key, PhysicalKey};
    use std::path::PathBuf;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ElementState {
        Pressed,
        Released,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum MouseButton {
        Left,
        Right,
        Middle,
        Back,
        Forward,
        Other(u16),
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum MouseScrollDelta {
        LineDelta(f32, f32),
        PixelDelta(PhysicalPosition<f64>),
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ModifiersState {
        pub shift: bool,
        pub ctrl: bool,
        pub alt: bool,
    }
    impl ModifiersState {
        pub fn shift_key(&self) -> bool {
            self.shift
        }
        pub fn control_key(&self) -> bool {
            self.ctrl
        }
        pub fn alt_key(&self) -> bool {
            self.alt
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Modifiers {
        state: ModifiersState,
    }
    impl Modifiers {
        pub fn new(shift: bool, ctrl: bool, alt: bool) -> Self {
            Self {
                state: ModifiersState { shift, ctrl, alt },
            }
        }
        pub fn state(&self) -> ModifiersState {
            self.state
        }
    }

    #[derive(Clone, Debug)]
    pub struct KeyEvent {
        pub physical_key: PhysicalKey,
        pub logical_key: Key,
        pub state: ElementState,
        pub repeat: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TouchPhase {
        Started,
        Moved,
        Ended,
        Cancelled,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TouchFingerId(pub u64);

    #[derive(Clone, Copy, Debug)]
    pub struct Touch {
        pub phase: TouchPhase,
        pub location: PhysicalPosition<f64>,
        pub id: u64,
    }

    #[derive(Clone, Debug)]
    pub enum WindowEvent {
        CloseRequested,
        Focused(bool),
        Resized(PhysicalSize<u32>),
        Moved(PhysicalPosition<i32>),
        ModifiersChanged(Modifiers),
        CursorMoved {
            position: PhysicalPosition<f64>,
        },
        MouseInput {
            state: ElementState,
            button: MouseButton,
        },
        MouseWheel {
            delta: MouseScrollDelta,
        },
        Touch(Touch),
        KeyboardInput {
            event: KeyEvent,
        },
        DroppedFile(PathBuf),
        RedrawRequested,
    }
}

pub mod window {
    use super::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct WindowId(pub HWND);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WindowLevel {
        AlwaysOnBottom,
        Normal,
        AlwaysOnTop,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CursorIcon {
        Default,
        Pointer,
        EwResize,
        NsResize,
        NwResize,
        NeResize,
        SwResize,
        SeResize,
        NwseResize,
        NeswResize,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ResizeDirection {
        East,
        North,
        NorthEast,
        NorthWest,
        South,
        SouthEast,
        SouthWest,
        West,
    }

    #[derive(Clone, Debug)]
    pub struct MonitorHandle {
        pub width: u32,
        pub height: u32,
    }
    impl MonitorHandle {
        pub fn position(&self) -> PhysicalPosition<i32> {
            PhysicalPosition::new(0, 0)
        }
        pub fn size(&self) -> PhysicalSize<u32> {
            PhysicalSize::new(self.width, self.height)
        }
    }

    #[derive(Clone, Debug)]
    pub struct WindowAttributes {
        pub title: String,
        pub decorations: bool,
        pub resizable: bool,
        pub skip_taskbar: bool,
        pub min_inner_size: Option<LogicalSize<f64>>,
        pub inner_size: Option<LogicalSize<f64>>,
        pub position: Option<PhysicalPosition<i32>>,
    }

    impl Default for WindowAttributes {
        fn default() -> Self {
            Self {
                title: String::from("MOD4WIN"),
                decorations: false,
                resizable: true,
                skip_taskbar: false,
                min_inner_size: None,
                inner_size: None,
                position: None,
            }
        }
    }

    impl WindowAttributes {
        pub fn with_title(mut self, title: impl Into<String>) -> Self {
            self.title = title.into();
            self
        }
        pub fn with_decorations(mut self, decorations: bool) -> Self {
            self.decorations = decorations;
            self
        }
        pub fn with_resizable(mut self, resizable: bool) -> Self {
            self.resizable = resizable;
            self
        }
        pub fn with_skip_taskbar(mut self, skip: bool) -> Self {
            self.skip_taskbar = skip;
            self
        }
        pub fn with_min_inner_size(mut self, size: LogicalSize<f64>) -> Self {
            self.min_inner_size = Some(size);
            self
        }
        pub fn with_inner_size(mut self, size: LogicalSize<f64>) -> Self {
            self.inner_size = Some(size);
            self
        }
        pub fn with_position(mut self, position: PhysicalPosition<i32>) -> Self {
            self.position = Some(position);
            self
        }
    }

    pub struct Window {
        pub(crate) hwnd: HWND,
        pub(crate) cursor_icon: AtomicUsize,
    }

    impl Window {
        pub fn default_attributes() -> WindowAttributes {
            WindowAttributes::default()
        }

        pub fn id(&self) -> WindowId {
            WindowId(self.hwnd)
        }

        pub fn scale_factor(&self) -> f64 {
            1.0
        }

        pub fn request_inner_size(&self, size: PhysicalSize<u32>) -> Option<PhysicalSize<u32>> {
            let w = size.width.max(1) as i32;
            let h = size.height.max(1) as i32;
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    0,
                    0,
                    0,
                    w,
                    h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            Some(size)
        }

        pub fn inner_size(&self) -> PhysicalSize<u32> {
            let mut rc = Rect::default();
            unsafe {
                if GetClientRect(self.hwnd, &mut rc) != 0 {
                    return PhysicalSize::new(
                        (rc.right - rc.left).max(1) as u32,
                        (rc.bottom - rc.top).max(1) as u32,
                    );
                }
            }
            self.outer_size()
        }

        pub fn outer_size(&self) -> PhysicalSize<u32> {
            let mut rc = Rect::default();
            unsafe {
                if GetWindowRect(self.hwnd, &mut rc) != 0 {
                    return PhysicalSize::new(
                        (rc.right - rc.left).max(1) as u32,
                        (rc.bottom - rc.top).max(1) as u32,
                    );
                }
            }
            PhysicalSize::new(320, 200)
        }

        pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, &'static str> {
            let mut rc = Rect::default();
            unsafe {
                if GetWindowRect(self.hwnd, &mut rc) != 0 {
                    return Ok(PhysicalPosition::new(rc.left, rc.top));
                }
            }
            Err("GetWindowRect failed")
        }

        pub fn set_outer_position(&self, pos: PhysicalPosition<i32>) {
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    0,
                    pos.x,
                    pos.y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        pub fn set_min_inner_size(&self, _size: Option<LogicalSize<f64>>) {}

        pub fn set_title(&self, title: &str) {
            let bytes = crate::win32s_shim::utf8_to_ansi_z(title);
            unsafe {
                SetWindowTextA(self.hwnd, bytes.as_ptr());
            }
        }

        pub fn set_cursor(&self, icon: CursorIcon) {
            let code = match icon {
                CursorIcon::Default => 0,
                CursorIcon::Pointer => 1,
                CursorIcon::EwResize => 2,
                CursorIcon::NsResize => 3,
                CursorIcon::NwResize | CursorIcon::SeResize | CursorIcon::NwseResize => 4,
                CursorIcon::NeResize | CursorIcon::SwResize | CursorIcon::NeswResize => 5,
            };
            self.cursor_icon.store(code, Ordering::Relaxed);
            apply_cursor_code(code);
        }

        pub fn is_visible(&self) -> Option<bool> {
            Some(true)
        }

        pub fn focus_window(&self) {
            self.raise_noactivate();
        }

        pub fn set_minimized(&self, minimized: bool) {
            unsafe {
                ShowWindow(self.hwnd, if minimized { 6 } else { 9 });
            }
        }

        pub fn set_window_level(&self, level: WindowLevel) {
            let insert_after: HWND = match level {
                WindowLevel::AlwaysOnTop => -1, // HWND_TOPMOST
                WindowLevel::Normal => -2,      // HWND_NOTOPMOST
                WindowLevel::AlwaysOnBottom => 1,
            };
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    insert_after,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }

        pub fn drag_window(&self) -> Result<(), &'static str> {
            start_Interactive_Move(self.hwnd);
            Ok(())
        }

        pub fn drag_resize_window(&self, direction: ResizeDirection) -> Result<(), &'static str> {
            start_interactive_resize(self.hwnd, direction);
            Ok(())
        }

        pub fn request_redraw(&self) {
            queue_window_event(
                WindowId(self.hwnd),
                super::event::WindowEvent::RedrawRequested,
            );
        }

        pub fn current_monitor(&self) -> Option<MonitorHandle> {
            unsafe {
                let sw = GetSystemMetrics(SM_CXSCREEN).max(640) as u32;
                let sh = GetSystemMetrics(SM_CYSCREEN).max(480) as u32;
                Some(MonitorHandle {
                    width: sw,
                    height: sh,
                })
            }
        }

        pub fn force_size(&self, phys_w: u32, phys_h: u32) {
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    0,
                    0,
                    0,
                    phys_w.max(1) as i32,
                    phys_h.max(1) as i32,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        pub fn raise_noactivate(&self) {
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    HWND_TOP,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            if self.hwnd != 0 {
                unregister_hwnd_surface(self.hwnd);
                unsafe {
                    DestroyWindow(self.hwnd);
                }
            }
        }
    }
}

pub mod event_loop {
    use super::window::{Window, WindowAttributes};
    use super::*;
    use std::time::Instant;

    #[derive(Clone, Copy, Debug)]
    pub enum ControlFlow {
        Poll,
        Wait,
        WaitUntil(Instant),
    }

    pub struct ActiveEventLoop {
        pub(crate) exiting: AtomicBool,
        pub(crate) control_flow: RefCell<ControlFlow>,
    }

    impl ActiveEventLoop {
        pub fn new() -> Self {
            Self {
                exiting: AtomicBool::new(false),
                control_flow: RefCell::new(ControlFlow::Poll),
            }
        }

        pub fn create_window(&self, attrs: WindowAttributes) -> Result<Window, &'static str> {
            ensure_window_class_registered();
            let (w, h) = attrs
                .inner_size
                .map(|s| (s.width.round().max(32.0) as i32, s.height.round().max(32.0) as i32))
                .unwrap_or((636, 135));
            let (x, y) = attrs
                .position
                .map(|p| (p.x, p.y))
                .unwrap_or_else(|| unsafe {
                    let sw = GetSystemMetrics(SM_CXSCREEN).max(640);
                    let sh = GetSystemMetrics(SM_CYSCREEN).max(480);
                    (((sw - w) / 2).max(0), ((sh - h) / 3).max(0))
                });

            let title_a = crate::win32s_shim::utf8_to_ansi_z(&attrs.title);

            const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
            let ex_style = if attrs.skip_taskbar {
                WS_EX_TOOLWINDOW
            } else {
                0
            };
            let hwnd = unsafe {
                let hinst = GetModuleHandleA(std::ptr::null());
                let hw = CreateWindowExA(
                    ex_style,
                    WND_CLASS_NAME.as_ptr(),
                    title_a.as_ptr(),
                    WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                    x,
                    y,
                    w,
                    h,
                    0,
                    0,
                    hinst,
                    std::ptr::null_mut(),
                );
                if hw != 0 {
                    dyn_drag_accept_files(hw, 1);
                    ShowWindow(hw, 5); // SW_SHOW
                    UpdateWindow(hw);
                    let hcur = LoadCursorA(0, IDC_ARROW);
                    if hcur != 0 {
                        SetCursor(hcur);
                    }
                }
                hw
            };

            if hwnd == 0 {
                return Err("CreateWindowExA failed");
            }

            register_main_hwnd_if_needed(hwnd);

            Ok(Window {
                hwnd,
                cursor_icon: AtomicUsize::new(0),
            })
        }

        pub fn exit(&self) {
            self.exiting.store(true, Ordering::SeqCst);
        }

        pub fn set_control_flow(&self, cf: ControlFlow) {
            *self.control_flow.borrow_mut() = cf;
        }
    }
}

pub mod application {
    use super::event::WindowEvent;
    use super::event_loop::ActiveEventLoop;
    use super::window::WindowId;

    pub trait ApplicationHandler {
        fn resumed(&mut self, event_loop: &ActiveEventLoop);
        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        );
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop);
    }
}

pub mod winit {
    pub use super::application;
    pub use super::dpi;
    pub use super::event;
    pub use super::event_loop;
    pub use super::keyboard;
    pub use super::window;
}

pub mod pixels {
    use super::window::Window;
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Copy, Debug)]
    pub enum ScalingMode {
        Fill,
    }

    pub mod wgpu {
        #[derive(Clone, Copy, Debug)]
        pub struct Backends(pub u32);
        impl Backends {
            pub const DX12: Self = Self(1);
            pub fn all() -> Self {
                Self(0xFF)
            }
        }

        #[derive(Clone, Copy, Debug)]
        pub enum PowerPreference {
            HighPerformance,
        }

        #[derive(Clone, Debug)]
        pub struct RequestAdapterOptions {
            pub power_preference: PowerPreference,
            pub force_fallback_adapter: bool,
            pub compatible_surface: Option<()>,
        }

        #[derive(Clone, Copy, Debug)]
        pub enum PresentMode {
            AutoNoVsync,
        }
    }

    pub struct SurfaceTexture {
        pub width: u32,
        pub height: u32,
        pub window: Arc<Window>,
    }

    impl SurfaceTexture {
        pub fn new(width: u32, height: u32, window: Arc<Window>) -> Self {
            Self {
                width: width.max(1),
                height: height.max(1),
                window,
            }
        }
    }

    pub struct PixelsBuilder {
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture,
    }

    impl PixelsBuilder {
        pub fn new(width: u32, height: u32, surface_texture: SurfaceTexture) -> Self {
            Self {
                width: width.max(1),
                height: height.max(1),
                surface_texture,
            }
        }
        pub fn wgpu_backend(self, _backends: wgpu::Backends) -> Self {
            self
        }
        pub fn request_adapter_options(self, _opts: wgpu::RequestAdapterOptions) -> Self {
            self
        }
        pub fn enable_vsync(self, _enable: bool) -> Self {
            self
        }
        pub fn present_mode(self, _mode: wgpu::PresentMode) -> Self {
            self
        }
        pub fn build(self) -> Result<Pixels<'static>, &'static str> {
            let w = self.width.max(1);
            let h = self.height.max(1);
            let stride = ((w * 3 + 3) & !3) as usize;
            let state = Rc::new(RefCell::new(GdiSurfaceBuffer {
                virt_w: w,
                virt_h: h,
                surf_w: self.surface_texture.width.max(1),
                surf_h: self.surface_texture.height.max(1),
                bgr24: vec![0u8; stride * (h as usize)],
            }));
            register_hwnd_surface(self.surface_texture.window.hwnd, Rc::clone(&state));
            Ok(Pixels {
                hwnd: self.surface_texture.window.hwnd,
                rgba: vec![0u8; (w * h * 4) as usize],
                state,
                _marker: std::marker::PhantomData,
            })
        }
    }

    pub struct Pixels<'a> {
        hwnd: HWND,
        rgba: Vec<u8>,
        state: Rc<RefCell<GdiSurfaceBuffer>>,
        _marker: std::marker::PhantomData<&'a ()>,
    }

    impl<'a> Pixels<'a> {
        pub fn set_scaling_mode(&mut self, _mode: ScalingMode) {}

        pub fn frame_mut(&mut self) -> &mut [u8] {
            &mut self.rgba
        }

        pub fn render(&self) -> Result<(), &'static str> {
            update_surface_bgr24(&mut self.state.borrow_mut(), &self.rgba);
            blit_hwnd_surface(self.hwnd, None);
            Ok(())
        }

        pub fn resize_buffer(&mut self, width: u32, height: u32) -> Result<(), &'static str> {
            let w = width.max(1);
            let h = height.max(1);
            let stride = ((w * 3 + 3) & !3) as usize;
            self.rgba.resize((w * h * 4) as usize, 0);
            let mut st = self.state.borrow_mut();
            st.virt_w = w;
            st.virt_h = h;
            st.bgr24.resize(stride * (h as usize), 0);
            Ok(())
        }

        pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), &'static str> {
            let mut st = self.state.borrow_mut();
            st.surf_w = width.max(1);
            st.surf_h = height.max(1);
            Ok(())
        }

        pub fn window_pos_to_pixel(&self, pos: (f32, f32)) -> Result<( usize, usize ), (isize, isize)> {
            let st = self.state.borrow();
            let sw = st.surf_w.max(1) as f32;
            let sh = st.surf_h.max(1) as f32;
            let vw = st.virt_w.max(1) as f32;
            let vh = st.virt_h.max(1) as f32;
            let vx = ((pos.0 / sw) * vw).floor() as isize;
            let vy = ((pos.1 / sh) * vh).floor() as isize;
            if vx >= 0 && vy >= 0 && (vx as u32) < st.virt_w && (vy as u32) < st.virt_h {
                Ok((vx as usize, vy as usize))
            } else {
                Err((vx, vy))
            }
        }
    }
}

// =============================================================================
// GDI DIB RENDERING & WIN32 EVENT-QUEUE (SINGLE-THREADED FÜR WIN32S & WIN95)
// =============================================================================

static WND_CLASS_NAME: &[u8] = b"MOD4WIN32_WND\0";
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

pub(crate) struct GdiSurfaceBuffer {
    virt_w: u32,
    virt_h: u32,
    surf_w: u32,
    surf_h: u32,
    bgr24: Vec<u8>,
}

#[derive(Clone, Copy)]
enum DragMode {
    None,
    MoveWindow {
        hwnd: HWND,
        start_mouse_x: i32,
        start_mouse_y: i32,
        start_win_x: i32,
        start_win_y: i32,
    },
    ResizeWindow {
        hwnd: HWND,
        dir: window::ResizeDirection,
        start_mouse_x: i32,
        start_mouse_y: i32,
        start_rect: Rect,
    },
}

struct ThreadUiHostState {
    surfaces: Vec<(HWND, Rc<RefCell<GdiSurfaceBuffer>>)>,
    events: VecDeque<(window::WindowId, event::WindowEvent)>,
    drag_mode: DragMode,
    main_hwnd: HWND,
    wom_done_triggered: bool,
}

thread_local! {
    static HOST_STATE: RefCell<ThreadUiHostState> = RefCell::new(ThreadUiHostState {
        surfaces: Vec::with_capacity(16),
        events: VecDeque::with_capacity(128),
        drag_mode: DragMode::None,
        main_hwnd: 0,
        wom_done_triggered: false,
    });
}

fn ensure_window_class_registered() {
    if CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        let hinst = GetModuleHandleA(std::ptr::null());
        let hicon = LoadIconA(hinst, 1usize as *const u8);
        let hcursor = LoadCursorA(0, IDC_ARROW);
        let wc = WndClassA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfn_wnd_proc: retro_wnd_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: hinst,
            h_icon: hicon,
            h_cursor: hcursor,
            hbr_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: WND_CLASS_NAME.as_ptr(),
        };
        RegisterClassA(&wc);
    }
}

fn register_main_hwnd_if_needed(hwnd: HWND) {
    HOST_STATE.with(|cell| {
        let mut st = cell.borrow_mut();
        if st.main_hwnd == 0 {
            st.main_hwnd = hwnd;
        }
    });
}

fn register_hwnd_surface(hwnd: HWND, surf: Rc<RefCell<GdiSurfaceBuffer>>) {
    HOST_STATE.with(|cell| {
        let mut st = cell.borrow_mut();
        if let Some(slot) = st.surfaces.iter_mut().find(|(h, _)| *h == hwnd) {
            slot.1 = surf;
        } else {
            st.surfaces.push((hwnd, surf));
        }
    });
}

fn unregister_hwnd_surface(hwnd: HWND) {
    HOST_STATE.with(|cell| {
        let mut st = cell.borrow_mut();
        st.surfaces.retain(|(h, _)| *h != hwnd);
    });
}

fn queue_window_event(id: window::WindowId, ev: event::WindowEvent) {
    HOST_STATE.with(|cell| {
        let mut st = cell.borrow_mut();
        if matches!(ev, event::WindowEvent::RedrawRequested) {
            if st
                .events
                .iter()
                .any(|(wid, e)| *wid == id && matches!(e, event::WindowEvent::RedrawRequested))
            {
                return;
            }
        }
        st.events.push_back((id, ev));
    });
}

fn apply_cursor_code(code: usize) {
    unsafe {
        let idc = match code {
            1 => IDC_HAND,
            2 => IDC_SIZEWE,
            3 => IDC_SIZENS,
            4 => IDC_SIZENWSE,
            5 => IDC_SIZENESW,
            _ => IDC_ARROW,
        };
        let mut hcur = LoadCursorA(0, idc);
        if hcur == 0 {
            hcur = LoadCursorA(0, IDC_ARROW);
        }
        if hcur != 0 {
            SetCursor(hcur);
        }
    }
}

fn start_Interactive_Move(hwnd: HWND) {
    let mut pt = Point::default();
    let mut rc = Rect::default();
    unsafe {
        GetCursorPos(&mut pt);
        GetWindowRect(hwnd, &mut rc);
        SetCapture(hwnd);
    }
    HOST_STATE.with(|cell| {
        cell.borrow_mut().drag_mode = DragMode::MoveWindow {
            hwnd,
            start_mouse_x: pt.x,
            start_mouse_y: pt.y,
            start_win_x: rc.left,
            start_win_y: rc.top,
        };
    });
}

fn start_interactive_resize(hwnd: HWND, dir: window::ResizeDirection) {
    let mut pt = Point::default();
    let mut rc = Rect::default();
    unsafe {
        GetCursorPos(&mut pt);
        GetWindowRect(hwnd, &mut rc);
        SetCapture(hwnd);
    }
    HOST_STATE.with(|cell| {
        cell.borrow_mut().drag_mode = DragMode::ResizeWindow {
            hwnd,
            dir,
            start_mouse_x: pt.x,
            start_mouse_y: pt.y,
            start_rect: rc,
        };
    });
}

/// Konvertiert den virtuellen RGBA-Puffer (`virt_w x virt_h`) in ein Bottom-Up 24-Bit BGR DIB
/// (`BI_RGB`, 4-Byte-aligned Scanlines).
fn update_surface_bgr24(st: &mut GdiSurfaceBuffer, rgba: &[u8]) {
    let vw = st.virt_w.max(1) as usize;
    let vh = st.virt_h.max(1) as usize;
    let stride = (vw * 3 + 3) & !3;
    if st.bgr24.len() < stride * vh {
        st.bgr24.resize(stride * vh, 0);
    }

    // Bottom-Up Konvertierung (Scanline 0 im DIB ist die unterste Bildschirmzeile `vh - 1`)
    for y in 0..vh {
        let src_row = (vh - 1 - y) * vw * 4;
        let dst_row = y * stride;
        if src_row + vw * 4 <= rgba.len() && dst_row + vw * 3 <= st.bgr24.len() {
            let src_slice = &rgba[src_row..src_row + vw * 4];
            let dst_slice = &mut st.bgr24[dst_row..dst_row + vw * 3];
            for x in 0..vw {
                let s = x * 4;
                let d = x * 3;
                dst_slice[d] = src_slice[s + 2]; // B
                dst_slice[d + 1] = src_slice[s + 1]; // G
                dst_slice[d + 2] = src_slice[s]; // R
            }
        }
    }
}

/// Blittet das vorbereitete Bottom-Up 24-Bit BGR DIB (`st.bgr24`) via `SetDIBitsToDevice` /
/// `StretchDIBits` auf das Win32-Fenster.
fn blit_hwnd_surface(hwnd: HWND, paint_hdc: Option<HDC>) {
    let surf_rc = HOST_STATE.with(|cell| {
        cell.borrow()
            .surfaces
            .iter()
            .find(|(h, _)| *h == hwnd)
            .map(|(_, s)| Rc::clone(s))
    });
    let Some(surf_rc) = surf_rc else {
        return;
    };

    let mut st = surf_rc.borrow_mut();
    let vw = st.virt_w.max(1) as usize;
    let vh = st.virt_h.max(1) as usize;
    let stride = (vw * 3 + 3) & !3;
    if st.bgr24.len() < stride * vh {
        st.bgr24.resize(stride * vh, 0);
    }

    let bmi = BitmapInfo24 {
        bmi_header: BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as DWORD,
            bi_width: vw as LONG,
            bi_height: vh as LONG, // Positiv = klassisches Bottom-Up DIB für Win3.11 GDI.EXE!
            bi_planes: 1,
            bi_bit_count: 24,
            bi_compression: BI_RGB,
            bi_size_image: (stride * vh) as DWORD,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        },
        bmi_colors: [0],
    };

    unsafe {
        let hdc = paint_hdc.unwrap_or_else(|| GetDC(hwnd));
        if hdc != 0 {
            let mut rc = Rect::default();
            let (dest_w, dest_h) = if GetClientRect(hwnd, &mut rc) != 0 {
                ((rc.right - rc.left).max(1), (rc.bottom - rc.top).max(1))
            } else {
                (st.surf_w as i32, st.surf_h as i32)
            };

            if dest_w == vw as i32 && dest_h == vh as i32 {
                SetDIBitsToDevice(
                    hdc,
                    0,
                    0,
                    vw as DWORD,
                    vh as DWORD,
                    0,
                    0,
                    0,
                    vh as UINT,
                    st.bgr24.as_ptr() as *const c_void,
                    &bmi,
                    DIB_RGB_COLORS,
                );
            } else {
                SetStretchBltMode(hdc, COLORONCOLOR);
                StretchDIBits(
                    hdc,
                    0,
                    0,
                    dest_w,
                    dest_h,
                    0,
                    0,
                    vw as i32,
                    vh as i32,
                    st.bgr24.as_ptr() as *const c_void,
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
            }

            if paint_hdc.is_none() {
                ReleaseDC(hwnd, hdc);
            }
        }
    }
}

fn current_modifiers() -> event::Modifiers {
    unsafe {
        let shift = (GetKeyState(0x10) as u16 & 0x8000) != 0; // VK_SHIFT
        let ctrl = (GetKeyState(0x11) as u16 & 0x8000) != 0;  // VK_CONTROL
        let alt = (GetKeyState(0x12) as u16 & 0x8000) != 0;   // VK_MENU
        event::Modifiers::new(shift, ctrl, alt)
    }
}

fn translate_vk_to_key_event(
    vk: UINT,
    l_param: LPARAM,
    state: event::ElementState,
) -> event::KeyEvent {
    use keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
    let scan_code = ((l_param as u32) >> 16) & 0xFF;
    let is_extended = ((l_param as u32) & (1 << 24)) != 0;
    let repeat = ((l_param as u32) & (1 << 30)) != 0 && state == event::ElementState::Pressed;

    let code = match vk {
        0x1B => KeyCode::Escape,
        0x0D => {
            if is_extended {
                KeyCode::NumpadEnter
            } else {
                KeyCode::Enter
            }
        }
        0x09 => KeyCode::Tab,
        0x20 => KeyCode::Space,
        0x08 => KeyCode::Backspace,
        0x2D => KeyCode::Insert,
        0x2E => KeyCode::Delete,
        0x24 => KeyCode::Home,
        0x23 => KeyCode::End,
        0x21 => KeyCode::PageUp,
        0x22 => KeyCode::PageDown,
        0x25 => KeyCode::ArrowLeft,
        0x26 => KeyCode::ArrowUp,
        0x27 => KeyCode::ArrowRight,
        0x28 => KeyCode::ArrowDown,
        0x70 => KeyCode::F1,
        0x71 => KeyCode::F2,
        0x72 => KeyCode::F3,
        0x73 => KeyCode::F4,
        0x74 => KeyCode::F5,
        0x75 => KeyCode::F6,
        0x76 => KeyCode::F7,
        0x77 => KeyCode::F8,
        0x78 => KeyCode::F9,
        0x79 => KeyCode::F10,
        0x7A => KeyCode::F11,
        0x7B => KeyCode::F12,
        0x30 => KeyCode::Digit0,
        0x31 => KeyCode::Digit1,
        0x32 => KeyCode::Digit2,
        0x33 => KeyCode::Digit3,
        0x34 => KeyCode::Digit4,
        0x35 => KeyCode::Digit5,
        0x36 => KeyCode::Digit6,
        0x37 => KeyCode::Digit7,
        0x38 => KeyCode::Digit8,
        0x39 => KeyCode::Digit9,
        0x41 => KeyCode::KeyA,
        0x42 => KeyCode::KeyB,
        0x43 => KeyCode::KeyC,
        0x44 => KeyCode::KeyD,
        0x45 => KeyCode::KeyE,
        0x46 => KeyCode::KeyF,
        0x47 => KeyCode::KeyG,
        0x48 => KeyCode::KeyH,
        0x49 => KeyCode::KeyI,
        0x4A => KeyCode::KeyJ,
        0x4B => KeyCode::KeyK,
        0x4C => KeyCode::KeyL,
        0x4D => KeyCode::KeyM,
        0x4E => KeyCode::KeyN,
        0x4F => KeyCode::KeyO,
        0x50 => KeyCode::KeyP,
        0x51 => KeyCode::KeyQ,
        0x52 => KeyCode::KeyR,
        0x53 => KeyCode::KeyS,
        0x54 => KeyCode::KeyT,
        0x55 => KeyCode::KeyU,
        0x56 => KeyCode::KeyV,
        0x57 => KeyCode::KeyW,
        0x58 => KeyCode::KeyX,
        0x59 => KeyCode::KeyY,
        0x5A => KeyCode::KeyZ,
        0x60 => KeyCode::Numpad0,
        0x61 => KeyCode::Numpad1,
        0x62 => KeyCode::Numpad2,
        0x63 => KeyCode::Numpad3,
        0x64 => KeyCode::Numpad4,
        0x65 => KeyCode::Numpad5,
        0x66 => KeyCode::Numpad6,
        0x67 => KeyCode::Numpad7,
        0x68 => KeyCode::Numpad8,
        0x69 => KeyCode::Numpad9,
        0x6A => KeyCode::NumpadMultiply,
        0x6B => KeyCode::NumpadAdd,
        0x6D => KeyCode::NumpadSubtract,
        0x6E => KeyCode::NumpadDecimal,
        0x6F => KeyCode::NumpadDivide,
        0xBA => KeyCode::Semicolon,
        0xBB => KeyCode::Equal,
        0xBC => KeyCode::Comma,
        0xBD => KeyCode::Minus,
        0xBE => KeyCode::Period,
        0xBF => KeyCode::Slash,
        0xC0 => KeyCode::Backquote,
        0xDB => KeyCode::BracketLeft,
        0xDC => KeyCode::Backslash,
        0xDD => KeyCode::BracketRight,
        0xDE => KeyCode::Quote,
        _ => KeyCode::Unidentified,
    };

    let named = match vk {
        0x1B => Some(NamedKey::Escape),
        0x0D => Some(NamedKey::Enter),
        0x09 => Some(NamedKey::Tab),
        0x20 => Some(NamedKey::Space),
        0x08 => Some(NamedKey::Backspace),
        0x2D => Some(NamedKey::Insert),
        0x2E => Some(NamedKey::Delete),
        0x24 => Some(NamedKey::Home),
        0x23 => Some(NamedKey::End),
        0x21 => Some(NamedKey::PageUp),
        0x22 => Some(NamedKey::PageDown),
        0x25 => Some(NamedKey::ArrowLeft),
        0x26 => Some(NamedKey::ArrowUp),
        0x27 => Some(NamedKey::ArrowRight),
        0x28 => Some(NamedKey::ArrowDown),
        0x70 => Some(NamedKey::F1),
        0x71 => Some(NamedKey::F2),
        0x72 => Some(NamedKey::F3),
        0x73 => Some(NamedKey::F4),
        0x74 => Some(NamedKey::F5),
        0x75 => Some(NamedKey::F6),
        0x76 => Some(NamedKey::F7),
        0x77 => Some(NamedKey::F8),
        0x78 => Some(NamedKey::F9),
        0x79 => Some(NamedKey::F10),
        0x7A => Some(NamedKey::F11),
        0x7B => Some(NamedKey::F12),
        _ => None,
    };

    let logical_key = if let Some(nk) = named {
        Key::Named(nk)
    } else {
        unsafe {
            let mut key_state = [0u8; 256];
            GetKeyboardState(key_state.as_mut_ptr());
            let mut ascii_word: u16 = 0;
            let res = ToAscii(vk, scan_code, key_state.as_ptr(), &mut ascii_word, 0);
            if res == 1 {
                let ch = (ascii_word & 0xFF) as u8;
                if ch >= 0x20 && ch != 0x7F {
                    Key::Character((ch as char).to_string())
                } else {
                    Key::Unidentified
                }
            } else {
                Key::Unidentified
            }
        }
    };

    event::KeyEvent {
        physical_key: PhysicalKey::Code(code),
        logical_key,
        state,
        repeat,
    }
}

unsafe extern "system" fn retro_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let win_id = window::WindowId(hwnd);

    match msg {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            let mut ps: PaintStruct = unsafe { std::mem::zeroed() };
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            if hdc != 0 {
                blit_hwnd_surface(hwnd, Some(hdc));
            }
            unsafe {
                EndPaint(hwnd, &ps);
            }
            return 0;
        }
        WM_CLOSE => {
            queue_window_event(win_id, event::WindowEvent::CloseRequested);
            return 0;
        }
        WM_MOVE => {
            let x = (l_param as u32 & 0xFFFF) as i16 as i32;
            let y = ((l_param as u32 >> 16) & 0xFFFF) as i16 as i32;
            queue_window_event(
                win_id,
                event::WindowEvent::Moved(dpi::PhysicalPosition::new(x, y)),
            );
            return 0;
        }
        WM_SIZE => {
            let w = (l_param as u32 & 0xFFFF) as u16 as u32;
            let h = ((l_param as u32 >> 16) & 0xFFFF) as u16 as u32;
            if w > 0 && h > 0 {
                queue_window_event(
                    win_id,
                    event::WindowEvent::Resized(dpi::PhysicalSize::new(w, h)),
                );
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            // Falls die linke Maustaste außerhalb/durch Fokusverlust losgelassen wurde (`MK_LBUTTON == 0x0001`),
            // hängendes Fenster-Drag/Resize sauber beenden!
            if (w_param & 0x0001) == 0 {
                let was_dragging = HOST_STATE.with(|cell| {
                    let mut st = cell.borrow_mut();
                    let active = !matches!(st.drag_mode, DragMode::None);
                    if active {
                        st.drag_mode = DragMode::None;
                    }
                    active
                });
                if was_dragging {
                    unsafe {
                        ReleaseCapture();
                    }
                }
            }
            // Prüfen, ob gerade ein Fenster gezogen oder an der Kante skaliert wird
            let active_drag = HOST_STATE.with(|cell| cell.borrow().drag_mode);
            match active_drag {
                DragMode::MoveWindow {
                    hwnd: drag_hwnd,
                    start_mouse_x,
                    start_mouse_y,
                    start_win_x,
                    start_win_y,
                } if drag_hwnd == hwnd => {
                    let mut pt = Point::default();
                    unsafe {
                        GetCursorPos(&mut pt);
                        let nx = start_win_x + (pt.x - start_mouse_x);
                        let ny = start_win_y + (pt.y - start_mouse_y);
                        SetWindowPos(
                            hwnd,
                            0,
                            nx,
                            ny,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    return 0;
                }
                DragMode::ResizeWindow {
                    hwnd: drag_hwnd,
                    dir,
                    start_mouse_x,
                    start_mouse_y,
                    start_rect,
                } if drag_hwnd == hwnd => {
                    let mut pt = Point::default();
                    unsafe {
                        GetCursorPos(&mut pt);
                    }
                    let dx = pt.x - start_mouse_x;
                    let dy = pt.y - start_mouse_y;
                    let mut left = start_rect.left;
                    let mut top = start_rect.top;
                    let mut right = start_rect.right;
                    let mut bottom = start_rect.bottom;
                    match dir {
                        window::ResizeDirection::East => right += dx,
                        window::ResizeDirection::South => bottom += dy,
                        window::ResizeDirection::SouthEast => {
                            right += dx;
                            bottom += dy;
                        }
                        window::ResizeDirection::West => left += dx,
                        window::ResizeDirection::North => top += dy,
                        window::ResizeDirection::NorthWest => {
                            left += dx;
                            top += dy;
                        }
                        window::ResizeDirection::NorthEast => {
                            right += dx;
                            top += dy;
                        }
                        window::ResizeDirection::SouthWest => {
                            left += dx;
                            bottom += dy;
                        }
                    }
                    let nw = (right - left).max(64);
                    let nh = (bottom - top).max(32);
                    unsafe {
                        SetWindowPos(hwnd, 0, left, top, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    return 0;
                }
                _ => {}
            }

            let x = (l_param as u32 & 0xFFFF) as i16 as f64;
            let y = ((l_param as u32 >> 16) & 0xFFFF) as i16 as f64;
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            queue_window_event(
                win_id,
                event::WindowEvent::CursorMoved {
                    position: dpi::PhysicalPosition::new(x, y),
                },
            );
            return 0;
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let x = (l_param as u32 & 0xFFFF) as i16 as f64;
            let y = ((l_param as u32 >> 16) & 0xFFFF) as i16 as f64;
            let button = match msg {
                WM_LBUTTONDOWN => event::MouseButton::Left,
                WM_RBUTTONDOWN => event::MouseButton::Right,
                _ => event::MouseButton::Middle,
            };
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            queue_window_event(
                win_id,
                event::WindowEvent::CursorMoved {
                    position: dpi::PhysicalPosition::new(x, y),
                },
            );
            queue_window_event(
                win_id,
                event::WindowEvent::MouseInput {
                    state: event::ElementState::Pressed,
                    button,
                },
            );
            return 0;
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            if msg == WM_LBUTTONUP {
                let was_dragging = HOST_STATE.with(|cell| {
                    let mut st = cell.borrow_mut();
                    let active = !matches!(st.drag_mode, DragMode::None);
                    st.drag_mode = DragMode::None;
                    active
                });
                if was_dragging {
                    unsafe {
                        ReleaseCapture();
                    }
                }
            }
            let button = match msg {
                WM_LBUTTONUP => event::MouseButton::Left,
                WM_RBUTTONUP => event::MouseButton::Right,
                _ => event::MouseButton::Middle,
            };
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            queue_window_event(
                win_id,
                event::WindowEvent::MouseInput {
                    state: event::ElementState::Released,
                    button,
                },
            );
            return 0;
        }
        WM_MOUSEWHEEL => {
            let z_delta = ((w_param as u32 >> 16) & 0xFFFF) as i16;
            let lines = (z_delta as f32) / 120.0;
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            queue_window_event(
                win_id,
                event::WindowEvent::MouseWheel {
                    delta: event::MouseScrollDelta::LineDelta(0.0, lines),
                },
            );
            return 0;
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            let kev =
                translate_vk_to_key_event(w_param as UINT, l_param, event::ElementState::Pressed);
            queue_window_event(win_id, event::WindowEvent::KeyboardInput { event: kev });
            if msg == WM_SYSKEYDOWN {
                return unsafe { DefWindowProcA(hwnd, msg, w_param, l_param) };
            }
            return 0;
        }
        WM_KEYUP | WM_SYSKEYUP => {
            queue_window_event(
                win_id,
                event::WindowEvent::ModifiersChanged(current_modifiers()),
            );
            let kev =
                translate_vk_to_key_event(w_param as UINT, l_param, event::ElementState::Released);
            queue_window_event(win_id, event::WindowEvent::KeyboardInput { event: kev });
            if msg == WM_SYSKEYUP {
                return unsafe { DefWindowProcA(hwnd, msg, w_param, l_param) };
            }
            return 0;
        }
        WM_DROPFILES => {
            let h_drop = w_param as HDROP;
            unsafe {
                let count = dyn_drag_query_file_a(h_drop, 0xFFFF_FFFF, std::ptr::null_mut(), 0);
                let mut buf = [0u8; 520];
                for i in 0..count {
                    let len =
                        dyn_drag_query_file_a(h_drop, i, buf.as_mut_ptr(), buf.len() as UINT)
                            as usize;
                    if len > 0 && len < buf.len() {
                        let s = crate::win32s_shim::ansi_bytes_to_string(&buf[..len]);
                        queue_window_event(
                            win_id,
                            event::WindowEvent::DroppedFile(PathBuf::from(s)),
                        );
                    }
                }
                dyn_drag_finish(h_drop);
            }
            return 0;
        }
        MM_WOM_DONE => {
            HOST_STATE.with(|cell| {
                cell.borrow_mut().wom_done_triggered = true;
            });
            return 0;
        }
        _ => unsafe { DefWindowProcA(hwnd, msg, w_param, l_param) },
    }
}

// =============================================================================
// WINMM WAVEOUT AUDIO ENGINE (REQUIREMENT #3: EINMALIGES PREPARE, IN-PLACE REFILL)
// =============================================================================

struct PreparedWaveBuffer {
    hdr: Box<WaveHdr>,
    pcm_bytes: Box<[u8]>,
}

struct RetroWaveOutEngine {
    hwo: HWAVEOUT,
    callback_hwnd: HWND,
    sample_rate: u32,
    channels: u16,
    bits: u16,
    frames_per_buf: usize,
    buffers: Vec<PreparedWaveBuffer>,
    stereo_scratch: Vec<(i16, i16)>,
    live_proc: LiveAudioProcessor,
    pending_settings: Option<SoundSettings>,
    dummy_finished: Arc<AtomicBool>,
}

impl RetroWaveOutEngine {
    /// Öffnet das `waveOut`-Gerät und bereitet alle `WAVEHDR`-Puffer **einmalig** im Heap vor
    /// (`waveOutPrepareHeader`). Bei `MM_WOM_DONE` werden die Puffer niemals neu prepared,
    /// sondern nur mit neuen Samples beschrieben und sofort via `waveOutWrite` eingereiht!
    fn open(
        callback_hwnd: HWND,
        ui_state: &Arc<Mutex<UiSharedState>>,
        seq_mutex: &Arc<Mutex<Sequencer>>,
    ) -> Self {
        let (desired_rate, desired_ch, desired_bits, num_bufs) = {
            let st = ui_state.lock().unwrap();
            (
                st.sound_settings.samplerate.clamp(8000, 48000) as u32,
                st.sound_settings.stereo.clamp(1, 2) as u16,
                if st.sound_settings.bits == 8 { 8u16 } else { 16u16 },
                st.sound_settings.n_buffers.clamp(4, 16) as usize,
            )
        };

        // Fallback-Kette für echte 486er Soundkarten (SB16, SB Pro, SB 2.0, WSS)
        let candidates: [(u32, u16, u16); 7] = [
            (desired_rate, desired_ch, desired_bits),
            (44100, 2, 16),
            (22050, 2, 16),
            (22050, 2, 8),
            (22050, 1, 8),
            (11025, 2, 8),
            (11025, 1, 8),
        ];

        let mut hwo: HWAVEOUT = 0;
        let mut actual_rate = desired_rate;
        let mut actual_ch = desired_ch;
        let mut actual_bits = desired_bits;

        for &(rate, ch, bits) in &candidates {
            let block_align = ch * (bits / 8);
            let wfx = WaveFormatEx {
                w_format_tag: WAVE_FORMAT_PCM,
                n_channels: ch,
                n_samples_per_sec: rate,
                n_avg_bytes_per_sec: rate * (block_align as u32),
                n_block_align: block_align,
                w_bits_per_sample: bits,
                cb_size: 0,
            };
            let mut try_hwo: HWAVEOUT = 0;
            let flags = if callback_hwnd != 0 {
                CALLBACK_WINDOW
            } else {
                0
            };
            let res = unsafe {
                waveOutOpen(
                    &mut try_hwo,
                    WAVE_MAPPER,
                    &wfx,
                    callback_hwnd as usize,
                    0,
                    flags,
                )
            };
            if res == 0 && try_hwo != 0 {
                hwo = try_hwo;
                actual_rate = rate;
                actual_ch = ch;
                actual_bits = bits;
                break;
            }
        }

        // ~40 ms pro Puffer (z.B. 1764 Frames bei 44.1 kHz, 882 Frames bei 22.05 kHz)
        let frames_per_buf = ((actual_rate as usize) / 25).clamp(256, 4096);
        let bytes_per_frame = (actual_ch as usize) * ((actual_bits / 8) as usize);
        let buf_byte_len = (frames_per_buf * bytes_per_frame).max(256);
        let silence_byte = if actual_bits == 8 { 0x80u8 } else { 0x00u8 };

        let mut buffers = Vec::with_capacity(num_bufs);
        if hwo != 0 {
            for _ in 0..num_bufs {
                let mut pcm_bytes = vec![silence_byte; buf_byte_len].into_boxed_slice();
                let mut hdr = Box::new(WaveHdr {
                    lp_data: pcm_bytes.as_mut_ptr(),
                    dw_buffer_length: buf_byte_len as DWORD,
                    dw_bytes_recorded: 0,
                    dw_user: 0,
                    dw_flags: 0,
                    dw_loops: 0,
                    lp_next: std::ptr::null_mut(),
                    reserved: 0,
                });
                let prep_ok = unsafe {
                    let prep_res = waveOutPrepareHeader(
                        hwo,
                        &mut *hdr as *mut WaveHdr,
                        std::mem::size_of::<WaveHdr>() as UINT,
                    );
                    if prep_res == 0 {
                        hdr.dw_flags |= WHDR_DONE;
                        true
                    } else {
                        false
                    }
                };
                if prep_ok {
                    buffers.push(PreparedWaveBuffer { hdr, pcm_bytes });
                }
            }
        }

        let mut engine = Self {
            hwo,
            callback_hwnd,
            sample_rate: actual_rate,
            channels: actual_ch,
            bits: actual_bits,
            frames_per_buf,
            buffers,
            stereo_scratch: vec![(0i16, 0i16); frames_per_buf],
            live_proc: LiveAudioProcessor::new(actual_rate),
            pending_settings: None,
            dummy_finished: Arc::new(AtomicBool::new(false)),
        };

        // Sofort alle vorbereiteten Puffer initial befüllen und per waveOutWrite einreihen
        engine.refill_completed_buffers(ui_state, seq_mutex);
        engine
    }

    /// Prüft alle einmalig vorbereiteten `WAVEHDR`-Puffer; jeder Puffer mit `WHDR_DONE` wird
    /// in-place neu mit PCM-Samples befüllt und unmittelbar per `waveOutWrite` wieder eingereiht
    /// (KEIN `waveOutUnprepareHeader`/`waveOutPrepareHeader`!).
    fn refill_completed_buffers(
        &mut self,
        ui_state: &Arc<Mutex<UiSharedState>>,
        seq_mutex: &Arc<Mutex<Sequencer>>,
    ) {
        if self.hwo == 0 {
            return;
        }

        let sr_f32 = (self.sample_rate as f32).max(8000.0);

        for buf in &mut self.buffers {
            if (buf.hdr.dw_flags & WHDR_DONE) == 0 {
                continue;
            }

            let (cached_playing, cached_paused, cached_stutter, cached_muted) = {
                let mut seq = seq_mutex.lock().unwrap();
                let mut ui = ui_state.lock().unwrap();
                handle_audio_cycle_and_sync(
                    &mut ui,
                    &mut seq,
                    &mut self.live_proc,
                    &mut self.pending_settings,
                    false,
                    &self.dummy_finished,
                );
                (
                    ui.is_playing,
                    ui.is_paused,
                    ui.ini_config.authentic_stutter && ui.cpu_telemetry.buffer_underrun,
                    ui.channel_muted,
                )
            };

            let num_frames = self.frames_per_buf;
            if self.stereo_scratch.len() < num_frames {
                self.stereo_scratch.resize(num_frames, (0, 0));
            }

            let t0 = Instant::now();
            {
                let mut seq = seq_mutex.lock().unwrap();
                self.live_proc.fill_cpal_stereo_frames(
                    &mut seq,
                    &mut self.stereo_scratch[..num_frames],
                    cached_playing,
                    cached_paused,
                    cached_stutter,
                    &cached_muted,
                    &mut self.pending_settings,
                );
            }
            let cb_secs = t0.elapsed().as_secs_f32();
            let buf_secs = (num_frames as f32) / sr_f32;
            let measured_ratio = (cb_secs / buf_secs.max(0.0005)).clamp(0.001, 1.0);

            // In-place Konvertierung in das Hardware-PCM-Format des vorbereiteten WAVEHDR-Puffers
            match (self.channels, self.bits) {
                (2, 16) => {
                    for (i, &(l, r)) in self.stereo_scratch[..num_frames].iter().enumerate() {
                        let off = i * 4;
                        let lb = l.to_le_bytes();
                        let rb = r.to_le_bytes();
                        buf.pcm_bytes[off] = lb[0];
                        buf.pcm_bytes[off + 1] = lb[1];
                        buf.pcm_bytes[off + 2] = rb[0];
                        buf.pcm_bytes[off + 3] = rb[1];
                    }
                }
                (1, 16) => {
                    for (i, &(l, r)) in self.stereo_scratch[..num_frames].iter().enumerate() {
                        let mono = ((l as i32 + r as i32) >> 1) as i16;
                        let mb = mono.to_le_bytes();
                        let off = i * 2;
                        buf.pcm_bytes[off] = mb[0];
                        buf.pcm_bytes[off + 1] = mb[1];
                    }
                }
                (2, 8) => {
                    for (i, &(l, r)) in self.stereo_scratch[..num_frames].iter().enumerate() {
                        let off = i * 2;
                        buf.pcm_bytes[off] = (((l >> 8) as i32) + 128).clamp(0, 255) as u8;
                        buf.pcm_bytes[off + 1] = (((r >> 8) as i32) + 128).clamp(0, 255) as u8;
                    }
                }
                _ => {
                    for (i, &(l, r)) in self.stereo_scratch[..num_frames].iter().enumerate() {
                        let mono = ((l as i32 + r as i32) >> 9) + 128;
                        buf.pcm_bytes[i] = mono.clamp(0, 255) as u8;
                    }
                }
            }

            // WHDR_DONE löschen, WHDR_PREPARED beibehalten und sofort wieder via waveOutWrite einreihen!
            buf.hdr.dw_flags = (buf.hdr.dw_flags & !WHDR_DONE) | WHDR_PREPARED;
            unsafe {
                let wr_res = waveOutWrite(
                    self.hwo,
                    &mut *buf.hdr as *mut WaveHdr,
                    std::mem::size_of::<WaveHdr>() as UINT,
                );
                if wr_res != 0 {
                    buf.hdr.dw_flags |= WHDR_DONE;
                }
            }

            // UI-Metadaten & VU-Meter aktualisieren
            {
                let seq = seq_mutex.lock().unwrap();
                let elapsed = (seq.module.state.play_time / 1000).max(0) as u32;
                let mut ui = ui_state.lock().unwrap();
                if cached_playing && !cached_paused {
                    ui.native_callback_ratio =
                        ui.native_callback_ratio * 0.8 + measured_ratio * 0.2;
                }
                if ui.current_file_path.is_some() || cached_playing {
                    ui.update_from_sequencer(&seq, elapsed);
                }
                ui.update_output_vu_from_pcm(&self.stereo_scratch[..num_frames]);
            }
        }
    }

    fn close_device(&mut self) {
        if self.hwo != 0 {
            unsafe {
                waveOutReset(self.hwo);
                for buf in &mut self.buffers {
                    waveOutUnprepareHeader(
                        self.hwo,
                        &mut *buf.hdr as *mut WaveHdr,
                        std::mem::size_of::<WaveHdr>() as UINT,
                    );
                }
                waveOutClose(self.hwo);
            }
            self.hwo = 0;
            self.buffers.clear();
        }
    }
}

impl Drop for RetroWaveOutEngine {
    fn drop(&mut self) {
        self.close_device();
    }
}

/// Startet die vereinheitlichte Single-Thread Win32s/Win95 Event- & `waveOut`-Schleife.
pub fn run_retro_win32_player(
    shared_ui_state: Arc<Mutex<UiSharedState>>,
    sequencer_mutex: Arc<Mutex<Sequencer>>,
    _no_gui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use application::ApplicationHandler;

    let is_win32s = is_running_on_win32s();
    let event_loop = event_loop::ActiveEventLoop::new();
    let mut app = Box::new(Mod4WinApp::new(Arc::clone(&shared_ui_state)));

    // 1. Hauptfenster öffnen (`resumed` erstellt das Main-HWND und registriert `main_hwnd`)
    app.resumed(&event_loop);

    let main_hwnd = HOST_STATE.with(|cell| cell.borrow().main_hwnd);

    // Sofort initiales Redraw & Message-Pump ausführen, damit das Fenster unter Windows 3.11 / Win95
    // bereits vollständig gezeichnet ist und die Windows-3.11-Start-Sanduhr (`IDC_WAIT`) verschwindet,
    // noch bevor das `waveOut`-Audiogerät initialisiert wird.
    if main_hwnd != 0 {
        queue_window_event(
            window::WindowId(main_hwnd),
            event::WindowEvent::RedrawRequested,
        );
        loop {
            let next_ev = HOST_STATE.with(|cell| cell.borrow_mut().events.pop_front());
            let Some((wid, ev)) = next_ev else {
                break;
            };
            app.window_event(&event_loop, wid, ev);
        }
        unsafe {
            UpdateWindow(main_hwnd);
            let hcur = LoadCursorA(0, IDC_ARROW);
            if hcur != 0 {
                SetCursor(hcur);
            }
        }
    }

    // 2. `waveOut`-Engine öffnen (einmalige Heap-Allokation & `waveOutPrepareHeader` aller Puffer!)
    let mut wave_engine =
        RetroWaveOutEngine::open(main_hwnd, &shared_ui_state, &sequencer_mutex);

    // 3. Single-Thread Win32 Message- & Audio-Loop
    while !event_loop.exiting.load(Ordering::SeqCst) {
        unsafe {
            let mut msg: Msg = std::mem::zeroed();
            while PeekMessageA(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    event_loop.exit();
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        }
        if event_loop.exiting.load(Ordering::SeqCst) {
            break;
        }

        // Audio-Puffer nachfüllen (ausgelöst durch `MM_WOM_DONE` oder zur Sicherheit bei jedem Zyklus)
        HOST_STATE.with(|cell| {
            cell.borrow_mut().wom_done_triggered = false;
        });
        wave_engine.refill_completed_buffers(&shared_ui_state, &sequencer_mutex);

        // Aufgelaufene Fenster-Events an `Mod4WinApp` zustellen
        loop {
            let next_ev = HOST_STATE.with(|cell| cell.borrow_mut().events.pop_front());
            let Some((wid, ev)) = next_ev else {
                break;
            };
            app.window_event(&event_loop, wid, ev);
            if event_loop.exiting.load(Ordering::SeqCst) {
                break;
            }
        }
        if event_loop.exiting.load(Ordering::SeqCst) {
            break;
        }

        app.about_to_wait(&event_loop);

        // Falls RedrawRequested-Events durch `about_to_wait` eingereiht wurden, sofort abarbeiten
        loop {
            let next_ev = HOST_STATE.with(|cell| cell.borrow_mut().events.pop_front());
            let Some((wid, ev)) = next_ev else {
                break;
            };
            app.window_event(&event_loop, wid, ev);
            if event_loop.exiting.load(Ordering::SeqCst) {
                break;
            }
        }

        // Unter Windows 95 / NT kurze 1-ms-Schonpause; unter Windows 3.11 (Win32s) ist `PeekMessageA`
        // bereits der kooperative 16-Bit-Yield-Punkt (während `Sleep` in `W32SKRNL.DLL` auf 55-ms-Timer-Ticks wartet).
        if !is_win32s {
            unsafe {
                Sleep(1);
            }
        }
    }

    wave_engine.close_device();
    Ok(())
}
