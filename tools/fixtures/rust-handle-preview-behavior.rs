// Compiled with real common/native_input/HANDLE adapters and the locked Windows rlib.
// Renderer substitutes observe dispatch and fail paths; they do not validate format parsing.
use std::cell::{Cell, RefCell};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::UNIX_EPOCH;

use ffi::handle_preview::{self, reopen_handle_input_v2};
use preview::ReaderPreviewError;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

const DATA: &[u8] = b"pinned bytes";
const HINT: &str = "C:\\not-authority\\renamed-é.txt";
const BASENAME: &str = "renamed-é.txt";

#[derive(Clone, Copy)]
enum Mode {
    Success,
    Empty,
    Error(ReaderPreviewError),
    Panic,
}

#[derive(Debug)]
struct Call {
    renderer: &'static str,
    name: String,
    length: u64,
    modified: Option<i64>,
    position: u64,
    callback: Option<CancelCallback>,
}

thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Success) };
    static REOPENS: Cell<usize> = const { Cell::new(0) };
    static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
    static SEQUENCE: Cell<usize> = const { Cell::new(0) };
    static CANCEL_POLLS: Cell<usize> = const { Cell::new(0) };
}

fn reset(mode: Mode) {
    MODE.set(mode);
    REOPENS.set(0);
    CALLS.with(|calls| calls.borrow_mut().clear());
    DATABASE_PATH_CALLS.with(|calls| calls.borrow_mut().clear());
    DATABASE_CALLS.with(|calls| calls.borrow_mut().clear());
    CANCEL_POLLS.set(0);
}

fn calls() -> usize {
    CALLS.with(|calls| calls.borrow().len())
}

fn handles() -> u32 {
    let mut count = 0;
    // SAFETY: The process pseudo-handle is valid and the output scalar is live/exclusive.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }
        .expect("process handle count");
    count
}

pub fn recorded_reopen(
    raw: isize,
    length: u64,
) -> Result<File, actual_native_input::NativeInputError> {
    REOPENS.set(REOPENS.get() + 1);
    actual_native_input::reopen_borrowed_disk_file(raw, length)
}

fn observe(
    file: &mut File,
    renderer: &'static str,
    name: &str,
    length: u64,
    modified: Option<i64>,
    cb: Option<CancelCallback>,
) {
    let position = file.stream_position().expect("provider position");
    CALLS.with(|calls| {
        calls.borrow_mut().push(Call {
            renderer,
            name: name.to_owned(),
            length,
            modified,
            position,
            callback: cb,
        })
    });
}

fn response(file: &mut File, renderer: &str) -> Result<String, ReaderPreviewError> {
    match MODE.get() {
        Mode::Panic => panic!("provider panic"),
        Mode::Empty => Ok(String::new()),
        Mode::Error(error) => Err(error),
        Mode::Success => {
            let mut bytes = Vec::new();
            file.take(16 * 1024)
                .read_to_end(&mut bytes)
                .expect("bounded fixture read");
            assert_eq!(bytes, DATA);
            Ok(format!(
                "{{\"renderer\":\"{renderer}\",\"bytes\":\"pinned bytes\"}}"
            ))
        }
    }
}

pub(crate) fn pdf_reader(
    mut file: File,
    length: u64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    observe(&mut file, "pdf", "", length, None, cb);
    response(&mut file, "pdf")
}

pub(crate) fn image_reader(
    file: &mut File,
    name: &str,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    let length = file.metadata().expect("image length").len();
    observe(file, "image", name, length, None, cb);
    response(file, "image")
}

fn container_reader(
    mut file: File,
    renderer: &'static str,
    name: &str,
    length: u64,
    modified: Option<i64>,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    observe(&mut file, renderer, name, length, modified, cb);
    response(&mut file, renderer)
}

pub(crate) fn archive_reader(
    file: File,
    name: &str,
    length: u64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    container_reader(file, "archive", name, length, Some(modified), cb)
}

pub(crate) fn package_reader(
    file: File,
    name: &str,
    length: u64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    container_reader(file, "package", name, length, None, cb)
}

