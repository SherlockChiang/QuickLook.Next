//! QuickLook Next — native spike (Rust cdylib).
//!
//! Validates the three native unknowns:
//!   1. C ABI export + Rust→C# function-pointer callback (string intents).
//!   2. WH_KEYBOARD_LL hook on a dedicated pumped thread; the hook proc stays cheap and posts a
//!      thread message, so the (latency-critical) COM selection read happens off the hook callback.
//!   3. Explorer current selection via COM (IShellWindows → IShellBrowser → IFolderView).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader};

mod native_input;
mod preview;
mod rar_listing;
mod win32;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const VK_SPACE_U32: u32 = 0x20;
const VK_ESCAPE_U32: u32 = 0x1B;
const VK_SHIFT_U32: u32 = 0x10;
const VK_CONTROL_U32: u32 = 0x11;
const VK_MENU_U32: u32 = 0x12;
const VK_LEFT_U32: u32 = 0x25;
const VK_UP_U32: u32 = 0x26;
const VK_RIGHT_U32: u32 = 0x27;
const VK_DOWN_U32: u32 = 0x28;
const VK_OEM_PLUS_U32: u32 = 0xBB; // '=' / '+'
const VK_OEM_MINUS_U32: u32 = 0xBD; // '-' / '_'
const VK_ADD_U32: u32 = 0x6B; // numpad +
const VK_SUBTRACT_U32: u32 = 0x6D; // numpad -
const VK_F5_U32: u32 = 0x74;
const VK_F11_U32: u32 = 0x7A;

type Callback = unsafe extern "C" fn(*const u16);
pub type CancelCallback = extern "C" fn() -> bool;
pub type AnimationOutputCallback = extern "C" fn(usize) -> *mut u8;
type AnimationFrameBgra = (u32, Vec<u8>);
type DecodedAnimationBgra = (u32, u32, Vec<AnimationFrameBgra>);

/// Reader adapter used at image-codec I/O boundaries. It cannot interrupt one already-running
/// OS read or a codec's internal CPU loop, but it makes cancellation observable before and after
/// every decoder read/seek so a stale preview does not wait for another full input pass.
struct CancelableImageReader<R> {
    reader: R,
    cancel_cb: Option<CancelCallback>,
}

impl<R> CancelableImageReader<R> {
    fn new(reader: R, cancel_cb: Option<CancelCallback>) -> Self {
        Self { reader, cancel_cb }
    }

    fn cancelled_error() -> io::Error {
        io::Error::other("preview cancelled")
    }
}

impl<R: Read> Read for CancelableImageReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if cancel_requested(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        let read = self.reader.read(buffer)?;
        if cancel_requested(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        Ok(read)
    }
}

impl<R: Seek> Seek for CancelableImageReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if cancel_requested(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        let offset = self.reader.seek(position)?;
        if cancel_requested(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        Ok(offset)
    }
}

static CALLBACK: Mutex<Option<Callback>> = Mutex::new(None);
static HOOK_THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
static SPACE_HELD: AtomicBool = AtomicBool::new(false);
static F5_HELD: AtomicBool = AtomicBool::new(false);
static F11_HELD: AtomicBool = AtomicBool::new(false);
static PREVIEW_VISIBLE: AtomicBool = AtomicBool::new(false);
const WM_QL_PREVIEW: u32 = WM_APP + 1;
const WM_QL_CLOSE: u32 = WM_APP + 3;
const WM_QL_ZOOM_IN: u32 = WM_APP + 4;
const WM_QL_ZOOM_OUT: u32 = WM_APP + 5;
const WM_QL_SWITCH_DELAYED: u32 = WM_APP + 6;
const WM_QL_RELOAD: u32 = WM_APP + 7;
const WM_QL_FULLSCREEN: u32 = WM_APP + 8;
const SWITCH_TIMER_ID: usize = 1;
static SWITCH_TIMER_ARMED: AtomicUsize = AtomicUsize::new(0);
static SVG_FONT_DATABASE: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();

thread_local! {
    static SHELL_WINDOWS_CACHE: std::cell::RefCell<Option<IShellWindows>> = const { std::cell::RefCell::new(None) };
}

// A valid extended Windows path may contain 32,767 UTF-16 units, each requiring up to four UTF-8 bytes.
const MAX_FFI_STRING_BYTES: usize = 128 * 1024;
const MAX_FFI_MAGIC_BYTES: usize = 4096;
const MAX_LOGICAL_NAME_BYTES: usize = 4 * 255;
const MAX_OFFICE_IMAGE_REF_BYTES: usize = 2048;
const MAX_ARCHIVE_ENTRY_NAME_BYTES: usize = u16::MAX as usize;
const QL_NATIVE_ABI_VERSION: u32 = 3;
const QL_FEATURE_HANDLE_TEXT: u64 = 1 << 0;
const QL_FEATURE_HANDLE_EXECUTABLE: u64 = 1 << 1;
const QL_FEATURE_HANDLE_TORRENT: u64 = 1 << 2;
const QL_FEATURE_HANDLE_SQLITE_SNAPSHOT: u64 = 1 << 3;
const QL_FEATURE_HANDLE_ARCHIVE: u64 = 1 << 4;
const QL_FEATURE_HANDLE_OFFICE: u64 = 1 << 5;
const QL_FEATURE_HANDLE_EBOOK: u64 = 1 << 6;
const QL_FEATURE_HANDLE_ARCHIVE_ENTRY: u64 = 1 << 7;
const QL_FEATURE_HANDLE_STATIC_IMAGE: u64 = 1 << 8;
const QL_FEATURE_HANDLE_SVG: u64 = 1 << 9;
const QL_FEATURE_HANDLE_GIF: u64 = 1 << 10;
const QL_FEATURE_HANDLE_PACKAGE: u64 = 1 << 11;
const QL_FEATURE_HANDLE_PACKAGE_ICON: u64 = 1 << 12;
const QL_FEATURE_HANDLE_PROBE: u64 = 1 << 13;
const QL_FEATURE_HANDLE_RASTER_IMAGE: u64 = 1 << 14;
const QL_FEATURE_HANDLE_ANIMATION: u64 = 1 << 15;
const QL_FEATURE_HANDLE_OFFICE_LAYOUT_IMAGE: u64 = 1 << 16;
const QL_FEATURE_HANDLE_IMAGE_WAVEFORM: u64 = 1 << 17;
const QL_FEATURE_HANDLE_ARCHIVE_ENTRY_OUTPUT: u64 = 1 << 18;
const QL_FEATURE_HANDLE_IMAGE_METADATA: u64 = 1 << 19;
const QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT: u64 = 1 << 20;
const QL_FEATURE_HANDLE_MAIL: u64 = 1 << 21;

const QL_OK: i32 = 0;
const QL_ERROR_INVALID_ARGUMENT: i32 = -1;
const QL_ERROR_BUFFER_TOO_SMALL: i32 = -2;
const QL_ERROR_CANCELLED: i32 = -3;
const QL_ERROR_MALFORMED: i32 = -4;
const QL_ERROR_IO: i32 = -5;
const QL_ERROR_INVALID_HANDLE: i32 = -6;
const QL_ERROR_LENGTH_MISMATCH: i32 = -7;
const QL_ERROR_INTERNAL: i32 = -8;
const QL_ERROR_LIMIT_EXCEEDED: i32 = -9;

#[no_mangle]
pub extern "C" fn ql_abi_version() -> u32 {
    QL_NATIVE_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn ql_capabilities() -> u64 {
    QL_FEATURE_HANDLE_TEXT
        | QL_FEATURE_HANDLE_EXECUTABLE
        | QL_FEATURE_HANDLE_TORRENT
        | QL_FEATURE_HANDLE_SQLITE_SNAPSHOT
        | QL_FEATURE_HANDLE_ARCHIVE
        | QL_FEATURE_HANDLE_OFFICE
        | QL_FEATURE_HANDLE_EBOOK
        | QL_FEATURE_HANDLE_ARCHIVE_ENTRY
        | QL_FEATURE_HANDLE_STATIC_IMAGE
        | QL_FEATURE_HANDLE_SVG
        | QL_FEATURE_HANDLE_GIF
        | QL_FEATURE_HANDLE_PACKAGE
        | QL_FEATURE_HANDLE_PACKAGE_ICON
        | QL_FEATURE_HANDLE_PROBE
        | QL_FEATURE_HANDLE_RASTER_IMAGE
        | QL_FEATURE_HANDLE_ANIMATION
        | QL_FEATURE_HANDLE_OFFICE_LAYOUT_IMAGE
        | QL_FEATURE_HANDLE_IMAGE_WAVEFORM
        | QL_FEATURE_HANDLE_ARCHIVE_ENTRY_OUTPUT
        | QL_FEATURE_HANDLE_IMAGE_METADATA
        | QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT
        | QL_FEATURE_HANDLE_MAIL
}
const MAX_NATIVE_IMAGE_DECODE_PIXELS: u64 = 48_000_000;
const MAX_NATIVE_IMAGE_DECODE_PEAK_BYTES: u64 = 896 * 1024 * 1024;
const MAX_ANIMATED_SOURCE_PIXELS: u64 = 16_000_000;
const MAX_ANIMATED_FRAME_DIMENSION: u32 = 1024;
const MAX_ANIMATED_FRAMES: usize = 120;
const MAX_ANIMATED_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_ANIMATION_HANDLE_INPUT_BYTES: u64 = 256 * 1024 * 1024;
fn utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() || len > max_len {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
}

/// Copy a bounded UTF-8 FFI argument into Rust-owned storage.
///
/// # Safety
/// `ptr` must be readable for `len` bytes for the duration of this call.
unsafe fn owned_utf8_arg(ptr: *const u8, len: usize, max_len: usize) -> Option<String> {
    if ptr.is_null() || len > max_len {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

fn optional_utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return (len == 0).then_some("");
    }
    utf8_arg(ptr, len, max_len)
}

fn optional_bytes_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return (len == 0).then_some(&[]);
    }
    if len > max_len {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Send a tagged UTF-16 string back to the managed host.
fn emit(msg: &str) {
    let cb = CALLBACK.lock().ok().and_then(|guard| *guard);
    if let Some(cb) = cb {
        let mut wide: Vec<u16> = msg.encode_utf16().collect();
        wide.push(0);
        unsafe { cb(wide.as_ptr()) };
    }
}

/// Trivial probe — confirms the cdylib loads and the C ABI works.
#[no_mangle]
pub extern "C" fn ql_probe(a: i32, b: i32) -> i32 {
    ffi_boundary(|| a + b)
}

/// Register the managed callback (a function pointer obtained from a kept-alive delegate).
#[no_mangle]
pub extern "C" fn ql_set_callback(cb: Option<Callback>) {
    if let Ok(mut slot) = CALLBACK.lock() {
        *slot = cb;
    }
}

/// Let the App tell native when the preview window is open. While visible, Space closes the preview.
/// Selection changes are still accepted only when Explorer is the foreground window.
#[no_mangle]
pub extern "C" fn ql_set_preview_visible(visible: i32) {
    PREVIEW_VISIBLE.store(visible != 0, Ordering::SeqCst);
}

/// Install the low-level keyboard hook on a dedicated thread with a message pump.
#[no_mangle]
pub extern "C" fn ql_start() -> i32 {
    ffi_boundary(|| {
        let Ok(mut runtime) = HOOK_THREAD.lock() else {
            return -8;
        };
        if runtime.is_some() {
            return 2;
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || hook_thread(ready_tx));
        *runtime = Some(thread);
        drop(runtime);
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(true) => 1,
            _ => {
                ql_stop();
                -8
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn ql_stop() -> i32 {
    ffi_boundary(|| {
        let thread = HOOK_THREAD
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.take());
        let Some(thread) = thread else {
            return 1;
        };
        let tid = HOOK_TID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        let _ = thread.join();
        HOOK_TID.store(0, Ordering::SeqCst);
        SPACE_HELD.store(false, Ordering::SeqCst);
        F5_HELD.store(false, Ordering::SeqCst);
        F11_HELD.store(false, Ordering::SeqCst);
        PREVIEW_VISIBLE.store(false, Ordering::SeqCst);
        SWITCH_TIMER_ARMED.store(0, Ordering::SeqCst);
        1
    })
}

fn hook_thread(ready_tx: mpsc::SyncSender<bool>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        HOOK_TID.store(GetCurrentThreadId(), Ordering::SeqCst);

        let keyboard_hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                emit(&format!("HOOK_FAILED\tKEYBOARD\t{}", e.code().0));
                let _ = ready_tx.send(false);
                HOOK_TID.store(0, Ordering::SeqCst);
                CoUninitialize();
                return;
            }
        };
        let mouse_hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                emit(&format!("HOOK_DEGRADED\tMOUSE\t{}", e.code().0));
                HHOOK(std::ptr::null_mut())
            }
        };
        emit("HOOK_READY");
        let _ = ready_tx.send(true);

        let mut msg = MSG::default();
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0).0;
            if result == 0 {
                break;
            }
            if result == -1 {
                emit("HOOK_FAILED\tPUMP\t-1");
                break;
            }
            match msg.message {
                WM_QL_PREVIEW => do_selection_and_emit("OPEN"),
                WM_QL_SWITCH_DELAYED => {
                    // Delayed switch: Explorer needs a beat to update its selection after the arrow key.
                    // Use a thread timer so repeated arrow/mouse events do not block this message pump.
                    SWITCH_TIMER_ARMED.store(1, Ordering::SeqCst);
                    let _ = SetTimer(None, SWITCH_TIMER_ID, 80, Some(switch_timer_proc));
                }
                WM_QL_CLOSE => emit("CLOSE"),
                WM_QL_ZOOM_IN => emit("ZOOM_IN"),
                WM_QL_ZOOM_OUT => emit("ZOOM_OUT"),
                WM_QL_RELOAD => emit("RELOAD"),
                WM_QL_FULLSCREEN => emit("FULLSCREEN"),
                _ => {}
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if SWITCH_TIMER_ARMED.swap(0, Ordering::SeqCst) != 0 {
            let _ = KillTimer(None, SWITCH_TIMER_ID);
        }
        let _ = UnhookWindowsHookEx(keyboard_hook);
        if !mouse_hook.0.is_null() {
            let _ = UnhookWindowsHookEx(mouse_hook);
        }
        HOOK_TID.store(0, Ordering::SeqCst);
        emit("HOOK_STOPPED");
        CoUninitialize();
    }
}

unsafe extern "system" fn switch_timer_proc(_hwnd: HWND, _msg: u32, id: usize, _tick: u32) {
    let _ = KillTimer(None, id);
    if SWITCH_TIMER_ARMED.swap(0, Ordering::SeqCst) != 0 {
        do_selection_and_emit("SWITCH");
    }
}

/// Keep this callback cheap: classify the key, post a thread message, return immediately.
/// No allocations, no locks, no callback into managed code — all of that happens on the pump thread.
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let tid = HOOK_TID.load(Ordering::SeqCst);
        let m = wparam.0 as u32;
        let is_down = m == WM_KEYDOWN || m == WM_SYSKEYDOWN;
        let is_up = m == WM_KEYUP || m == WM_SYSKEYUP;
        let bare_key = !modifier_key_down();
        let explorer_foreground = foreground_is_explorer_window();
        let text_input_active = explorer_foreground && explorer_text_input_active();

        if kb.vkCode == VK_SPACE_U32 {
            if is_down
                && explorer_foreground
                && bare_key
                && !text_input_active
                && !SPACE_HELD.swap(true, Ordering::SeqCst)
            {
                let message = if PREVIEW_VISIBLE.load(Ordering::SeqCst) {
                    WM_QL_CLOSE
                } else {
                    WM_QL_PREVIEW
                };
                let _ = PostThreadMessageW(tid, message, WPARAM(0), LPARAM(0));
            } else if is_up {
                SPACE_HELD.store(false, Ordering::SeqCst);
            }
        } else if matches!(
            kb.vkCode,
            VK_LEFT_U32 | VK_UP_U32 | VK_RIGHT_U32 | VK_DOWN_U32
        ) {
            if is_down && explorer_foreground && bare_key && !text_input_active {
                let _ = PostThreadMessageW(tid, WM_QL_SWITCH_DELAYED, WPARAM(0), LPARAM(0));
            }
        } else if kb.vkCode == VK_ESCAPE_U32 {
            if is_down
                && (explorer_foreground || PREVIEW_VISIBLE.load(Ordering::SeqCst))
                && !text_input_active
            {
                let _ = PostThreadMessageW(tid, WM_QL_CLOSE, WPARAM(0), LPARAM(0));
            }
        } else if matches!(kb.vkCode, VK_OEM_PLUS_U32 | VK_ADD_U32) {
            if is_down && explorer_foreground && bare_key && PREVIEW_VISIBLE.load(Ordering::SeqCst)
            {
                let _ = PostThreadMessageW(tid, WM_QL_ZOOM_IN, WPARAM(0), LPARAM(0));
            }
        } else if matches!(kb.vkCode, VK_OEM_MINUS_U32 | VK_SUBTRACT_U32) {
            if is_down && explorer_foreground && bare_key && PREVIEW_VISIBLE.load(Ordering::SeqCst)
            {
                let _ = PostThreadMessageW(tid, WM_QL_ZOOM_OUT, WPARAM(0), LPARAM(0));
            }
        } else if kb.vkCode == VK_F5_U32 {
            if is_down
                && bare_key
                && PREVIEW_VISIBLE.load(Ordering::SeqCst)
                && !F5_HELD.swap(true, Ordering::SeqCst)
            {
                let _ = PostThreadMessageW(tid, WM_QL_RELOAD, WPARAM(0), LPARAM(0));
            } else if is_up {
                F5_HELD.store(false, Ordering::SeqCst);
            }
        } else if kb.vkCode == VK_F11_U32 {
            if is_down
                && bare_key
                && PREVIEW_VISIBLE.load(Ordering::SeqCst)
                && !F11_HELD.swap(true, Ordering::SeqCst)
            {
                let _ = PostThreadMessageW(tid, WM_QL_FULLSCREEN, WPARAM(0), LPARAM(0));
            } else if is_up {
                F11_HELD.store(false, Ordering::SeqCst);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe fn foreground_is_explorer_window() -> bool {
    let foreground = GetForegroundWindow();
    if foreground.0.is_null() {
        return false;
    }
    root_window_is_explorer(foreground)
}

unsafe fn modifier_key_down() -> bool {
    key_down(VK_SHIFT_U32) || key_down(VK_CONTROL_U32) || key_down(VK_MENU_U32)
}

unsafe fn key_down(vk: u32) -> bool {
    (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0
}

unsafe fn explorer_text_input_active() -> bool {
    let foreground = GetForegroundWindow();
    if foreground.0.is_null() {
        return false;
    }

    let thread_id = GetWindowThreadProcessId(foreground, None);
    if thread_id == 0 {
        return false;
    }

    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        flags: GUITHREADINFO_FLAGS(0),
        hwndActive: HWND(std::ptr::null_mut()),
        hwndFocus: HWND(std::ptr::null_mut()),
        hwndCapture: HWND(std::ptr::null_mut()),
        hwndMenuOwner: HWND(std::ptr::null_mut()),
        hwndMoveSize: HWND(std::ptr::null_mut()),
        hwndCaret: HWND(std::ptr::null_mut()),
        rcCaret: RECT::default(),
    };
    if GetGUIThreadInfo(thread_id, &mut info).is_err() {
        return false;
    }

    let focus = if !info.hwndFocus.0.is_null() {
        info.hwndFocus
    } else {
        info.hwndCaret
    };
    if focus.0.is_null() {
        return false;
    }

    let mut class_name = [0u16; 128];
    let len = GetClassNameW(focus, &mut class_name);
    if len <= 0 {
        return false;
    }

    let name = String::from_utf16_lossy(&class_name[..len as usize]);
    is_text_input_class_name(&name)
}

fn is_text_input_class_name(name: &str) -> bool {
    matches!(name, "Edit" | "RichEdit20W" | "RichEdit50W" | "RICHEDIT50W")
}

fn is_explorer_window_class_name(name: &str) -> bool {
    matches!(name, "CabinetWClass" | "ExploreWClass")
}

/// Test-only ABI used by smoke-native.ps1 to lock the Explorer rename guard's class filter.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_test_is_text_input_class(
    class_utf8: *const u8,
    class_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let Some(class_name) = utf8_arg(class_utf8, class_len, 256) else {
            return 0;
        };
        if is_text_input_class_name(class_name) {
            1
        } else {
            0
        }
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_test_is_explorer_window_class(
    class_utf8: *const u8,
    class_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let Some(class_name) = utf8_arg(class_utf8, class_len, 256) else {
            return 0;
        };
        if is_explorer_window_class_name(class_name) {
            1
        } else {
            0
        }
    })
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && PREVIEW_VISIBLE.load(Ordering::SeqCst)
        && wparam.0 as u32 == WM_LBUTTONUP
        && mouse_up_target_is_explorer(lparam)
    {
        let tid = HOOK_TID.load(Ordering::SeqCst);
        let _ = PostThreadMessageW(tid, WM_QL_SWITCH_DELAYED, WPARAM(0), LPARAM(0));
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe fn mouse_up_target_is_explorer(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let mouse = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    let hwnd = WindowFromPoint(mouse.pt);
    if hwnd.0.is_null() {
        return false;
    }
    let root = GetAncestor(hwnd, GA_ROOT);
    root_window_is_explorer(root)
}

unsafe fn root_window_is_explorer(root: HWND) -> bool {
    if root.0.is_null() {
        return false;
    }

    let mut class_name = [0u16; 128];
    let len = GetClassNameW(root, &mut class_name);
    if len <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&class_name[..len as usize]);
    is_explorer_window_class_name(&name)
}

/// Read the current Explorer selection on a fresh STA thread (avoids apartment conflicts with the
/// managed caller). Emits the result through the callback.
#[no_mangle]
pub extern "C" fn ql_get_selection() {
    ffi_void_boundary(|| {
        let h = std::thread::spawn(|| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            do_selection_and_emit("SELECTION");
            CoUninitialize();
        });
        let _ = h.join();
    });
}

fn do_selection_and_emit(tag: &str) {
    match unsafe { get_explorer_selection() } {
        Ok(paths) if !paths.is_empty() => emit(&format!("{tag}\t{}", paths.join("\t"))),
        Ok(_) => emit(&format!("{tag}\t<no selection / not in Explorer>")),
        Err(e) => emit(&format!("{tag}_ERR\t{e:?}")),
    }
}

/// Enumerate shell windows; return the foreground Explorer window's selection only.
/// If the foreground window is not an Explorer window, returns empty (no preview) — so pressing
/// space in another app doesn't trigger a preview from a lingering Explorer selection.
unsafe fn get_explorer_selection() -> Result<Vec<String>> {
    let foreground = GetForegroundWindow();
    let mut shell_windows = cached_shell_windows()?;
    let count = match shell_windows.Count() {
        Ok(count) => count,
        Err(_) => {
            SHELL_WINDOWS_CACHE.with(|cache| *cache.borrow_mut() = None);
            shell_windows = cached_shell_windows()?;
            shell_windows.Count()?
        }
    };

    for i in 0..count {
        let idx = VARIANT::from(i);
        let disp = match shell_windows.Item(&idx) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let wb: IWebBrowser2 = match disp.cast() {
            Ok(w) => w,
            Err(_) => continue,
        };
        let hwnd = HWND(wb.HWND().unwrap_or(SHANDLE_PTR(0)).0 as *mut _);
        if hwnd == foreground {
            let paths = read_window_selection(&wb).unwrap_or_default();
            return Ok(paths);
        }
    }
    Ok(Vec::new())
}

unsafe fn read_window_selection(wb: &IWebBrowser2) -> Result<Vec<String>> {
    let sp: IServiceProvider = wb.cast()?;
    let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser)?;
    let view: IShellView = browser.QueryActiveShellView()?;
    let folder_view: IFolderView = view.cast()?;
    let items: IShellItemArray = folder_view.Items(SVGIO_SELECTION)?;
    let n = items.GetCount()?;
    if n == 0 {
        return Ok(Vec::new());
    }
    let item = items.GetItemAt(0)?;
    let pw = PwstrGuard(item.GetDisplayName(SIGDN_FILESYSPATH)?);
    let path =
        pw.0.to_string()
            .map_err(|_| Error::from_hresult(HRESULT(0x80070057u32 as i32)))?;
    Ok(vec![path])
}

unsafe fn cached_shell_windows() -> Result<IShellWindows> {
    SHELL_WINDOWS_CACHE.with(|cache| {
        if let Some(shell_windows) = cache.borrow().as_ref() {
            return Ok(shell_windows.clone());
        }
        let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        *cache.borrow_mut() = Some(shell_windows.clone());
        Ok(shell_windows)
    })
}

struct PwstrGuard(PWSTR);
impl Drop for PwstrGuard {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0 .0 as *const _));
        }
    }
}

// ── File probe + cache ───────────────────────────────────────────────────────────────────────
// The native layer is the single source of truth for "what is this file": extension, magic prefix,
// a coarse kind, and metadata — cached by path+exact mtime+size so rapid edits cannot reuse stale data.

struct ProbeCacheEntry {
    modified: Option<SystemTime>,
    size: u64,
    sequence: u64,
    json: String,
}

static PROBE_CACHE: OnceLock<Mutex<HashMap<String, ProbeCacheEntry>>> = OnceLock::new();
static PROBE_CACHE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PROBE_CACHE_MAX: usize = 500;

fn probe_cache() -> &'static Mutex<HashMap<String, ProbeCacheEntry>> {
    PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Evict oldest entries when the cache exceeds PROBE_CACHE_MAX. Called after insertion.
fn probe_cache_evict(cache: &mut HashMap<String, ProbeCacheEntry>) {
    if cache.len() <= PROBE_CACHE_MAX {
        return;
    }
    let oldest_key = cache
        .iter()
        .min_by_key(|(_, entry)| entry.sequence)
        .map(|(k, _)| k.clone());
    if let Some(key) = oldest_key {
        cache.remove(&key);
    }
}

/// Probe a file (UTF-8 path) and write its FileProbe JSON (UTF-8) into `out`.
/// Returns the JSON byte length, `-needed` if the buffer is too small, or a negative error.
///
/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_probe_file(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let json = match probe_json(path) {
            Some(j) => j,
            None => return -2,
        };
        let bytes = json.as_bytes();
        if out.is_null() || out_cap < bytes.len() {
            return -(bytes.len() as i32);
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
        bytes.len() as i32
    })
}

