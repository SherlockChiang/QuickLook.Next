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
use std::io::{BufReader, Read};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::{AnimationDecoder, ImageReader};

mod preview;

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

type Callback = unsafe extern "C" fn(*const u16);
pub type CancelCallback = extern "C" fn() -> bool;

static CALLBACK: Mutex<Option<Callback>> = Mutex::new(None);
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
static SPACE_HELD: AtomicBool = AtomicBool::new(false);
static PREVIEW_VISIBLE: AtomicBool = AtomicBool::new(false);
const WM_QL_PREVIEW: u32 = WM_APP + 1;
const WM_QL_CLOSE: u32 = WM_APP + 3;
const WM_QL_ZOOM_IN: u32 = WM_APP + 4;
const WM_QL_ZOOM_OUT: u32 = WM_APP + 5;
const WM_QL_SWITCH_DELAYED: u32 = WM_APP + 6;
const SWITCH_TIMER_ID: usize = 1;
static SWITCH_TIMER_ARMED: AtomicUsize = AtomicUsize::new(0);
static THUMBNAIL_STA: OnceLock<ThumbnailStaWorker> = OnceLock::new();

// A valid extended Windows path may contain 32,767 UTF-16 units, each requiring up to four UTF-8 bytes.
const MAX_FFI_STRING_BYTES: usize = 128 * 1024;
const MAX_FFI_MAGIC_BYTES: usize = 4096;
const MAX_NATIVE_IMAGE_DECODE_PIXELS: u64 = 48_000_000;
const MAX_ANIMATED_FRAME_DIMENSION: u32 = 1024;
const MAX_ANIMATED_FRAMES: usize = 120;
const MAX_ANIMATED_FRAME_BYTES: usize = 64 * 1024 * 1024;
const QL_THUMBNAIL_FLAG_CACHE_ONLY: u32 = 1;
const QL_THUMBNAIL_KNOWN_FLAGS: u32 = QL_THUMBNAIL_FLAG_CACHE_ONLY;

type ThumbnailResult = Option<(u32, u32, Vec<u8>)>;

struct ThumbnailRequest {
    path: String,
    size: i32,
    flags: u32,
    reply: mpsc::Sender<ThumbnailResult>,
}

struct ThumbnailStaWorker {
    sender: mpsc::Sender<ThumbnailRequest>,
}

fn utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() || len > max_len {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
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
    a + b
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
    std::thread::spawn(hook_thread);
    1
}

fn hook_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        HOOK_TID.store(GetCurrentThreadId(), Ordering::SeqCst);

        let keyboard_hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                emit(&format!("HOOK_ERR\t{e:?}"));
                return;
            }
        };
        let mouse_hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                emit(&format!("MOUSE_HOOK_ERR\t{e:?}"));
                HHOOK(std::ptr::null_mut())
            }
        };
        emit("HOOK_INSTALLED");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
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
                _ => {}
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(keyboard_hook);
        if mouse_hook.0 != std::ptr::null_mut() {
            let _ = UnhookWindowsHookEx(mouse_hook);
        }
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
            if is_down {
                let _ = PostThreadMessageW(tid, WM_QL_ZOOM_IN, WPARAM(0), LPARAM(0));
            }
        } else if matches!(kb.vkCode, VK_OEM_MINUS_U32 | VK_SUBTRACT_U32) {
            if is_down {
                let _ = PostThreadMessageW(tid, WM_QL_ZOOM_OUT, WPARAM(0), LPARAM(0));
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
#[no_mangle]
pub extern "C" fn ql_test_is_text_input_class(class_utf8: *const u8, class_len: usize) -> i32 {
    let Some(class_name) = utf8_arg(class_utf8, class_len, 256) else {
        return 0;
    };
    if is_text_input_class_name(class_name) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ql_test_is_explorer_window_class(class_utf8: *const u8, class_len: usize) -> i32 {
    let Some(class_name) = utf8_arg(class_utf8, class_len, 256) else {
        return 0;
    };
    if is_explorer_window_class_name(class_name) {
        1
    } else {
        0
    }
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
    let h = std::thread::spawn(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        do_selection_and_emit("SELECTION");
        CoUninitialize();
    });
    let _ = h.join();
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
    let preview_visible = PREVIEW_VISIBLE.load(Ordering::SeqCst);
    let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let count = shell_windows.Count()?;
    emit(&format!(
        "DBG windows={count} fg=0x{:X} visible={preview_visible}",
        foreground.0 as isize
    ));

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
            emit(&format!(
                "DBG win=0x{:X} sel={} (foreground)",
                hwnd.0 as isize,
                paths.len()
            ));
            return Ok(paths);
        }
    }
    emit("DBG foreground is not Explorer — ignoring space");
    Ok(Vec::new())
}