pub(crate) fn office_reader(
    file: File,
    name: &str,
    length: u64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    container_reader(file, "office", name, length, Some(modified), cb)
}

pub(crate) fn ebook_reader(
    file: File,
    name: &str,
    length: u64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    container_reader(file, "ebook", name, length, Some(modified), cb)
}

pub(crate) fn text_reader(file: &mut File, name: &str, cb: Option<CancelCallback>) -> String {
    let length = file.metadata().expect("text length").len();
    observe(file, "text", name, length, None, cb);
    response(file, "text").expect("text provider has no typed error result")
}

pub(crate) fn executable_reader(
    file: &mut File,
    name: &str,
    length: i64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    observe(file, "executable", name, length as u64, Some(modified), cb);
    response(file, "executable")
}

pub(crate) fn torrent_reader(
    file: &mut File,
    name: &str,
    length: i64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    observe(file, "torrent", name, length as u64, Some(modified), cb);
    response(file, "torrent")
}

pub(crate) fn mail_reader(
    file: File,
    name: &str,
    length: u64,
    modified: i64,
    cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    container_reader(file, "mail", name, length, Some(modified), cb)
}

fn expected_status(error: ReaderPreviewError) -> i32 {
    match error {
        ReaderPreviewError::Cancelled => QL_ERROR_CANCELLED,
        ReaderPreviewError::Io => QL_ERROR_IO,
        ReaderPreviewError::Malformed => QL_ERROR_MALFORMED,
        ReaderPreviewError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        ReaderPreviewError::LimitExceeded => QL_ERROR_LIMIT_EXCEEDED,
    }
}

pub fn probe_reader(
    file: &mut File,
    name: &str,
    length: u64,
    modified: i64,
) -> Result<String, i32> {
    observe(file, "probe", name, length, Some(modified), None);
    if matches!(MODE.get(), Mode::Success) {
        preview::actual_file_probe::probe_reader_json(file, name, length, modified)
    } else {
        response(file, "probe").map_err(expected_status)
    }
}

struct Source {
    path: PathBuf,
    file: File,
    length: u64,
}

impl Source {
    fn new(share: u32) -> Self {
        Self::with_bytes(share, DATA)
    }

    fn with_bytes(share: u32, bytes: &[u8]) -> Self {
        SEQUENCE.set(SEQUENCE.get() + 1);
        let path = PathBuf::from(std::env::args().nth(1).expect("owned fixture directory")).join(
            format!("input-{}-{}.bin", std::process::id(), SEQUENCE.get()),
        );
        fs::write(&path, bytes).expect("write fixture");
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(share)
            .open(&path)
            .expect("open source");
        file.seek(SeekFrom::Start(3)).expect("position source");
        Self {
            path,
            file,
            length: bytes.len() as u64,
        }
    }

    fn resize(&mut self, length: u64) {
        OpenOptions::new()
            .write(true)
            .share_mode(7)
            .open(&self.path)
            .expect("open bounded fixture for resize")
            .set_len(length)
            .expect("resize fixture");
        self.length = length;
    }

    fn input(&self, name: &[u8]) -> Input {
        Input {
            raw: self.file.as_raw_handle() as isize,
            length: self.length,
            name: name.as_ptr(),
            name_len: name.len(),
        }
    }

    fn unchanged(&mut self) {
        assert_eq!(self.file.stream_position().expect("source still owned"), 3);
        assert_eq!(
            self.file.metadata().expect("source still open").len(),
            self.length
        );
    }
}

#[derive(Clone, Copy)]
struct Input {
    raw: isize,
    length: u64,
    name: *const u8,
    name_len: usize,
}

#[derive(Clone, Copy)]
enum Export {
    Probe,
    Pdf,
    Image,
}