fn probe_json(path: &str) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let precise_modified = meta.modified().ok();
    let modified = precise_modified
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if let Ok(cache) = probe_cache().lock() {
        if let Some(entry) = cache.get(path) {
            if entry.modified == precise_modified && entry.size == size {
                return Some(entry.json.clone());
            }
        }
    }

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    let mut buf = [0u8; 64];
    let n = if meta.is_dir() {
        0
    } else {
        fs::File::open(path)
            .ok()
            .map(|mut f| f.read(&mut buf).unwrap_or(0))
            .unwrap_or(0)
    };
    let magic = &buf[..n];

    let kind = if meta.is_dir() {
        "folder"
    } else {
        classify(file_name, &ext, magic, size == 0)
    };
    let magic_hex: String = magic.iter().map(|b| format!("{b:02X}")).collect();
    let animation = if kind == "image" {
        fs::File::open(path)
            .ok()
            .and_then(|mut file| preview::probe_image_animation_reader(&mut file, file_name, size))
    } else {
        None
    }
    .unwrap_or_default();
    let is_animated = match animation.is_animated {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };

    let json = format!(
        "{{\"path\":\"{}\",\"extension\":\"{}\",\"magicHex\":\"{}\",\"kind\":\"{}\",\"size\":{},\"modifiedUnix\":{},\"isAnimated\":{}}}",
        json_escape(path),
        json_escape(&ext),
        magic_hex,
        kind,
        size,
        modified,
        is_animated
    );

    {
        if let Ok(mut cache) = probe_cache().lock() {
            cache.insert(
                path.to_string(),
                ProbeCacheEntry {
                    modified: precise_modified,
                    size,
                    sequence: PROBE_CACHE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                    json: json.clone(),
                },
            );
            probe_cache_evict(&mut cache);
        }
    }
    Some(json)
}

/// Coarse type classification. Container formats are recognized by extension first (e.g. .docx is a
/// ZIP by magic but should be "office"), then images/pdf/archives by magic, then text.
fn classify(file_name: &str, ext: &str, magic: &[u8], is_empty: bool) -> &'static str {
    const OFFICE_EXTS: &[&str] = &[
        ".doc", ".docx", ".docm", ".xls", ".xlsx", ".xlsm", ".ppt", ".pptx", ".pptm", ".rtf",
        ".odt", ".ods", ".odp",
    ];
    const VIDEO_EXTS: &[&str] = &[
        ".mp4", ".mkv", ".avi", ".mov", ".webm", ".flv", ".wmv", ".m4v", ".mpg", ".mpeg", ".3gp",
    ];
    const AUDIO_EXTS: &[&str] = &[
        ".mp3", ".wav", ".flac", ".aac", ".ogg", ".m4a", ".wma", ".opus", ".mid",
    ];
    const ARCHIVE_EXTS: &[&str] = &[
        ".zip", ".jar", ".nupkg", ".vsix", ".whl", ".cbz", ".xpi", ".tar", ".tgz", ".gz",
    ];
    const EBOOK_EXTS: &[&str] = &[".epub", ".fb2", ".mobi", ".azw", ".azw3"];
    const IMAGE_EXTS: &[&str] = &[
        ".png", ".jpg", ".jpeg", ".jpe", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".webp", ".ico",
        ".heic", ".heif", ".avif", ".jxl", ".svg",
    ];
    const PACKAGE_EXTS: &[&str] = &[
        ".apk",
        ".apks",
        ".aab",
        ".msix",
        ".msixbundle",
        ".appx",
        ".appxbundle",
    ];
    const DISK_IMAGE_EXTS: &[&str] = &[".img", ".iso", ".vhd", ".vhdx", ".vmdk", ".dmg"];
    const EXECUTABLE_EXTS: &[&str] = &[".exe", ".dll", ".sys", ".scr", ".cpl", ".ocx"];
    const CERTIFICATE_EXTS: &[&str] = &[".cer", ".crt", ".der", ".pem", ".p7b", ".p7c"];
    const FONT_EXTS: &[&str] = &[".ttf", ".otf", ".ttc", ".otc", ".woff", ".woff2"];
    const DATABASE_EXTS: &[&str] = &[
        ".sqlite",
        ".sqlite3",
        ".db",
        ".db3",
        ".s3db",
        ".sqlite-shm",
        ".sqlite-wal",
        ".mdb",
        ".accdb",
    ];
    const MAIL_EXTS: &[&str] = &[".eml", ".msg", ".mbox", ".emlx"];
    const CHM_EXTS: &[&str] = &[".chm"];
    const DUMP_EXTS: &[&str] = &[".dmp", ".mdmp", ".dump", ".core"];
    const ELF_EXTS: &[&str] = &[".elf", ".so", ".o"];
    if OFFICE_EXTS.contains(&ext) {
        return "office";
    }
    if EBOOK_EXTS.contains(&ext) {
        return "ebook";
    }
    if CERTIFICATE_EXTS.contains(&ext) {
        return "certificate";
    }
    if EXECUTABLE_EXTS.contains(&ext) || magic.starts_with(b"MZ") {
        return "executable";
    }
    if FONT_EXTS.contains(&ext) {
        return "font";
    }
    let lower_file_name = file_name.to_ascii_lowercase();
    if DATABASE_EXTS.contains(&ext)
        || lower_file_name.ends_with("-wal")
        || lower_file_name.ends_with("-shm")
    {
        return "database";
    }
    if MAIL_EXTS.contains(&ext) {
        return "mail";
    }
    if CHM_EXTS.contains(&ext) {
        return "chm";
    }
    if DUMP_EXTS.contains(&ext) {
        return "dump";
    }
    if ELF_EXTS.contains(&ext) {
        return "elf";
    }
    if ext == ".torrent" {
        return "torrent";
    }
    if DISK_IMAGE_EXTS.contains(&ext) {
        return "disk-image";
    }
    if PACKAGE_EXTS.contains(&ext) {
        return "package";
    }
    if VIDEO_EXTS.contains(&ext) {
        return "video";
    }
    if AUDIO_EXTS.contains(&ext) {
        return "audio";
    }
    if ARCHIVE_EXTS.contains(&ext) {
        return "archive";
    }
    if IMAGE_EXTS.contains(&ext) {
        return "image";
    }

    let m = magic;
    if m.starts_with(&[0x89, 0x50, 0x4E, 0x47])              // PNG
        || m.starts_with(&[0xFF, 0xD8, 0xFF])               // JPEG
        || m.starts_with(b"GIF8")
        || m.starts_with(b"BM")
        || m.starts_with(&[0x49, 0x49, 0x2A, 0x00])         // TIFF (LE)
        || m.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])         // TIFF (BE)
        || (m.len() >= 12 && &m[0..4] == b"RIFF" && &m[8..12] == b"WEBP")
    {
        return "image";
    }
    if m.starts_with(b"%PDF") {
        return "pdf";
    }
    if m.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || m.starts_with(b"OTTO")
        || m.starts_with(b"ttcf")
        || m.starts_with(b"wOFF")
        || m.starts_with(b"wOF2")
    {
        return "font";
    }
    if m.starts_with(b"SQLite format 3\0") {
        return "database";
    }
    if m.starts_with(b"ITSF") {
        return "chm";
    }
    if m.starts_with(b"MDMP") {
        return "dump";
    }
    if m.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return "elf";
    }
    if m.starts_with(&[0x50, 0x4B, 0x03, 0x04])             // ZIP / OOXML
        || m.starts_with(&[0x1F, 0x8B])
    // gzip
        || rar_listing::is_rar_magic(m)
    {
        return "archive";
    }

    // Specialized extensions and binary signatures win above. For everything else, accept known
    // text formats or a conservative printable-text prefix so uncommon config files remain useful.
    if is_empty || preview::is_text_file(file_name, ext, magic) {
        return "text";
    }
    "binary"
}

// ── Native image decode ──────────────────────────────────────────────────────────────────────
// Decode common image formats in Rust and return a constrained BGRA raster for the .NET raster host.
// Output layout: [w:u32 LE][h:u32 LE][orig_w:u32 LE][orig_h:u32 LE]
// [decode_ms:u32 LE][resize_ms:u32 LE][convert_ms:u32 LE][premultiplied BGRA bytes].

const MAX_IMAGE_RASTER_DIMENSION: u32 = 2048;
const IMAGE_PACKET_HEADER_BYTES: usize = 28;
const IMAGE_WAVEFORM_WIDTH: u32 = 192;
const IMAGE_WAVEFORM_HEIGHT: u32 = 96;
const IMAGE_WAVEFORM_CHANNELS: usize = 3;
const IMAGE_WAVEFORM_SAMPLE_LIMIT: f64 = 1_000_000.0;
const IMAGE_WAVEFORM_PLANE_BYTES: usize =
    IMAGE_WAVEFORM_WIDTH as usize * IMAGE_WAVEFORM_HEIGHT as usize;
const IMAGE_WAVEFORM_DENSITY_BYTES: usize = IMAGE_WAVEFORM_PLANE_BYTES * IMAGE_WAVEFORM_CHANNELS;
const IMAGE_WAVEFORM_PACKET_HEADER_BYTES: usize = 40;
const MAX_IMAGE_WAVEFORM_PACKET_BYTES: usize = IMAGE_WAVEFORM_PACKET_HEADER_BYTES
    + MAX_IMAGE_RASTER_DIMENSION as usize * MAX_IMAGE_RASTER_DIMENSION as usize * 4
    + IMAGE_WAVEFORM_DENSITY_BYTES;

struct ImageWaveformAccumulator {
    image_width: usize,
    sample_step: usize,
    counts: Vec<u32>,
}

impl ImageWaveformAccumulator {
    fn new(width: u32, height: u32) -> Self {
        let pixel_count = u64::from(width) * u64::from(height);
        // Keep sampling identical to ImageWaveformBuilder: a square-grid stride derived from the
        // one-million-sample budget. The final raster is bounded to 2048x2048 independently.
        let sample_step = ((pixel_count as f64 / IMAGE_WAVEFORM_SAMPLE_LIMIT)
            .sqrt()
            .ceil() as usize)
            .max(1);
        Self {
            image_width: width as usize,
            sample_step,
            counts: vec![0; IMAGE_WAVEFORM_DENSITY_BYTES],
        }
    }

    fn add_straight_rgba(&mut self, pixel_index: usize, rgba: &[u8]) {
        self.add_rgb(pixel_index, rgba[0], rgba[1], rgba[2], rgba[3]);
    }

    fn add_premultiplied_rgba(&mut self, pixel_index: usize, rgba: &[u8]) {
        let alpha = rgba[3];
        if alpha == 0 {
            return;
        }
        self.add_rgb(
            pixel_index,
            unpremultiply_channel(rgba[0], alpha),
            unpremultiply_channel(rgba[1], alpha),
            unpremultiply_channel(rgba[2], alpha),
            alpha,
        );
    }

    fn add_rgb(&mut self, pixel_index: usize, red: u8, green: u8, blue: u8, alpha: u8) {
        if alpha == 0 || self.image_width == 0 {
            return;
        }
        let x = pixel_index % self.image_width;
        let y = pixel_index / self.image_width;
        if !x.is_multiple_of(self.sample_step) || !y.is_multiple_of(self.sample_step) {
            return;
        }

        let column = (x * IMAGE_WAVEFORM_WIDTH as usize / self.image_width)
            .min(IMAGE_WAVEFORM_WIDTH as usize - 1);
        self.add_channel(0, column, red);
        self.add_channel(1, column, green);
        self.add_channel(2, column, blue);
    }

    fn add_channel(&mut self, channel: usize, column: usize, value: u8) {
        let row = IMAGE_WAVEFORM_HEIGHT as usize
            - 1
            - value as usize * (IMAGE_WAVEFORM_HEIGHT as usize - 1) / 255;
        let index =
            channel * IMAGE_WAVEFORM_PLANE_BYTES + row * IMAGE_WAVEFORM_WIDTH as usize + column;
        self.counts[index] = self.counts[index].saturating_add(1);
    }

    fn finish(self, cancel_cb: Option<CancelCallback>) -> Option<Vec<u8>> {
        let maximum = self.counts.iter().copied().max().unwrap_or(0);
        let mut density = vec![0u8; IMAGE_WAVEFORM_DENSITY_BYTES];
        if maximum == 0 {
            return Some(density);
        }

        let denominator = f64::from(maximum + 1).log2();
        for (index, count) in self.counts.into_iter().enumerate() {
            if index % 16_384 == 0 && cancel_requested(cancel_cb) {
                return None;
            }
            density[index] = (255.0 * f64::from(count + 1).log2() / denominator)
                .round_ties_even()
                .clamp(0.0, 255.0) as u8;
        }
        Some(density)
    }
}

fn unpremultiply_channel(value: u8, alpha: u8) -> u8 {
    if alpha == 255 {
        value
    } else {
        (((u32::from(value) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255)) as u8
    }
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| unsafe { ql_decode_image_cancelable(path_utf8, path_len, out, out_cap, None) })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_decode_image_sized_cancelable(path_utf8, path_len, 0, 0, out, out_cap, cancel_cb)
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };

        let (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ) = match decode_image_bgra(path, target_width, target_height, cancel_cb) {
            Some(decoded) => decoded,
            None => return -2,
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }

        let total = 28 + bgra.len();
        if out.is_null() || out_cap < total {
            return -(total as i32);
        }

        unsafe {
            std::ptr::copy_nonoverlapping(width.to_le_bytes().as_ptr(), out, 4);
            std::ptr::copy_nonoverlapping(height.to_le_bytes().as_ptr(), out.add(4), 4);
            std::ptr::copy_nonoverlapping(original_width.to_le_bytes().as_ptr(), out.add(8), 4);
            std::ptr::copy_nonoverlapping(original_height.to_le_bytes().as_ptr(), out.add(12), 4);
            std::ptr::copy_nonoverlapping(decode_ms.to_le_bytes().as_ptr(), out.add(16), 4);
            std::ptr::copy_nonoverlapping(resize_ms.to_le_bytes().as_ptr(), out.add(20), 4);
            std::ptr::copy_nonoverlapping(convert_ms.to_le_bytes().as_ptr(), out.add(24), 4);
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(28), bgra.len());
        }
        total as i32
    })
}

fn probe_reader_json(
    file: &mut fs::File,
    logical_name: &str,
    size: u64,
    modified_unix: i64,
) -> std::result::Result<String, i32> {
    let ext = std::path::Path::new(logical_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_lowercase()))
        .unwrap_or_default();
    let mut prefix = [0u8; 64];
    let read = file.read(&mut prefix).map_err(|_| QL_ERROR_IO)?;
    let magic = &prefix[..read];
    let kind = classify(logical_name, &ext, magic, size == 0);
    let magic_hex: String = magic.iter().map(|value| format!("{value:02X}")).collect();
    let animation = if kind == "image" {
        preview::probe_image_animation_reader(file, logical_name, size).unwrap_or_default()
    } else {
        Default::default()
    };
    let is_animated = match animation.is_animated {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    Ok(format!(
        "{{\"path\":\"{}\",\"extension\":\"{}\",\"magicHex\":\"{}\",\"kind\":\"{}\",\"size\":{},\"modifiedUnix\":{},\"isAnimated\":{}}}",
        json_escape(logical_name),
        json_escape(&ext),
        magic_hex,
        kind,
        size,
        modified_unix,
        is_animated
    ))
}

/// Decode a native-safe static image from a borrowed Windows file handle.
/// Output layout matches `ql_decode_image_sized_cancelable`.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep all pointers valid for this call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if expected_length > 256 * 1024 * 1024 {
            return QL_ERROR_LIMIT_EXCEEDED;
        }
        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extension = Path::new(&logical_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let decoded = if extension == "svg" {
            if expected_length > MAX_SVG_INPUT_BYTES {
                return QL_ERROR_LIMIT_EXCEEDED;
            }
            let mut data = Vec::with_capacity(expected_length as usize);
            let mut chunk = [0u8; 64 * 1024];
            loop {
                if cancel_requested(cancel_cb) {
                    return QL_ERROR_CANCELLED;
                }
                let read = match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => return QL_ERROR_IO,
                };
                if data.len().saturating_add(read) > expected_length as usize {
                    return QL_ERROR_LENGTH_MISMATCH;
                }
                data.extend_from_slice(&chunk[..read]);
            }
            if data.len() as u64 != expected_length {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            decode_svg_bgra_bytes(&data, target_width, target_height, cancel_cb)
        } else {
            let required_format = match extension.as_str() {
                "png" => ImageFormat::Png,
                "jpg" | "jpeg" | "jpe" => ImageFormat::Jpeg,
                "gif" => ImageFormat::Gif,
                "bmp" => ImageFormat::Bmp,
                "ico" => ImageFormat::Ico,
                "tif" | "tiff" => ImageFormat::Tiff,
                "webp" => ImageFormat::WebP,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
            let required = match preflight_native_image_packet_length(
                &mut file,
                required_format,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                target_width,
                target_height,
                false,
            ) {
                Ok(required) => required,
                Err(status) => return status,
            };
            if cancel_requested(cancel_cb) {
                return QL_ERROR_CANCELLED;
            }
            *out_required = required;
            if required > out_cap {
                return QL_ERROR_BUFFER_TOO_SMALL;
            }
            decode_image_bgra_reader(
                file,
                &logical_name,
                target_width,
                target_height,
                cancel_cb,
                Some(required_format),
            )
        };
        let (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ) = match decoded {
            Some(decoded) => decoded,
            None => {
                return if cancel_requested(cancel_cb) {
                    QL_ERROR_CANCELLED
                } else {
                    QL_ERROR_MALFORMED
                }
            }
        };
        let mut packet = Vec::with_capacity(IMAGE_PACKET_HEADER_BYTES + bgra.len());
        packet.extend_from_slice(&width.to_le_bytes());
        packet.extend_from_slice(&height.to_le_bytes());
        packet.extend_from_slice(&original_width.to_le_bytes());
        packet.extend_from_slice(&original_height.to_le_bytes());
        packet.extend_from_slice(&decode_ms.to_le_bytes());
        packet.extend_from_slice(&resize_ms.to_le_bytes());
        packet.extend_from_slice(&convert_ms.to_le_bytes());
        packet.extend_from_slice(&bgra);
        write_v2_out(&packet, out_buf, out_cap, out_required)
    })
}

/// Decode a native-safe static image and derive its bounded RGB density scope during the final
/// pixel-conversion pass.
///
/// Output layout is ten little-endian `u32` values followed by two exact payloads:
/// `[w, h, original_w, original_h, decode_ms, resize_ms, convert_ms, 192, 96, density_len]`,
/// then `w*h*4` premultiplied BGRA bytes, then planar row-major R/G/B density bytes.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep all pointers valid for this call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_with_waveform_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if expected_length > 256 * 1024 * 1024 {
            return QL_ERROR_LIMIT_EXCEEDED;
        }

        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extension = Path::new(&logical_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let decoded = if extension == "svg" {
            if expected_length > MAX_SVG_INPUT_BYTES {
                return QL_ERROR_LIMIT_EXCEEDED;
            }
            let mut data = Vec::with_capacity(expected_length as usize);
            let mut chunk = [0u8; 64 * 1024];
            loop {
                if cancel_requested(cancel_cb) {
                    return QL_ERROR_CANCELLED;
                }
                let read = match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => return QL_ERROR_IO,
                };
                if data.len().saturating_add(read) > expected_length as usize {
                    return QL_ERROR_LENGTH_MISMATCH;
                }
                data.extend_from_slice(&chunk[..read]);
            }
            if data.len() as u64 != expected_length {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            decode_svg_bgra_bytes_with_waveform(&data, target_width, target_height, cancel_cb)
        } else {
            let required_format = match extension.as_str() {
                "png" => ImageFormat::Png,
                "jpg" | "jpeg" | "jpe" => ImageFormat::Jpeg,
                "bmp" => ImageFormat::Bmp,
                "ico" => ImageFormat::Ico,
                "tif" | "tiff" => ImageFormat::Tiff,
                "webp" => ImageFormat::WebP,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
            let required = match preflight_native_image_packet_length(
                &mut file,
                required_format,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                target_width,
                target_height,
                true,
            ) {
                Ok(required) => required,
                Err(status) => return status,
            };
            if cancel_requested(cancel_cb) {
                return QL_ERROR_CANCELLED;
            }
            *out_required = required;
            if required > out_cap {
                return QL_ERROR_BUFFER_TOO_SMALL;
            }
            decode_image_bgra_reader_with_waveform(
                file,
                &logical_name,
                target_width,
                target_height,
                cancel_cb,
                Some(required_format),
            )
        };
        let (
            (
                width,
                height,
                original_width,
                original_height,
                decode_ms,
                resize_ms,
                convert_ms,
                bgra,
            ),
            density,
        ) = match decoded {
            Some(decoded) => decoded,
            None => {
                return if cancel_requested(cancel_cb) {
                    QL_ERROR_CANCELLED
                } else {
                    QL_ERROR_MALFORMED
                }
            }
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let packet = ImageWaveformPacket {
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra: &bgra,
            density: &density,
        };
        write_image_waveform_packet(packet, out_buf, out_cap, out_required)
    })
}

struct ImageWaveformPacket<'a> {
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
    decode_ms: u32,
    resize_ms: u32,
    convert_ms: u32,
    bgra: &'a [u8],
    density: &'a [u8],
}

unsafe fn write_image_waveform_packet(
    packet: ImageWaveformPacket<'_>,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    let raster_bytes = match (packet.width as usize)
        .checked_mul(packet.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    {
        Some(bytes) => bytes,
        None => return QL_ERROR_INTERNAL,
    };
    if packet.width == 0
        || packet.height == 0
        || packet.width > MAX_IMAGE_RASTER_DIMENSION
        || packet.height > MAX_IMAGE_RASTER_DIMENSION
        || packet.bgra.len() != raster_bytes
        || packet.density.len() != IMAGE_WAVEFORM_DENSITY_BYTES
    {
        return QL_ERROR_INTERNAL;
    }
    let total = match IMAGE_WAVEFORM_PACKET_HEADER_BYTES
        .checked_add(raster_bytes)
        .and_then(|bytes| bytes.checked_add(packet.density.len()))
    {
        Some(total) if total <= MAX_IMAGE_WAVEFORM_PACKET_BYTES => total,
        _ => return QL_ERROR_LIMIT_EXCEEDED,
    };

    unsafe { *out_required = total };
    if total > out_cap {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if out_buf.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }

    let header = [
        packet.width,
        packet.height,
        packet.original_width,
        packet.original_height,
        packet.decode_ms,
        packet.resize_ms,
        packet.convert_ms,
        IMAGE_WAVEFORM_WIDTH,
        IMAGE_WAVEFORM_HEIGHT,
        IMAGE_WAVEFORM_DENSITY_BYTES as u32,
    ];
    for (index, value) in header.into_iter().enumerate() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), out_buf.add(index * 4), 4);
        }
    }
    unsafe {
        std::ptr::copy_nonoverlapping(
            packet.bgra.as_ptr(),
            out_buf.add(IMAGE_WAVEFORM_PACKET_HEADER_BYTES),
            raster_bytes,
        );
        std::ptr::copy_nonoverlapping(
            packet.density.as_ptr(),
            out_buf.add(IMAGE_WAVEFORM_PACKET_HEADER_BYTES + raster_bytes),
            packet.density.len(),
        );
    }
    QL_OK
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HandleAnimationFormat {
    Gif,
    WebP,
    Png,
}

impl HandleAnimationFormat {
    fn from_logical_name(logical_name: &str) -> Option<Self> {
        match Path::new(logical_name)
            .extension()
            .and_then(|extension| extension.to_str())?
            .to_ascii_lowercase()
            .as_str()
        {
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::WebP),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Gif => ImageFormat::Gif,
            Self::WebP => ImageFormat::WebP,
            Self::Png => ImageFormat::Png,
        }
    }
}

#[derive(Clone, Copy)]
struct AnimationHandleRequest {
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    gif_only: bool,
}

#[allow(clippy::too_many_arguments)]
unsafe fn decode_animation_frames_handle_v2(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
    gif_only: bool,
) -> i32 {
    if out_required.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    unsafe { *out_required = 0 };
    if out_buf.is_null() && out_cap != 0 {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    let request = AnimationHandleRequest {
        source_handle,
        expected_length,
        logical_name_utf8,
        logical_name_len,
        target_width,
        target_height,
        cancel_cb,
        gif_only,
    };
    let (width, height, frames) = match unsafe { decode_animation_frames_handle(request) } {
        Ok(decoded) => decoded,
        Err(status) => return status,
    };
    unsafe { write_animation_frames_v2(width, height, &frames, out_buf, out_cap, out_required) }
}

unsafe fn decode_animation_frames_handle(
    request: AnimationHandleRequest,
) -> std::result::Result<DecodedAnimationBgra, i32> {
    if request.expected_length > MAX_ANIMATION_HANDLE_INPUT_BYTES {
        return Err(QL_ERROR_LIMIT_EXCEEDED);
    }

    let (mut file, logical_name, _, _) = unsafe {
        reopen_handle_input_v2(
            request.source_handle,
            request.expected_length,
            request.logical_name_utf8,
            request.logical_name_len,
            request.cancel_cb,
        )
    }?;
    let format =
        HandleAnimationFormat::from_logical_name(&logical_name).ok_or(QL_ERROR_INVALID_ARGUMENT)?;
    if request.gif_only && format != HandleAnimationFormat::Gif {
        return Err(QL_ERROR_INVALID_ARGUMENT);
    }
    validate_handle_image_dimensions(&mut file, format.image_format(), MAX_ANIMATED_SOURCE_PIXELS)?;

    let decoded = match format {
        HandleAnimationFormat::Gif => decode_gif_frames_bgra_reader(
            file,
            request.target_width,
            request.target_height,
            request.cancel_cb,
        ),
        HandleAnimationFormat::WebP => decode_webp_frames_bgra_reader(
            file,
            request.target_width,
            request.target_height,
            request.cancel_cb,
        ),
        HandleAnimationFormat::Png => decode_png_frames_bgra_reader(
            file,
            request.target_width,
            request.target_height,
            request.cancel_cb,
        ),
    };
    match decoded {
        Some(decoded) if !decoded.2.is_empty() => Ok(decoded),
        _ if cancel_requested(request.cancel_cb) => Err(QL_ERROR_CANCELLED),
        _ => Err(QL_ERROR_MALFORMED),
    }
}

/// Decode bounded GIF animation frames from a borrowed Windows file handle.
/// Output layout matches `ql_decode_gif_frames_sized_cancelable`.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep all pointers valid for this call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_gif_frames_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        decode_animation_frames_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            target_width,
            target_height,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            true,
        )
    })
}

/// Decode a bounded GIF exactly once, then ask the caller for an exact-size output buffer.
/// This avoids repeating the expensive frame decode when the final packet exceeds a guessed
/// managed buffer size.
///
/// # Safety
/// The caller retains ownership of `source_handle`; `output_cb` must return writable storage for
/// exactly the requested byte count and keep it valid until this call returns.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_gif_frames_handle_direct(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_required: *mut usize,
    output_cb: Option<AnimationOutputCallback>,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        let request = AnimationHandleRequest {
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            target_width,
            target_height,
            cancel_cb,
            gif_only: true,
        };
        let (width, height, frames) = match decode_animation_frames_handle(request) {
            Ok(decoded) => decoded,
            Err(status) => return status,
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_animation_frames_direct(width, height, &frames, out_required, output_cb, cancel_cb)
    })
}

/// Decode bounded GIF, animated WebP, or APNG frames from a borrowed Windows file handle.
/// Output layout matches the existing path-based animation packet exports.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep all pointers valid for this call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_animation_frames_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        decode_animation_frames_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            target_width,
            target_height,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            false,
        )
    })
}

