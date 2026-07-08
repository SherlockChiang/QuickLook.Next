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
use std::io::Read;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use image::ImageReader;

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

const MAX_FFI_STRING_BYTES: usize = 32 * 1024;
const MAX_FFI_MAGIC_BYTES: usize = 4096;
const MAX_NATIVE_IMAGE_DECODE_PIXELS: u64 = 48_000_000;

type ThumbnailResult = Option<(u32, u32, Vec<u8>)>;

struct ThumbnailRequest {
    path: String,
    size: i32,
    cancel_cb: Option<CancelCallback>,
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
// a coarse kind, and metadata — cached by path+mtime so repeated previews don't re-stat/re-read.

static PROBE_CACHE: OnceLock<Mutex<HashMap<String, (i64, String)>>> = OnceLock::new();
const PROBE_CACHE_MAX: usize = 500;

fn probe_cache() -> &'static Mutex<HashMap<String, (i64, String)>> {
    PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Evict oldest entries when the cache exceeds PROBE_CACHE_MAX. Called after insertion.
fn probe_cache_evict(cache: &mut HashMap<String, (i64, String)>) {
    if cache.len() <= PROBE_CACHE_MAX {
        return;
    }
    // Evict the entry with the smallest mtime (oldest probed file).
    let oldest_key = cache
        .iter()
        .min_by_key(|(_, (mt, _))| *mt)
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
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if let Ok(cache) = probe_cache().lock() {
        if let Some((m, json)) = cache.get(path) {
            if *m == modified {
                return Some(json.clone());
            }
        }
    }

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
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
        classify(&ext, magic)
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
            cache.insert(path.to_string(), (modified, json.clone()));
            probe_cache_evict(&mut cache);
        }
    }
    Some(json)
}

/// Coarse type classification. Container formats are recognized by extension first (e.g. .docx is a
/// ZIP by magic but should be "office"), then images/pdf/archives by magic, then text.
fn classify(ext: &str, magic: &[u8]) -> &'static str {
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

    // Text by extension only. Unknown binary formats often have ASCII-looking headers
    // (.torrent, disk images, package metadata), so avoid decoding arbitrary bytes as text.
    const TEXT_EXTS: &[&str] = &[
        ".txt",
        ".md",
        ".markdown",
        ".log",
        ".csv",
        ".tsv",
        ".env",
        ".json",
        ".xml",
        ".xaml",
        ".xsd",
        ".resx",
        ".config",
        ".ini",
        ".cfg",
        ".conf",
        ".properties",
        ".yml",
        ".yaml",
        ".toml",
        ".bat",
        ".cmd",
        ".ps1",
        ".sh",
        ".bash",
        ".zsh",
        ".cs",
        ".csproj",
        ".sln",
        ".props",
        ".targets",
        ".rs",
        ".js",
        ".jsx",
        ".mjs",
        ".cjs",
        ".ts",
        ".tsx",
        ".css",
        ".scss",
        ".sass",
        ".less",
        ".html",
        ".htm",
        ".py",
        ".c",
        ".h",
        ".cc",
        ".cpp",
        ".cxx",
        ".hpp",
        ".hxx",
        ".java",
        ".go",
        ".php",
        ".rb",
        ".pl",
        ".swift",
        ".kt",
        ".kts",
        ".sql",
        ".lua",
        ".fs",
        ".fsx",
        ".vb",
        ".dart",
        ".scala",
        ".r",
        ".dockerfile",
    ];
    if TEXT_EXTS.contains(&ext) {
        return "text";
    }
    "binary"
}

// ── Native image decode ──────────────────────────────────────────────────────────────────────
// Decode common image formats in Rust and return a constrained BGRA raster for the .NET raster host.
// Output layout: [w:u32 LE][h:u32 LE][orig_w:u32 LE][orig_h:u32 LE][premultiplied BGRA bytes].

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
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return -1,
    };

    let (width, height, original_width, original_height, bgra) =
        match decode_image_bgra(path, cancel_cb) {
            Some(decoded) => decoded,
            None => return -2,
        };
    if cancel_requested(cancel_cb) {
        return -3;
    }

    let total = 16 + bgra.len();
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }

    unsafe {
        std::ptr::copy_nonoverlapping(width.to_le_bytes().as_ptr(), out, 4);
        std::ptr::copy_nonoverlapping(height.to_le_bytes().as_ptr(), out.add(4), 4);
        std::ptr::copy_nonoverlapping(original_width.to_le_bytes().as_ptr(), out.add(8), 4);
        std::ptr::copy_nonoverlapping(original_height.to_le_bytes().as_ptr(), out.add(12), 4);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(16), bgra.len());
    }
    total as i32
}

fn decode_image_bgra(
    path: &str,
    cancel_cb: Option<CancelCallback>,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
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

    let image = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    if cancel_requested(cancel_cb) {
        return None;
    }

    if original_width == 0 || original_height == 0 {
        return None;
    }

    let largest = original_width.max(original_height);
    let scale = if largest > MAX_IMAGE_RASTER_DIMENSION {
        MAX_IMAGE_RASTER_DIMENSION as f64 / largest as f64
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    if cancel_requested(cancel_cb) {
        return None;
    }

    let raster = if width == original_width && height == original_height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };
    if cancel_requested(cancel_cb) {
        return None;
    }

    let rgba = raster.to_rgba8();
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

    Some((width, height, original_width, original_height, bgra))
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

    #[test]
    fn native_image_decode_skips_extreme_pixel_counts() {
        assert!(!should_skip_native_image_decode(8_000, 6_000));
        assert!(should_skip_native_image_decode(8_001, 6_000));
        assert!(should_skip_native_image_decode(0, 6_000));
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
    ql_get_thumbnail_cancelable(path_utf8, path_len, size, out, out_cap, None)
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
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s.to_string(),
        None => return -1,
    };

    let result = shell_thumbnail_on_sta(path, size.max(16), cancel_cb);
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
                if cancel_requested(request.cancel_cb) {
                    let _ = request.reply.send(None);
                    continue;
                }
                let result = shell_thumbnail(&request.path, request.size.max(16));
                if cancel_requested(request.cancel_cb) {
                    let _ = request.reply.send(None);
                    continue;
                }
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
    cancel_cb: Option<CancelCallback>,
) -> ThumbnailResult {
    let (reply, result) = mpsc::channel();
    let request = ThumbnailRequest {
        path,
        size,
        cancel_cb,
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

    let (w, h, bgra) = match preview::extract_package_icon_bgra(path) {
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

    let (w, h, bgra) = match preview::extract_office_image_bgra(path) {
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

unsafe fn shell_thumbnail(path: &str, size: i32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
    };
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let hbm = factory
        .GetImage(SIZE { cx: size, cy: size }, SIIGBF_BIGGERSIZEOK)
        .ok()?;
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
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_text(path);
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
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_executable(path);
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
    let Some(path) = preview::extract_archive_entry_to_temp(archive_path, entry_path) else {
        return 0;
    };
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
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_ebook(path);
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
    if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
        Some(s) => s,
        None => return 0,
    };
    let json = preview::render_torrent(path);
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