impl Export {
    /// # Safety
    /// The borrowed HANDLE and non-null input/output ranges must remain valid/disjoint.
    /// Null/oversized probes must use paths that reject them before access.
    unsafe fn call(
        self,
        input: Input,
        out: *mut u8,
        cap: usize,
        required: *mut usize,
        cb: Option<CancelCallback>,
    ) -> i32 {
        // SAFETY: The probe caller establishes the HANDLE/buffer contracts; forwarding preserves lengths.
        unsafe {
            match self {
                Self::Probe => handle_preview::ql_probe_file_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                ),
                Self::Pdf => handle_preview::ql_extract_pdf_text_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Image => handle_preview::ql_preview_image_metadata_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
            }
        }
    }

    fn renderer(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::Pdf => "pdf",
            Self::Image => "image",
        }
    }
}

#[derive(Clone, Copy)]
enum FamilyExport {
    Archive,
    Package,
    Office,
    Ebook,
    Text,
    Executable,
    Torrent,
    Mail,
}

impl FamilyExport {
    /// # Safety
    /// The borrowed HANDLE and non-null input/output ranges must remain valid/disjoint.
    unsafe fn call(
        self,
        input: Input,
        out: *mut u8,
        cap: usize,
        required: *mut usize,
        cb: Option<CancelCallback>,
    ) -> i32 {
        // SAFETY: Each export shares the documented ABI 2 borrowed-HANDLE contract.
        unsafe {
            match self {
                Self::Archive => ffi::handle_container::ql_preview_archive_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Package => ffi::handle_container::ql_preview_package_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Office => ffi::handle_container::ql_preview_office_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Ebook => ffi::handle_container::ql_preview_ebook_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Text => ffi::handle_structured::ql_preview_text_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Executable => ffi::handle_structured::ql_preview_executable_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Torrent => ffi::handle_structured::ql_preview_torrent_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Mail => ffi::handle_structured::ql_preview_mail_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
            }
        }
    }

    fn renderer(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Package => "package",
            Self::Office => "office",
            Self::Ebook => "ebook",
            Self::Text => "text",
            Self::Executable => "executable",
            Self::Torrent => "torrent",
            Self::Mail => "mail",
        }
    }
}

const CONTAINER_EXPORTS: [FamilyExport; 4] = [
    FamilyExport::Archive,
    FamilyExport::Package,
    FamilyExport::Office,
    FamilyExport::Ebook,
];

const STRUCTURED_EXPORTS: [FamilyExport; 4] = [
    FamilyExport::Text,
    FamilyExport::Executable,
    FamilyExport::Torrent,
    FamilyExport::Mail,
];

const EXPORTS: [Export; 3] = [Export::Probe, Export::Pdf, Export::Image];
extern "C" fn never_cancel() -> bool {
    false
}
extern "C" fn pre_cancel() -> bool {
    true
}
extern "C" fn after_reopen() -> bool {
    REOPENS.get() > 0
}
extern "C" fn after_reader() -> bool {
    calls() > 0
}
extern "C" fn cancel_on_second_poll() -> bool {
    let poll = CANCEL_POLLS.get() + 1;
    CANCEL_POLLS.set(poll);
    poll == 2
}

extern "C" fn cancel_on_fourth_poll() -> bool {
    let poll = CANCEL_POLLS.get() + 1;
    CANCEL_POLLS.set(poll);
    poll == 4
}

fn native_ownership() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    reset(Mode::Success);
    let baseline = handles();
    // SAFETY: Source and UTF-8 hint remain live; each successful reopen returns a separately owned File.
    let (mut first, name, length, modified) = unsafe {
        reopen_handle_input_v2(input.raw, input.length, input.name, input.name_len, None)
    }
    .expect("first reopen");
    assert_eq!(name, BASENAME);
    assert_eq!(length, DATA.len() as i64);
    let expected_modified = source
        .file
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(modified, expected_modified);
    // SAFETY: Same live borrowed input; a second owned reopen must be independently positioned.
    let (mut second, _, _, _) = unsafe {
        reopen_handle_input_v2(input.raw, input.length, input.name, input.name_len, None)
    }
    .expect("second reopen");
    assert_ne!(first.as_raw_handle(), source.file.as_raw_handle());
    assert_ne!(second.as_raw_handle(), first.as_raw_handle());
    assert_eq!(first.stream_position().unwrap(), 0);
    assert_eq!(second.stream_position().unwrap(), 0);
    let mut prefix = [0; 4];
    first.read_exact(&mut prefix).unwrap();
    assert_eq!(&prefix, &DATA[..4]);
    assert_eq!(second.stream_position().unwrap(), 0);
    source.unchanged();
    drop(first);
    drop(second);
    assert_eq!(handles(), baseline);

    // Changing the filesystem entry cannot replace the exact borrowed object.
    let pinned = source.path.with_extension("retained");
    fs::rename(&source.path, &pinned).expect("rename with delete-sharing");
    fs::write(&source.path, b"decoy bytes!").expect("replace path with decoy");
    // SAFETY: The original source remains open after rename; hint is not path authority.
    let (mut reopened, _, _, _) = unsafe {
        reopen_handle_input_v2(input.raw, input.length, input.name, input.name_len, None)
    }
    .expect("reopen pinned object");
    let mut bytes = Vec::new();
    reopened.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, DATA);
    assert_ne!(fs::read(&source.path).unwrap(), DATA);
    drop(reopened);
    source.unchanged();
    assert_eq!(handles(), baseline);
}