fn validate_handle_image_dimensions<R: Read + Seek>(
    reader: &mut R,
    required_format: ImageFormat,
    max_pixels: u64,
) -> std::result::Result<(u32, u32), i32> {
    reader.seek(SeekFrom::Start(0)).map_err(|_| QL_ERROR_IO)?;
    let image_reader = ImageReader::new(BufReader::new(&mut *reader))
        .with_guessed_format()
        .map_err(|_| QL_ERROR_MALFORMED)?;
    if image_reader.format() != Some(required_format) {
        return Err(QL_ERROR_MALFORMED);
    }
    let (width, height) = image_reader
        .into_dimensions()
        .map_err(|_| QL_ERROR_MALFORMED)?;
    reader.seek(SeekFrom::Start(0)).map_err(|_| QL_ERROR_IO)?;
    if width == 0
        || height == 0
        || should_skip_native_image_decode(width, height)
        || u64::from(width)
            .checked_mul(u64::from(height))
            .is_none_or(|pixels| pixels > max_pixels)
    {
        return Err(QL_ERROR_LIMIT_EXCEEDED);
    }
    Ok((width, height))
}

fn preflight_native_image_packet_length<R: Read + Seek>(
    reader: &mut R,
    required_format: ImageFormat,
    max_pixels: u64,
    target_width: u32,
    target_height: u32,
    include_waveform: bool,
) -> std::result::Result<usize, i32> {
    let (source_width, source_height) =
        validate_handle_image_dimensions(reader, required_format, max_pixels)?;
    let orientation = if required_format == ImageFormat::Jpeg {
        reader.seek(SeekFrom::Start(0)).map_err(|_| QL_ERROR_IO)?;
        jpeg_metadata_from_reader(&mut *reader)
            .map_err(|_| QL_ERROR_MALFORMED)?
            .orientation
    } else {
        None
    };
    reader.seek(SeekFrom::Start(0)).map_err(|_| QL_ERROR_IO)?;
    let (oriented_width, oriented_height) = match orientation {
        Some(5..=8) => (source_height, source_width),
        _ => (source_width, source_height),
    };
    let (width, height) = native_image_target_dimensions(
        oriented_width,
        oriented_height,
        target_width,
        target_height,
    );
    let width = usize::try_from(width).map_err(|_| QL_ERROR_LIMIT_EXCEEDED)?;
    let height = usize::try_from(height).map_err(|_| QL_ERROR_LIMIT_EXCEEDED)?;
    checked_native_image_packet_length(width, height, include_waveform)
        .ok_or(QL_ERROR_LIMIT_EXCEEDED)
}

fn checked_native_image_packet_length(
    width: usize,
    height: usize,
    include_waveform: bool,
) -> Option<usize> {
    let raster_bytes = width.checked_mul(height)?.checked_mul(4)?;
    let fixed_bytes = if include_waveform {
        IMAGE_WAVEFORM_PACKET_HEADER_BYTES.checked_add(IMAGE_WAVEFORM_DENSITY_BYTES)?
    } else {
        IMAGE_PACKET_HEADER_BYTES
    };
    fixed_bytes.checked_add(raster_bytes)
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_gif_frames_sized(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        ql_decode_gif_frames_sized_cancelable(
            path_utf8,
            path_len,
            target_width,
            target_height,
            out,
            out_cap,
            None,
        )
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_gif_frames_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let (width, height, frames) =
            match decode_gif_frames_bgra(path, target_width, target_height, cancel_cb) {
                Some(decoded) => decoded,
                None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
            };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_animation_frames(width, height, frames, out, out_cap)
    })
}

/// Path compatibility form of the exact-size, single-decode GIF handoff.
///
/// # Safety
/// `output_cb` must return writable storage for exactly the requested byte count and keep it valid
/// until this call returns.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_gif_frames_sized_direct(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out_required: *mut usize,
    output_cb: Option<AnimationOutputCallback>,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(path) => path,
            None => return QL_ERROR_INVALID_ARGUMENT,
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let (width, height, frames) =
            match decode_gif_frames_bgra(path, target_width, target_height, cancel_cb) {
                Some(decoded) if !decoded.2.is_empty() => decoded,
                _ if cancel_requested(cancel_cb) => return QL_ERROR_CANCELLED,
                _ => return QL_ERROR_MALFORMED,
            };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_animation_frames_direct(width, height, &frames, out_required, output_cb, cancel_cb)
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_webp_frames_sized(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        ql_decode_webp_frames_sized_cancelable(
            path_utf8,
            path_len,
            target_width,
            target_height,
            out,
            out_cap,
            None,
        )
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_webp_frames_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let (width, height, frames) =
            match decode_webp_frames_bgra(path, target_width, target_height, cancel_cb) {
                Some(decoded) => decoded,
                None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
            };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_animation_frames(width, height, frames, out, out_cap)
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_png_frames_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let (width, height, frames) =
            match decode_png_frames_bgra(path, target_width, target_height, cancel_cb) {
                Some(decoded) => decoded,
                None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
            };
        write_animation_frames(width, height, frames, out, out_cap)
    })
}

type DecodedImageBgra = (u32, u32, u32, u32, u32, u32, u32, Vec<u8>);

fn decode_image_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedImageBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }

    if std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    {
        return decode_svg_bgra(path, target_width, target_height, cancel_cb);
    }

    let file = fs::File::open(path).ok()?;
    decode_image_bgra_reader(file, path, target_width, target_height, cancel_cb, None)
}

fn decode_image_bgra_reader<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    required_format: Option<ImageFormat>,
) -> Option<DecodedImageBgra> {
    decode_image_bgra_reader_internal(
        reader,
        logical_name,
        target_width,
        target_height,
        cancel_cb,
        required_format,
        false,
    )
    .map(|(decoded, _)| decoded)
}

fn decode_image_bgra_reader_with_waveform<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    required_format: Option<ImageFormat>,
) -> Option<(DecodedImageBgra, Vec<u8>)> {
    let (decoded, waveform) = decode_image_bgra_reader_internal(
        reader,
        logical_name,
        target_width,
        target_height,
        cancel_cb,
        required_format,
        true,
    )?;
    Some((decoded, waveform?))
}

fn native_image_target_dimensions(
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> (u32, u32) {
    let target_width = if target_width > 0 {
        target_width
    } else {
        MAX_IMAGE_RASTER_DIMENSION
    };
    let target_height = if target_height > 0 {
        target_height
    } else {
        MAX_IMAGE_RASTER_DIMENSION
    };
    let target_width = target_width.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let target_height = target_height.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let scale = if width > target_width || height > target_height {
        (target_width as f64 / width as f64).min(target_height as f64 / height as f64)
    } else {
        1.0
    };
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

#[derive(Clone, Copy)]
struct NativeImageDecodeBudgetInput {
    source_width: u32,
    source_height: u32,
    decoded_bytes: u64,
    decoded_bytes_per_pixel: u64,
    target_width: u32,
    target_height: u32,
    orientation: Option<u16>,
    include_waveform: bool,
}

fn checked_native_image_decode_peak_bytes(input: NativeImageDecodeBudgetInput) -> Option<u64> {
    let NativeImageDecodeBudgetInput {
        source_width,
        source_height,
        decoded_bytes,
        decoded_bytes_per_pixel,
        target_width,
        target_height,
        orientation,
        include_waveform,
    } = input;
    if source_width == 0 || source_height == 0 || decoded_bytes_per_pixel == 0 {
        return None;
    }

    let source_pixels = u64::from(source_width).checked_mul(u64::from(source_height))?;
    let color_type_bytes = source_pixels.checked_mul(decoded_bytes_per_pixel)?;
    let source_bytes = decoded_bytes.max(color_type_bytes);
    let (oriented_width, oriented_height) = match orientation {
        Some(5..=8) => (source_height, source_width),
        _ => (source_width, source_height),
    };
    let (width, height) = native_image_target_dimensions(
        oriented_width,
        oriented_height,
        target_width,
        target_height,
    );
    let target_pixels = u64::from(width).checked_mul(u64::from(height))?;
    let target_native_bytes = target_pixels.checked_mul(decoded_bytes_per_pixel)?;
    let target_rgba_bytes = target_pixels.checked_mul(4)?;

    // EXIF orientations 5 and 7 currently compose a flip and a rotation. During that transform
    // the source, flipped image, and rotated image can coexist; the other transforms need at most
    // one additional source-sized image.
    let orientation_source_copies = match orientation {
        Some(5 | 7) => 3,
        Some(2 | 3 | 4 | 6 | 8) => 2,
        _ => 1,
    };
    let orientation_peak = source_bytes.checked_mul(orientation_source_copies)?;

    let resize_native_bytes = if (width, height) == (oriented_width, oriented_height) {
        0
    } else {
        target_native_bytes
    };
    let waveform_bytes = if include_waveform {
        u64::try_from(IMAGE_WAVEFORM_DENSITY_BYTES)
            .ok()?
            .checked_mul(u64::try_from(size_of::<u32>()).ok()?)?
            .checked_add(u64::try_from(IMAGE_WAVEFORM_DENSITY_BYTES).ok()?)?
    } else {
        0
    };
    let conversion_peak = source_bytes
        .checked_add(resize_native_bytes)?
        .checked_add(target_rgba_bytes)?
        .checked_add(target_rgba_bytes)?
        .checked_add(waveform_bytes)?;
    let packet_peak = target_rgba_bytes
        .checked_mul(2)?
        .checked_add(u64::try_from(IMAGE_PACKET_HEADER_BYTES).ok()?)?;

    Some(orientation_peak.max(conversion_peak).max(packet_peak))
}

fn native_image_decode_fits_peak_budget(input: NativeImageDecodeBudgetInput) -> bool {
    checked_native_image_decode_peak_bytes(input)
        .is_some_and(|peak_bytes| peak_bytes <= MAX_NATIVE_IMAGE_DECODE_PEAK_BYTES)
}

fn decode_image_bgra_reader_internal<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    required_format: Option<ImageFormat>,
    include_waveform: bool,
) -> Option<(DecodedImageBgra, Option<Vec<u8>>)> {
    let mut reader = CancelableImageReader::new(reader, cancel_cb);
    if cancel_requested(cancel_cb) {
        return None;
    }
    let ext = Path::new(logical_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let jpeg_metadata = if matches!(ext.as_str(), "jpg" | "jpeg" | "jpe") {
        let metadata = jpeg_metadata_from_reader(&mut reader).ok()?;
        reader.seek(SeekFrom::Start(0)).ok()?;
        metadata
    } else {
        JpegMetadata::default()
    };
    let guessed = ImageReader::new(BufReader::new(&mut reader))
        .with_guessed_format()
        .ok()?;
    if required_format.is_some_and(|required| guessed.format() != Some(required)) {
        return None;
    }
    let (original_width, original_height) = match jpeg_metadata.dimensions {
        Some(dimensions) => dimensions,
        None => guessed.into_dimensions().ok()?,
    };
    if should_skip_native_image_decode(original_width, original_height) {
        return None;
    }
    if cancel_requested(cancel_cb) {
        return None;
    }

    let decode_start = Instant::now();
    reader.seek(SeekFrom::Start(0)).ok()?;
    let decoder = ImageReader::new(BufReader::new(reader))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (decoded_width, decoded_height) = decoder.dimensions();
    if should_skip_native_image_decode(decoded_width, decoded_height)
        || !native_image_decode_fits_peak_budget(NativeImageDecodeBudgetInput {
            source_width: decoded_width,
            source_height: decoded_height,
            decoded_bytes: decoder.total_bytes(),
            decoded_bytes_per_pixel: u64::from(decoder.color_type().bytes_per_pixel()),
            target_width,
            target_height,
            orientation: jpeg_metadata.orientation,
            include_waveform,
        })
    {
        return None;
    }
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    let decode_ms = elapsed_ms_u32(decode_start);
    if cancel_requested(cancel_cb) {
        return None;
    }

    if let Some(orientation) = jpeg_metadata.orientation {
        image = apply_exif_orientation(image, orientation);
    }

    let (oriented_width, oriented_height) = (image.width(), image.height());
    if oriented_width == 0 || oriented_height == 0 {
        return None;
    }

    let (width, height) = native_image_target_dimensions(
        oriented_width,
        oriented_height,
        target_width,
        target_height,
    );
    if cancel_requested(cancel_cb) {
        return None;
    }

    let resize_start = Instant::now();
    let raster = if width == oriented_width && height == oriented_height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };
    let resize_ms = elapsed_ms_u32(resize_start);
    if cancel_requested(cancel_cb) {
        return None;
    }

    let convert_start = Instant::now();
    let mut rgba = raster.to_rgba8();
    if let Some(profile) = jpeg_metadata.icc_profile.as_deref() {
        if !apply_icc_to_srgb_rgba(rgba.as_mut(), profile) {
            return None;
        }
    }
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    let mut waveform = include_waveform.then(|| ImageWaveformAccumulator::new(width, height));
    for (index, px) in rgba.chunks_exact(4).enumerate() {
        if index % 65_536 == 0 && cancel_requested(cancel_cb) {
            return None;
        }
        // Density accumulation deliberately shares the final RGBA -> premultiplied BGRA pass.
        // The completed BGRA raster is never scanned again by the native waveform path.
        if let Some(accumulator) = waveform.as_mut() {
            accumulator.add_straight_rgba(index, px);
        }
        let r = px[0] as u32;
        let g = px[1] as u32;
        let b = px[2] as u32;
        let a = px[3] as u32;
        bgra.push(((b * a + 127) / 255) as u8);
        bgra.push(((g * a + 127) / 255) as u8);
        bgra.push(((r * a + 127) / 255) as u8);
        bgra.push(a as u8);
    }
    if cancel_requested(cancel_cb) {
        return None;
    }
    let waveform = match waveform {
        Some(accumulator) => Some(accumulator.finish(cancel_cb)?),
        None => None,
    };
    let convert_ms = elapsed_ms_u32(convert_start);

    Some((
        (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ),
        waveform,
    ))
}

fn decode_gif_frames_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    decode_gif_frames_bgra_reader(file, target_width, target_height, cancel_cb)
}

fn decode_gif_frames_bgra_reader<R: Read>(
    reader: R,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let reader = CancelableImageReader::new(reader, cancel_cb);
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut reader = options.read_info(BufReader::new(reader)).ok()?;
    let original_width = u32::from(reader.width());
    let original_height = u32::from(reader.height());
    if should_skip_native_image_decode(original_width, original_height)
        || u64::from(original_width) * u64::from(original_height) > MAX_ANIMATED_SOURCE_PIXELS
    {
        return None;
    }

    let target_width = if target_width > 0 {
        target_width
    } else {
        MAX_ANIMATED_FRAME_DIMENSION
    };
    let target_height = if target_height > 0 {
        target_height
    } else {
        MAX_ANIMATED_FRAME_DIMENSION
    };
    let target_width = target_width.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let target_height = target_height.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let scale = if original_width > target_width || original_height > target_height {
        (target_width as f64 / original_width as f64)
            .min(target_height as f64 / original_height as f64)
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let frame_bytes = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let max_frames_by_bytes = (MAX_ANIMATED_FRAME_BYTES / (frame_bytes + 4)).max(1);
    let max_frames = MAX_ANIMATED_FRAMES.min(max_frames_by_bytes);

    let mut decoded = Vec::new();
    let mut canvas = vec![
        0u8;
        (original_width as usize)
            .checked_mul(original_height as usize)?
            .checked_mul(4)?
    ];
    let mut previous_disposal = gif::DisposalMethod::Keep;
    let mut previous_rect = (0u16, 0u16, 0u16, 0u16);
    let mut previous_canvas: Option<Vec<u8>> = None;
    while decoded.len() < max_frames {
        if cancel_requested(cancel_cb) {
            return None;
        }
        apply_gif_disposal(
            &mut canvas,
            previous_disposal,
            previous_rect,
            previous_canvas.take(),
            original_width,
        );
        let frame = match reader.read_next_frame().ok()? {
            Some(frame) => frame,
            None => break,
        };
        if cancel_requested(cancel_cb) {
            return None;
        }
        let delay_ms = u32::from(frame.delay).saturating_mul(10).clamp(20, 1_000);
        let saved_canvas = if frame.dispose == gif::DisposalMethod::Previous {
            Some(canvas.clone())
        } else {
            None
        };
        composite_rgba_over_at(
            &mut canvas,
            &frame.buffer,
            original_width,
            original_height,
            RasterRect {
                left: u32::from(frame.left),
                top: u32::from(frame.top),
                width: u32::from(frame.width),
                height: u32::from(frame.height),
            },
        );
        let rgba = image::RgbaImage::from_raw(original_width, original_height, canvas.clone())?;
        let raster = if width == original_width && height == original_height {
            image::DynamicImage::ImageRgba8(rgba)
        } else {
            image::DynamicImage::ImageRgba8(rgba).resize_exact(
                width,
                height,
                image::imageops::FilterType::Triangle,
            )
        };
        let rgba = raster.to_rgba8();
        let mut bgra = Vec::with_capacity(frame_bytes);
        for (index, px) in rgba.chunks_exact(4).enumerate() {
            if index % 65_536 == 0 && cancel_requested(cancel_cb) {
                return None;
            }
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            let a = px[3] as u32;
            bgra.push(((b * a + 127) / 255) as u8);
            bgra.push(((g * a + 127) / 255) as u8);
            bgra.push(((r * a + 127) / 255) as u8);
            bgra.push(a as u8);
        }
        decoded.push((delay_ms, bgra));
        previous_disposal = frame.dispose;
        previous_rect = (frame.left, frame.top, frame.width, frame.height);
        previous_canvas = saved_canvas;
    }
    if cancel_requested(cancel_cb) {
        return None;
    }
    Some((width, height, decoded))
}

const MAX_SVG_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SVG_MARKUP_TOKENS: usize = 100_000;

fn decode_svg_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedImageBgra> {
    if fs::metadata(path).ok()?.len() > MAX_SVG_INPUT_BYTES || cancel_requested(cancel_cb) {
        return None;
    }

    let decode_start = Instant::now();
    let data = fs::read(path).ok()?;
    decode_svg_bgra_bytes_timed(&data, target_width, target_height, cancel_cb, decode_start)
}

fn decode_svg_bgra_bytes(
    data: &[u8],
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedImageBgra> {
    if data.len() as u64 > MAX_SVG_INPUT_BYTES || cancel_requested(cancel_cb) {
        return None;
    }
    decode_svg_bgra_bytes_timed(data, target_width, target_height, cancel_cb, Instant::now())
}

fn decode_svg_bgra_bytes_timed(
    data: &[u8],
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    decode_start: Instant,
) -> Option<DecodedImageBgra> {
    decode_svg_bgra_bytes_timed_internal(
        data,
        target_width,
        target_height,
        cancel_cb,
        decode_start,
        false,
    )
    .map(|(decoded, _)| decoded)
}

fn decode_svg_bgra_bytes_with_waveform(
    data: &[u8],
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<(DecodedImageBgra, Vec<u8>)> {
    if data.len() as u64 > MAX_SVG_INPUT_BYTES || cancel_requested(cancel_cb) {
        return None;
    }
    let (decoded, waveform) = decode_svg_bgra_bytes_timed_internal(
        data,
        target_width,
        target_height,
        cancel_cb,
        Instant::now(),
        true,
    )?;
    Some((decoded, waveform?))
}

fn decode_svg_bgra_bytes_timed_internal(
    data: &[u8],
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
    decode_start: Instant,
    include_waveform: bool,
) -> Option<(DecodedImageBgra, Option<Vec<u8>>)> {
    if !svg_markup_budget_ok(data, cancel_cb) {
        return None;
    }
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options.fontdb = SVG_FONT_DATABASE
        .get_or_init(|| {
            let mut database = resvg::usvg::fontdb::Database::new();
            database.load_system_fonts();
            Arc::new(database)
        })
        .clone();
    let tree = resvg::usvg::Tree::from_data(data, &options).ok()?;
    let original = tree.size();
    let original_width = original.width().ceil() as u32;
    let original_height = original.height().ceil() as u32;
    if original_width == 0 || original_height == 0 || cancel_requested(cancel_cb) {
        return None;
    }
    let decode_ms = elapsed_ms_u32(decode_start);

    let target_width = if target_width > 0 {
        target_width
    } else {
        MAX_IMAGE_RASTER_DIMENSION
    };
    let target_height = if target_height > 0 {
        target_height
    } else {
        MAX_IMAGE_RASTER_DIMENSION
    };
    let target_width = target_width.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let target_height = target_height.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let scale = (target_width as f64 / original.width() as f64)
        .min(target_height as f64 / original.height() as f64)
        .min(1.0);
    let width = ((original.width() as f64 * scale).round() as u32).max(1);
    let height = ((original.height() as f64 * scale).round() as u32).max(1);

    let resize_start = Instant::now();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let resize_ms = elapsed_ms_u32(resize_start);
    if cancel_requested(cancel_cb) {
        return None;
    }

    let convert_start = Instant::now();
    let mut bgra = pixmap.take();
    let mut waveform = include_waveform.then(|| ImageWaveformAccumulator::new(width, height));
    for (index, pixel) in bgra.chunks_exact_mut(4).enumerate() {
        if index % 65_536 == 0 && cancel_requested(cancel_cb) {
            return None;
        }
        // tiny-skia exposes premultiplied RGBA. Accumulate straight channel values while this same
        // pass swaps R/B into the final premultiplied BGRA layout.
        if let Some(accumulator) = waveform.as_mut() {
            accumulator.add_premultiplied_rgba(index, pixel);
        }
        pixel.swap(0, 2);
    }
    let waveform = match waveform {
        Some(accumulator) => Some(accumulator.finish(cancel_cb)?),
        None => None,
    };
    let convert_ms = elapsed_ms_u32(convert_start);
    Some((
        (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ),
        waveform,
    ))
}

fn svg_markup_budget_ok(data: &[u8], cancel_cb: Option<CancelCallback>) -> bool {
    let mut tokens = 0usize;
    for (index, byte) in data.iter().enumerate() {
        if index % 65_536 == 0 && cancel_requested(cancel_cb) {
            return false;
        }
        if *byte == b'<' {
            tokens += 1;
            if tokens > MAX_SVG_MARKUP_TOKENS {
                return false;
            }
        }
    }
    true
}

fn decode_webp_frames_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if let Some(frames) =
        decode_webp_frames_bgra_reader(file, target_width, target_height, cancel_cb)
    {
        return Some(frames);
    }
    if cancel_requested(cancel_cb) {
        return None;
    }
    let (width, height, _, _, _, _, _, bgra) =
        decode_image_bgra(path, target_width, target_height, cancel_cb)?;
    Some((width, height, vec![(100, bgra)]))
}

fn decode_webp_frames_bgra_reader<R: Read + Seek>(
    reader: R,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let reader = CancelableImageReader::new(reader, cancel_cb);
    let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(reader)).ok()?;
    if !decoder.has_animation() {
        return None;
    }
    decode_animation_frames_bgra(
        decoder.into_frames(),
        target_width,
        target_height,
        cancel_cb,
    )
}

fn decode_png_frames_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    decode_png_frames_bgra_reader(file, target_width, target_height, cancel_cb)
}

fn decode_png_frames_bgra_reader<R: Read + Seek>(
    reader: R,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let reader = CancelableImageReader::new(reader, cancel_cb);
    let decoder = image::codecs::png::PngDecoder::new(BufReader::new(reader)).ok()?;
    let (original_width, original_height) = decoder.dimensions();
    if !decoder.is_apng().ok()?
        || u64::from(original_width) * u64::from(original_height) > MAX_ANIMATED_SOURCE_PIXELS
    {
        return None;
    }
    decode_animation_frames_bgra(
        decoder.apng().ok()?.into_frames(),
        target_width,
        target_height,
        cancel_cb,
    )
}

fn write_animation_frames(
    width: u32,
    height: u32,
    frames: Vec<AnimationFrameBgra>,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let total = match animation_frames_packet_length(width, height, &frames) {
        Ok(total) => total,
        Err(_) => return -2,
    };
    if total > i32::MAX as usize {
        return -2;
    }
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }

    unsafe {
        let output = std::slice::from_raw_parts_mut(out, total);
        if write_animation_frames_packet(width, height, &frames, output).is_err() {
            return -2;
        }
    }
    total as i32
}

unsafe fn write_animation_frames_v2(
    width: u32,
    height: u32,
    frames: &[(u32, Vec<u8>)],
    out: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    unsafe { *out_required = 0 };
    let total = match animation_frames_packet_length(width, height, frames) {
        Ok(total) => total,
        Err(AnimationPacketError::Internal) => return QL_ERROR_INTERNAL,
        Err(AnimationPacketError::LimitExceeded) => return QL_ERROR_LIMIT_EXCEEDED,
    };
    unsafe { *out_required = total };
    if out.is_null() || out_cap < total {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }

    let output = unsafe { std::slice::from_raw_parts_mut(out, total) };
    match write_animation_frames_packet(width, height, frames, output) {
        Ok(()) => QL_OK,
        Err(AnimationPacketError::Internal) => QL_ERROR_INTERNAL,
        Err(AnimationPacketError::LimitExceeded) => QL_ERROR_LIMIT_EXCEEDED,
    }
}