unsafe fn read_window_selection(wb: &IWebBrowser2) -> Result<Vec<String>> {
    let sp: IServiceProvider = wb.cast()?;
    let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser)?;
    let view: IShellView = browser.QueryActiveShellView()?;
    let folder_view: IFolderView = view.cast()?;
    let items: IShellItemArray = folder_view.Items(SVGIO_SELECTION)?;
    let n = items.GetCount()?;
    let mut out = Vec::with_capacity(n as usize);
    for k in 0..n {
        let item = items.GetItemAt(k)?;
        let pw = PwstrGuard(item.GetDisplayName(SIGDN_FILESYSPATH)?);
        out.push(
            pw.0.to_string()
                .map_err(|_| Error::from_hresult(HRESULT(0x80070057u32 as i32)))?,
        );
    }
    Ok(out)
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
#[no_mangle]
pub extern "C" fn ql_probe_file(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
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

    let json = format!(
        "{{\"path\":\"{}\",\"extension\":\"{}\",\"magicHex\":\"{}\",\"kind\":\"{}\",\"size\":{},\"modifiedUnix\":{}}}",
        json_escape(path),
        json_escape(&ext),
        magic_hex,
        kind,
        size,
        modified
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
        ".heic", ".heif", ".avif", ".jxl",
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
    if DATABASE_EXTS.contains(&ext) {
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

#[no_mangle]
pub extern "C" fn ql_decode_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_decode_image_cancelable(path_utf8, path_len, out, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_decode_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ql_decode_image_sized_cancelable(path_utf8, path_len, 0, 0, out, out_cap, cancel_cb)
}

#[no_mangle]
pub extern "C" fn ql_decode_image_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };

    let (width, height, original_width, original_height, decode_ms, resize_ms, convert_ms, bgra) =
        match decode_image_bgra(path, target_width, target_height, cancel_cb) {
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
}

#[no_mangle]
pub extern "C" fn ql_decode_gif_frames_sized(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };
    let (width, height, frames) = match decode_gif_frames_bgra(path, target_width, target_height) {
        Some(decoded) => decoded,
        None => return -2,
    };

    let frame_bytes = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    let total = 12usize.saturating_add(frames.len().saturating_mul(4usize.saturating_add(frame_bytes)));
    if total > i32::MAX as usize {
        return -2;
    }
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }

    unsafe {
        std::ptr::copy_nonoverlapping((frames.len() as u32).to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(width.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(height.to_le_bytes().as_ptr(), out.add(8), 4);
        let mut offset = 12usize;
        for (delay_ms, bgra) in frames {
            std::ptr::copy_nonoverlapping(delay_ms.to_le_bytes().as_ptr(), out.add(offset), 4);
            offset += 4;
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(offset), bgra.len());
            offset += bgra.len();
        }
    }
    total as i32
}

#[no_mangle]
pub extern "C" fn ql_decode_webp_frames_sized(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };
    let (width, height, frames) = match decode_webp_frames_bgra(path, target_width, target_height) {
        Some(decoded) => decoded,
        None => return -2,
    };
    write_animation_frames(width, height, frames, out, out_cap)
}