fn handle_outputs() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    for export in EXPORTS {
        reset(Mode::Success);
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Live borrowed source/hint, exclusive scalar; null/zero is a size probe.
            unsafe { export.call(input, null_mut(), 0, &mut required, Some(never_cancel)) },
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert!(required > 0 && required < 2048);
        assert_eq!(REOPENS.get(), 1);
        let expected_size = required;
        for capacity in [expected_size - 1, expected_size, 2048] {
            reset(Mode::Success);
            required = usize::MAX;
            let mut out = [0xa5; 2064];
            // SAFETY: Declared output is exclusive/disjoint and fits its initialized allocation.
            let status = unsafe {
                export.call(
                    input,
                    out[4..].as_mut_ptr(),
                    capacity,
                    &mut required,
                    Some(never_cancel),
                )
            };
            assert_eq!(required, expected_size);
            assert_eq!(
                status,
                if capacity < expected_size {
                    QL_ERROR_BUFFER_TOO_SMALL
                } else {
                    QL_OK
                }
            );
            assert_eq!(REOPENS.get(), 1);
            assert_eq!(calls(), 1);
            CALLS.with(|calls| {
                let call = &calls.borrow()[0];
                assert_eq!(call.renderer, export.renderer());
                assert_eq!(call.position, 0);
                assert_eq!(call.length, DATA.len() as u64);
                assert_eq!(
                    call.name,
                    if matches!(export, Export::Pdf) {
                        ""
                    } else {
                        BASENAME
                    }
                );
                assert_eq!(call.callback.is_some(), !matches!(export, Export::Probe));
            });
            if capacity < expected_size {
                assert_eq!(out, [0xa5; 2064]);
            } else {
                let payload = std::str::from_utf8(&out[4..4 + required]).expect("UTF-8 payload");
                if matches!(export, Export::Probe) {
                    assert!(payload.contains("\"path\":\"renamed-é.txt\""));
                    assert!(payload.contains("\"kind\":\"text\""));
                    assert!(payload.contains("\"magicHex\":\"70696E6E6564206279746573\""));
                } else {
                    assert_eq!(
                        payload,
                        format!(
                            "{{\"renderer\":\"{}\",\"bytes\":\"pinned bytes\"}}",
                            export.renderer()
                        )
                    );
                }
                assert!(out[..4]
                    .iter()
                    .chain(out[4 + required..].iter())
                    .all(|b| *b == 0xa5));
            }
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
    }
}