unsafe fn write_animation_frames_direct(
    width: u32,
    height: u32,
    frames: &[(u32, Vec<u8>)],
    out_required: *mut usize,
    output_cb: Option<AnimationOutputCallback>,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    unsafe { *out_required = 0 };
    let total = match animation_frames_packet_length(width, height, frames) {
        Ok(total) => total,
        Err(AnimationPacketError::Internal) => return QL_ERROR_INTERNAL,
        Err(AnimationPacketError::LimitExceeded) => return QL_ERROR_LIMIT_EXCEEDED,
    };
    unsafe { *out_required = total };
    let Some(output_cb) = output_cb else {
        return QL_ERROR_INVALID_ARGUMENT;
    };
    let output_ptr = output_cb(total);
    if output_ptr.is_null() {
        return QL_ERROR_INTERNAL;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(output_ptr, total) };
    if cancel_requested(cancel_cb) {
        return QL_ERROR_CANCELLED;
    }
    output[0..4].copy_from_slice(&(frames.len() as u32).to_le_bytes());
    output[4..8].copy_from_slice(&width.to_le_bytes());
    output[8..12].copy_from_slice(&height.to_le_bytes());
    let mut offset = 12;
    for (delay_ms, bgra) in frames {
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        output[offset..offset + 4].copy_from_slice(&delay_ms.to_le_bytes());
        offset += 4;
        output[offset..offset + bgra.len()].copy_from_slice(bgra);
        offset += bgra.len();
    }
    if offset == total {
        QL_OK
    } else {
        QL_ERROR_INTERNAL
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AnimationPacketError {
    Internal,
    LimitExceeded,
}

fn animation_frames_packet_length(
    width: u32,
    height: u32,
    frames: &[(u32, Vec<u8>)],
) -> std::result::Result<usize, AnimationPacketError> {
    if width == 0 || height == 0 || frames.is_empty() {
        return Err(AnimationPacketError::Internal);
    }
    let frame_bytes = usize::try_from(width)
        .map_err(|_| AnimationPacketError::LimitExceeded)?
        .checked_mul(usize::try_from(height).map_err(|_| AnimationPacketError::LimitExceeded)?)
        .and_then(|bytes| bytes.checked_mul(4))
        .ok_or(AnimationPacketError::LimitExceeded)?;
    if frames.iter().any(|(_, bgra)| bgra.len() != frame_bytes) {
        return Err(AnimationPacketError::Internal);
    }
    if frames.len() > u32::MAX as usize {
        return Err(AnimationPacketError::LimitExceeded);
    }
    let frame_packet_bytes = 4usize
        .checked_add(frame_bytes)
        .ok_or(AnimationPacketError::LimitExceeded)?;
    let total = frames
        .len()
        .checked_mul(frame_packet_bytes)
        .and_then(|bytes| 12usize.checked_add(bytes))
        .ok_or(AnimationPacketError::LimitExceeded)?;
    if total > MAX_ANIMATED_FRAME_BYTES + 12 {
        return Err(AnimationPacketError::LimitExceeded);
    }
    Ok(total)
}

fn write_animation_frames_packet(
    width: u32,
    height: u32,
    frames: &[(u32, Vec<u8>)],
    output: &mut [u8],
) -> std::result::Result<(), AnimationPacketError> {
    let total = animation_frames_packet_length(width, height, frames)?;
    if output.len() != total {
        return Err(AnimationPacketError::Internal);
    }

    output[0..4].copy_from_slice(&(frames.len() as u32).to_le_bytes());
    output[4..8].copy_from_slice(&width.to_le_bytes());
    output[8..12].copy_from_slice(&height.to_le_bytes());
    let mut offset = 12;
    for (delay_ms, bgra) in frames {
        output[offset..offset + 4].copy_from_slice(&delay_ms.to_le_bytes());
        offset += 4;
        output[offset..offset + bgra.len()].copy_from_slice(bgra);
        offset += bgra.len();
    }
    if offset != total {
        return Err(AnimationPacketError::Internal);
    }
    Ok(())
}

fn decode_animation_frames_bgra(
    frames: impl IntoIterator<Item = image::ImageResult<image::Frame>>,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<DecodedAnimationBgra> {
    if cancel_requested(cancel_cb) {
        return None;
    }
    let mut frames = frames.into_iter();
    let first = frames.next()?.ok()?;
    let original_width = first.buffer().width();
    let original_height = first.buffer().height();
    if should_skip_native_image_decode(original_width, original_height)
        || u64::from(original_width) * u64::from(original_height) > MAX_ANIMATED_SOURCE_PIXELS
    {
        return None;
    }

    let target_width = if target_width > 0 {
        target_width
    } else {
        MAX_ANIMATED_FRAME_DIMENSION
    };
    let target_height = if target_height > 0 {
        target_height
    } else {
        MAX_ANIMATED_FRAME_DIMENSION
    };
    let target_width = target_width.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let target_height = target_height.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let scale = if original_width > target_width || original_height > target_height {
        (target_width as f64 / original_width as f64)
            .min(target_height as f64 / original_height as f64)
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let frame_bytes = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let max_frames_by_bytes = (MAX_ANIMATED_FRAME_BYTES / (frame_bytes + 4)).max(1);
    let max_frames = MAX_ANIMATED_FRAMES.min(max_frames_by_bytes);

    let mut decoded = Vec::new();
    for frame in std::iter::once(Ok(first)).chain(frames).take(max_frames) {
        let frame = frame.ok()?;
        if cancel_requested(cancel_cb) {
            return None;
        }
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = num.checked_div(den).unwrap_or(100).clamp(20, 1_000);
        let rgba = frame.into_buffer();
        let raster = if width == original_width && height == original_height {
            image::DynamicImage::ImageRgba8(rgba)
        } else {
            image::DynamicImage::ImageRgba8(rgba).resize_exact(
                width,
                height,
                image::imageops::FilterType::Triangle,
            )
        };
        let rgba = raster.to_rgba8();
        let mut bgra = Vec::with_capacity(frame_bytes);
        for (index, px) in rgba.chunks_exact(4).enumerate() {
            if index % 65_536 == 0 && cancel_requested(cancel_cb) {
                return None;
            }
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            let a = px[3] as u32;
            bgra.push(((b * a + 127) / 255) as u8);
            bgra.push(((g * a + 127) / 255) as u8);
            bgra.push(((r * a + 127) / 255) as u8);
            bgra.push(a as u8);
        }
        decoded.push((delay_ms, bgra));
    }
    if cancel_requested(cancel_cb) {
        return None;
    }
    Some((width, height, decoded))
}

#[derive(Default)]
struct JpegMetadata {
    dimensions: Option<(u32, u32)>,
    orientation: Option<u16>,
    icc_profile: Option<Vec<u8>>,
}

#[cfg(test)]
fn jpeg_icc_profile_from_bytes(bytes: &[u8]) -> std::result::Result<Option<Vec<u8>>, ()> {
    Ok(jpeg_metadata_from_reader(std::io::Cursor::new(bytes))?.icc_profile)
}

fn jpeg_metadata_from_reader(mut reader: impl Read) -> std::result::Result<JpegMetadata, ()> {
    const MAX_JPEG_HEADER_BYTES: usize = 8 * 1024 * 1024;
    let mut signature = [0u8; 2];
    if reader.read_exact(&mut signature).is_err() || signature != [0xFF, 0xD8] {
        return Ok(JpegMetadata::default());
    }
    let mut scanned = 2usize;
    let mut chunks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut expected_count = 0u8;
    let mut metadata = JpegMetadata::default();
    loop {
        let mut marker_byte = [0u8; 1];
        reader.read_exact(&mut marker_byte).map_err(|_| ())?;
        scanned += 1;
        if marker_byte[0] != 0xFF {
            return Ok(metadata);
        }
        while marker_byte[0] == 0xFF {
            reader.read_exact(&mut marker_byte).map_err(|_| ())?;
            scanned += 1;
        }
        let marker = marker_byte[0];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        let mut length_bytes = [0u8; 2];
        reader.read_exact(&mut length_bytes).map_err(|_| ())?;
        let len = u16::from_be_bytes(length_bytes) as usize;
        if len < 2 {
            return Err(());
        }
        let payload_len = len - 2;
        scanned = scanned.checked_add(2 + payload_len).ok_or(())?;
        if scanned > MAX_JPEG_HEADER_BYTES {
            return Err(());
        }
        let mut segment = vec![0u8; payload_len];
        reader.read_exact(&mut segment).map_err(|_| ())?;
        if is_jpeg_sof_marker(marker) && segment.len() >= 5 {
            let height = u16::from_be_bytes([segment[1], segment[2]]) as u32;
            let width = u16::from_be_bytes([segment[3], segment[4]]) as u32;
            if width > 0 && height > 0 {
                metadata.dimensions = Some((width, height));
            }
        } else if marker == 0xE1 && segment.starts_with(b"Exif\0\0") {
            metadata.orientation = tiff_orientation(&segment[6..]);
        } else if marker == 0xE2 && segment.len() > 14 && segment.starts_with(b"ICC_PROFILE\0") {
            let sequence = segment[12];
            let count = segment[13];
            if sequence == 0 || count == 0 || sequence > count || count > 16 {
                return Err(());
            }
            expected_count = expected_count.max(count);
            chunks.push((sequence, segment[14..].to_vec()));
        }
    }
    if expected_count == 0 || chunks.len() != expected_count as usize {
        return Ok(metadata);
    }
    chunks.sort_by_key(|(sequence, _)| *sequence);
    for (index, (sequence, _)) in chunks.iter().enumerate() {
        if *sequence as usize != index + 1 {
            return Err(());
        }
    }
    let total = chunks.iter().map(|(_, chunk)| chunk.len()).sum::<usize>();
    if total == 0 || total > 4 * 1024 * 1024 {
        return Err(());
    }
    let mut profile = Vec::with_capacity(total);
    for (_, chunk) in chunks {
        profile.extend_from_slice(&chunk);
    }
    metadata.icc_profile = Some(profile);
    Ok(metadata)
}

fn is_jpeg_sof_marker(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

fn apply_icc_to_srgb_rgba(rgba: &mut [u8], profile: &[u8]) -> bool {
    if rgba.is_empty() || !rgba.len().is_multiple_of(4) || profile.len() > 4 * 1024 * 1024 {
        return false;
    }
    let Some(input) = qcms::Profile::new_from_slice(profile, false) else {
        return false;
    };
    if input.is_sRGB() {
        return true;
    }
    let output = qcms::Profile::new_sRGB();
    let Some(transform) = qcms::Transform::new_to(
        &input,
        &output,
        qcms::DataType::RGBA8,
        qcms::DataType::RGBA8,
        qcms::Intent::Perceptual,
    ) else {
        return false;
    };
    transform.apply(rgba);
    true
}

fn apply_gif_disposal(
    canvas: &mut [u8],
    disposal: gif::DisposalMethod,
    rect: (u16, u16, u16, u16),
    previous_canvas: Option<Vec<u8>>,
    canvas_width: u32,
) {
    match disposal {
        gif::DisposalMethod::Background => {
            let (left, top, width, height) = rect;
            let canvas_stride = canvas_width as usize * 4;
            for y in top as usize..(top as usize + height as usize) {
                for x in left as usize..(left as usize + width as usize) {
                    let offset = y * canvas_stride + x * 4;
                    if offset + 4 <= canvas.len() {
                        canvas[offset..offset + 4].fill(0);
                    }
                }
            }
        }
        gif::DisposalMethod::Previous => {
            if let Some(previous_canvas) = previous_canvas {
                if previous_canvas.len() == canvas.len() {
                    canvas.copy_from_slice(&previous_canvas);
                }
            }
        }
        _ => {}
    }
}

#[derive(Clone, Copy)]
struct RasterRect {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

fn composite_rgba_over_at(
    canvas: &mut [u8],
    frame: &[u8],
    canvas_width: u32,
    canvas_height: u32,
    rect: RasterRect,
) {
    let copy_width = rect.width.min(canvas_width.saturating_sub(rect.left)) as usize;
    let copy_height = rect.height.min(canvas_height.saturating_sub(rect.top)) as usize;
    let canvas_stride = canvas_width as usize * 4;
    let frame_stride = rect.width as usize * 4;
    for y in 0..copy_height {
        for x in 0..copy_width {
            let src = y * frame_stride + x * 4;
            let dst = (rect.top as usize + y) * canvas_stride + (rect.left as usize + x) * 4;
            let a = frame[src + 3] as u32;
            if a == 0 {
                continue;
            }
            if a == 255 {
                canvas[dst..dst + 4].copy_from_slice(&frame[src..src + 4]);
                continue;
            }
            let inv_a = 255 - a;
            for channel in 0..3 {
                let blended =
                    (frame[src + channel] as u32 * a + canvas[dst + channel] as u32 * inv_a + 127)
                        / 255;
                canvas[dst + channel] = blended as u8;
            }
            canvas[dst + 3] = (a + canvas[dst + 3] as u32 * inv_a / 255).min(255) as u8;
        }
    }
}

fn apply_exif_orientation(image: image::DynamicImage, orientation: u16) -> image::DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.fliph().rotate90(),
        6 => image.rotate90(),
        7 => image.fliph().rotate270(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn tiff_orientation(tiff: &[u8]) -> Option<u16> {
    let endian = match tiff.get(0..2)? {
        b"II" => 0,
        b"MM" => 1,
        _ => return None,
    };
    if read_u16(tiff, 2, endian)? != 42 {
        return None;
    }
    let ifd = read_u32(tiff, 4, endian)? as usize;
    let count = read_u16(tiff, ifd, endian)? as usize;
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if entry + 12 > tiff.len() {
            return None;
        }
        if read_u16(tiff, entry, endian)? == 0x0112 {
            let field_type = read_u16(tiff, entry + 2, endian)?;
            let value_count = read_u32(tiff, entry + 4, endian)?;
            if field_type == 3 && value_count == 1 {
                return read_u16(tiff, entry + 8, endian);
            }
        }
    }
    None
}

fn read_u16(bytes: &[u8], offset: usize, endian: u8) -> Option<u16> {
    let raw = [*bytes.get(offset)?, *bytes.get(offset + 1)?];
    Some(if endian == 0 {
        u16::from_le_bytes(raw)
    } else {
        u16::from_be_bytes(raw)
    })
}

fn read_u32(bytes: &[u8], offset: usize, endian: u8) -> Option<u32> {
    let raw = [
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ];
    Some(if endian == 0 {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

fn elapsed_ms_u32(start: Instant) -> u32 {
    start.elapsed().as_millis().min(u32::MAX as u128) as u32
}

fn should_skip_native_image_decode(width: u32, height: u32) -> bool {
    width == 0
        || height == 0
        || (width as u64).saturating_mul(height as u64) > MAX_NATIVE_IMAGE_DECODE_PIXELS
}

fn cancel_requested(cancel_cb: Option<CancelCallback>) -> bool {
    cancel_cb.map(|cb| cb()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    extern "C" fn always_cancel() -> bool {
        true
    }

    static IMAGE_DECODER_CANCEL_POLLS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn cancel_after_decoder_read() -> bool {
        IMAGE_DECODER_CANCEL_POLLS.fetch_add(1, Ordering::SeqCst) >= 2
    }

    #[test]
    fn simple_preview_exports_honor_cancellation_before_file_access() {
        let path = b"missing.file";
        let mut output = [0u8; 16];
        let calls = [
            ql_preview_text_cancelable,
            ql_preview_ebook_cancelable,
            ql_preview_executable_cancelable,
            ql_preview_torrent_cancelable,
        ];

        for call in calls {
            assert_eq!(
                unsafe {
                    call(
                        path.as_ptr(),
                        path.len(),
                        output.as_mut_ptr(),
                        output.len(),
                        Some(always_cancel),
                    )
                },
                -3
            );
        }
    }

    #[test]
    fn animation_exports_honor_cancellation_before_file_access() {
        let path = b"missing.gif";
        let mut output = [0u8; 16];

        assert_eq!(
            unsafe {
                ql_decode_gif_frames_sized_cancelable(
                    path.as_ptr(),
                    path.len(),
                    0,
                    0,
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                )
            },
            -3
        );
        assert_eq!(
            unsafe {
                ql_decode_webp_frames_sized_cancelable(
                    path.as_ptr(),
                    path.len(),
                    0,
                    0,
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                )
            },
            -3
        );
    }

    #[test]
    fn database_preview_export_honors_cancellation_before_file_access() {
        extern "C" fn cancelled() -> bool {
            true
        }
        let path = b"Z:\\missing\\cancelled.db";
        let mut out = vec![0u8; 1024];

        let result = unsafe {
            ql_preview_database_cancelable(
                path.as_ptr(),
                path.len(),
                0,
                0,
                out.as_mut_ptr(),
                out.len(),
                Some(cancelled),
            )
        };

        assert_eq!(result, 0);
    }

    #[test]
    fn archive_entry_export_honors_cancellation_before_file_access() {
        let archive = b"missing.zip";
        let entry = b"entry.txt";
        let mut output = [0u8; 16];

        assert_eq!(
            unsafe {
                ql_extract_archive_entry_cancelable(
                    archive.as_ptr(),
                    archive.len(),
                    entry.as_ptr(),
                    entry.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                )
            },
            -3
        );
    }

    #[test]
    fn hero_exports_honor_cancellation_before_file_access() {
        let path = b"missing.zip";
        let mut output = [0u8; 16];
        let calls = [
            ql_extract_package_icon_cancelable,
            ql_extract_office_image_cancelable,
        ];

        for call in calls {
            assert_eq!(
                unsafe {
                    call(
                        path.as_ptr(),
                        path.len(),
                        output.as_mut_ptr(),
                        output.len(),
                        Some(always_cancel),
                    )
                },
                -3
            );
        }
    }

    #[test]
    fn thumbnail_export_rejects_oversized_request_before_shell_dispatch() {
        let path = b"missing.file";
        let mut output = [0u8; 16];
        assert_eq!(
            unsafe {
                ql_get_thumbnail_cancelable_with_flags(
                    path.as_ptr(),
                    path.len(),
                    513,
                    0,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            QL_ERROR_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn ffi_accepts_multibyte_windows_path_sized_strings() {
        let value = "界".repeat(12_000);
        assert!(value.len() > 32 * 1024);
        assert_eq!(
            utf8_arg(value.as_ptr(), value.len(), MAX_FFI_STRING_BYTES),
            Some(value.as_str())
        );
    }

    #[test]
    fn classify_accepts_known_and_sniffed_config_text() {
        assert_eq!(
            classify("app.config", ".config", b"<configuration>", false),
            "text"
        );
        assert_eq!(
            classify("mysql.cnf", ".cnf", b"[client]\r\nport=3306\r\n", false),
            "text"
        );
        assert_eq!(
            classify(
                "vendor.custom",
                ".custom",
                b"feature.enabled=true\r\n",
                false
            ),
            "text"
        );
        assert_eq!(classify("settings", "", b"root = true\r\n", false), "text");
        assert_eq!(
            classify("legacy.vendor", ".vendor", b"name=caf\xE9\r\n", false),
            "text"
        );
    }

    #[test]
    fn classify_routes_svg_to_image_preview() {
        assert_eq!(
            classify("drawing.svg", ".svg", b"<svg xmlns=", false),
            "image"
        );
    }

    #[test]
    fn classify_routes_sqlite_auxiliary_suffixes_to_database_preview() {
        assert_eq!(classify("data.db-wal", ".db-wal", &[], false), "database");
        assert_eq!(
            classify("data.sqlite3-shm", ".sqlite3-shm", &[], false),
            "database"
        );
    }

    #[test]
    fn classify_requires_rar_magic_instead_of_trusting_the_extension() {
        assert_eq!(
            classify("archive.rar", ".rar", rar_listing::RAR5_SIGNATURE, false),
            "archive"
        );
        assert_eq!(
            classify("legacy.bin", ".bin", rar_listing::RAR4_SIGNATURE, false),
            "archive"
        );
        assert_eq!(
            classify("renamed.rar", ".rar", &[0, 1, 2, 3, 4, 5, 6, 7], false),
            "binary"
        );
        assert!(!preview::is_archive(
            ".rar",
            "archive",
            b"Rar!\x1a\x07\x02\x00"
        ));
    }

    #[test]
    fn classify_accepts_known_text_file_names_with_empty_content() {
        assert_eq!(classify("Dockerfile", "", b"", true), "text");
        assert_eq!(classify("Makefile", "", b"", true), "text");
        assert_eq!(classify(".editorconfig", "", b"", true), "text");
        assert_eq!(classify(".gitignore", "", b"", true), "text");
        assert_eq!(classify(".env", "", b"", true), "text");
        assert_eq!(classify("settings.vendor", ".vendor", b"", true), "text");
        assert_eq!(classify("settings", "", b"", true), "text");
        assert_eq!(classify("empty.zip", ".zip", b"", true), "archive");
    }

    #[test]
    fn classify_accepts_utf16_windows_config_text() {
        let utf16_le = [0xFF, 0xFE, b'W', 0, b'i', 0, b'n', 0];
        let utf16_be = [0xFE, 0xFF, 0, b'W', 0, b'i', 0, b'n'];
        let utf16_localized = [0xFF, 0xFE, 0x4D, 0x50, 0x3D, 0, 0x3C, 0x50];
        assert_eq!(classify("settings.reg", ".reg", &utf16_le, false), "text");
        assert_eq!(
            classify("settings.unknown", ".unknown", &utf16_be, false),
            "text"
        );
        assert_eq!(
            classify("settings.unknown", ".unknown", &utf16_localized, false),
            "text"
        );
    }

    #[test]
    fn classify_does_not_treat_binary_prefixes_as_text() {
        assert_eq!(
            classify("file.unknown", ".unknown", &[0, 1, 2, 3, 4], false),
            "binary"
        );
        assert_eq!(
            classify("file.unknown", ".unknown", &[0xFF, 0xD9, 0x80], false),
            "binary"
        );
        assert_eq!(
            classify("file.bin", ".bin", b"d4:fake4:datae", false),
            "binary"
        );
        assert_eq!(
            classify("file.unknown", ".unknown", b"MZprintable header", false),
            "executable"
        );
    }

    #[test]
    fn probe_cache_invalidates_when_file_changes_within_one_second() {
        let path = temp_image_path("vendor");
        fs::write(&path, b"enabled=true\r\n").expect("write text config");
        let first = probe_json(path.to_str().unwrap()).expect("probe text config");
        assert!(first.contains("\"kind\":\"text\""));

        fs::write(&path, [0u8, 1, 2, 3, 4]).expect("replace with binary");
        let second = probe_json(path.to_str().unwrap()).expect("probe replaced config");
        let _ = fs::remove_file(path);

        assert!(second.contains("\"kind\":\"binary\""));
        assert!(second.contains("\"size\":5"));
    }

    #[test]
    fn native_image_decode_skips_extreme_pixel_counts() {
        assert!(!should_skip_native_image_decode(8_000, 6_000));
        assert!(should_skip_native_image_decode(8_001, 6_000));
        assert!(should_skip_native_image_decode(0, 6_000));
    }

    #[test]
    fn native_image_decode_peak_budget_accepts_48mp_rgba32f_without_orientation() {
        let input = NativeImageDecodeBudgetInput {
            source_width: 8_000,
            source_height: 6_000,
            decoded_bytes: 48_000_000 * 16,
            decoded_bytes_per_pixel: 16,
            target_width: MAX_IMAGE_RASTER_DIMENSION,
            target_height: MAX_IMAGE_RASTER_DIMENSION,
            orientation: None,
            include_waveform: false,
        };
        let peak = checked_native_image_decode_peak_bytes(input).expect("checked 48 MP peak");

        assert_eq!(peak, 843_497_472);
        assert!(peak <= MAX_NATIVE_IMAGE_DECODE_PEAK_BYTES);
        assert!(native_image_decode_fits_peak_budget(input));
    }

    #[test]
    fn native_image_decode_peak_budget_rejects_three_source_orientation_peak() {
        let input = NativeImageDecodeBudgetInput {
            source_width: 8_000,
            source_height: 6_000,
            decoded_bytes: 48_000_000 * 16,
            decoded_bytes_per_pixel: 16,
            target_width: MAX_IMAGE_RASTER_DIMENSION,
            target_height: MAX_IMAGE_RASTER_DIMENSION,
            orientation: Some(5),
            include_waveform: false,
        };
        let peak = checked_native_image_decode_peak_bytes(input).expect("checked orientation peak");

        assert_eq!(peak, 2_304_000_000);
        assert!(peak > MAX_NATIVE_IMAGE_DECODE_PEAK_BYTES);
        assert!(!native_image_decode_fits_peak_budget(input));
    }

    #[test]
    fn native_image_decode_peak_budget_rejects_checked_arithmetic_overflow() {
        assert!(
            checked_native_image_decode_peak_bytes(NativeImageDecodeBudgetInput {
                source_width: u32::MAX,
                source_height: u32::MAX,
                decoded_bytes: u64::MAX,
                decoded_bytes_per_pixel: 16,
                target_width: MAX_IMAGE_RASTER_DIMENSION,
                target_height: MAX_IMAGE_RASTER_DIMENSION,
                orientation: None,
                include_waveform: true,
            })
            .is_none()
        );
    }

    #[test]
    fn native_image_packet_length_checks_plain_waveform_and_overflow() {
        assert_eq!(
            checked_native_image_packet_length(2, 1, false),
            Some(IMAGE_PACKET_HEADER_BYTES + 8)
        );
        assert_eq!(
            checked_native_image_packet_length(2, 1, true),
            Some(IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 8 + IMAGE_WAVEFORM_DENSITY_BYTES)
        );
        assert_eq!(
            checked_native_image_packet_length(usize::MAX, 2, false),
            None
        );
    }

    #[test]
    fn checked_raster_writer_writes_exact_packet_and_rejects_bad_layout() {
        let bgra = [3u8, 2, 1, 255, 30, 20, 10, 128];
        let mut packet = [0u8; 16];
        let mut required = 0usize;

        assert_eq!(
            unsafe {
                write_raster_packet_v2(
                    2,
                    1,
                    &bgra,
                    packet.as_mut_ptr(),
                    packet.len(),
                    &mut required,
                )
            },
            QL_OK
        );
        assert_eq!(required, packet.len());
        assert_eq!(&packet[..4], &2u32.to_le_bytes());
        assert_eq!(&packet[4..8], &1u32.to_le_bytes());
        assert_eq!(&packet[8..], &bgra);

        required = usize::MAX;
        assert_eq!(
            unsafe {
                write_raster_packet_v2(
                    2,
                    1,
                    &bgra[..4],
                    packet.as_mut_ptr(),
                    packet.len(),
                    &mut required,
                )
            },
            QL_ERROR_INTERNAL
        );
        assert_eq!(required, 0);
    }

    #[test]
    fn animation_packet_distinguishes_internal_layout_from_output_limit() {
        let bad_frame = vec![(20, vec![0u8; 3])];
        let mut output = [0u8; 32];
        let mut required = usize::MAX;
        assert_eq!(
            unsafe {
                write_animation_frames_v2(
                    1,
                    1,
                    &bad_frame,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut required,
                )
            },
            QL_ERROR_INTERNAL
        );
        assert_eq!(required, 0);

        let overflowing_dimensions = vec![(20, Vec::new())];
        assert_eq!(
            animation_frames_packet_length(u32::MAX, u32::MAX, &overflowing_dimensions,),
            Err(AnimationPacketError::LimitExceeded)
        );
    }

    #[test]
    fn native_png_decode_preserves_alpha_premultiply() {
        let path = temp_image_path("png");
        let pixels = [255u8, 0, 0, 128, 0, 255, 0, 255];
        image::save_buffer(&path, &pixels, 2, 1, image::ColorType::Rgba8).expect("write png");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode png");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7, vec![0, 0, 128, 128, 0, 255, 0, 255]);
    }

    #[test]
    fn native_bmp_decode_honors_target_size() {
        let path = temp_image_path("bmp");
        let pixels = vec![64u8; 4 * 4 * 3];
        image::save_buffer(&path, &pixels, 4, 4, image::ColorType::Rgb8).expect("write bmp");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 2, 2, None).expect("decode bmp");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 2);
        assert_eq!(decoded.2, 4);
        assert_eq!(decoded.3, 4);
        assert_eq!(decoded.7.len(), 2 * 2 * 4);
    }

    #[test]
    fn native_svg_decode_honors_target_size_and_premultiplies_alpha() {
        let path = temp_image_path("svg");
        fs::write(
            &path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200"><rect width="400" height="200" fill="#ff0000" fill-opacity="0.5"/></svg>"##,
        )
        .expect("write svg");

        let decoded =
            decode_image_bgra(path.to_str().unwrap(), 100, 100, None).expect("decode svg");
        let _ = fs::remove_file(path);

        assert_eq!((decoded.0, decoded.1), (100, 50));
        assert_eq!((decoded.2, decoded.3), (400, 200));
        assert_eq!(decoded.7.len(), 100 * 50 * 4);
        assert_eq!(&decoded.7[..4], &[0, 0, 128, 128]);
    }

    #[test]
    fn native_svg_decode_does_not_read_external_images() {
        let path = temp_image_path("svg");
        let external_path = path.with_extension("png");
        image::save_buffer(
            &external_path,
            &[255u8, 0, 0, 255],
            1,
            1,
            image::ColorType::Rgba8,
        )
        .expect("write external image");
        let external_name = external_path.file_name().unwrap().to_string_lossy();
        fs::write(
            &path,
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image href="{external_name}" width="1" height="1"/></svg>"#
            ),
        )
        .expect("write svg");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 1, 1, None).expect("decode svg");
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(external_path);

        assert_eq!(decoded.7, vec![0, 0, 0, 0]);
    }

    #[test]
    fn native_svg_decode_does_not_read_data_uri_images() {
        // This is a tiny nested SVG encoded as a data URI.  Keeping the payload inline makes the
        // test independent of the filesystem and exercises the separate `resolve_data` hook.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
            <image href="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxIiBoZWlnaHQ9IjEiPjxyZWN0IHdpZHRoPSIxIiBoZWlnaHQ9IjEiIGZpbGw9InJlZCIvPjwvc3ZnPg==" width="1" height="1"/>
        </svg>"##;

        let decoded = decode_svg_bgra_bytes(svg, 1, 1, None).expect("decode svg");

        assert_eq!(decoded.7, vec![0, 0, 0, 0]);
    }

    #[test]
    fn native_svg_decode_clamps_huge_filter_regions() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <defs>
                <filter id="huge" x="-1000000" y="-1000000" width="2000000" height="2000000">
                    <feFlood flood-color="#ff0000"/>
                </filter>
            </defs>
            <rect width="4" height="4" filter="url(#huge)"/>
        </svg>"##;

        let decoded = decode_svg_bgra_bytes(svg, 4, 4, None).expect("decode filtered svg");

        assert_eq!((decoded.0, decoded.1), (4, 4));
        assert_eq!(decoded.7.len(), 4 * 4 * 4);
    }

    #[test]
    fn native_svg_decode_clamps_default_object_bounding_box_filter_regions() {
        // With the SVG default `objectBoundingBox` units, this region used to turn into a
        // multi-terabyte intermediate allocation in the 0.47 renderer.  Keep this exact shape as
        // a regression test for the 0.48 source-pixmap intersection and filter-region clamp.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <defs>
                <filter id="huge" x="-1000" y="-1000" width="2800" height="2600">
                    <feGaussianBlur stdDeviation="8"/>
                </filter>
            </defs>
            <g filter="url(#huge)">
                <rect x="0" y="0" width="4" height="4" fill="#2463eb"/>
            </g>
        </svg>"##;

        let decoded = decode_svg_bgra_bytes(svg, 4, 4, None).expect("decode filtered svg");

        assert_eq!((decoded.0, decoded.1), (4, 4));
        assert_eq!(decoded.7.len(), 4 * 4 * 4);
    }

    #[test]
    fn native_svg_decode_handles_non_finite_filter_arithmetic_without_unbounded_output() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
            <defs>
                <filter id="arithmetic" x="0" y="0" width="2" height="2">
                    <feComposite in="SourceGraphic" in2="SourceGraphic" operator="arithmetic" k1="1e100" k2="0" k3="0" k4="0"/>
                </filter>
            </defs>
            <rect width="2" height="2" fill="#ff0000" filter="url(#arithmetic)"/>
        </svg>"##;

        // Depending on the parser's finite-value policy, malformed arithmetic is either rejected
        // before rendering or clamped by resvg.  Both outcomes must remain fail-closed and bounded.
        if let Some(decoded) = decode_svg_bgra_bytes(svg, 2, 2, None) {
            assert_eq!((decoded.0, decoded.1), (2, 2));
            assert_eq!(decoded.7.len(), 2 * 2 * 4);
        }
    }

    #[test]
    fn native_svg_markup_budget_honors_cancellation_before_tree_parse() {
        static MARKUP_CANCEL_POLLS: AtomicUsize = AtomicUsize::new(0);

        extern "C" fn cancel_after_first_markup_chunk() -> bool {
            MARKUP_CANCEL_POLLS.fetch_add(1, Ordering::SeqCst) >= 1
        }

        let mut svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1"/></svg>"##.to_vec();
        svg.resize(128 * 1024, b' ');
        MARKUP_CANCEL_POLLS.store(0, Ordering::SeqCst);

        assert!(
            decode_svg_bgra_bytes(svg.as_slice(), 1, 1, Some(cancel_after_first_markup_chunk))
                .is_none()
        );
        assert!(MARKUP_CANCEL_POLLS.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn native_jpeg_decode_accepts_exif_orientation_corpus() {
        let path = temp_image_path("jpg");
        let jpeg = jpeg_with_orientation_segment(6);
        std::fs::write(&path, jpeg).expect("write jpeg");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode jpeg");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.2, 1);
        assert_eq!(decoded.3, 2);
    }

    #[test]
    fn native_image_packet_preflight_honors_jpeg_orientation() {
        let mut jpeg = std::io::Cursor::new(jpeg_with_orientation_segment(6));

        assert_eq!(
            preflight_native_image_packet_length(
                &mut jpeg,
                ImageFormat::Jpeg,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                1,
                2,
                false,
            ),
            Ok(IMAGE_PACKET_HEADER_BYTES + 4)
        );
        assert_eq!(
            preflight_native_image_packet_length(
                &mut jpeg,
                ImageFormat::Jpeg,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                1,
                2,
                true,
            ),
            Ok(IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 4 + IMAGE_WAVEFORM_DENSITY_BYTES)
        );
    }

    #[test]
    fn native_jpeg_decode_rejects_invalid_icc_profile_corpus() {
        let path = temp_image_path("jpg");
        let jpeg = jpeg_with_icc_segment();
        std::fs::write(&path, jpeg).expect("write jpeg");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None);
        let _ = std::fs::remove_file(path);

        assert!(decoded.is_none());
    }

    #[test]
    fn jpeg_icc_profile_from_bytes_reassembles_segments() {
        let jpeg = jpeg_with_split_icc_segments();
        let profile = jpeg_icc_profile_from_bytes(&jpeg)
            .expect("parse jpeg")
            .expect("icc profile");

        assert_eq!(profile, b"quicklook-next-test-icc");
    }

    #[test]
    fn jpeg_icc_stream_stops_before_scan_data() {
        let mut jpeg = jpeg_with_split_icc_segments();
        jpeg.extend(std::iter::repeat_n(0x7f, 16 * 1024 * 1024));
        let mut reader = std::io::Cursor::new(&jpeg);

        let metadata = jpeg_metadata_from_reader(&mut reader).expect("parse jpeg");
        let profile = metadata.icc_profile.expect("icc profile");

        assert_eq!(profile, b"quicklook-next-test-icc");
        assert!(reader.position() < 1024 * 1024);
    }

    #[test]
    fn jpeg_metadata_stream_reads_dimensions_and_orientation() {
        let jpeg = jpeg_with_orientation_segment(6);
        let metadata = jpeg_metadata_from_reader(std::io::Cursor::new(jpeg)).expect("parse jpeg");

        assert_eq!(metadata.dimensions, Some((1, 2)));
        assert_eq!(metadata.orientation, Some(6));
    }

    #[test]
    fn native_jpeg_decode_accepts_adobe_transform_corpus() {
        let path = temp_image_path("jpg");
        let jpeg = jpeg_with_adobe_transform_segment();
        std::fs::write(&path, jpeg).expect("write adobe jpeg");

        let decoded =
            decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode adobe jpeg");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7.len(), 2 * 4);
    }

    #[test]
    fn native_tiff_decode_handles_16_bit_luma_corpus() {
        let path = temp_image_path("tiff");
        let pixels = [0u8, 0, 255, 255];
        image::save_buffer(&path, &pixels, 2, 1, image::ColorType::L16).expect("write tiff");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode tiff");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn native_webp_decode_corpus_preserves_pixels() {
        let path = temp_image_path("webp");
        let pixels = [10u8, 20, 30, 255, 200, 210, 220, 255];
        image::save_buffer(&path, &pixels, 2, 1, image::ColorType::Rgba8).expect("write webp");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode webp");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7, vec![30, 20, 10, 255, 220, 210, 200, 255]);
    }

    #[test]
    fn native_webp_frame_extraction_accepts_static_corpus() {
        let path = temp_image_path("webp");
        let pixels = [10u8, 20, 30, 255, 200, 210, 220, 255];
        image::save_buffer(&path, &pixels, 2, 1, image::ColorType::Rgba8).expect("write webp");

        let decoded = decode_webp_frames_bgra(path.to_str().unwrap(), 0, 0, None)
            .expect("decode webp frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.2.len(), 1);
        assert_eq!(decoded.2[0].1, vec![30, 20, 10, 255, 220, 210, 200, 255]);
    }

    #[test]
    #[ignore]
    fn external_image_corpus_smoke() {
        let corpus_dir = match std::env::var("QL_IMAGE_CORPUS_DIR") {
            Ok(value) => std::path::PathBuf::from(value),
            Err(_) => return,
        };

        for file in ["jpeg-cmyk.jpg", "jpeg-wide-gamut-icc.jpg"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                let decoded = decode_image_bgra(path.to_str().unwrap(), 1024, 1024, None)
                    .expect("decode external jpeg sample");
                assert_eq!(
                    jpeg_external_golden(file),
                    Some((decoded.0, decoded.1, decoded.7.len(), fnv1a64(&decoded.7)))
                );
            }
        }
        for file in ["gif-disposal-background.gif", "gif-disposal-previous.gif"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                let frames = decode_gif_frames_bgra(path.to_str().unwrap(), 512, 512, None)
                    .expect("decode external gif sample");
                assert!(!frames.2.is_empty());
            }
        }
        for file in [
            "webp-animated.webp",
            "webp-animated-alpha.webp",
            "webp-animated-blend.webp",
        ] {
            let path = corpus_dir.join(file);
            if path.exists() {
                let frames = decode_webp_frames_bgra(path.to_str().unwrap(), 512, 512, None)
                    .expect("decode external webp sample");
                assert!(
                    frames.2.len() > 1,
                    "animated WebP sample should decode multiple frames: {file}"
                );
                assert_eq!(
                    webp_external_golden(file),
                    Some((
                        frames.0,
                        frames.1,
                        frames.2.len(),
                        fnv1a64(&frames.2[0].1),
                        fnv1a64(&frames.2.last().unwrap().1)
                    ))
                );

                use std::os::windows::io::AsRawHandle;
                let mut source = fs::File::open(&path).expect("open external WebP handle");
                source
                    .seek(SeekFrom::Start(1))
                    .expect("position external WebP handle");
                let position = source.stream_position().expect("WebP handle position");
                let logical_name = file.as_bytes();
                let length = source.metadata().expect("WebP metadata").len();
                let mut required = 0usize;
                assert_eq!(
                    unsafe {
                        ql_decode_animation_frames_handle(
                            source.as_raw_handle() as isize,
                            length,
                            logical_name.as_ptr(),
                            logical_name.len(),
                            512,
                            512,
                            std::ptr::null_mut(),
                            0,
                            &mut required,
                            None,
                        )
                    },
                    QL_ERROR_BUFFER_TOO_SMALL
                );
                let mut packet = vec![0u8; required];
                assert_eq!(
                    unsafe {
                        ql_decode_animation_frames_handle(
                            source.as_raw_handle() as isize,
                            length,
                            logical_name.as_ptr(),
                            logical_name.len(),
                            512,
                            512,
                            packet.as_mut_ptr(),
                            packet.len(),
                            &mut required,
                            None,
                        )
                    },
                    QL_OK
                );
                assert_eq!(
                    u32::from_le_bytes(packet[..4].try_into().unwrap()) as usize,
                    frames.2.len()
                );
                assert_eq!(source.stream_position().unwrap(), position);
            }
        }
        for file in ["avif-still.avif", "heic-still.heic", "jxl-still.jxl"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                assert!(
                    decode_image_bgra(path.to_str().unwrap(), 512, 512, None).is_none(),
                    "modern format unexpectedly gained Rust native decode: {file}"
                );
            }
        }
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    fn webp_external_golden(file: &str) -> Option<(u32, u32, usize, u64, u64)> {
        match file {
            "webp-animated.webp" => Some((483, 512, 8, 16886177616233196080, 12174948178456794470)),
            "webp-animated-alpha.webp" => {
                Some((483, 512, 8, 16886177616233196080, 12174948178456794470))
            }
            "webp-animated-blend.webp" => {
                Some((483, 512, 8, 16886177616233196080, 12174948178456794470))
            }
            _ => None,
        }
    }

    fn jpeg_external_golden(file: &str) -> Option<(u32, u32, usize, u64)> {
        match file {
            "jpeg-cmyk.jpg" => Some((200, 133, 106400, 8550377178255403641)),
            "jpeg-wide-gamut-icc.jpg" => Some((864, 576, 1990656, 3104830790765744668)),
            _ => None,
        }
    }

    #[test]
    fn native_gif_decode_uses_first_animation_frame_corpus() {
        use image::codecs::gif::{GifEncoder, Repeat};

        let path = temp_image_path("gif");
        let first = image::RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        let second = image::RgbaImage::from_raw(1, 1, vec![0, 0, 255, 255]).unwrap();
        let file = std::fs::File::create(&path).expect("create gif");
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).expect("set repeat");
        encoder
            .encode_frame(image::Frame::new(first))
            .expect("write first frame");
        encoder
            .encode_frame(image::Frame::new(second))
            .expect("write second frame");
        drop(encoder);

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode gif");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 1);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7, vec![0, 0, 255, 255]);
    }

    #[test]
    fn native_gif_frame_extraction_returns_bounded_frames() {
        let path = write_two_frame_gif();

        let decoded =
            decode_gif_frames_bgra(path.to_str().unwrap(), 1, 1, None).expect("decode gif frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 1);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.2.len(), 2);
        assert_eq!(decoded.2[0].1, vec![0, 0, 255, 255]);
        assert_eq!(decoded.2[1].1, vec![255, 0, 0, 255]);
    }

    #[test]
    fn native_gif_frame_extraction_honors_background_disposal() {
        let path = write_disposal_gif(gif::DisposalMethod::Background);

        let decoded =
            decode_gif_frames_bgra(path.to_str().unwrap(), 2, 1, None).expect("decode gif frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.2.len(), 3);
        assert_eq!(decoded.2[2].1, vec![0, 255, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn native_gif_frame_extraction_honors_previous_disposal() {
        let path = write_disposal_gif(gif::DisposalMethod::Previous);

        let decoded =
            decode_gif_frames_bgra(path.to_str().unwrap(), 2, 1, None).expect("decode gif frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.2.len(), 3);
        assert_eq!(decoded.2[2].1, vec![0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn image_decoder_reads_honor_cancellation_boundaries() {
        let mut png_bytes = Cursor::new(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("write PNG header");
            writer
                .write_image_data(&[255, 0, 0, 255])
                .expect("write PNG pixels");
            writer.finish().expect("finish PNG");
        }

        IMAGE_DECODER_CANCEL_POLLS.store(0, Ordering::SeqCst);
        assert!(decode_image_bgra_reader(
            Cursor::new(png_bytes.into_inner()),
            "cancel.png",
            1,
            1,
            Some(cancel_after_decoder_read),
            Some(ImageFormat::Png),
        )
        .is_none());
        assert!(IMAGE_DECODER_CANCEL_POLLS.load(Ordering::SeqCst) >= 3);

        let mut gif_bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut gif_bytes, 1, 1, &[]).expect("create GIF");
            let mut pixels = vec![255, 0, 0, 255];
            let frame = gif::Frame::from_rgba_speed(1, 1, &mut pixels, 10);
            encoder.write_frame(&frame).expect("write GIF frame");
        }

        IMAGE_DECODER_CANCEL_POLLS.store(0, Ordering::SeqCst);
        assert!(decode_gif_frames_bgra_reader(
            Cursor::new(gif_bytes),
            1,
            1,
            Some(cancel_after_decoder_read),
        )
        .is_none());
        assert!(IMAGE_DECODER_CANCEL_POLLS.load(Ordering::SeqCst) >= 3);

        let mut apng_bytes = Cursor::new(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut apng_bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(2, 0).expect("enable APNG");
            let mut writer = encoder.write_header().expect("write APNG header");
            writer.set_frame_delay(1, 10).expect("first APNG delay");
            writer
                .write_image_data(&[255, 0, 0, 255])
                .expect("write first APNG frame");
            writer.set_frame_delay(2, 10).expect("second APNG delay");
            writer
                .write_image_data(&[0, 255, 0, 255])
                .expect("write second APNG frame");
            writer.finish().expect("finish APNG");
        }

        IMAGE_DECODER_CANCEL_POLLS.store(0, Ordering::SeqCst);
        assert!(decode_png_frames_bgra_reader(
            Cursor::new(apng_bytes.into_inner()),
            1,
            1,
            Some(cancel_after_decoder_read),
        )
        .is_none());
        assert!(IMAGE_DECODER_CANCEL_POLLS.load(Ordering::SeqCst) >= 3);
    }

    fn temp_image_path(ext: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("quicklook-next-native-{nanos}.{ext}"))
    }

    fn write_two_frame_gif() -> std::path::PathBuf {
        use image::codecs::gif::{GifEncoder, Repeat};

        let path = temp_image_path("gif");
        let first = image::RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        let second = image::RgbaImage::from_raw(1, 1, vec![0, 0, 255, 255]).unwrap();
        let file = std::fs::File::create(&path).expect("create gif");
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).expect("set repeat");
        encoder
            .encode_frame(image::Frame::new(first))
            .expect("write first frame");
        encoder
            .encode_frame(image::Frame::new(second))
            .expect("write second frame");
        path
    }

    fn write_disposal_gif(disposal: gif::DisposalMethod) -> std::path::PathBuf {
        let path = temp_image_path("gif");
        let file = std::fs::File::create(&path).expect("create gif");
        let mut encoder = gif::Encoder::new(file, 2, 1, &[]).expect("gif encoder");
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .expect("set repeat");

        let mut first_pixels = vec![255, 0, 0, 255, 255, 0, 0, 255];
        let mut first = gif::Frame::from_rgba_speed(2, 1, &mut first_pixels, 10);
        first.delay = 10;
        first.dispose = gif::DisposalMethod::Keep;
        encoder.write_frame(&first).expect("write first frame");

        let mut second_pixels = vec![0, 0, 255, 255];
        let mut second = gif::Frame::from_rgba_speed(1, 1, &mut second_pixels, 10);
        second.left = 1;
        second.delay = 10;
        second.dispose = disposal;
        encoder.write_frame(&second).expect("write second frame");

        let mut third_pixels = vec![0, 255, 0, 255];
        let mut third = gif::Frame::from_rgba_speed(1, 1, &mut third_pixels, 10);
        third.left = 0;
        third.delay = 10;
        third.dispose = gif::DisposalMethod::Keep;
        encoder.write_frame(&third).expect("write third frame");
        path
    }

    fn jpeg_with_orientation_segment(orientation: u16) -> Vec<u8> {
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder
            .encode(
                &[255, 0, 0, 0, 0, 255],
                1,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .expect("encode jpeg");
        drop(encoder);

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut app1 = Vec::new();
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;

        let mut output = Vec::with_capacity(jpeg.len() + app1.len() + 4);
        output.extend_from_slice(&jpeg[..2]);
        output.extend_from_slice(&[0xFF, 0xE1]);
        output.extend_from_slice(&len.to_be_bytes());
        output.extend_from_slice(&app1);
        output.extend_from_slice(&jpeg[2..]);
        output
    }

    fn jpeg_with_icc_segment() -> Vec<u8> {
        jpeg_with_icc_chunks(&[b"quicklook-next-test-icc".as_slice()])
    }

    #[test]
    fn native_apng_frame_extraction_returns_composited_frames() {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-animation-{}.png",
            std::process::id()
        ));
        let file = std::fs::File::create(&path).expect("create APNG");
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).expect("enable APNG");
        let mut writer = encoder.write_header().expect("write APNG header");
        writer.set_frame_delay(1, 10).expect("first delay");
        writer
            .write_image_data(&[255, 0, 0, 255, 255, 0, 0, 255])
            .expect("first frame");
        writer.set_frame_delay(2, 10).expect("second delay");
        writer
            .write_image_data(&[0, 255, 0, 255, 0, 255, 0, 255])
            .expect("second frame");
        writer.finish().expect("finish APNG");

        let (width, height, frames) =
            decode_png_frames_bgra(path.to_str().unwrap(), 2, 1, None).expect("decode APNG");
        let _ = std::fs::remove_file(path);

        assert_eq!((width, height, frames.len()), (2, 1, 2));
        assert_eq!(frames[0].0, 100);
        assert_eq!(frames[1].0, 200);
        assert_eq!(&frames[0].1[..4], &[0, 0, 255, 255]);
        assert_eq!(&frames[1].1[..4], &[0, 255, 0, 255]);
    }

    fn jpeg_with_split_icc_segments() -> Vec<u8> {
        jpeg_with_icc_chunks(&[b"quicklook-next-".as_slice(), b"test-icc".as_slice()])
    }

    fn jpeg_with_icc_chunks(chunks: &[&[u8]]) -> Vec<u8> {
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder
            .encode(
                &[255, 0, 0, 0, 255, 0],
                2,
                1,
                image::ExtendedColorType::Rgb8,
            )
            .expect("encode jpeg");
        drop(encoder);

        let mut output = Vec::new();
        output.extend_from_slice(&jpeg[..2]);
        for (index, chunk) in chunks.iter().enumerate() {
            let mut app2 = Vec::new();
            app2.extend_from_slice(b"ICC_PROFILE\0");
            app2.push((index + 1) as u8);
            app2.push(chunks.len() as u8);
            app2.extend_from_slice(chunk);
            let len = (app2.len() + 2) as u16;
            output.extend_from_slice(&[0xFF, 0xE2]);
            output.extend_from_slice(&len.to_be_bytes());
            output.extend_from_slice(&app2);
        }
        output.extend_from_slice(&jpeg[2..]);
        output
    }

    fn jpeg_with_adobe_transform_segment() -> Vec<u8> {
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder
            .encode(
                &[255, 255, 0, 0, 255, 255],
                2,
                1,
                image::ExtendedColorType::Rgb8,
            )
            .expect("encode jpeg");
        drop(encoder);

        let mut app14 = Vec::new();
        app14.extend_from_slice(b"Adobe");
        app14.extend_from_slice(&100u16.to_be_bytes());
        app14.extend_from_slice(&0u16.to_be_bytes());
        app14.extend_from_slice(&0u16.to_be_bytes());
        app14.push(1);
        let len = (app14.len() + 2) as u16;

        let mut output = Vec::with_capacity(jpeg.len() + app14.len() + 4);
        output.extend_from_slice(&jpeg[..2]);
        output.extend_from_slice(&[0xFF, 0xEE]);
        output.extend_from_slice(&len.to_be_bytes());
        output.extend_from_slice(&app14);
        output.extend_from_slice(&jpeg[2..]);
        output
    }
}

// ── Shell thumbnail (fallback preview for any file type) ───────────────────────────────────────
// Ask the Windows thumbnail cache (the same images Explorer shows) via IShellItemImageFactory, and
// return them as top-down premultiplied-ish BGRA. Output layout: [w:u32 LE][h:u32 LE][BGRA bytes].

/// Get a shell thumbnail for `path` at roughly `size` px. Returns total bytes written, or `-needed`.
///
/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_get_thumbnail_cancelable_with_flags(path_utf8, path_len, size, 0, out, out_cap, None)
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_get_thumbnail_cancelable_with_flags(
            path_utf8, path_len, size, 0, out, out_cap, cancel_cb,
        )
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail_cancelable_with_flags(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    flags: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s.to_string(),
            None => return -1,
        };

        let (width, height, bgra) =
            match win32::shell_thumbnail::request(path, size, flags, cancel_cb) {
                Ok(thumbnail) => thumbnail,
                Err(win32::shell_thumbnail::ThumbnailError::InvalidFlags) => {
                    return QL_ERROR_INVALID_ARGUMENT
                }
                Err(win32::shell_thumbnail::ThumbnailError::LimitExceeded) => {
                    return QL_ERROR_LIMIT_EXCEEDED
                }
                Err(win32::shell_thumbnail::ThumbnailError::Cancelled) => {
                    return QL_ERROR_CANCELLED
                }
                Err(win32::shell_thumbnail::ThumbnailError::Unavailable) => {
                    return QL_ERROR_BUFFER_TOO_SMALL
                }
            };
        write_raster_packet(width, height, &bgra, out, out_cap)
    })
}

fn checked_raster_packet_length(width: u32, height: u32, bgra_len: usize) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected_bgra_len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if bgra_len != expected_bgra_len {
        return None;
    }
    8usize.checked_add(expected_bgra_len)
}

fn write_checked_raster_packet(width: u32, height: u32, bgra: &[u8], output: &mut [u8]) -> bool {
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return false;
    };
    if output.len() != total {
        return false;
    }
    output[..4].copy_from_slice(&width.to_le_bytes());
    output[4..8].copy_from_slice(&height.to_le_bytes());
    output[8..].copy_from_slice(bgra);
    true
}

fn write_raster_packet(width: u32, height: u32, bgra: &[u8], out: *mut u8, out_cap: usize) -> i32 {
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return -2;
    };
    if total > i32::MAX as usize {
        return -2;
    }
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }
    let output = unsafe { std::slice::from_raw_parts_mut(out, total) };
    if !write_checked_raster_packet(width, height, bgra, output) {
        return -2;
    }
    total as i32
}

/// # Safety
/// `out_required` must be writable. When the output buffer is large enough, `out_buf` must be
/// writable for `out_cap` bytes and must not overlap `bgra`.
unsafe fn write_raster_packet_v2(
    width: u32,
    height: u32,
    bgra: &[u8],
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    unsafe { *out_required = 0 };
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return QL_ERROR_INTERNAL;
    };
    unsafe { *out_required = total };
    if out_cap < total {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if out_buf.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(out_buf, total) };
    if !write_checked_raster_packet(width, height, bgra, output) {
        return QL_ERROR_INTERNAL;
    }
    QL_OK
}

/// Extract the most likely app/package icon from ZIP-based packages (MSIX/AppX/APK/APKS/AAB).
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };

        let (w, h, bgra) = match preview::extract_package_icon_bgra(path, None) {
            Some(x) => x,
            None => return -2,
        };
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let (w, h, bgra) = match preview::extract_package_icon_bgra(path, cancel_cb) {
            Some(value) => value,
            None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

/// Extract the first useful embedded image from an OOXML Office document.
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };

        let (w, h, bgra) = match preview::extract_office_image_bgra(path, None) {
            Some(x) => x,
            None => return -2,
        };
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let (w, h, bgra) = match preview::extract_office_image_bgra(path, cancel_cb) {
            Some(value) => value,
            None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(&mut out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

// ── Native preview providers (Text/Info/Archive/Folder) (FFI) ────────────────

/// Render a text file preview. Returns JSON length in `out_buf`, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_text_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_text(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render an info-only preview (size + mtime). Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_info(
    path_utf8: *const u8,
    path_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    size: i64,
    modified_unix: i64,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let kind = optional_utf8_arg(kind_utf8, kind_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let json = preview::render_info(path, kind, size, modified_unix);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render an Office document preview. OOXML/ODF paths are parsed in Rust; legacy OLE formats fall back to info.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_office(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_office(path, cancel_cb);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render bounded Rust-native image metadata. Returns JSON length, 0 on failure/no metadata.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_image_metadata(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_image_metadata(path);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render a PE executable metadata preview. Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_executable_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

/// Render a bounded database metadata preview with cancellation support.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_database_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    size: i64,
    modified_unix: i64,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_database_info(path, size, modified_unix, cancel_cb);
        if cancel_requested(cancel_cb) || json.is_empty() {
            return 0;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Shared implementation for ABI 2 HANDLE preview entry points.
///
/// # Safety
/// The pointer and handle requirements are the same as the exported HANDLE entry points. The caller
/// must contain this function in `ffi_boundary`.
unsafe fn reopen_handle_input_v2(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    cancel_cb: Option<CancelCallback>,
) -> std::result::Result<(fs::File, String, i64, i64), i32> {
    if cancel_requested(cancel_cb) {
        return Err(QL_ERROR_CANCELLED);
    }
    let logical_name =
        unsafe { owned_utf8_arg(logical_name_utf8, logical_name_len, MAX_LOGICAL_NAME_BYTES) }
            .ok_or(QL_ERROR_INVALID_ARGUMENT)?;
    let logical_name = std::path::Path::new(&logical_name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .map(str::to_string)
        .ok_or(QL_ERROR_INVALID_ARGUMENT)?;

    let file = native_input::reopen_borrowed_disk_file(source_handle, expected_length).map_err(
        |error| match error {
            native_input::NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE,
            native_input::NativeInputError::Io => QL_ERROR_IO,
            native_input::NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        },
    )?;
    let size = i64::try_from(expected_length).map_err(|_| QL_ERROR_LENGTH_MISMATCH)?;
    let modified_unix = file
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);
    if cancel_requested(cancel_cb) {
        return Err(QL_ERROR_CANCELLED);
    }
    Ok((file, logical_name, size, modified_unix))
}

#[allow(clippy::too_many_arguments)]
// Each HANDLE preview owns its independently reopened file. Moving that file into the one-shot
// renderer keeps path and HANDLE readers on the same `File` monomorphization without dynamic
// dispatch; the original caller-owned HANDLE remains untouched.
unsafe fn preview_handle_v2(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
    renderer: impl FnOnce(fs::File, &str, i64, i64) -> std::result::Result<String, i32>,
) -> i32 {
    if out_required.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    unsafe { *out_required = 0 };
    if out_buf.is_null() && out_cap != 0 {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    if cancel_requested(cancel_cb) {
        return QL_ERROR_CANCELLED;
    }
    let (file, logical_name, size, modified_unix) = match unsafe {
        reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        )
    } {
        Ok(input) => input,
        Err(status) => return status,
    };

    let rendered = renderer(file, &logical_name, size, modified_unix);
    if cancel_requested(cancel_cb) {
        return QL_ERROR_CANCELLED;
    }
    let json = match rendered {
        Ok(json) => json,
        Err(status) => return status,
    };
    if json.is_empty() {
        return QL_ERROR_INTERNAL;
    }
    unsafe { write_v2_out(json.as_bytes(), out_buf, out_cap, out_required) }
}

/// Probe a borrowed Windows file handle without resolving the logical filename as a path.
///
/// # Safety
/// Pointer, output, handle ownership, and logical-name requirements match the ABI 2 HANDLE preview
/// entry points. The caller retains the source handle; Rust reopens it with an independent position.
#[no_mangle]
pub unsafe extern "C" fn ql_probe_file_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            None,
            |mut file, logical_name, size, modified_unix| {
                probe_reader_json(&mut file, logical_name, size as u64, modified_unix)
            },
        )
    })
}

fn reader_preview_status(error: preview::ReaderPreviewError) -> i32 {
    match error {
        preview::ReaderPreviewError::Cancelled => QL_ERROR_CANCELLED,
        preview::ReaderPreviewError::Io => QL_ERROR_IO,
        preview::ReaderPreviewError::Malformed => QL_ERROR_MALFORMED,
        preview::ReaderPreviewError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        preview::ReaderPreviewError::LimitExceeded => QL_ERROR_LIMIT_EXCEEDED,
    }
}

/// Render bounded Rust-native image metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`. The caller retains the source handle; Rust reopens the disk file with
/// an independent position and treats `logical_name` only as a bounded filename hint.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_image_metadata_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, _, _| {
                preview::render_image_metadata_reader(&mut file, logical_name, cancel_cb)
                    .map_err(reader_preview_status)
            },
        )
    })
}