fn decode_image_bgra(
    path: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<CancelCallback>,
) -> Option<(u32, u32, u32, u32, u32, u32, u32, Vec<u8>)> {
    if cancel_requested(cancel_cb) {
        return None;
    }

    let (original_width, original_height) = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if should_skip_native_image_decode(original_width, original_height) {
        return None;
    }
    if cancel_requested(cancel_cb) {
        return None;
    }

    let decode_start = Instant::now();
    let mut image = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let decode_ms = elapsed_ms_u32(decode_start);
    if cancel_requested(cancel_cb) {
        return None;
    }

    let orientation = jpeg_exif_orientation(path);
    if let Some(orientation) = orientation {
        image = apply_exif_orientation(image, orientation);
    }

    let (oriented_width, oriented_height) = (image.width(), image.height());
    if oriented_width == 0 || oriented_height == 0 {
        return None;
    }

    let target_width = if target_width > 0 { target_width } else { MAX_IMAGE_RASTER_DIMENSION };
    let target_height = if target_height > 0 { target_height } else { MAX_IMAGE_RASTER_DIMENSION };
    let target_width = target_width.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let target_height = target_height.clamp(1, MAX_IMAGE_RASTER_DIMENSION);
    let scale = if oriented_width > target_width || oriented_height > target_height {
        (target_width as f64 / oriented_width as f64).min(target_height as f64 / oriented_height as f64)
    } else {
        1.0
    };
    let width = ((oriented_width as f64 * scale).round() as u32).max(1);
    let height = ((oriented_height as f64 * scale).round() as u32).max(1);
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
    if let Some(profile) = jpeg_icc_profile(path).ok()? {
        if !apply_icc_to_srgb_rgba(rgba.as_mut(), &profile) {
            return None;
        }
    }
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
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
    if cancel_requested(cancel_cb) {
        return None;
    }
    let convert_ms = elapsed_ms_u32(convert_start);

    Some((width, height, original_width, original_height, decode_ms, resize_ms, convert_ms, bgra))
}