fn handle_arguments() {
    let mut source = Source::new(7);
    let valid = source.input(HINT.as_bytes());
    let invalid = [0xff];
    let oversized = vec![b'a'; MAX_LOGICAL_NAME_BYTES + 1];
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    for export in EXPORTS {
        reset(Mode::Success);
        let mut out = [0xa5; 64];
        assert_eq!(
            // SAFETY: Null required must return before input, callback, output or HANDLE access.
            unsafe {
                export.call(
                    valid,
                    out.as_mut_ptr(),
                    out.len(),
                    null_mut(),
                    Some(pre_cancel),
                )
            },
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(REOPENS.get(), 0);
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Exclusive scalar; null output with nonzero capacity rejects before I/O.
            unsafe { export.call(valid, null_mut(), 1, &mut required, Some(pre_cancel)) },
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);
        assert_eq!(REOPENS.get(), 0);
        for (name, name_len) in [
            (null(), 0),
            (null(), 1),
            (invalid.as_ptr(), 1),
            (oversized.as_ptr(), oversized.len()),
            (dangling, MAX_LOGICAL_NAME_BYTES + 1),
            (b"".as_ptr(), 0),
            (b".".as_ptr(), 1),
            (b"..".as_ptr(), 2),
            (b"C:\\".as_ptr(), 3),
        ] {
            reset(Mode::Success);
            required = usize::MAX;
            let input = Input {
                name,
                name_len,
                ..valid
            };
            assert_eq!(
                // SAFETY: Live non-null ranges are readable; oversized dangling/null probes must reject before access.
                unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
                QL_ERROR_INVALID_ARGUMENT
            );
            assert_eq!(required, 0);
            assert_eq!(REOPENS.get(), 0);
            assert_eq!(calls(), 0);
            assert_eq!(out, [0xa5; 64]);
        }
        reset(Mode::Success);
        let input = Input {
            name: oversized.as_ptr(),
            name_len: MAX_LOGICAL_NAME_BYTES,
            ..valid
        };
        assert_eq!(
            // SAFETY: The exact-limit name is fully readable and only a hint, never a filesystem path to open.
            unsafe { export.call(input, null_mut(), 0, &mut required, None) },
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(REOPENS.get(), 1);
        source.unchanged();
    }
}

fn native_errors() {
    let mut source = Source::new(7);
    let valid = source.input(HINT.as_bytes());
    let non_disk = File::open("NUL").expect("Windows non-disk character device");
    let exclusive = Source::new(0);
    let baseline = handles();
    for export in EXPORTS {
        for (input, expected) in [
            (Input { raw: 0, ..valid }, QL_ERROR_INVALID_HANDLE),
            (Input { raw: -1, ..valid }, QL_ERROR_INVALID_HANDLE),
            (
                Input {
                    raw: non_disk.as_raw_handle() as isize,
                    ..valid
                },
                QL_ERROR_INVALID_HANDLE,
            ),
            (
                Input {
                    length: valid.length + 1,
                    ..valid
                },
                QL_ERROR_LENGTH_MISMATCH,
            ),
            (exclusive.input(HINT.as_bytes()), QL_ERROR_IO),
        ] {
            reset(Mode::Success);
            let mut out = [0xa5; 64];
            let mut required = usize::MAX;
            assert_eq!(
                // SAFETY: Invalid handles are validated by Win32, never adopted; all buffers are live/disjoint.
                unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
                expected
            );
            assert_eq!(required, 0);
            assert_eq!(calls(), 0);
            assert_eq!(out, [0xa5; 64]);
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
    }
}

fn reader_errors() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    let errors = [
        ReaderPreviewError::Cancelled,
        ReaderPreviewError::Io,
        ReaderPreviewError::Malformed,
        ReaderPreviewError::LengthMismatch,
        ReaderPreviewError::LimitExceeded,
    ];
    for error in errors {
        assert_eq!(
            handle_preview::reader_preview_status(error),
            expected_status(error)
        );
    }
    for export in EXPORTS {
        for mode in errors.map(Mode::Error).into_iter().chain([Mode::Empty]) {
            reset(mode);
            let mut out = [0xa5; 64];
            let mut required = usize::MAX;
            let expected = match mode {
                Mode::Error(error) => expected_status(error),
                Mode::Empty => QL_ERROR_INTERNAL,
                _ => unreachable!(),
            };
            assert_eq!(
                // SAFETY: Borrowed source/hint and exclusive outputs are live/disjoint on every failure path.
                unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
                expected
            );
            assert_eq!(required, 0);
            assert_eq!(calls(), 1);
            assert_eq!(out, [0xa5; 64]);
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
    }
}

fn handle_cancellation() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    for (callback, expected_reopens) in [
        (pre_cancel as CancelCallback, 0),
        (after_reopen as CancelCallback, 1),
    ] {
        reset(Mode::Success);
        assert_eq!(
            // SAFETY: Live borrowed input; static non-unwinding callbacks cancel before/after an actual reopen.
            unsafe {
                reopen_handle_input_v2(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    Some(callback),
                )
            }
            .unwrap_err(),
            QL_ERROR_CANCELLED
        );
        assert_eq!(REOPENS.get(), expected_reopens);
        assert_eq!(handles(), baseline);
    }
    for export in [Export::Pdf, Export::Image] {
        for (callback, reopens, readers) in [
            (pre_cancel as CancelCallback, 0, 0),
            // The outer preview check must consume the first false poll. If it is removed,
            // the inner reopen check sees the first poll instead and the call reopens the file.
            (cancel_on_second_poll as CancelCallback, 0, 0),
            (after_reopen as CancelCallback, 1, 0),
            (after_reader as CancelCallback, 1, 1),
        ] {
            reset(Mode::Success);
            let mut out = [0xa5; 256];
            let mut required = usize::MAX;
            assert_eq!(
                // SAFETY: All buffers/borrowed source are live and disjoint; callbacks have static lifetime.
                unsafe {
                    export.call(
                        input,
                        out.as_mut_ptr(),
                        out.len(),
                        &mut required,
                        Some(callback),
                    )
                },
                QL_ERROR_CANCELLED
            );
            assert_eq!(required, 0);
            assert_eq!(REOPENS.get(), reopens);
            assert_eq!(calls(), readers);
            if readers > 0 {
                CALLS.with(|calls| {
                    assert!(std::ptr::fn_addr_eq(
                        calls.borrow()[0]
                            .callback
                            .expect("forwarded cancellation callback"),
                        callback,
                    ));
                });
            }
            assert_eq!(out, [0xa5; 256]);
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
        reset(Mode::Error(ReaderPreviewError::Malformed));
        let mut out = [0xa5; 256];
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Static cancellation callback must take precedence over a completed renderer error.
            unsafe {
                export.call(
                    input,
                    out.as_mut_ptr(),
                    out.len(),
                    &mut required,
                    Some(after_reader),
                )
            },
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(handles(), baseline);
        reset(Mode::Success);
        assert_eq!(
            // SAFETY: A fresh noncancelled call reuses the still-live borrowed source and valid outputs.
            unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
            QL_OK
        );
        source.unchanged();
        assert_eq!(handles(), baseline);
    }
}

fn handle_panics() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    for export in EXPORTS {
        reset(Mode::Panic);
        let mut out = [0xa5; 256];
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Source/buffers are live/disjoint; provider panics stay inside the actual export's boundary.
            unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
            QL_ERROR_INTERNAL
        );
        assert_eq!(required, 0);
        assert_eq!(calls(), 1);
        assert_eq!(out, [0xa5; 256]);
        assert_eq!(handles(), baseline);
        reset(Mode::Success);
        assert_eq!(
            // SAFETY: The original borrowed source remains owned by this fixture after unwinding.
            unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
            QL_OK
        );
        source.unchanged();
        assert_eq!(handles(), baseline);
    }
}

