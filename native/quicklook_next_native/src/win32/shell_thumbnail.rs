use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
    SIIGBF_INCACHEONLY,
};

use crate::CancelCallback;

const FLAG_CACHE_ONLY: u32 = 1;
const FLAG_BOUNDED_SIZE: u32 = 2;
const KNOWN_FLAGS: u32 = FLAG_CACHE_ONLY | FLAG_BOUNDED_SIZE;
const MIN_EDGE: i32 = 16;
const MAX_EDGE: i32 = 512;
const MAX_BITMAP_BYTES: usize = 512 * 512 * 4;

pub(crate) type Thumbnail = (u32, u32, Vec<u8>);
type ThumbnailResult = Option<Thumbnail>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ThumbnailError {
    InvalidFlags,
    LimitExceeded,
    Cancelled,
    Unavailable,
}

struct ThumbnailRequest {
    path: String,
    size: i32,
    flags: u32,
    reply: mpsc::Sender<ThumbnailResult>,
}

struct ThumbnailStaWorker {
    sender: mpsc::Sender<ThumbnailRequest>,
}

static THUMBNAIL_STA: OnceLock<ThumbnailStaWorker> = OnceLock::new();

pub(crate) fn request(
    path: String,
    size: i32,
    flags: u32,
    cancel_cb: Option<CancelCallback>,
) -> Result<Thumbnail, ThumbnailError> {
    if !flags_valid(flags) {
        return Err(ThumbnailError::InvalidFlags);
    }
    let Some(size) = checked_request_size(size) else {
        return Err(ThumbnailError::LimitExceeded);
    };

    let result = shell_thumbnail_on_sta(path, size, flags, cancel_cb);
    if cancel_requested(cancel_cb) {
        return Err(ThumbnailError::Cancelled);
    }
    result.ok_or(ThumbnailError::Unavailable)
}

fn flags_valid(flags: u32) -> bool {
    flags & !KNOWN_FLAGS == 0
}

fn cache_only(flags: u32) -> bool {
    flags & FLAG_CACHE_ONLY != 0
}

fn bounded_size(flags: u32) -> bool {
    flags & FLAG_BOUNDED_SIZE != 0
}

fn checked_request_size(size: i32) -> Option<i32> {
    let size = size.max(MIN_EDGE);
    (size <= MAX_EDGE).then_some(size)
}

fn checked_bitmap_layout(width: i32, height: i32) -> Option<(u32, u32, usize)> {
    let width = u32::try_from(width).ok()?;
    let height = u32::try_from(height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }

    let byte_len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if width > MAX_EDGE as u32 || height > MAX_EDGE as u32 || byte_len > MAX_BITMAP_BYTES {
        return None;
    }

    Some((width, height, byte_len))
}

fn cancel_requested(cancel_cb: Option<CancelCallback>) -> bool {
    cancel_cb.is_some_and(|callback| callback())
}

fn thumbnail_sta_worker() -> &'static ThumbnailStaWorker {
    THUMBNAIL_STA.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<ThumbnailRequest>();
        std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            while let Ok(request) = receiver.recv() {
                let result = shell_thumbnail(&request.path, request.size, request.flags);
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

struct OwnedShellBitmap(HBITMAP);

impl OwnedShellBitmap {
    fn new(handle: HBITMAP) -> Option<Self> {
        (!handle.0.is_null()).then_some(Self(handle))
    }
}

impl Drop for OwnedShellBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

struct ScreenDc(HDC);

impl ScreenDc {
    unsafe fn acquire() -> Option<Self> {
        let handle = unsafe { GetDC(None) };
        (!handle.0.is_null()).then_some(Self(handle))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseDC(None, self.0);
        }
    }
}

unsafe fn shell_thumbnail(path: &str, size: i32, flags: u32) -> ThumbnailResult {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let shell_flags = match (cache_only(flags), bounded_size(flags)) {
        (true, true) => SIIGBF_INCACHEONLY,
        (true, false) => SIIGBF_BIGGERSIZEOK | SIIGBF_INCACHEONLY,
        (false, true) => Default::default(),
        (false, false) => SIIGBF_BIGGERSIZEOK,
    };
    let bitmap = OwnedShellBitmap::new(
        unsafe { factory.GetImage(SIZE { cx: size, cy: size }, shell_flags) }.ok()?,
    )?;
    unsafe { hbitmap_to_bgra(bitmap.0) }
}

unsafe fn hbitmap_to_bgra(bitmap: HBITMAP) -> ThumbnailResult {
    let mut metadata = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut metadata as *mut _ as *mut _),
        )
    };
    if got == 0 {
        return None;
    }
    let (width, height, byte_len) = checked_bitmap_layout(metadata.bmWidth, metadata.bmHeight)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(byte_len).ok()?;
    pixels.resize(byte_len, 0);

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let device_context = unsafe { ScreenDc::acquire() }?;
    let lines = unsafe {
        GetDIBits(
            device_context.0,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if lines != height as i32 {
        return None;
    }

    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha != 255 {
            pixel[0] = ((pixel[0] as u32 * alpha + 127) / 255) as u8;
            pixel[1] = ((pixel[1] as u32 * alpha + 127) / 255) as u8;
            pixel[2] = ((pixel[2] as u32 * alpha + 127) / 255) as u8;
        }
    }
    Some((width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_reject_unknown_bits() {
        assert!(flags_valid(0));
        assert!(flags_valid(FLAG_CACHE_ONLY));
        assert!(flags_valid(FLAG_BOUNDED_SIZE));
        assert!(cache_only(FLAG_CACHE_ONLY));
        assert!(bounded_size(FLAG_BOUNDED_SIZE));
        assert!(!cache_only(0));
        assert!(!flags_valid(KNOWN_FLAGS | 4));
    }

    #[test]
    fn request_size_is_bounded_before_shell_dispatch() {
        assert_eq!(checked_request_size(i32::MIN), Some(16));
        assert_eq!(checked_request_size(1), Some(16));
        assert_eq!(checked_request_size(512), Some(512));
        assert_eq!(checked_request_size(513), None);
        assert_eq!(checked_request_size(i32::MAX), None);
    }

    #[test]
    fn bitmap_layout_rejects_invalid_and_hostile_dimensions() {
        assert_eq!(checked_bitmap_layout(1, 1), Some((1, 1, 4)));
        assert_eq!(
            checked_bitmap_layout(512, 512),
            Some((512, 512, 512 * 512 * 4))
        );
        assert_eq!(checked_bitmap_layout(0, 1), None);
        assert_eq!(checked_bitmap_layout(-1, 1), None);
        assert_eq!(checked_bitmap_layout(513, 1), None);
        assert_eq!(checked_bitmap_layout(1, 513), None);
        assert_eq!(checked_bitmap_layout(65_536, 16_384), None);
        assert_eq!(checked_bitmap_layout(i32::MAX, i32::MAX), None);
    }
}