fn decode_gif_frames_bgra(path: &str, target_width: u32, target_height: u32) -> Option<(u32, u32, Vec<(u32, Vec<u8>)>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut reader = options.read_info(BufReader::new(file)).ok()?;
    let original_width = u32::from(reader.width());
    let original_height = u32::from(reader.height());
    if should_skip_native_image_decode(original_width, original_height) {
        return None;
    }

    let target_width = if target_width > 0 { target_width } else { MAX_ANIMATED_FRAME_DIMENSION };
    let target_height = if target_height > 0 { target_height } else { MAX_ANIMATED_FRAME_DIMENSION };
    let target_width = target_width.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let target_height = target_height.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let scale = if original_width > target_width || original_height > target_height {
        (target_width as f64 / original_width as f64).min(target_height as f64 / original_height as f64)
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let frame_bytes = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    let max_frames_by_bytes = (MAX_ANIMATED_FRAME_BYTES / (frame_bytes + 4)).max(1);
    let max_frames = MAX_ANIMATED_FRAMES.min(max_frames_by_bytes);

    let mut decoded = Vec::new();
    let mut canvas = vec![0u8; (original_width as usize).checked_mul(original_height as usize)?.checked_mul(4)?];
    let mut previous_disposal = gif::DisposalMethod::Keep;
    let mut previous_rect = (0u16, 0u16, 0u16, 0u16);
    let mut previous_canvas: Option<Vec<u8>> = None;
    while decoded.len() < max_frames {
        apply_gif_disposal(&mut canvas, previous_disposal, previous_rect, previous_canvas.take(), original_width);
        let frame = match reader.read_next_frame().ok()? {
            Some(frame) => frame,
            None => break,
        };
        let delay_ms = u32::from(frame.delay).saturating_mul(10).clamp(20, 1_000);
        let saved_canvas = if frame.dispose == gif::DisposalMethod::Previous { Some(canvas.clone()) } else { None };
        composite_rgba_over_at(
            &mut canvas,
            &frame.buffer,
            original_width,
            original_height,
            u32::from(frame.left),
            u32::from(frame.top),
            u32::from(frame.width),
            u32::from(frame.height));
        let rgba = image::RgbaImage::from_raw(original_width, original_height, canvas.clone())?;
        let raster = if width == original_width && height == original_height {
            image::DynamicImage::ImageRgba8(rgba)
        } else {
            image::DynamicImage::ImageRgba8(rgba).resize_exact(width, height, image::imageops::FilterType::Triangle)
        };
        let rgba = raster.to_rgba8();
        let mut bgra = Vec::with_capacity(frame_bytes);
        for px in rgba.chunks_exact(4) {
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
    Some((width, height, decoded))
}

fn decode_webp_frames_bgra(path: &str, target_width: u32, target_height: u32) -> Option<(u32, u32, Vec<(u32, Vec<u8>)>)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(file)).ok()?;
    let frames = match decoder.into_frames().collect_frames() {
        Ok(frames) if !frames.is_empty() => frames,
        _ => {
            let (width, height, _, _, _, _, _, bgra) = decode_image_bgra(path, target_width, target_height, None)?;
            return Some((width, height, vec![(100, bgra)]));
        }
    };
    decode_animation_frames_bgra(frames, target_width, target_height)
}

fn write_animation_frames(width: u32, height: u32, frames: Vec<(u32, Vec<u8>)>, out: *mut u8, out_cap: usize) -> i32 {
    let frame_bytes = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    let total = 12usize.saturating_add(frames.len().saturating_mul(4usize.saturating_add(frame_bytes)));
    if total > i32::MAX as usize {
        return -2;
    }
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }

    unsafe {
        std::ptr::copy_nonoverlapping((frames.len() as u32).to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(width.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(height.to_le_bytes().as_ptr(), out.add(8), 4);
        let mut offset = 12usize;
        for (delay_ms, bgra) in frames {
            std::ptr::copy_nonoverlapping(delay_ms.to_le_bytes().as_ptr(), out.add(offset), 4);
            offset += 4;
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(offset), bgra.len());
            offset += bgra.len();
        }
    }
    total as i32
}

fn decode_animation_frames_bgra(frames: Vec<image::Frame>, target_width: u32, target_height: u32) -> Option<(u32, u32, Vec<(u32, Vec<u8>)>)> {
    let first = frames.first()?;
    let original_width = first.buffer().width();
    let original_height = first.buffer().height();
    if should_skip_native_image_decode(original_width, original_height) {
        return None;
    }

    let target_width = if target_width > 0 { target_width } else { MAX_ANIMATED_FRAME_DIMENSION };
    let target_height = if target_height > 0 { target_height } else { MAX_ANIMATED_FRAME_DIMENSION };
    let target_width = target_width.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let target_height = target_height.clamp(1, MAX_ANIMATED_FRAME_DIMENSION);
    let scale = if original_width > target_width || original_height > target_height {
        (target_width as f64 / original_width as f64).min(target_height as f64 / original_height as f64)
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let frame_bytes = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    let max_frames_by_bytes = (MAX_ANIMATED_FRAME_BYTES / (frame_bytes + 4)).max(1);
    let max_frames = MAX_ANIMATED_FRAMES.min(max_frames_by_bytes);

    let mut decoded = Vec::new();
    for frame in frames.into_iter().take(max_frames) {
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = if den == 0 { 100 } else { (num / den).clamp(20, 1_000) };
        let rgba = frame.into_buffer();
        let raster = if width == original_width && height == original_height {
            image::DynamicImage::ImageRgba8(rgba)
        } else {
            image::DynamicImage::ImageRgba8(rgba).resize_exact(width, height, image::imageops::FilterType::Triangle)
        };
        let rgba = raster.to_rgba8();
        let mut bgra = Vec::with_capacity(frame_bytes);
        for px in rgba.chunks_exact(4) {
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
    Some((width, height, decoded))
}

fn jpeg_icc_profile(path: &str) -> std::result::Result<Option<Vec<u8>>, ()> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" && ext != "jpe" {
        return Ok(None);
    }
    let mut file = std::fs::File::open(path).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| ())?;
    jpeg_icc_profile_from_bytes(&bytes)
}

fn jpeg_icc_profile_from_bytes(bytes: &[u8]) -> std::result::Result<Option<Vec<u8>>, ()> {
    if bytes.len() < 4 || bytes.get(0..2) != Some(&[0xFF, 0xD8]) {
        return Ok(None);
    }
    let mut pos = 2usize;
    let mut chunks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut expected_count = 0u8;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            return Ok(None);
        }
        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }
        let marker = *bytes.get(pos).ok_or(())?;
        pos += 1;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes(bytes.get(pos..pos + 2).ok_or(())?.try_into().map_err(|_| ())?) as usize;
        pos += 2;
        if len < 2 || pos + len - 2 > bytes.len() {
            return Err(());
        }
        let segment = &bytes[pos..pos + len - 2];
        if marker == 0xE2 && segment.len() > 14 && segment.starts_with(b"ICC_PROFILE\0") {
            let sequence = segment[12];
            let count = segment[13];
            if sequence == 0 || count == 0 || sequence > count || count > 16 {
                return Err(());
            }
            expected_count = expected_count.max(count);
            chunks.push((sequence, segment[14..].to_vec()));
        }
        pos += len - 2;
    }
    if expected_count == 0 || chunks.len() != expected_count as usize {
        return Ok(None);
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
    Ok(Some(profile))
}