fn container_dispatch() {
    family_dispatch(&CONTAINER_EXPORTS);
}

fn structured_dispatch() {
    family_dispatch(&STRUCTURED_EXPORTS);
}

fn family_dispatch(exports: &[FamilyExport]) {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let expected_modified = source
        .file
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let baseline = handles();
    for &export in exports {
        reset(Mode::Success);
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Null/zero is the documented size query over a live borrowed HANDLE.
            unsafe { export.call(input, null_mut(), 0, &mut required, Some(never_cancel)) },
            QL_ERROR_BUFFER_TOO_SMALL
        );
        let expected_size = required;
        assert!(expected_size > 0 && expected_size < 2048);
        for capacity in [expected_size - 1, expected_size, 2048] {
            reset(Mode::Success);
            required = usize::MAX;
            let mut out = [0xa5; 2064];
            assert_eq!(
                // SAFETY: Output is initialized, exclusive and disjoint from the live input.
                unsafe {
                    export.call(
                        input,
                        out[4..].as_mut_ptr(),
                        capacity,
                        &mut required,
                        Some(never_cancel),
                    )
                },
                if capacity < expected_size {
                    QL_ERROR_BUFFER_TOO_SMALL
                } else {
                    QL_OK
                }
            );
            assert_eq!(required, expected_size);
            if capacity < expected_size {
                assert_eq!(out, [0xa5; 2064]);
            } else {
                let payload = std::str::from_utf8(&out[4..4 + required]).expect("UTF-8 payload");
                assert_eq!(
                    payload,
                    format!(
                        "{{\"renderer\":\"{}\",\"bytes\":\"pinned bytes\"}}",
                        export.renderer()
                    )
                );
                assert!(out[..4]
                    .iter()
                    .chain(out[4 + required..].iter())
                    .all(|byte| *byte == 0xa5));
            }
            assert_eq!(REOPENS.get(), 1);
            assert_eq!(calls(), 1);
            CALLS.with(|calls| {
                let call = &calls.borrow()[0];
                assert_eq!(call.renderer, export.renderer());
                assert_eq!(call.name, BASENAME);
                assert_eq!(call.length, DATA.len() as u64);
                assert_eq!(
                    call.modified,
                    if matches!(export, FamilyExport::Package | FamilyExport::Text) {
                        None
                    } else {
                        Some(expected_modified)
                    }
                );
                assert_eq!(call.position, 0);
                assert!(std::ptr::fn_addr_eq(
                    call.callback.expect("forwarded callback"),
                    never_cancel as CancelCallback,
                ));
            });
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
    }
}