fn reopen_optional_handle(
    source_handle: isize,
    expected_length: u64,
) -> std::result::Result<Option<fs::File>, i32> {
    if source_handle == 0 {
        return if expected_length == 0 {
            Ok(None)
        } else {
            Err(QL_ERROR_INVALID_ARGUMENT)
        };
    }
    native_input::reopen_borrowed_disk_file(source_handle, expected_length)
        .map(Some)
        .map_err(|error| match error {
            native_input::NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE,
            native_input::NativeInputError::Io => QL_ERROR_IO,
            native_input::NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        })
}

/// Render text from a borrowed Windows file handle.
///
/// # Safety
/// `logical_name_utf8` must be readable for `logical_name_len` bytes. `out_required` must point to
/// writable `usize` storage. When `out_buf` is non-null it must be writable for `out_cap` bytes and
/// must not alias Rust-owned output storage. A non-invalid source handle must remain open and stable
/// for the complete call. The caller retains ownership; Rust reopens the file with an independent
/// position and never resolves `logical_name` as a path.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, _, _| {
                let json = preview::render_text_reader(&mut file, logical_name, cancel_cb);
                if json.is_empty() {
                    Err(if cancel_requested(cancel_cb) {
                        QL_ERROR_CANCELLED
                    } else {
                        QL_ERROR_IO
                    })
                } else {
                    Ok(json)
                }
            },
        )
    })
}