fn apply_icc_to_srgb_rgba(rgba: &mut [u8], profile: &[u8]) -> bool {
    if rgba.is_empty() || rgba.len() % 4 != 0 || profile.len() > 4 * 1024 * 1024 {
        return false;
    }
    let Some(input) = qcms::Profile::new_from_slice(profile, false) else {
        return false;
    };
    if input.is_sRGB() {
        return true;
    }
    let output = qcms::Profile::new_sRGB();
    let Some(transform) = qcms::Transform::new_to(&input, &output, qcms::DataType::RGBA8, qcms::DataType::RGBA8, qcms::Intent::Perceptual) else {
        return false;
    };
    transform.apply(rgba);
    true
}

fn apply_gif_disposal(canvas: &mut [u8], disposal: gif::DisposalMethod, rect: (u16, u16, u16, u16), previous_canvas: Option<Vec<u8>>, canvas_width: u32) {
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

fn composite_rgba_over_at(canvas: &mut [u8], frame: &[u8], canvas_width: u32, canvas_height: u32, left: u32, top: u32, frame_width: u32, frame_height: u32) {
    let copy_width = frame_width.min(canvas_width.saturating_sub(left)) as usize;
    let copy_height = frame_height.min(canvas_height.saturating_sub(top)) as usize;
    let canvas_stride = canvas_width as usize * 4;
    let frame_stride = frame_width as usize * 4;
    for y in 0..copy_height {
        for x in 0..copy_width {
            let src = y * frame_stride + x * 4;
            let dst = (top as usize + y) * canvas_stride + (left as usize + x) * 4;
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
                let blended = (frame[src + channel] as u32 * a + canvas[dst + channel] as u32 * inv_a + 127) / 255;
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

fn jpeg_exif_orientation(path: &str) -> Option<u16> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" && ext != "jpe" {
        return None;
    }

    let bytes = std::fs::read(path).ok()?;
    let mut pos = 2usize;
    if bytes.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            return None;
        }
        let marker = bytes[pos + 1];
        pos += 2;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        if len < 2 || pos + len > bytes.len() {
            return None;
        }
        let payload = pos + 2;
        let payload_end = pos + len;
        if marker == 0xE1 && bytes.get(payload..payload + 6) == Some(b"Exif\0\0") {
            return tiff_orientation(bytes.get(payload + 6..payload_end)?);
        }
        pos = payload_end;
    }
    None
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
    Some(if endian == 0 { u16::from_le_bytes(raw) } else { u16::from_be_bytes(raw) })
}