fn container_errors_cancellation() {
    family_errors_cancellation(&CONTAINER_EXPORTS);
}

fn structured_errors_cancellation() {
    family_errors_cancellation(&STRUCTURED_EXPORTS);
}

fn family_errors_cancellation(exports: &[FamilyExport]) {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    let errors = [
        ReaderPreviewError::Cancelled,
        ReaderPreviewError::Io,
        ReaderPreviewError::Malformed,
        ReaderPreviewError::LengthMismatch,
        ReaderPreviewError::LimitExceeded,
    ];
    for &export in exports {
        reset(Mode::Success);
        let mut required = usize::MAX;
        let invalid = Input {
            raw: 0,
            length: 0,
            name: null(),
            name_len: 0,
        };
        assert_eq!(
            // SAFETY: Pre-cancellation must reject null input/invalid HANDLE before accessing either.
            unsafe { export.call(invalid, null_mut(), 0, &mut required, Some(pre_cancel)) },
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(REOPENS.get(), 0);
        assert_eq!(calls(), 0);
        assert_eq!(handles(), baseline);
        for mode in errors.map(Mode::Error).into_iter().chain([Mode::Empty]) {
            if matches!(export, FamilyExport::Text) && matches!(mode, Mode::Error(_)) {
                continue;
            }
            reset(mode);
            let mut out = [0xa5; 64];
            let mut required = usize::MAX;
            let expected = match mode {
                Mode::Error(error) => expected_status(error),
                Mode::Empty if matches!(export, FamilyExport::Text) => QL_ERROR_IO,
                Mode::Empty => QL_ERROR_INTERNAL,
                _ => unreachable!(),
            };
            assert_eq!(
                // SAFETY: Borrowed input and exclusive initialized outputs remain live.
                unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
                expected
            );
            assert_eq!(required, 0);
            assert_eq!(calls(), 1);
            assert_eq!(out, [0xa5; 64]);
            source.unchanged();
            assert_eq!(handles(), baseline);
        }

        for (callback, reopens, readers) in [
            (pre_cancel as CancelCallback, 0, 0),
            (cancel_on_second_poll as CancelCallback, 0, 0),
            (after_reopen as CancelCallback, 1, 0),
            (after_reader as CancelCallback, 1, 1),
        ] {
            reset(Mode::Success);
            let mut out = [0xa5; 256];
            let mut required = usize::MAX;
            assert_eq!(
                // SAFETY: Callback is static/non-unwinding; all pointer ranges are live and disjoint.
                unsafe {
                    export.call(
                        input,
                        out.as_mut_ptr(),
                        out.len(),
                        &mut required,
                        Some(callback),
                    )
                },
                QL_ERROR_CANCELLED
            );
            assert_eq!(required, 0);
            assert_eq!(REOPENS.get(), reopens);
            assert_eq!(calls(), readers);
            if readers > 0 {
                CALLS.with(|calls| {
                    assert!(std::ptr::fn_addr_eq(
                        calls.borrow()[0]
                            .callback
                            .expect("forwarded cancellation callback"),
                        callback,
                    ));
                });
            }
            assert_eq!(out, [0xa5; 256]);
            source.unchanged();
            assert_eq!(handles(), baseline);
        }

        reset(Mode::Panic);
        let mut out = [0xa5; 256];
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Provider panics remain contained by the actual exported FFI boundary.
            unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
            QL_ERROR_INTERNAL
        );
        assert_eq!(required, 0);
        assert_eq!(calls(), 1);
        assert_eq!(out, [0xa5; 256]);
        source.unchanged();
        assert_eq!(handles(), baseline);
        reset(Mode::Success);
        assert_eq!(
            // SAFETY: The source remains borrowed and usable after cancellation and panic unwinding.
            unsafe { export.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
            QL_OK
        );
        source.unchanged();
        assert_eq!(handles(), baseline);
    }
}

fn text_empty_semantics() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let baseline = handles();
    reset(Mode::Empty);
    let mut out = [0xa5; 256];
    let mut required = usize::MAX;
    assert_eq!(
        // SAFETY: Source and initialized outputs are live/disjoint; callback is static/non-unwinding.
        unsafe {
            FamilyExport::Text.call(
                input,
                out.as_mut_ptr(),
                out.len(),
                &mut required,
                Some(cancel_on_fourth_poll),
            )
        },
        QL_ERROR_CANCELLED
    );
    // The callback is true only inside the text-empty branch. The shared post-render
    // check sees false, so it cannot hide a wrong/missing inner cancellation result.
    assert_eq!(CANCEL_POLLS.get(), 5);
    assert_eq!(REOPENS.get(), 1);
    assert_eq!(calls(), 1);
    assert_eq!(required, 0);
    assert_eq!(out, [0xa5; 256]);
    source.unchanged();
    assert_eq!(handles(), baseline);
    reset(Mode::Success);
    assert_eq!(
        // SAFETY: A fresh call reuses the still-live borrowed source and exclusive outputs.
        unsafe { FamilyExport::Text.call(input, out.as_mut_ptr(), out.len(), &mut required, None) },
        QL_OK
    );
    source.unchanged();
    assert_eq!(handles(), baseline);
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let cases: [(&str, fn()); 18] = [
        ("native_ownership", native_ownership),
        ("handle_outputs", handle_outputs),
        ("handle_arguments", handle_arguments),
        ("native_errors", native_errors),
        ("reader_errors", reader_errors),
        ("handle_cancellation", handle_cancellation),
        ("handle_panics", handle_panics),
        ("container_dispatch", container_dispatch),
        (
            "container_errors_cancellation",
            container_errors_cancellation,
        ),
        ("structured_dispatch", structured_dispatch),
        (
            "structured_errors_cancellation",
            structured_errors_cancellation,
        ),
        ("text_empty_semantics", text_empty_semantics),
        ("database_path_outputs", database_path_outputs),
        ("database_path_failures", database_path_failures),
        ("database_dispatch", database_dispatch),
        ("database_arguments", database_arguments),
        ("database_limits", database_limits),
        ("database_errors_cancellation", database_errors_cancellation),
    ];
    let group_count = cases.len();
    for (name, probe) in cases {
        if std::panic::catch_unwind(probe).is_err() {
            eprintln!("HANDLE_CONTRACT_FAILED:{name}");
            std::process::exit(1);
        }
    }
    println!("HANDLE_CONTRACT_PASSED:{group_count}");
}