/// Render executable metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, size, modified_unix| {
                preview::render_executable_reader(
                    &mut file,
                    logical_name,
                    size,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render torrent metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, size, modified_unix| {
                preview::render_torrent_reader(
                    &mut file,
                    logical_name,
                    size,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render bounded RFC 5322 or Outlook MSG metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_mail_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, modified_unix| {
                preview::render_mail_reader(
                    file,
                    logical_name,
                    expected_length,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render an archive listing from a borrowed Windows file handle.
///
/// Package formats intentionally remain on the legacy path pipeline. The returned archive listing
/// has an empty root path so callers cannot accidentally resolve the logical filename as a path.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_archive_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, modified_unix| {
                preview::render_archive_reader(
                    file,
                    logical_name,
                    expected_length,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render package metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_package_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, _| {
                preview::render_package_reader(file, logical_name, expected_length, cancel_cb)
                    .map_err(reader_preview_status)
            },
        )
    })
}

/// Render an Office preview from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_office_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, modified_unix| {
                preview::render_office_reader(
                    file,
                    logical_name,
                    expected_length,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Extract a useful embedded Office image from a borrowed Windows file handle.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let (width, height, bgra) = match preview::extract_office_image_bgra_reader(
            file,
            expected_length,
            &logical_name,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

/// Extract one Office layout image by its canonical ZIP media reference.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep every pointer valid for the
/// complete call. Rust reopens the source HANDLE with an independent file position. `image_ref`
/// must be a canonical, relative ZIP path under the media root matching `logical_name`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_layout_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    image_ref_utf8: *const u8,
    image_ref_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if target_width == 0
            || target_height == 0
            || target_width > preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
            || target_height > preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
        {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let image_ref =
            match owned_utf8_arg(image_ref_utf8, image_ref_len, MAX_OFFICE_IMAGE_REF_BYTES) {
                Some(image_ref) if !image_ref.is_empty() => image_ref,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
        if !preview::office_layout_image_ref_is_valid(&logical_name, &image_ref) {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (width, height, bgra) = match preview::extract_office_layout_image_bgra_reader(
            file,
            expected_length,
            &logical_name,
            &image_ref,
            target_width,
            target_height,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

/// Extract a package icon from a borrowed Windows file handle.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let (width, height, bgra) = match preview::extract_package_icon_bgra_reader(
            file,
            expected_length,
            &logical_name,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

/// Render an ebook from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, modified_unix| {
                preview::render_ebook_reader(
                    file,
                    logical_name,
                    expected_length,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render a bounded SQLite snapshot from borrowed main, WAL, and SHM Windows file handles.
///
/// `wal_handle` and `shm_handle` are independently optional. Absence is represented only by a
/// `(0, 0)` handle/length pair; a nonzero handle with a zero length represents a present empty
/// companion. The WAL is the only source used to update the visible database snapshot. SHM data is
/// bounded diagnostic input and is never trusted for database correctness.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle` and apply to every nonzero handle. The caller retains ownership of all
/// handles. Rust reopens each one with an independent position and never resolves `logical_name` as
/// a path.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_sqlite_handles(
    main_handle: isize,
    main_expected_length: u64,
    wal_handle: isize,
    wal_expected_length: u64,
    shm_handle: isize,
    shm_expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            main_handle,
            main_expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut main, logical_name, _, modified_unix| {
                if wal_handle == 0 && wal_expected_length != 0
                    || shm_handle == 0 && shm_expected_length != 0
                {
                    return Err(QL_ERROR_INVALID_ARGUMENT);
                }
                if main_expected_length > preview::MAX_DATABASE_HANDLE_BYTES
                    || wal_expected_length > preview::MAX_SQLITE_WAL_BYTES
                    || shm_expected_length > preview::MAX_SQLITE_SHM_BYTES
                {
                    return Err(QL_ERROR_LIMIT_EXCEEDED);
                }
                let mut wal = reopen_optional_handle(wal_handle, wal_expected_length)?;
                let mut shm = reopen_optional_handle(shm_handle, shm_expected_length)?;
                let wal_reader = wal.as_mut().map(|file| file as &mut dyn Read);
                let shm_reader = shm.as_mut().map(|file| file as &mut dyn Read);
                preview::render_database_reader(
                    &mut main,
                    main_expected_length,
                    preview::DatabaseCompanionReader {
                        reader: wal_reader,
                        length: wal_expected_length,
                    },
                    preview::DatabaseCompanionReader {
                        reader: shm_reader,
                        length: shm_expected_length,
                    },
                    logical_name,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_executable(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render an archive listing. Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_archive(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_archive(path, cancel_cb);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Extract a previewable archive entry into a bounded temp cache. Returns UTF-8 path length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if archive_path_utf8.is_null()
            || entry_path_utf8.is_null()
            || out_buf.is_null()
            || out_cap == 0
        {
            return 0;
        }
        let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES)
        {
            Some(s) => s,
            None => return 0,
        };
        let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let Some(path) = preview::extract_archive_entry_to_temp(archive_path, entry_path, None)
        else {
            return 0;
        };
        write_json_out(&path, out_buf, out_cap)
    })
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_cancelable(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if archive_path_utf8.is_null()
            || entry_path_utf8.is_null()
            || out_buf.is_null()
            || out_cap == 0
        {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES)
        {
            Some(s) => s,
            None => return 0,
        };
        let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let Some(path) =
            preview::extract_archive_entry_to_temp(archive_path, entry_path, cancel_cb)
        else {
            return if cancel_requested(cancel_cb) { -3 } else { 0 };
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&path, out_buf, out_cap)
    })
}

/// Extract a bounded ZIP entry from a borrowed Windows file handle into the private temp cache.
///
/// On success, output is the non-NUL-terminated UTF-8 temp path. A size probe or undersized buffer
/// returns `QL_ERROR_BUFFER_TOO_SMALL`, sets `out_required`, and removes the temporary extraction so
/// two-pass negotiation cannot leak temp roots.
///
/// # Safety
/// Both UTF-8 pointers must be readable for their declared lengths. `out_required` must be writable.
/// When non-null, `out_buf` must be writable for `out_cap` bytes. The source handle must remain open
/// and stable for the complete call; ownership remains with the caller.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let entry_path = match owned_utf8_arg(
            entry_path_utf8,
            entry_path_len,
            MAX_ARCHIVE_ENTRY_NAME_BYTES,
        ) {
            Some(entry_path) => entry_path,
            None => return QL_ERROR_INVALID_ARGUMENT,
        };
        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extracted = match preview::extract_archive_entry_to_temp_reader(
            &mut file,
            expected_length,
            &logical_name,
            &entry_path,
            cancel_cb,
        ) {
            Ok(path) => path,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            preview::discard_archive_extract_path(&extracted);
            return QL_ERROR_CANCELLED;
        }
        let status = write_v2_out(extracted.as_bytes(), out_buf, out_cap, out_required);
        if status != QL_OK || cancel_requested(cancel_cb) {
            preview::discard_archive_extract_path(&extracted);
            return if cancel_requested(cancel_cb) {
                QL_ERROR_CANCELLED
            } else {
                status
            };
        }
        QL_OK
    })
}

/// Stream a bounded ZIP entry into a caller-owned output disk-file HANDLE.
///
/// The output object must be a newly created zero-length file whose handle permits write sharing.
/// Rust validates and reopens both handles with independent file positions, writes at most
/// `output_capacity` bytes, and reports the exact resulting length through `out_written`.
///
/// # Safety
/// Both UTF-8 pointers must be readable for their declared lengths and `out_written` must be
/// writable. Both raw handles must remain valid for the complete call. Ownership stays with the
/// caller; Rust closes only its independently reopened handles.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_to_output_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    output_handle: isize,
    output_capacity: u64,
    out_written: *mut u64,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_written.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_written = 0;
        if source_handle == output_handle
            || output_capacity == 0
            || output_capacity > preview::MAX_ARCHIVE_EXTRACT_BYTES
        {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let entry_path = match owned_utf8_arg(
            entry_path_utf8,
            entry_path_len,
            MAX_ARCHIVE_ENTRY_NAME_BYTES,
        ) {
            Some(entry_path) => entry_path,
            None => return QL_ERROR_INVALID_ARGUMENT,
        };
        let (mut source, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let mut output = match native_input::reopen_borrowed_disk_file_for_output(output_handle, 0)
        {
            Ok(output) => output,
            Err(native_input::NativeInputError::InvalidHandle) => {
                return QL_ERROR_INVALID_HANDLE;
            }
            Err(native_input::NativeInputError::LengthMismatch) => {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            Err(native_input::NativeInputError::Io) => return QL_ERROR_IO,
        };
        if output.seek(SeekFrom::Start(0)).is_err() || output.set_len(0).is_err() {
            return QL_ERROR_IO;
        }

        let written = match preview::extract_archive_entry_to_writer_reader(
            &mut source,
            expected_length,
            &logical_name,
            &entry_path,
            &mut output,
            output_capacity,
            cancel_cb,
        ) {
            Ok(written) => written,
            Err(error) => {
                let _ = output.set_len(0);
                return reader_preview_status(error);
            }
        };
        if cancel_requested(cancel_cb) {
            let _ = output.set_len(0);
            return QL_ERROR_CANCELLED;
        }
        if !matches!(output.metadata(), Ok(metadata) if metadata.len() == written) {
            let _ = output.set_len(0);
            return QL_ERROR_IO;
        }
        *out_written = written;
        QL_OK
    })
}

/// Render an ebook preview. Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_ebook_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_ebook(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render a torrent metadata preview. Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_torrent_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_torrent(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Render a folder listing. Returns JSON length, 0 on failure.
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_folder(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_folder(path, cancel_cb);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Check if a file is text-like (for routing in the App).
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_is_text(
    ext_utf8: *const u8,
    ext_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let ext = optional_utf8_arg(ext_utf8, ext_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let magic = match optional_bytes_arg(magic, magic_len, MAX_FFI_MAGIC_BYTES) {
            Some(bytes) => bytes,
            None => return 0,
        };
        if preview::is_text(ext, magic) {
            1
        } else {
            0
        }
    })
}

/// Check if a file is an archive (for routing).
#[doc = include_str!("ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_is_archive(
    ext_utf8: *const u8,
    ext_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let ext = optional_utf8_arg(ext_utf8, ext_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let kind = optional_utf8_arg(kind_utf8, kind_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let magic = match optional_bytes_arg(magic, magic_len, MAX_FFI_MAGIC_BYTES) {
            Some(bytes) => bytes,
            None => return 0,
        };
        if preview::is_archive(ext, kind, magic) {
            1
        } else {
            0
        }
    })
}

fn write_json_out(json: &str, out_buf: *mut u8, out_cap: usize) -> i32 {
    let bytes = json.as_bytes();
    let needed = bytes.len();
    if needed > out_cap {
        return -(needed as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);
    }
    needed as i32
}

/// # Safety
/// `out_required` must be writable and, when output is copied, `out_buf` must be writable for
/// `out_cap` bytes and must not overlap `bytes`.
unsafe fn write_v2_out(
    bytes: &[u8],
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    unsafe { *out_required = bytes.len() };
    if bytes.len() > out_cap {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        if out_buf.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len()) };
    }
    QL_OK
}

fn ffi_boundary(body: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(QL_ERROR_INTERNAL)
}

fn ffi_void_boundary(body: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(body));
}

#[test]
fn native_abi_version_is_stable() {
    assert_eq!(ql_abi_version(), 3);
    let required = QL_FEATURE_HANDLE_TEXT
        | QL_FEATURE_HANDLE_EXECUTABLE
        | QL_FEATURE_HANDLE_TORRENT
        | QL_FEATURE_HANDLE_SQLITE_SNAPSHOT
        | QL_FEATURE_HANDLE_ARCHIVE
        | QL_FEATURE_HANDLE_OFFICE
        | QL_FEATURE_HANDLE_EBOOK
        | QL_FEATURE_HANDLE_ARCHIVE_ENTRY;
    let required = required | QL_FEATURE_HANDLE_STATIC_IMAGE;
    let required = required | QL_FEATURE_HANDLE_SVG;
    let required = required | QL_FEATURE_HANDLE_GIF;
    let required = required | QL_FEATURE_HANDLE_PACKAGE;
    let required = required | QL_FEATURE_HANDLE_PACKAGE_ICON;
    let required = required | QL_FEATURE_HANDLE_PROBE;
    let required = required | QL_FEATURE_HANDLE_RASTER_IMAGE;
    let required = required | QL_FEATURE_HANDLE_ANIMATION;
    let required = required | QL_FEATURE_HANDLE_OFFICE_LAYOUT_IMAGE;
    let required = required | QL_FEATURE_HANDLE_IMAGE_WAVEFORM;
    let required = required | QL_FEATURE_HANDLE_ARCHIVE_ENTRY_OUTPUT;
    let required = required | QL_FEATURE_HANDLE_IMAGE_METADATA;
    let required = required | QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT;
    let required = required | QL_FEATURE_HANDLE_MAIL;
    assert_eq!(ql_capabilities() & required, required);
}

#[cfg(test)]
mod handle_v2_tests {
    use super::*;
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use std::os::windows::io::AsRawHandle;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // SystemTime's observable precision can be lower than nanoseconds on Windows. The
    // counter keeps parallel tests from writing to the same temporary input file.
    static HANDLE_TEST_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn create_input(extension: &str, bytes: &[u8]) -> (PathBuf, fs::File) {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-handle-v2-{}-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            HANDLE_TEST_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        fs::write(&path, bytes).expect("write handle input");
        let file = fs::File::open(&path).expect("open handle input");
        (path, file)
    }

    fn create_output(extension: &str, bytes: &[u8]) -> (PathBuf, fs::File) {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-handle-v2-output-{}-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            HANDLE_TEST_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create handle output");
        file.write_all(bytes).expect("initialize handle output");
        file.flush().expect("flush handle output");
        file.seek(SeekFrom::Start(0))
            .expect("position handle output");
        (path, file)
    }

    extern "C" fn always_cancel() -> bool {
        true
    }

    static IMAGE_METADATA_CANCEL_POLLS: AtomicUsize = AtomicUsize::new(0);
    static STATIC_IMAGE_PREFLIGHT_CANCEL_POLLS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn cancel_image_metadata_during_read() -> bool {
        IMAGE_METADATA_CANCEL_POLLS.fetch_add(1, Ordering::SeqCst) >= 4
    }

    extern "C" fn cancel_static_image_after_preflight() -> bool {
        STATIC_IMAGE_PREFLIGHT_CANCEL_POLLS.fetch_add(1, Ordering::SeqCst) >= 2
    }

    static ARCHIVE_OUTPUT_CANCEL_POLLS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn cancel_archive_output_after_validation() -> bool {
        ARCHIVE_OUTPUT_CANCEL_POLLS.fetch_add(1, Ordering::SeqCst) >= 2
    }