fn read_u32(bytes: &[u8], offset: usize, endian: u8) -> Option<u32> {
    let raw = [*bytes.get(offset)?, *bytes.get(offset + 1)?, *bytes.get(offset + 2)?, *bytes.get(offset + 3)?];
    Some(if endian == 0 { u32::from_le_bytes(raw) } else { u32::from_be_bytes(raw) })
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

fn thumbnail_flags_valid(flags: u32) -> bool {
    flags & !QL_THUMBNAIL_KNOWN_FLAGS == 0
}

fn thumbnail_cache_only(flags: u32) -> bool {
    flags & QL_THUMBNAIL_FLAG_CACHE_ONLY != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    extern "C" fn always_cancel() -> bool {
        true
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
                call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                ),
                -3
            );
        }
    }

    #[test]
    fn archive_entry_export_honors_cancellation_before_file_access() {
        let archive = b"missing.zip";
        let entry = b"entry.txt";
        let mut output = [0u8; 16];

        assert_eq!(
            ql_extract_archive_entry_cancelable(
                archive.as_ptr(),
                archive.len(),
                entry.as_ptr(),
                entry.len(),
                output.as_mut_ptr(),
                output.len(),
                Some(always_cancel),
            ),
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
                call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                ),
                -3
            );
        }
    }

    #[test]
    fn thumbnail_flags_reject_unknown_bits() {
        assert!(thumbnail_flags_valid(0));
        assert!(thumbnail_flags_valid(QL_THUMBNAIL_FLAG_CACHE_ONLY));
        assert!(thumbnail_cache_only(QL_THUMBNAIL_FLAG_CACHE_ONLY));
        assert!(!thumbnail_cache_only(0));
        assert!(!thumbnail_flags_valid(QL_THUMBNAIL_FLAG_CACHE_ONLY | 2));
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
        assert_eq!(classify("app.config", ".config", b"<configuration>", false), "text");
        assert_eq!(classify("mysql.cnf", ".cnf", b"[client]\r\nport=3306\r\n", false), "text");
        assert_eq!(classify("vendor.custom", ".custom", b"feature.enabled=true\r\n", false), "text");
        assert_eq!(classify("settings", "", b"root = true\r\n", false), "text");
        assert_eq!(classify("legacy.vendor", ".vendor", b"name=caf\xE9\r\n", false), "text");
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
        assert_eq!(classify("settings.unknown", ".unknown", &utf16_be, false), "text");
        assert_eq!(classify("settings.unknown", ".unknown", &utf16_localized, false), "text");
    }

    #[test]
    fn classify_does_not_treat_binary_prefixes_as_text() {
        assert_eq!(classify("file.unknown", ".unknown", &[0, 1, 2, 3, 4], false), "binary");
        assert_eq!(classify("file.unknown", ".unknown", &[0xFF, 0xD9, 0x80], false), "binary");
        assert_eq!(classify("file.unknown", ".unknown", b"MZprintable header", false), "executable");
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
        let profile = jpeg_icc_profile_from_bytes(&jpeg).expect("parse jpeg").expect("icc profile");

        assert_eq!(profile, b"quicklook-next-test-icc");
    }

    #[test]
    fn native_jpeg_decode_accepts_adobe_transform_corpus() {
        let path = temp_image_path("jpg");
        let jpeg = jpeg_with_adobe_transform_segment();
        std::fs::write(&path, jpeg).expect("write adobe jpeg");

        let decoded = decode_image_bgra(path.to_str().unwrap(), 0, 0, None).expect("decode adobe jpeg");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.0, 2);
        assert_eq!(decoded.1, 1);
        assert_eq!(decoded.7.len(), 2 * 1 * 4);
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

        let decoded = decode_webp_frames_bgra(path.to_str().unwrap(), 0, 0).expect("decode webp frames");
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
                let decoded = decode_image_bgra(path.to_str().unwrap(), 1024, 1024, None).expect("decode external jpeg sample");
                assert_eq!(jpeg_external_golden(file), Some((decoded.0, decoded.1, decoded.7.len(), fnv1a64(&decoded.7))));
            }
        }
        for file in ["gif-disposal-background.gif", "gif-disposal-previous.gif"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                let frames = decode_gif_frames_bgra(path.to_str().unwrap(), 512, 512).expect("decode external gif sample");
                assert!(!frames.2.is_empty());
            }
        }
        for file in ["webp-animated.webp", "webp-animated-alpha.webp", "webp-animated-blend.webp"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                let frames = decode_webp_frames_bgra(path.to_str().unwrap(), 512, 512).expect("decode external webp sample");
                assert!(frames.2.len() > 1, "animated WebP sample should decode multiple frames: {file}");
                assert_eq!(webp_external_golden(file), Some((frames.0, frames.1, frames.2.len(), fnv1a64(&frames.2[0].1), fnv1a64(&frames.2.last().unwrap().1))));
            }
        }
        for file in ["avif-still.avif", "heic-still.heic", "jxl-still.jxl"] {
            let path = corpus_dir.join(file);
            if path.exists() {
                assert!(decode_image_bgra(path.to_str().unwrap(), 512, 512, None).is_none(), "modern format unexpectedly gained Rust native decode: {file}");
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
            "webp-animated-alpha.webp" => Some((483, 512, 8, 16886177616233196080, 12174948178456794470)),
            "webp-animated-blend.webp" => Some((483, 512, 8, 16886177616233196080, 12174948178456794470)),
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

        let decoded = decode_gif_frames_bgra(path.to_str().unwrap(), 1, 1).expect("decode gif frames");
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

        let decoded = decode_gif_frames_bgra(path.to_str().unwrap(), 2, 1).expect("decode gif frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.2.len(), 3);
        assert_eq!(decoded.2[2].1, vec![0, 255, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn native_gif_frame_extraction_honors_previous_disposal() {
        let path = write_disposal_gif(gif::DisposalMethod::Previous);

        let decoded = decode_gif_frames_bgra(path.to_str().unwrap(), 2, 1).expect("decode gif frames");
        let _ = std::fs::remove_file(path);

        assert_eq!(decoded.2.len(), 3);
        assert_eq!(decoded.2[2].1, vec![0, 255, 0, 255, 0, 0, 255, 255]);
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
        encoder.set_repeat(gif::Repeat::Infinite).expect("set repeat");

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
            .encode(&[255, 0, 0, 0, 0, 255], 1, 2, image::ExtendedColorType::Rgb8)
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

    fn jpeg_with_split_icc_segments() -> Vec<u8> {
        jpeg_with_icc_chunks(&[b"quicklook-next-".as_slice(), b"test-icc".as_slice()])
    }

    fn jpeg_with_icc_chunks(chunks: &[&[u8]]) -> Vec<u8> {
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder
            .encode(&[255, 0, 0, 0, 255, 0], 2, 1, image::ExtendedColorType::Rgb8)
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
            .encode(&[255, 255, 0, 0, 255, 255], 2, 1, image::ExtendedColorType::Rgb8)
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
#[no_mangle]
pub extern "C" fn ql_get_thumbnail(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_get_thumbnail_cancelable_with_flags(path_utf8, path_len, size, 0, out, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_get_thumbnail_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ql_get_thumbnail_cancelable_with_flags(
        path_utf8, path_len, size, 0, out, out_cap, cancel_cb,
    )
}

#[no_mangle]
pub extern "C" fn ql_get_thumbnail_cancelable_with_flags(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    flags: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if !thumbnail_flags_valid(flags) {
        return -1;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s.to_string(),
        None => return -1,
    };

    let result = shell_thumbnail_on_sta(path, size.max(16), flags, cancel_cb);
    if cancel_requested(cancel_cb) {
        return -3;
    }

    let (w, h, bgra) = match result {
        Some(x) => x,
        None => return -2,
    };
    let total = 8 + bgra.len();
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(h.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(8), bgra.len());
    }
    total as i32
}

fn thumbnail_sta_worker() -> &'static ThumbnailStaWorker {
    THUMBNAIL_STA.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<ThumbnailRequest>();
        std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            while let Ok(request) = receiver.recv() {
                let result = shell_thumbnail(&request.path, request.size.max(16), request.flags);
                let _ = request.reply.send(result);
            }
            CoUninitialize();
        });
        ThumbnailStaWorker { sender }
    })
}

fn shell_thumbnail_on_sta(
    path: String,
    size: i32,
    flags: u32,
    cancel_cb: Option<CancelCallback>,
) -> ThumbnailResult {
    let (reply, result) = mpsc::channel();
    let request = ThumbnailRequest {
        path,
        size,
        flags,
        reply,
    };
    if thumbnail_sta_worker().sender.send(request).is_err() {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if cancel_requested(cancel_cb) || Instant::now() >= deadline {
            return None;
        }
        match result.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => return value,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Extract the most likely app/package icon from ZIP-based packages (MSIX/AppX/APK/APKS/AAB).
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[no_mangle]
pub extern "C" fn ql_extract_package_icon(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };

    let (w, h, bgra) = match preview::extract_package_icon_bgra(path, None) {
        Some(x) => x,
        None => return -2,
    };
    let total = 8 + bgra.len();
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(h.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(8), bgra.len());
    }
    total as i32
}

#[no_mangle]
pub extern "C" fn ql_extract_package_icon_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if cancel_requested(cancel_cb) { return -3; }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };
    let (w, h, bgra) = match preview::extract_package_icon_bgra(path, cancel_cb) {
        Some(value) => value,
        None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
    };
    if cancel_requested(cancel_cb) { return -3; }
    let total = 8 + bgra.len();
    if out.is_null() || out_cap < total { return -(total as i32); }
    unsafe {
        std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(h.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(8), bgra.len());
    }
    total as i32
}

/// Extract the first useful embedded image from an OOXML Office document.
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[no_mangle]
pub extern "C" fn ql_extract_office_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };

    let (w, h, bgra) = match preview::extract_office_image_bgra(path, None) {
        Some(x) => x,
        None => return -2,
    };
    let total = 8 + bgra.len();
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(h.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(8), bgra.len());
    }
    total as i32
}

#[no_mangle]
pub extern "C" fn ql_extract_office_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if cancel_requested(cancel_cb) { return -3; }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };
    let (w, h, bgra) = match preview::extract_office_image_bgra(path, cancel_cb) {
        Some(value) => value,
        None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
    };
    if cancel_requested(cancel_cb) { return -3; }
    let total = 8 + bgra.len();
    if out.is_null() || out_cap < total { return -(total as i32); }
    unsafe {
        std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(h.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(8), bgra.len());
    }
    total as i32
}

unsafe fn shell_thumbnail(path: &str, size: i32, flags: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_INCACHEONLY,
    };
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let shell_flags = if thumbnail_cache_only(flags) {
        SIIGBF_BIGGERSIZEOK | SIIGBF_INCACHEONLY
    } else {
        SIIGBF_BIGGERSIZEOK
    };
    let hbm = factory.GetImage(SIZE { cx: size, cy: size }, shell_flags).ok()?;
    let result = hbitmap_to_bgra(hbm);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(hbm.into());
    result
}