    #[allow(clippy::too_many_arguments)]
    fn call_text_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_text_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    fn call_probe_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
    ) -> i32 {
        unsafe {
            ql_probe_file_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_executable_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_executable_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_torrent_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_torrent_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_mail_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_mail_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_archive_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_archive_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_package_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_package_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_image_metadata_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_image_metadata_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_office_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_office_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_office_image_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_extract_office_image_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_office_layout_image_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        image_ref_utf8: *const u8,
        image_ref_len: usize,
        target_width: u32,
        target_height: u32,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_extract_office_layout_image_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                image_ref_utf8,
                image_ref_len,
                target_width,
                target_height,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_package_icon_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_extract_package_icon_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_image_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        target_width: u32,
        target_height: u32,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_decode_image_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                target_width,
                target_height,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_image_with_waveform_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        target_width: u32,
        target_height: u32,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_decode_image_with_waveform_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                target_width,
                target_height,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_gif_frames_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        target_width: u32,
        target_height: u32,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_decode_gif_frames_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                target_width,
                target_height,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_animation_frames_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        target_width: u32,
        target_height: u32,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_decode_animation_frames_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                target_width,
                target_height,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_ebook_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_ebook_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_archive_entry_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        entry_path_utf8: *const u8,
        entry_path_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_extract_archive_entry_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                entry_path_utf8,
                entry_path_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_archive_entry_output_handle(
        source_handle: isize,
        expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        entry_path_utf8: *const u8,
        entry_path_len: usize,
        output_handle: isize,
        output_capacity: u64,
        out_written: *mut u64,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_extract_archive_entry_to_output_handle(
                source_handle,
                expected_length,
                logical_name_utf8,
                logical_name_len,
                entry_path_utf8,
                entry_path_len,
                output_handle,
                output_capacity,
                out_written,
                cancel_cb,
            )
        }
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start ZIP entry");
            writer.write_all(bytes).expect("write ZIP entry");
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    fn valid_epub_bytes() -> Vec<u8> {
        zip_bytes(&[
            (
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            (
                "OEBPS/content.opf",
                br#"<package><metadata><dc:title>Handle Book</dc:title><dc:creator>Rust</dc:creator></metadata><manifest><item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
            ),
            (
                "OEBPS/chapter.xhtml",
                br#"<html><body><h1>Handle Chapter</h1><p>Reader content.</p></body></html>"#,
            ),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    fn call_sqlite_handles(
        main_handle: isize,
        main_expected_length: u64,
        wal_handle: isize,
        wal_expected_length: u64,
        shm_handle: isize,
        shm_expected_length: u64,
        logical_name_utf8: *const u8,
        logical_name_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_required: *mut usize,
        cancel_cb: Option<CancelCallback>,
    ) -> i32 {
        unsafe {
            ql_preview_sqlite_handles(
                main_handle,
                main_expected_length,
                wal_handle,
                wal_expected_length,
                shm_handle,
                shm_expected_length,
                logical_name_utf8,
                logical_name_len,
                out_buf,
                out_cap,
                out_required,
                cancel_cb,
            )
        }
    }

    type SafeHandleCall =
        fn(isize, u64, *const u8, usize, *mut u8, usize, *mut usize, Option<CancelCallback>) -> i32;

    fn preview_json_with(
        call: SafeHandleCall,
        file: &fs::File,
        logical_name: &str,
    ) -> serde_json::Value {
        let logical_name = logical_name.as_bytes();
        let mut required = 0usize;
        let status = call(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > 0);

        let mut output = vec![0u8; required];
        let status = call(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_OK);
        serde_json::from_slice(&output[..required]).expect("handle preview JSON")
    }

    fn preview_json(file: &fs::File, logical_name: &str) -> serde_json::Value {
        preview_json_with(call_text_handle, file, logical_name)
    }

    #[test]
    fn text_handle_preview_obeys_buffer_contract_without_moving_caller_position() {
        let (path, mut file) = create_input("md", b"# Handle preview\n\nRust input");
        file.seek(SeekFrom::Start(5))
            .expect("position caller handle");
        let original_position = file.stream_position().expect("read caller position");
        let logical_name = b"README.md";
        let expected_length = file.metadata().unwrap().len();

        let mut required = usize::MAX;
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > 8);
        assert_eq!(file.stream_position().unwrap(), original_position);

        let mut small = [0u8; 8];
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            small.as_mut_ptr(),
            small.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > small.len());
        assert_eq!(file.stream_position().unwrap(), original_position);

        let exact_length = required;
        let mut exact = vec![0u8; exact_length];
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            exact.as_mut_ptr(),
            exact.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_OK);
        assert_eq!(required, exact_length);
        let json: serde_json::Value =
            serde_json::from_slice(&exact).expect("exact handle preview JSON");
        assert_eq!(json["kind"], "markdown");
        assert_eq!(json["title"], "README.md");
        let rendered = json.to_string();
        assert!(rendered.contains("Handle preview"));
        assert!(rendered.contains("Rust input"));
        assert_eq!(file.stream_position().unwrap(), original_position);

        let mut oversized = vec![0u8; exact_length + 32];
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            oversized.as_mut_ptr(),
            oversized.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_OK);
        assert_eq!(required, exact_length);
        assert_eq!(file.stream_position().unwrap(), original_position);

        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            oversized.as_mut_ptr(),
            oversized.len(),
            std::ptr::null_mut(),
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            expected_length,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            1,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
        assert_eq!(
            file.stream_position().expect("caller position after FFI"),
            original_position
        );

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_metadata_handle_obeys_buffer_contract_without_moving_caller_position() {
        let bytes = b"GIF89a\x02\x00\x03\x00\x00\x00\x00\x3B";
        let (path, mut file) = create_input("bin", bytes);
        file.seek(SeekFrom::Start(7))
            .expect("position caller image handle");
        let position = file.stream_position().unwrap();
        let expected_length = file.metadata().unwrap().len();
        let logical_name = b"pinned-image.gif";
        let mut required = usize::MAX;

        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 8);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut small = [0xA5u8; 8];
        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                small.as_mut_ptr(),
                small.len(),
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > small.len());
        assert_eq!(small, [0xA5; 8]);
        assert_eq!(file.stream_position().unwrap(), position);

        let exact_length = required;
        let mut output = vec![0u8; exact_length];
        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(required, exact_length);
        let metadata: serde_json::Value =
            serde_json::from_slice(&output).expect("image metadata JSON");
        assert_eq!(metadata["format"], "GIF");
        assert_eq!(metadata["width"], 2);
        assert_eq!(metadata["height"], 3);
        assert_eq!(metadata["animated"], false);
        assert_eq!(file.stream_position().unwrap(), position);

        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                output.as_mut_ptr(),
                output.len(),
                std::ptr::null_mut(),
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        required = usize::MAX;
        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                1,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_metadata_handle_rejects_invalid_non_disk_wrong_length_and_logical_name() {
        let bytes = b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x3B";
        let (path, file) = create_input("gif", bytes);
        let logical_name = b"image.gif";
        let mut required = usize::MAX;

        assert_eq!(
            call_image_metadata_handle(
                0,
                0,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(required, 0);

        let non_disk = fs::OpenOptions::new()
            .read(true)
            .open("NUL")
            .expect("open non-disk NUL device");
        required = usize::MAX;
        assert_eq!(
            call_image_metadata_handle(
                non_disk.as_raw_handle() as isize,
                0,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len() + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                MAX_LOGICAL_NAME_BYTES + 1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);

        drop(non_disk);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_metadata_handle_honors_cancellation_without_moving_caller_position() {
        let bytes = b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x3B";
        let (path, mut file) = create_input("gif", bytes);
        file.seek(SeekFrom::Start(4))
            .expect("position cancelled image handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"cancelled.gif";
        let mut required = usize::MAX;
        IMAGE_METADATA_CANCEL_POLLS.store(0, Ordering::SeqCst);

        assert_eq!(
            call_image_metadata_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(cancel_image_metadata_during_read),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn handle_probe_obeys_buffer_contract_without_moving_caller_position() {
        let (path, mut file) = create_input("svg", br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        file.seek(SeekFrom::Start(7))
            .expect("position caller handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"renamed.svg";
        let length = file.metadata().unwrap().len();
        let mut required = 0;

        let status = call_probe_handle(
            file.as_raw_handle() as isize,
            length,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > 0);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut output = vec![0; required];
        let status = call_probe_handle(
            file.as_raw_handle() as isize,
            length,
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
        );
        assert_eq!(status, QL_OK);
        let probe: serde_json::Value = serde_json::from_slice(&output).expect("handle probe JSON");
        assert_eq!(probe["path"], "renamed.svg");
        assert_eq!(probe["extension"], ".svg");
        assert_eq!(probe["kind"], "image");
        assert_eq!(probe["size"], length);
        assert_eq!(file.stream_position().unwrap(), position);

        assert_eq!(
            call_probe_handle(
                file.as_raw_handle() as isize,
                length + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(file.stream_position().unwrap(), position);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn path_and_handle_probes_share_apng_animation_metadata() {
        let mut apng = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut apng, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(2, 0).expect("enable APNG");
            let mut writer = encoder.write_header().expect("write APNG header");
            writer.set_frame_delay(1, 10).expect("first APNG delay");
            writer
                .write_image_data(&[255, 0, 0, 255, 255, 0, 0, 255])
                .expect("first APNG frame");
            writer.set_frame_delay(2, 10).expect("second APNG delay");
            writer
                .write_image_data(&[0, 255, 0, 255, 0, 255, 0, 255])
                .expect("second APNG frame");
            writer.finish().expect("finish APNG");
        }

        let (path, mut file) = create_input("png", &apng);
        let path_probe: serde_json::Value =
            serde_json::from_str(&probe_json(path.to_str().unwrap()).expect("path probe"))
                .expect("path probe JSON");
        assert_eq!(path_probe["isAnimated"], true);

        file.seek(SeekFrom::Start(5))
            .expect("position caller APNG handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"renamed.png";
        let length = file.metadata().unwrap().len();
        let mut required = 0usize;
        assert_eq!(
            call_probe_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut output = vec![0u8; required];
        assert_eq!(
            call_probe_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            ),
            QL_OK
        );
        let handle_probe: serde_json::Value =
            serde_json::from_slice(&output).expect("HANDLE APNG probe JSON");
        assert_eq!(handle_probe["isAnimated"], path_probe["isAnimated"]);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn text_handle_preview_rejects_invalid_non_disk_and_wrong_length_handles() {
        let logical_name = b"sample.txt";
        let mut output = [0u8; 128];
        let mut required = usize::MAX;
        let status = call_text_handle(
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            Some(always_cancel),
        );
        assert_eq!(status, QL_ERROR_CANCELLED);
        assert_eq!(required, 0);

        let status = call_text_handle(
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_HANDLE);
        assert_eq!(required, 0);

        let thread = unsafe { windows::Win32::System::Threading::GetCurrentThread() };
        let status = call_text_handle(
            thread.0 as isize,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_HANDLE);

        let (path, file) = create_input("txt", b"length check");
        let status = call_text_handle(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len() + 1,
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_LENGTH_MISMATCH);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn package_handles_use_logical_name_and_preserve_caller_position_and_contract() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            24,
            20,
            image::Rgba([30, 120, 220, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode package icon");
        let icon = png.into_inner();
        let package = zip_bytes(&[
            (
                "AppxManifest.xml",
                br#"<Package><Identity Name="Handle.Package" Version="1.2.3.4" Publisher="CN=Rust"/><Applications><Application Executable="handle.exe"><uap:VisualElements DisplayName="Handle Package" Square150x150Logo="Assets/icon.png"/></Application></Applications></Package>"#,
            ),
            ("Assets/icon.png", icon.as_slice()),
        ]);
        let (path, mut file) = create_input("bin", &package);
        file.seek(SeekFrom::Start(11))
            .expect("position package handle");
        let position = file.stream_position().unwrap();
        let expected_length = file.metadata().unwrap().len();
        let logical_name = br"Z:\missing\nonexistent.appx";

        let mut required = 0usize;
        assert_eq!(
            call_package_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 0);
        assert_eq!(file.stream_position().unwrap(), position);
        let mut json = vec![0u8; required];
        assert_eq!(
            call_package_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                json.as_mut_ptr(),
                json.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        let json: serde_json::Value = serde_json::from_slice(&json[..required]).unwrap();
        assert_eq!(json["kind"], "package");
        assert_eq!(json["title"], "Handle Package - 1.2.3.4");
        assert!(json["text"]
            .as_str()
            .unwrap()
            .contains("Preview image: found"));

        required = 0;
        assert_eq!(
            call_package_icon_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 8);
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_package_icon_handle(
                file.as_raw_handle() as isize,
                expected_length,
                logical_name.as_ptr(),
                logical_name.len(),
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 24);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 20);
        assert_eq!(file.stream_position().unwrap(), position);

        for call in [call_package_handle, call_package_icon_handle] {
            assert_eq!(
                call(
                    file.as_raw_handle() as isize,
                    expected_length + 1,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    None,
                ),
                QL_ERROR_LENGTH_MISMATCH
            );
            assert_eq!(
                call(
                    file.as_raw_handle() as isize,
                    expected_length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    Some(always_cancel),
                ),
                QL_ERROR_CANCELLED
            );
        }
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn text_handle_preview_uses_only_a_bounded_logical_filename() {
        let (markdown_path, markdown_file) =
            create_input("txt", b"# Logical name controls format\n");
        let markdown = preview_json(&markdown_file, r"C:\missing\README.md");
        assert_eq!(markdown["kind"], "markdown");
        assert_eq!(markdown["title"], "README.md");

        let (csv_path, csv_file) = create_input("txt", "名称,值\nRust,安全\n".as_bytes());
        let csv = preview_json(&csv_file, "资料.csv");
        assert_eq!(csv["kind"], "table");
        assert!(csv["title"].as_str().unwrap().starts_with("资料.csv"));

        let (tsv_path, tsv_file) = create_input("txt", b"name\tvalue\nRust\thandle\n");
        let tsv = preview_json(&tsv_file, "data.tsv");
        assert_eq!(tsv["kind"], "table");
        assert!(tsv["title"].as_str().unwrap().starts_with("data.tsv"));

        let mut required = usize::MAX;
        let mut output = [0u8; 128];
        let overlong_name = vec![b'a'; MAX_LOGICAL_NAME_BYTES + 1];
        let status = call_text_handle(
            markdown_file.as_raw_handle() as isize,
            markdown_file.metadata().unwrap().len(),
            overlong_name.as_ptr(),
            overlong_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
        assert_eq!(required, 0);
        let status = call_text_handle(
            markdown_file.as_raw_handle() as isize,
            markdown_file.metadata().unwrap().len(),
            std::ptr::null(),
            0,
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);

        drop(markdown_file);
        drop(csv_file);
        drop(tsv_file);
        let _ = fs::remove_file(markdown_path);
        let _ = fs::remove_file(csv_path);
        let _ = fs::remove_file(tsv_path);
    }

    #[test]
    fn executable_handle_preview_reads_from_zero_without_moving_caller_position() {
        let mut bytes = vec![0u8; 512];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84usize;
        bytes[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[coff + 4..coff + 8].copy_from_slice(&0x6543_2100u32.to_le_bytes());
        bytes[coff + 16..coff + 18].copy_from_slice(&0x70u16.to_le_bytes());
        bytes[coff + 18..coff + 20].copy_from_slice(&0x0022u16.to_le_bytes());
        let optional = coff + 20;
        bytes[optional..optional + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        bytes[optional + 16..optional + 20].copy_from_slice(&0x1234u32.to_le_bytes());
        bytes[optional + 24..optional + 32].copy_from_slice(&0x1400_0000u64.to_le_bytes());
        bytes[optional + 32..optional + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[optional + 36..optional + 40].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[optional + 56..optional + 60].copy_from_slice(&0x5000u32.to_le_bytes());
        bytes[optional + 68..optional + 70].copy_from_slice(&3u16.to_le_bytes());

        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(19))
            .expect("position executable handle");
        let position = file.stream_position().unwrap();
        let json = preview_json_with(
            call_executable_handle,
            &file,
            r"C:\missing\logical-demo.exe",
        );
        assert_eq!(json["kind"], "executable");
        assert_eq!(json["title"], "logical-demo.exe - x64");
        assert!(json["text"].as_str().unwrap().contains("Machine: x64"));
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn torrent_handle_preview_is_bounded_and_reports_malformed_input() {
        let torrent = b"d8:announce16:https://tracker/4:infod6:lengthi123e4:name10:sample.binee";
        let (path, mut file) = create_input("bin", torrent);
        file.seek(SeekFrom::Start(7))
            .expect("position torrent handle");
        let position = file.stream_position().unwrap();
        let json = preview_json_with(call_torrent_handle, &file, r"C:\missing\logical.torrent");
        assert_eq!(json["kind"], "torrent");
        assert_eq!(json["title"], "sample.bin - 1 files");
        assert!(json["text"]
            .as_str()
            .unwrap()
            .contains("Tracker: https://tracker/"));
        assert_eq!(file.stream_position().unwrap(), position);
        drop(file);
        let _ = fs::remove_file(path);

        let (malformed_path, malformed_file) = create_input("bin", b"not-bencode");
        let logical_name = b"broken.torrent";
        let mut required = usize::MAX;
        let status = call_torrent_handle(
            malformed_file.as_raw_handle() as isize,
            malformed_file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_MALFORMED);
        assert_eq!(required, 0);
        drop(malformed_file);
        let _ = fs::remove_file(malformed_path);
    }

    #[test]
    fn mail_handle_preview_uses_pinned_bytes_without_moving_caller_position() {
        let mail = b"From: sender@example.test\r\nSubject: Pinned Outlook message\r\n\r\nbody";
        let (path, mut file) = create_input("bin", mail);
        file.seek(SeekFrom::Start(9)).expect("position mail handle");
        let position = file.stream_position().unwrap();
        let logical_path = r"C:\missing\logical.eml";

        let json = preview_json_with(call_mail_handle, &file, logical_path);

        assert_eq!(json["kind"], "mail");
        assert!(json["text"]
            .as_str()
            .unwrap()
            .contains("Subject: Pinned Outlook message"));
        assert_eq!(file.stream_position().unwrap(), position);

        let logical_name = b"logical.eml";
        let mut required = usize::MAX;
        assert_eq!(
            call_mail_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len() + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(required, 0);
        assert_eq!(
            call_mail_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn archive_handle_preview_uses_pinned_bytes_empty_root_and_buffer_contract() {
        let bytes = zip_bytes(&[
            ("folder/readme.txt", b"pinned archive content"),
            ("root.bin", b"\x01\x02"),
        ]);
        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(11))
            .expect("position archive handle");
        let position = file.stream_position().unwrap();
        let json = preview_json_with(call_archive_handle, &file, r"C:\does-not-exist\logical.zip");
        assert_eq!(json["kind"], "archive");
        assert_eq!(json["listing"]["rootName"], "logical.zip");
        assert_eq!(json["listing"]["rootPath"], "");
        assert_eq!(json["listing"]["canPreviewEntries"], true);
        assert!(json["listing"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "folder/readme.txt"));
        assert_eq!(file.stream_position().unwrap(), position);

        let logical_name = b"logical.zip";
        let mut small = [0u8; 8];
        let mut required = usize::MAX;
        let status = call_archive_handle(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            small.as_mut_ptr(),
            small.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > small.len());
        assert_eq!(file.stream_position().unwrap(), position);

        required = usize::MAX;
        assert_eq!(
            call_archive_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(
            call_archive_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len() + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(required, 0);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn office_handle_preview_and_hero_use_same_pinned_file_without_moving_position() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            8,
            8,
            image::Rgba([20, 80, 160, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode Office image");
        let png = png.into_inner();
        let bytes = zip_bytes(&[
            (
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Office HANDLE marker</w:t></w:r></w:p></w:body></w:document>"#,
            ),
            ("word/media/image1.png", &png),
        ]);
        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(13))
            .expect("position Office handle");
        let position = file.stream_position().unwrap();

        let json = preview_json_with(call_office_handle, &file, r"C:\missing\logical.docx");
        assert_eq!(json["kind"], "office");
        assert!(json["text"]
            .as_str()
            .unwrap()
            .contains("Office HANDLE marker"));
        assert_eq!(file.stream_position().unwrap(), position);

        let logical_name = b"logical.docx";
        let mut required = 0usize;
        assert_eq!(
            call_office_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 8);
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_office_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(required, packet.len());
        assert!(u32::from_le_bytes(packet[..4].try_into().unwrap()) > 0);
        assert!(u32::from_le_bytes(packet[4..8].try_into().unwrap()) > 0);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn office_layout_json_uses_image_references_instead_of_base64() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            8,
            6,
            image::Rgba([15, 90, 180, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode Office layout image");
        let image = encoded.into_inner();
        let bytes = zip_bytes(&[
            (
                "word/document.xml",
                br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Lazy image</w:t></w:r></w:p></w:body></w:document>"#,
            ),
            ("word/media/image1.png", image.as_slice()),
            ("word/media/../traversal.png", b"unsafe"),
            ("/word/media/absolute.png", b"unsafe"),
            (r"word\media\backslash.png", b"unsafe"),
        ]);
        let (path, file) = create_input("bin", &bytes);
        let json = preview_json_with(call_office_handle, &file, r"C:\missing\layout.docx");
        let items = json["officeLayout"]["pages"][0]["items"]
            .as_array()
            .expect("Office layout items");
        let image_item = items
            .iter()
            .find(|item| item["kind"] == "image")
            .expect("Office image layout item");

        assert_eq!(
            items.iter().filter(|item| item["kind"] == "image").count(),
            1
        );
        assert_eq!(image_item["imageRef"], "word/media/image1.png");
        assert_eq!(image_item["imageByteLength"], image.len() as u64);
        assert_eq!(image_item["imageName"], "image1.png");
        assert_eq!(image_item["mimeType"], "image/png");
        assert!(image_item.get("imageBase64").is_none());
        assert!(!serde_json::to_string(&json)
            .expect("serialize Office preview")
            .contains("imageBase64"));

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn office_layout_with_eighteen_large_images_stays_below_pipe_limit() {
        let mut pixels = vec![0u8; 384 * 384 * 4];
        let mut state = 0x8f31_29abu32;
        for pixel in pixels.chunks_exact_mut(4) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pixel[0] = (state >> 24) as u8;
            pixel[1] = (state >> 16) as u8;
            pixel[2] = (state >> 8) as u8;
            pixel[3] = 255;
        }
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(
            image::ImageBuffer::from_raw(384, 384, pixels).expect("large image pixels"),
        )
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode large Office image");
        let image = encoded.into_inner();
        assert!(image.len() > 256 * 1024);
        assert!(image.len() <= 768 * 1024);

        let mut slide =
            String::from(r#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r"><p:cSld><p:spTree>"#);
        let mut relationships = String::from(r#"<Relationships xmlns="rels">"#);
        for index in 1..=18 {
            slide.push_str(&format!(
                r#"<p:pic><p:blipFill><a:blip r:embed="rId{index}"/></p:blipFill><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr></p:pic>"#,
                ((index - 1) % 6) * 1_000_000,
                ((index - 1) / 6) * 1_000_000,
            ));
            relationships.push_str(&format!(
                r#"<Relationship Id="rId{index}" Target="../media/image{index}.png"/>"#
            ));
        }
        slide.push_str("</p:spTree></p:cSld></p:sld>");
        relationships.push_str("</Relationships>");

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, payload) in [
            (
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="p"><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
                    .as_slice(),
            ),
            ("ppt/slides/slide1.xml", slide.as_bytes()),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                relationships.as_bytes(),
            ),
        ] {
            writer.start_file(name, options).expect("start PPT part");
            writer.write_all(payload).expect("write PPT part");
        }
        for index in 1..=18 {
            writer
                .start_file(format!("ppt/media/image{index}.png"), options)
                .expect("start PPT image");
            writer.write_all(&image).expect("write PPT image");
        }
        let bytes = writer.finish().expect("finish PPTX").into_inner();
        let (path, file) = create_input("bin", &bytes);
        let logical_name = b"large-images.pptx";
        let mut required = 0usize;
        assert_eq!(
            call_office_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required < 4 * 1024 * 1024);
        let mut output = vec![0u8; required];
        assert_eq!(
            call_office_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        let json: serde_json::Value =
            serde_json::from_slice(&output[..required]).expect("large Office layout JSON");
        let image_items = json["officeLayout"]["pages"][0]["items"]
            .as_array()
            .expect("PPT layout items");
        assert_eq!(image_items.len(), 18);
        assert!(image_items.iter().all(|item| {
            item.get("imageBase64").is_none()
                && item["imageRef"].as_str().is_some()
                && item["imageByteLength"] == image.len() as u64
        }));

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn office_layout_image_handle_decodes_exact_ref_and_preserves_position() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            2048,
            1024,
            image::Rgba([40, 120, 220, 128]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode large Office raster");
        let image = encoded.into_inner();
        let bytes = zip_bytes(&[
            (
                "word/document.xml",
                br#"<w:document xmlns:w="w"><w:body/></w:document>"#,
            ),
            ("word/media/image1.png", image.as_slice()),
        ]);
        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(17))
            .expect("position Office layout image handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"layout.docx";
        let image_ref = b"word/media/image1.png";
        let length = file.metadata().unwrap().len();

        let mut required = 0usize;
        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                1024,
                1024,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(required, 8 + 1024 * 512 * 4);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut small = vec![0u8; required - 1];
        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                1024,
                1024,
                small.as_mut_ptr(),
                small.len(),
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                1024,
                1024,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 1024);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 512);
        assert_eq!(&packet[8..12], &[110, 60, 20, 128]);
        assert_eq!(file.stream_position().unwrap(), position);

        for (target_width, target_height) in [(0, 64), (64, 0), (1025, 64), (64, 1025)] {
            assert_eq!(
                call_office_layout_image_handle(
                    file.as_raw_handle() as isize,
                    length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    image_ref.as_ptr(),
                    image_ref.len(),
                    target_width,
                    target_height,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    None,
                ),
                QL_ERROR_INVALID_ARGUMENT
            );
            assert_eq!(required, 0);
        }
        assert_eq!(file.stream_position().unwrap(), position);

        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                64,
                64,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn office_layout_image_handle_rejects_untrusted_refs_and_bad_entries() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            4,
            3,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode Office image");
        let image = encoded.into_inner();
        let bytes = zip_bytes(&[
            ("word/media/image.png", image.as_slice()),
            ("word/media/not-image.png", b"not a PNG"),
        ]);
        let (path, file) = create_input("bin", &bytes);
        let logical_name = b"layout.docx";
        let length = file.metadata().unwrap().len();
        let mut required = usize::MAX;

        for image_ref in [
            "/word/media/image.png",
            r"word\media\image.png",
            "word/media/../image.png",
            "word/media/./image.png",
            "ppt/media/image.png",
            "C:/word/media/image.png",
        ] {
            assert_eq!(
                call_office_layout_image_handle(
                    file.as_raw_handle() as isize,
                    length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    image_ref.as_ptr(),
                    image_ref.len(),
                    64,
                    64,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    None,
                ),
                QL_ERROR_INVALID_ARGUMENT,
                "unexpected status for {image_ref}"
            );
            assert_eq!(required, 0);
        }

        for image_ref in ["word/media/missing.png", "word/media/not-image.png"] {
            assert_eq!(
                call_office_layout_image_handle(
                    file.as_raw_handle() as isize,
                    length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    image_ref.as_ptr(),
                    image_ref.len(),
                    64,
                    64,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    None,
                ),
                QL_ERROR_MALFORMED,
                "unexpected status for {image_ref}"
            );
            assert_eq!(required, 0);
        }

        let overlong_ref = vec![b'a'; MAX_OFFICE_IMAGE_REF_BYTES + 1];
        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                overlong_ref.as_ptr(),
                overlong_ref.len(),
                64,
                64,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );

        drop(file);
        let _ = fs::remove_file(path);

        let oversized = vec![0u8; 768 * 1024 + 1];
        let bytes = zip_bytes(&[("word/media/large.png", oversized.as_slice())]);
        let (oversized_path, oversized_file) = create_input("bin", &bytes);
        let image_ref = b"word/media/large.png";
        assert_eq!(
            call_office_layout_image_handle(
                oversized_file.as_raw_handle() as isize,
                oversized_file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                64,
                64,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LIMIT_EXCEEDED
        );
        drop(oversized_file);
        let _ = fs::remove_file(oversized_path);
    }

    #[test]
    fn office_layout_image_handle_enforces_handle_length_and_cancel_contract() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode Office image");
        let image = encoded.into_inner();
        let bytes = zip_bytes(&[("word/media/image.png", image.as_slice())]);
        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(9))
            .expect("position Office layout source");
        let position = file.stream_position().unwrap();
        let logical_name = b"layout.docx";
        let image_ref = b"word/media/image.png";
        let length = file.metadata().unwrap().len();
        let mut required = usize::MAX;

        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                2,
                2,
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(
            call_office_layout_image_handle(
                file.as_raw_handle() as isize,
                length + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                2,
                2,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(
            call_office_layout_image_handle(
                0,
                0,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                2,
                2,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        let thread = unsafe { windows::Win32::System::Threading::GetCurrentThread() };
        assert_eq!(
            call_office_layout_image_handle(
                thread.0 as isize,
                0,
                logical_name.as_ptr(),
                logical_name.len(),
                image_ref.as_ptr(),
                image_ref.len(),
                2,
                2,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(required, 0);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn static_image_handle_decodes_ico_without_moving_caller_position() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            16,
            8,
            image::Rgba([30, 120, 220, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Ico)
        .expect("encode ICO");
        let (path, mut file) = create_input("bin", &encoded.into_inner());
        file.seek(SeekFrom::Start(7))
            .expect("position image handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"logical.ico";
        let mut required = 0usize;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                8,
                8,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                8,
                8,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(packet[12..16].try_into().unwrap()), 8);
        assert_eq!(file.stream_position().unwrap(), position);

        let wrong_name = b"logical.avif";
        required = usize::MAX;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                wrong_name.as_ptr(),
                wrong_name.len(),
                8,
                8,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);

        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            2,
            2,
            image::Rgba([10, 20, 30, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode PNG");
        let (path, file) = create_input("bin", &png.into_inner());
        required = usize::MAX;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                8,
                8,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_MALFORMED
        );
        assert_eq!(required, 0);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn gif_handle_decodes_static_and_animation_packets_without_moving_caller_position() {
        let mut encoded = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut encoded, 2, 1, &[]).expect("GIF encoder");
            let mut first_pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
            let mut first = gif::Frame::from_rgba_speed(2, 1, &mut first_pixels, 10);
            first.delay = 2;
            encoder.write_frame(&first).expect("first GIF frame");
            let mut second_pixels = vec![0, 0, 255, 255, 255, 255, 0, 255];
            let mut second = gif::Frame::from_rgba_speed(2, 1, &mut second_pixels, 10);
            second.delay = 3;
            encoder.write_frame(&second).expect("second GIF frame");
        }
        let (path, mut file) = create_input("bin", &encoded);
        file.seek(SeekFrom::Start(5)).expect("position GIF handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"logical.gif";
        let length = file.metadata().unwrap().len();

        let mut required = 0usize;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 28);
        assert_eq!(file.stream_position().unwrap(), position);

        required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(
            required, 0,
            "GIF must never enter the native RGB waveform path"
        );
        assert_eq!(file.stream_position().unwrap(), position);

        required = 0;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 1);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut generic_required = 0usize;
        assert_eq!(
            call_animation_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut generic_required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut generic_packet = vec![0u8; generic_required];
        assert_eq!(
            call_animation_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                generic_packet.as_mut_ptr(),
                generic_packet.len(),
                &mut generic_required,
                None,
            ),
            QL_OK
        );
        assert_eq!(generic_packet, packet);
        assert_eq!(file.stream_position().unwrap(), position);

        let wrong_name = b"logical.png";
        required = usize::MAX;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length,
                wrong_name.as_ptr(),
                wrong_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);

        drop(file);
        let _ = fs::remove_file(path);

        let (path, file) = create_input("bin", b"not a GIF image");
        required = usize::MAX;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_MALFORMED
        );
        assert_eq!(required, 0);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn animation_handle_decodes_apng_and_rejects_static_webp_without_moving_position() {
        let mut apng = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut apng, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(2, 0).expect("enable APNG");
            let mut writer = encoder.write_header().expect("write APNG header");
            writer.set_frame_delay(1, 10).expect("first APNG delay");
            writer
                .write_image_data(&[255, 0, 0, 255, 255, 0, 0, 255])
                .expect("first APNG frame");
            writer.set_frame_delay(2, 10).expect("second APNG delay");
            writer
                .write_image_data(&[0, 255, 0, 255, 0, 255, 0, 255])
                .expect("second APNG frame");
            writer.finish().expect("finish APNG");
        }

        let (path, mut file) = create_input("bin", &apng);
        file.seek(SeekFrom::Start(7)).expect("position APNG handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"logical.png";
        let length = file.metadata().unwrap().len();
        let mut required = 0usize;
        assert_eq!(
            call_animation_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_animation_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 1);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut legacy_required = usize::MAX;
        assert_eq!(
            call_gif_frames_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut legacy_required,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(legacy_required, 0);
        assert_eq!(file.stream_position().unwrap(), position);
        drop(file);
        let _ = fs::remove_file(path);

        let mut webp = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            2,
            1,
            image::Rgba([10, 20, 30, 255]),
        ))
        .write_to(&mut webp, image::ImageFormat::WebP)
        .expect("encode static WebP");
        let (path, mut file) = create_input("bin", &webp.into_inner());
        file.seek(SeekFrom::Start(3)).expect("position WebP handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"logical.webp";
        let mut required = usize::MAX;
        assert_eq!(
            call_animation_frames_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_MALFORMED
        );
        assert_eq!(required, 0);
        assert_eq!(file.stream_position().unwrap(), position);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn static_image_handle_decodes_svg_without_path_or_external_resources() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <image href="missing.png" width="40" height="20"/>
            <rect width="20" height="20" fill="#2463eb"/>
        </svg>"##;
        let (path, mut file) = create_input("bin", svg);
        file.seek(SeekFrom::Start(9)).expect("position SVG handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"missing-logical.svg";
        let mut required = 0usize;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 20);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 10);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 40);
        assert_eq!(u32::from_le_bytes(packet[12..16].try_into().unwrap()), 20);
        assert_eq!(file.stream_position().unwrap(), position);
        assert!(packet[28..].chunks_exact(4).any(|pixel| pixel[3] != 0));

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn raster_image_handle_decodes_png_without_moving_caller_position() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128])
                .unwrap();
        }
        let (path, mut file) = create_input("bin", &png_bytes);
        file.seek(SeekFrom::Start(5)).expect("position PNG handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"missing-logical.png";
        let mut required = 0;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0; required];
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                2,
                1,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 1);
        assert_eq!(file.stream_position().unwrap(), position);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn raster_image_handle_sizes_output_before_full_pixel_decode() {
        const EDGE: u32 = MAX_IMAGE_RASTER_DIMENSION;
        let raster_bytes = usize::try_from(EDGE).unwrap() * usize::try_from(EDGE).unwrap() * 4;
        let claimed_file_bytes = u32::try_from(54 + raster_bytes).unwrap();
        let mut truncated_bmp = vec![0u8; 54];
        truncated_bmp[0..2].copy_from_slice(b"BM");
        truncated_bmp[2..6].copy_from_slice(&claimed_file_bytes.to_le_bytes());
        truncated_bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        truncated_bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        truncated_bmp[18..22].copy_from_slice(&(EDGE as i32).to_le_bytes());
        truncated_bmp[22..26].copy_from_slice(&(EDGE as i32).to_le_bytes());
        truncated_bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        truncated_bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
        truncated_bmp[34..38].copy_from_slice(&(raster_bytes as u32).to_le_bytes());

        let (path, mut file) = create_input("bin", &truncated_bmp);
        file.seek(SeekFrom::Start(7))
            .expect("position truncated BMP handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"truncated.bmp";
        let length = file.metadata().unwrap().len();

        let mut required = 0usize;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                EDGE,
                EDGE,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(required, IMAGE_PACKET_HEADER_BYTES + raster_bytes);
        assert!(required > 8 * 1024 * 1024);

        let mut waveform_required = 0usize;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                EDGE,
                EDGE,
                std::ptr::null_mut(),
                0,
                &mut waveform_required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(
            waveform_required,
            IMAGE_WAVEFORM_PACKET_HEADER_BYTES + raster_bytes + IMAGE_WAVEFORM_DENSITY_BYTES
        );
        assert_eq!(file.stream_position().unwrap(), position);

        STATIC_IMAGE_PREFLIGHT_CANCEL_POLLS.store(0, Ordering::SeqCst);
        let mut cancelled_required = usize::MAX;
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                EDGE,
                EDGE,
                std::ptr::null_mut(),
                0,
                &mut cancelled_required,
                Some(cancel_static_image_after_preflight),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(cancelled_required, 0);

        STATIC_IMAGE_PREFLIGHT_CANCEL_POLLS.store(0, Ordering::SeqCst);
        cancelled_required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                EDGE,
                EDGE,
                std::ptr::null_mut(),
                0,
                &mut cancelled_required,
                Some(cancel_static_image_after_preflight),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(cancelled_required, 0);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut exact = vec![0u8; required];
        assert_eq!(
            call_image_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                EDGE,
                EDGE,
                exact.as_mut_ptr(),
                exact.len(),
                &mut required,
                None,
            ),
            QL_ERROR_MALFORMED
        );
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_waveform_handle_returns_exact_extended_packet_in_one_decode_contract() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 4, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[
                    255, 0, 0, 255, // opaque red
                    0, 255, 0, 128, // translucent green
                    0, 0, 255, 0, // transparent blue: excluded from the density scope
                    255, 255, 255, 255, // opaque white
                ])
                .unwrap();
        }

        let (path, mut file) = create_input("bin", &png_bytes);
        file.seek(SeekFrom::Start(5))
            .expect("position PNG waveform handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"waveform.png";
        let length = file.metadata().unwrap().len();
        let expected_packet_bytes =
            IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 4 * 4 + IMAGE_WAVEFORM_DENSITY_BYTES;

        let mut required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                4,
                1,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(required, expected_packet_bytes);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut undersized = vec![0u8; required - 1];
        let mut still_required = 0usize;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                4,
                1,
                undersized.as_mut_ptr(),
                undersized.len(),
                &mut still_required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(still_required, expected_packet_bytes);

        let mut packet = vec![0u8; required];
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                4,
                1,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(required, packet.len());
        assert_eq!(file.stream_position().unwrap(), position);

        let header: Vec<u32> = packet[..IMAGE_WAVEFORM_PACKET_HEADER_BYTES]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(header[0..4], [4, 1, 4, 1]);
        assert_eq!(header[7], IMAGE_WAVEFORM_WIDTH);
        assert_eq!(header[8], IMAGE_WAVEFORM_HEIGHT);
        assert_eq!(header[9] as usize, IMAGE_WAVEFORM_DENSITY_BYTES);

        let raster =
            &packet[IMAGE_WAVEFORM_PACKET_HEADER_BYTES..IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 16];
        assert_eq!(
            raster,
            &[
                0, 0, 255, 255, // red BGRA
                0, 128, 0, 128, // premultiplied translucent green BGRA
                0, 0, 0, 0, // transparent blue becomes transparent BGRA
                255, 255, 255, 255, // white BGRA
            ]
        );
        let density = &packet[IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 16..];
        let plane = IMAGE_WAVEFORM_PLANE_BYTES;
        assert!(density[0] > 0, "red at x=0 belongs at row 0");
        assert!(
            density[plane + 48] > 0,
            "green at x=1 belongs at row 0, column 48"
        );
        assert!(
            density[2 * plane + 144] > 0,
            "white at x=3 contributes blue at row 0, column 144"
        );
        for channel in 0..IMAGE_WAVEFORM_CHANNELS {
            for row in 0..IMAGE_WAVEFORM_HEIGHT as usize {
                assert_eq!(
                    density[channel * plane + row * IMAGE_WAVEFORM_WIDTH as usize + 96],
                    0,
                    "fully transparent x=2 must not contribute to any channel"
                );
            }
        }

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_waveform_handle_supports_all_native_static_raster_formats() {
        let formats = [
            (ImageFormat::Png, "png"),
            (ImageFormat::Jpeg, "jpg"),
            (ImageFormat::Bmp, "bmp"),
            (ImageFormat::Tiff, "tiff"),
            (ImageFormat::WebP, "webp"),
        ];
        for (format, extension) in formats {
            let mut encoded = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(3, 2, |x, y| {
                image::Rgb([
                    (30 + x * 50) as u8,
                    (40 + y * 80) as u8,
                    (200 - x * 20) as u8,
                ])
            }))
            .write_to(&mut encoded, format)
            .unwrap_or_else(|error| panic!("encode {extension}: {error}"));
            let (path, mut file) = create_input("bin", &encoded.into_inner());
            file.seek(SeekFrom::Start(3))
                .unwrap_or_else(|error| panic!("position {extension}: {error}"));
            let position = file.stream_position().unwrap();
            let logical_name = format!("sample.{extension}");
            let logical_name = logical_name.as_bytes();
            let length = file.metadata().unwrap().len();
            let mut required = 0usize;
            assert_eq!(
                call_image_with_waveform_handle(
                    file.as_raw_handle() as isize,
                    length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    3,
                    2,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    None,
                ),
                QL_ERROR_BUFFER_TOO_SMALL,
                "{extension}"
            );
            assert_eq!(
                required,
                IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 3 * 2 * 4 + IMAGE_WAVEFORM_DENSITY_BYTES,
                "{extension}"
            );
            let mut packet = vec![0u8; required];
            assert_eq!(
                call_image_with_waveform_handle(
                    file.as_raw_handle() as isize,
                    length,
                    logical_name.as_ptr(),
                    logical_name.len(),
                    3,
                    2,
                    packet.as_mut_ptr(),
                    packet.len(),
                    &mut required,
                    None,
                ),
                QL_OK,
                "{extension}"
            );
            assert_eq!(file.stream_position().unwrap(), position, "{extension}");
            assert!(
                packet[IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 3 * 2 * 4..]
                    .iter()
                    .any(|value| *value != 0),
                "{extension} density"
            );
            drop(file);
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn image_waveform_handle_supports_svg_and_enforces_v2_boundaries() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <rect width="20" height="20" fill="#ff0000" fill-opacity="0.5"/>
            <rect x="20" width="20" height="20" fill="#00ff00"/>
        </svg>"##;
        let (path, mut file) = create_input("bin", svg);
        file.seek(SeekFrom::Start(7))
            .expect("position SVG waveform handle");
        let position = file.stream_position().unwrap();
        let logical_name = b"scope.svg";
        let length = file.metadata().unwrap().len();
        let mut required = 0usize;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let mut packet = vec![0u8; required];
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                packet.as_mut_ptr(),
                packet.len(),
                &mut required,
                None,
            ),
            QL_OK
        );
        assert_eq!(u32::from_le_bytes(packet[..4].try_into().unwrap()), 20);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 10);
        assert!(packet[IMAGE_WAVEFORM_PACKET_HEADER_BYTES + 20 * 10 * 4..]
            .iter()
            .any(|value| *value != 0));
        assert_eq!(file.stream_position().unwrap(), position);

        required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length + 1,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);

        required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                0,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(required, 0);

        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                length,
                logical_name.as_ptr(),
                logical_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(file.stream_position().unwrap(), position);

        drop(file);
        let _ = fs::remove_file(path);

        let (path, file) = create_input("bin", b"not a PNG");
        let png_name = b"bad.png";
        required = usize::MAX;
        assert_eq!(
            call_image_with_waveform_handle(
                file.as_raw_handle() as isize,
                file.metadata().unwrap().len(),
                png_name.as_ptr(),
                png_name.len(),
                20,
                20,
                std::ptr::null_mut(),
                0,
                &mut required,
                None,
            ),
            QL_ERROR_MALFORMED
        );
        assert_eq!(required, 0);
        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn image_waveform_sampling_matches_the_one_million_grid_budget() {
        assert_eq!(ImageWaveformAccumulator::new(1000, 1000).sample_step, 1);
        assert_eq!(ImageWaveformAccumulator::new(1001, 1000).sample_step, 2);
        assert_eq!(ImageWaveformAccumulator::new(2048, 2048).sample_step, 3);
    }

    #[test]
    fn archive_entry_handle_extracts_original_zip_index_without_logical_path() {
        let bytes = zip_bytes(&[(r"folder\item.txt", b"entry from pinned ZIP")]);
        let (path, mut file) = create_input("bin", &bytes);
        file.seek(SeekFrom::Start(9))
            .expect("position archive entry handle");
        let position = file.stream_position().unwrap();
        let logical_name = br"C:\missing\renamed.zip";
        let entry_path = b"folder/item.txt";
        let mut required = usize::MAX;

        let status = call_archive_entry_handle(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            entry_path.as_ptr(),
            entry_path.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > 0);
        assert_eq!(file.stream_position().unwrap(), position);

        let mut output = vec![0u8; required];
        let status = call_archive_entry_handle(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            entry_path.as_ptr(),
            entry_path.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_OK);
        let extracted = std::str::from_utf8(&output[..required])
            .expect("UTF-8 extraction path")
            .to_string();
        assert_eq!(
            fs::read(&extracted).expect("read extracted entry"),
            b"entry from pinned ZIP"
        );
        assert_eq!(file.stream_position().unwrap(), position);
        preview::discard_archive_extract_path(&extracted);

        required = usize::MAX;
        assert_eq!(
            call_archive_entry_handle(
                0,
                0,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
                Some(always_cancel),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn archive_entry_output_handle_streams_without_moving_caller_positions() {
        let expected = b"entry streamed directly into caller output";
        let bytes = zip_bytes(&[(r"folder\item.txt", expected)]);
        let (source_path, mut source) = create_input("bin", &bytes);
        source
            .seek(SeekFrom::Start(9))
            .expect("position archive source handle");
        let source_position = source.stream_position().unwrap();
        let (output_path, mut output) = create_output("bin", &[]);
        output
            .seek(SeekFrom::Start(23))
            .expect("position archive output handle");
        let output_position = output.stream_position().unwrap();
        let logical_name = br"C:\missing\renamed.zip";
        let entry_path = b"folder/item.txt";
        let mut written = u64::MAX;

        let status = call_archive_entry_output_handle(
            source.as_raw_handle() as isize,
            source.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            entry_path.as_ptr(),
            entry_path.len(),
            output.as_raw_handle() as isize,
            preview::MAX_ARCHIVE_EXTRACT_BYTES,
            &mut written,
            None,
        );
        assert_eq!(status, QL_OK);
        assert_eq!(written, expected.len() as u64);
        assert_eq!(source.stream_position().unwrap(), source_position);
        assert_eq!(output.stream_position().unwrap(), output_position);
        assert_eq!(fs::read(&output_path).unwrap(), expected);

        assert_eq!(
            call_archive_entry_output_handle(
                source.as_raw_handle() as isize,
                source.metadata().unwrap().len(),
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                output.as_raw_handle() as isize,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                std::ptr::null_mut(),
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(source.stream_position().unwrap(), source_position);
        assert_eq!(output.stream_position().unwrap(), output_position);

        drop(output);
        drop(source);
        let _ = fs::remove_file(output_path);
        let _ = fs::remove_file(source_path);
    }

    #[test]
    fn archive_entry_output_handle_rejects_invalid_outputs_limits_and_cancellation() {
        let expected = vec![0x5au8; 256 * 1024];
        let bytes = zip_bytes(&[("payload.bin", &expected)]);
        let (source_path, source) = create_input("bin", &bytes);
        let logical_name = b"payload.zip";
        let entry_path = b"payload.bin";
        let source_handle = source.as_raw_handle() as isize;
        let source_length = source.metadata().unwrap().len();
        let mut written = u64::MAX;

        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                0,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                &mut written,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(written, 0);

        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                source_handle,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                &mut written,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(written, 0);

        let thread = unsafe { windows::Win32::System::Threading::GetCurrentThread() };
        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                thread.0 as isize,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                &mut written,
                None,
            ),
            QL_ERROR_INVALID_HANDLE
        );
        assert_eq!(written, 0);

        let (nonzero_path, nonzero_output) = create_output("bin", b"x");
        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                nonzero_output.as_raw_handle() as isize,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                &mut written,
                None,
            ),
            QL_ERROR_LENGTH_MISMATCH
        );
        assert_eq!(written, 0);
        assert_eq!(nonzero_output.metadata().unwrap().len(), 1);

        let (small_path, small_output) = create_output("bin", &[]);
        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                small_output.as_raw_handle() as isize,
                1024,
                &mut written,
                None,
            ),
            QL_ERROR_LIMIT_EXCEEDED
        );
        assert_eq!(written, 0);
        assert_eq!(small_output.metadata().unwrap().len(), 0);

        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                small_output.as_raw_handle() as isize,
                preview::MAX_ARCHIVE_EXTRACT_BYTES + 1,
                &mut written,
                None,
            ),
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(written, 0);

        ARCHIVE_OUTPUT_CANCEL_POLLS.store(0, Ordering::SeqCst);
        written = u64::MAX;
        assert_eq!(
            call_archive_entry_output_handle(
                source_handle,
                source_length,
                logical_name.as_ptr(),
                logical_name.len(),
                entry_path.as_ptr(),
                entry_path.len(),
                small_output.as_raw_handle() as isize,
                preview::MAX_ARCHIVE_EXTRACT_BYTES,
                &mut written,
                Some(cancel_archive_output_after_validation),
            ),
            QL_ERROR_CANCELLED
        );
        assert_eq!(written, 0);
        assert_eq!(small_output.metadata().unwrap().len(), 0);

        drop(small_output);
        drop(nonzero_output);
        drop(source);
        let _ = fs::remove_file(small_path);
        let _ = fs::remove_file(nonzero_path);
        let _ = fs::remove_file(source_path);
    }

    #[test]
    fn ebook_handle_renders_epub_fb2_and_binary_metadata_from_reader() {
        let epub = valid_epub_bytes();
        let (epub_path, mut epub_file) = create_input("bin", &epub);
        epub_file
            .seek(SeekFrom::Start(13))
            .expect("position EPUB handle");
        let epub_position = epub_file.stream_position().unwrap();
        let epub_json =
            preview_json_with(call_ebook_handle, &epub_file, r"C:\missing\renamed.epub");
        assert_eq!(epub_json["kind"], "ebook");
        assert_eq!(epub_json["title"], "Handle Book - epub");
        assert!(epub_json["text"]
            .as_str()
            .unwrap()
            .contains("Handle Chapter"));
        assert!(epub_json["text"]
            .as_str()
            .unwrap()
            .contains("Reader content."));
        assert_eq!(epub_file.stream_position().unwrap(), epub_position);

        let fb2 = br#"<?xml version="1.0"?><FictionBook><description><title-info><book-title>Handle FB2</book-title><lang>en</lang></title-info></description><body><section><title><p>Chapter One</p></title><p>FB2 reader text.</p></section></body></FictionBook>"#;
        let (fb2_path, fb2_file) = create_input("bin", fb2);
        let fb2_json = preview_json_with(call_ebook_handle, &fb2_file, r"C:\missing\logical.fb2");
        assert_eq!(fb2_json["kind"], "ebook");
        assert_eq!(fb2_json["title"], "Handle FB2 - fb2");
        assert!(fb2_json["text"]
            .as_str()
            .unwrap()
            .contains("FB2 reader text."));

        let (binary_path, binary_file) = create_input("bin", b"binary ebook");
        let binary_json =
            preview_json_with(call_ebook_handle, &binary_file, r"C:\missing\logical.mobi");
        assert_eq!(binary_json["kind"], "ebook");
        assert_eq!(binary_json["title"], "logical.mobi - ebook");
        assert!(binary_json["text"].as_str().unwrap().contains("Size: 12"));

        drop(epub_file);
        drop(fb2_file);
        drop(binary_file);
        let _ = fs::remove_file(epub_path);
        let _ = fs::remove_file(fb2_path);
        let _ = fs::remove_file(binary_path);
    }

    #[test]
    fn ebook_handle_missing_opf_falls_back_to_same_zip_and_malformed_is_stable() {
        let fallback = zip_bytes(&[
            (
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="missing.opf"/></rootfiles></container>"#,
            ),
            ("notes.txt", b"archive fallback"),
        ]);
        let (fallback_path, fallback_file) = create_input("bin", &fallback);
        let json = preview_json_with(
            call_ebook_handle,
            &fallback_file,
            r"C:\missing\fallback.epub",
        );
        assert_eq!(json["kind"], "archive");
        assert_eq!(json["listing"]["listingKind"], "archive");
        assert_eq!(json["listing"]["rootPath"], "");
        assert_eq!(json["listing"]["canPreviewEntries"], true);
        assert!(json["listing"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "notes.txt"));

        let unusable = zip_bytes(&[
            (
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            ("content.opf", b"<package><manifest>"),
            ("fallback.txt", b"invalid OPF fallback"),
        ]);
        let (unusable_path, unusable_file) = create_input("bin", &unusable);
        let unusable_json = preview_json_with(
            call_ebook_handle,
            &unusable_file,
            r"C:\missing\unusable.epub",
        );
        assert_eq!(unusable_json["kind"], "archive");
        assert_eq!(unusable_json["listing"]["canPreviewEntries"], true);
        assert!(unusable_json["listing"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "fallback.txt"));

        let (malformed_path, malformed_file) = create_input("bin", b"not an EPUB");
        let logical_name = b"broken.epub";
        let mut required = usize::MAX;
        let status = call_ebook_handle(
            malformed_file.as_raw_handle() as isize,
            malformed_file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_MALFORMED);
        assert_eq!(required, 0);

        drop(fallback_file);
        drop(unusable_file);
        drop(malformed_file);
        let _ = fs::remove_file(fallback_path);
        let _ = fs::remove_file(unusable_path);
        let _ = fs::remove_file(malformed_path);
    }

    #[test]
    fn ebook_handle_rejects_sparse_input_over_limit() {
        let (path, read_only) = create_input("bin", &[]);
        drop(read_only);
        let writer = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open sparse ebook");
        writer
            .set_len(preview::MAX_EBOOK_HANDLE_INPUT_BYTES + 1)
            .expect("create sparse oversized ebook");
        drop(writer);
        let file = fs::File::open(&path).expect("open sparse ebook for reading");
        let logical_name = b"oversized.mobi";
        let mut required = usize::MAX;
        let status = call_ebook_handle(
            file.as_raw_handle() as isize,
            file.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_LIMIT_EXCEEDED);
        assert_eq!(required, 0);

        drop(file);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn sqlite_handle_preview_uses_optional_handles_without_moving_caller_positions() {
        let mut database = vec![0u8; 512];
        database[0..16].copy_from_slice(b"SQLite format 3\0");
        database[16..18].copy_from_slice(&512u16.to_be_bytes());
        database[18] = 2;
        database[19] = 2;
        database[21] = 64;
        database[22] = 32;
        database[23] = 32;
        database[28..32].copy_from_slice(&1u32.to_be_bytes());
        database[44..48].copy_from_slice(&4u32.to_be_bytes());
        database[56..60].copy_from_slice(&1u32.to_be_bytes());
        database[60..64].copy_from_slice(&17u32.to_be_bytes());
        database[100] = 0x0D;
        database[105..107].copy_from_slice(&512u16.to_be_bytes());

        let (main_path, mut main) = create_input("bin", &database);
        let (wal_path, mut wal) = create_input("bin", &[]);
        let (shm_path, mut shm) = create_input("bin", &[0u8; 48]);
        main.seek(SeekFrom::Start(9)).unwrap();
        wal.seek(SeekFrom::Start(0)).unwrap();
        shm.seek(SeekFrom::Start(13)).unwrap();
        let positions = (
            main.stream_position().unwrap(),
            wal.stream_position().unwrap(),
            shm.stream_position().unwrap(),
        );
        let logical_name = br"C:\missing\renamed.sqlite";
        let mut required = usize::MAX;

        let status = call_sqlite_handles(
            main.as_raw_handle() as isize,
            main.metadata().unwrap().len(),
            wal.as_raw_handle() as isize,
            0,
            shm.as_raw_handle() as isize,
            shm.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_BUFFER_TOO_SMALL);
        assert!(required > 0);
        assert_eq!(
            positions,
            (
                main.stream_position().unwrap(),
                wal.stream_position().unwrap(),
                shm.stream_position().unwrap(),
            )
        );

        let mut output = vec![0u8; required];
        let status = call_sqlite_handles(
            main.as_raw_handle() as isize,
            main.metadata().unwrap().len(),
            wal.as_raw_handle() as isize,
            0,
            shm.as_raw_handle() as isize,
            shm.metadata().unwrap().len(),
            logical_name.as_ptr(),
            logical_name.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut required,
            None,
        );
        assert_eq!(status, QL_OK);
        let json: serde_json::Value = serde_json::from_slice(&output[..required]).unwrap();
        assert_eq!(json["kind"], "database");
        assert!(json["title"]
            .as_str()
            .unwrap()
            .starts_with("renamed.sqlite"));
        let text = json["text"].as_str().unwrap();
        assert!(text.contains("User version: 17"));
        assert!(text.contains("WAL HANDLE: empty"));
        assert!(text.contains("SHM HANDLE: diagnostic only"));
        assert_eq!(
            positions,
            (
                main.stream_position().unwrap(),
                wal.stream_position().unwrap(),
                shm.stream_position().unwrap(),
            )
        );

        drop(main);
        drop(wal);
        drop(shm);
        let _ = fs::remove_file(main_path);
        let _ = fs::remove_file(wal_path);
        let _ = fs::remove_file(shm_path);
    }

    #[test]
    fn sqlite_handle_preview_validates_optional_tuples_and_limits() {
        let mut database = vec![0u8; 512];
        database[0..16].copy_from_slice(b"SQLite format 3\0");
        database[16..18].copy_from_slice(&512u16.to_be_bytes());
        let (path, main) = create_input("sqlite", &database);
        let (wal_path, wal) = create_input("wal", &[0x37]);
        let logical_name = b"bounded.sqlite";
        let mut required = usize::MAX;

        let status = call_sqlite_handles(
            main.as_raw_handle() as isize,
            main.metadata().unwrap().len(),
            0,
            1,
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
        assert_eq!(required, 0);

        let status = call_sqlite_handles(
            main.as_raw_handle() as isize,
            main.metadata().unwrap().len(),
            main.as_raw_handle() as isize,
            preview::MAX_SQLITE_WAL_BYTES + 1,
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_LIMIT_EXCEEDED);
        assert_eq!(required, 0);

        let status = call_sqlite_handles(
            main.as_raw_handle() as isize,
            main.metadata().unwrap().len(),
            wal.as_raw_handle() as isize,
            wal.metadata().unwrap().len() + 1,
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            None,
        );
        assert_eq!(status, QL_ERROR_LENGTH_MISMATCH);
        assert_eq!(required, 0);

        let status = call_sqlite_handles(
            0,
            0,
            0,
            0,
            0,
            0,
            logical_name.as_ptr(),
            logical_name.len(),
            std::ptr::null_mut(),
            0,
            &mut required,
            Some(always_cancel),
        );
        assert_eq!(status, QL_ERROR_CANCELLED);
        assert_eq!(required, 0);

        drop(main);
        drop(wal);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(wal_path);
    }

    #[test]
    fn ffi_boundary_contains_panics() {
        assert_eq!(ffi_boundary(|| panic!("test panic")), QL_ERROR_INTERNAL);
    }

    #[test]
    fn ffi_void_boundary_contains_panics() {
        ffi_void_boundary(|| panic!("test panic"));
    }
}