unsafe fn hbitmap_to_bgra(
    hbm: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::*;
    let mut bm = BITMAP::default();
    let got = GetObjectW(
        hbm.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    );
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let w = bm.bmWidth as u32;
    let h = bm.bmHeight as u32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // negative → top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let hdc = GetDC(None);
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut info,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, hdc);
    if lines == 0 {
        return None;
    }

    // Shell thumbnails come back with straight alpha; the composition swapchain expects premultiplied,
    // so premultiply here to avoid light halos around transparent icon edges.
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 255 {
            px[0] = ((px[0] as u32 * a + 127) / 255) as u8; // B
            px[1] = ((px[1] as u32 * a + 127) / 255) as u8; // G
            px[2] = ((px[2] as u32 * a + 127) / 255) as u8; // R
        }
    }
    Some((w, h, pixels))
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
#[no_mangle]
pub extern "C" fn ql_preview_text(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_preview_text_cancelable(path_utf8, path_len, out_buf, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_preview_text_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
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
}

/// Render an info-only preview (size + mtime). Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_info(
    path_utf8: *const u8,
    path_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    size: i64,
    modified_unix: i64,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
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
}

/// Render an Office document preview. OOXML/ODF paths are parsed in Rust; legacy OLE formats fall back to info.
#[no_mangle]
pub extern "C" fn ql_preview_office(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_office(path, cancel_cb);
    write_json_out(&json, out_buf, out_cap)
}

/// Render bounded Rust-native image metadata. Returns JSON length, 0 on failure/no metadata.
#[no_mangle]
pub extern "C" fn ql_preview_image_metadata(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_image_metadata(path);
    write_json_out(&json, out_buf, out_cap)
}

/// Render a PE executable metadata preview. Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_executable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_preview_executable_cancelable(path_utf8, path_len, out_buf, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_preview_executable_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
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
}

/// Render an archive listing. Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_archive(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_archive(path, cancel_cb);
    write_json_out(&json, out_buf, out_cap)
}

/// Extract a previewable archive entry into a bounded temp cache. Returns UTF-8 path length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_extract_archive_entry(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    if archive_path_utf8.is_null() || entry_path_utf8.is_null() || out_buf.is_null() || out_cap == 0
    {
        return 0;
    }
    let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let Some(path) = preview::extract_archive_entry_to_temp(archive_path, entry_path, None) else {
        return 0;
    };
    write_json_out(&path, out_buf, out_cap)
}

#[no_mangle]
pub extern "C" fn ql_extract_archive_entry_cancelable(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if archive_path_utf8.is_null() || entry_path_utf8.is_null() || out_buf.is_null() || out_cap == 0
    {
        return 0;
    }
    if cancel_requested(cancel_cb) {
        return -3;
    }
    let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let Some(path) = preview::extract_archive_entry_to_temp(archive_path, entry_path, cancel_cb) else {
        return if cancel_requested(cancel_cb) { -3 } else { 0 };
    };
    if cancel_requested(cancel_cb) {
        return -3;
    }
    write_json_out(&path, out_buf, out_cap)
}

/// Render an ebook preview. Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_ebook(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_preview_ebook_cancelable(path_utf8, path_len, out_buf, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_preview_ebook_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
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
    let json = preview::render_ebook(path);
    if cancel_requested(cancel_cb) {
        return -3;
    }
    write_json_out(&json, out_buf, out_cap)
}

/// Render a torrent metadata preview. Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_torrent(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ql_preview_torrent_cancelable(path_utf8, path_len, out_buf, out_cap, None)
}

#[no_mangle]
pub extern "C" fn ql_preview_torrent_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
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
}

/// Render a folder listing. Returns JSON length, 0 on failure.
#[no_mangle]
pub extern "C" fn ql_preview_folder(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_folder(path, cancel_cb);
    write_json_out(&json, out_buf, out_cap)
}

/// Check if a file is text-like (for routing in the App).
#[no_mangle]
pub extern "C" fn ql_is_text(
    ext_utf8: *const u8,
    ext_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
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
}

/// Check if a file is an archive (for routing).
#[no_mangle]
pub extern "C" fn ql_is_archive(
    ext_utf8: *const u8,
    ext_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
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
