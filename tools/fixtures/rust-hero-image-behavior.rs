// Actual Hero FFI adapters and raster writers with observable extraction providers.
// Office/package parser behavior remains covered by the real Rust provider tests.
const HERO_BGRA: &[u8] = &[3, 2, 1, 255, 30, 20, 10, 128];
const HERO_REF: &str = "word/media/cover.png";

#[derive(Debug)]
struct HeroCall {
    kind: &'static str,
    name: String,
    bytes: Vec<u8>,
    length: u64,
    image_ref: Option<String>,
    target: Option<(u32, u32)>,
    callback: Option<CancelCallback>,
}

thread_local! {
    static HERO_CALLS: RefCell<Vec<HeroCall>> = const { RefCell::new(Vec::new()) };
    static HERO_VALIDATIONS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

fn hero_result() -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    match MODE.get() {
        Mode::Success => Ok((2, 1, HERO_BGRA.to_vec())),
        Mode::Empty => Err(ReaderPreviewError::Malformed),
        Mode::Error(error) => Err(error),
        Mode::Panic => panic!("hero provider panic"),
    }
}

fn record_hero(
    kind: &'static str,
    name: &str,
    bytes: Vec<u8>,
    length: u64,
    image_ref: Option<&str>,
    target: Option<(u32, u32)>,
    callback: Option<CancelCallback>,
) {
    HERO_CALLS.with(|calls| {
        calls.borrow_mut().push(HeroCall {
            kind,
            name: name.to_owned(),
            bytes,
            length,
            image_ref: image_ref.map(str::to_owned),
            target,
            callback,
        })
    });
}

pub(crate) fn hero_office_path(
    path: &str,
    cb: Option<CancelCallback>,
) -> Option<(u32, u32, Vec<u8>)> {
    record_hero("office-path", path, Vec::new(), 0, None, None, cb);
    hero_result().ok()
}

pub(crate) fn hero_package_path(
    path: &str,
    cb: Option<CancelCallback>,
) -> Option<(u32, u32, Vec<u8>)> {
    record_hero("package-path", path, Vec::new(), 0, None, None, cb);
    hero_result().ok()
}

fn hero_reader(
    file: File,
    kind: &'static str,
    name: &str,
    length: u64,
    image_ref: Option<&str>,
    target: Option<(u32, u32)>,
    cb: Option<CancelCallback>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    let mut bytes = Vec::new();
    file.take(32)
        .read_to_end(&mut bytes)
        .map_err(|_| ReaderPreviewError::Io)?;
    record_hero(kind, name, bytes, length, image_ref, target, cb);
    hero_result()
}

pub(crate) fn hero_office_reader(
    file: File,
    length: u64,
    name: &str,
    cb: Option<CancelCallback>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    hero_reader(file, "office-handle", name, length, None, None, cb)
}

pub(crate) fn hero_package_reader(
    file: File,
    length: u64,
    name: &str,
    cb: Option<CancelCallback>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    hero_reader(file, "package-handle", name, length, None, None, cb)
}

pub(crate) fn hero_layout_reader(
    file: File,
    length: u64,
    name: &str,
    image_ref: &str,
    width: u32,
    height: u32,
    cb: Option<CancelCallback>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    hero_reader(
        file,
        "layout-handle",
        name,
        length,
        Some(image_ref),
        Some((width, height)),
        cb,
    )
}

pub(crate) fn hero_validate_ref(name: &str, image_ref: &str) -> bool {
    HERO_VALIDATIONS.with(|calls| {
        calls
            .borrow_mut()
            .push((name.to_owned(), image_ref.to_owned()))
    });
    name.to_ascii_lowercase().ends_with(".docx")
        && image_ref.starts_with("word/media/")
        && !image_ref.contains("..")
        && [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"]
            .iter()
            .any(|suffix| image_ref.to_ascii_lowercase().ends_with(suffix))
}

#[derive(Clone, Copy)]
enum HeroPathExport {
    Package,
    PackageCancelable,
    Office,
    OfficeCancelable,
}

impl HeroPathExport {
    fn kind(self) -> &'static str {
        match self {
            Self::Package | Self::PackageCancelable => "package-path",
            Self::Office | Self::OfficeCancelable => "office-path",
        }
    }

    /// # Safety
    /// Every non-null input/output range is live and disjoint for the call.
    unsafe fn call(
        self,
        path: *const u8,
        path_len: usize,
        out: *mut u8,
        cap: usize,
        cb: Option<CancelCallback>,
    ) -> i32 {
        // SAFETY: Caller establishes each slice range; this only forwards ABI arguments.
        unsafe {
            match self {
                Self::Package => ffi::hero_image::ql_extract_package_icon(path, path_len, out, cap),
                Self::PackageCancelable => ffi::hero_image::ql_extract_package_icon_cancelable(
                    path, path_len, out, cap, cb,
                ),
                Self::Office => ffi::hero_image::ql_extract_office_image(path, path_len, out, cap),
                Self::OfficeCancelable => ffi::hero_image::ql_extract_office_image_cancelable(
                    path, path_len, out, cap, cb,
                ),
            }
        }
    }
}

const HERO_PATH_EXPORTS: [HeroPathExport; 4] = [
    HeroPathExport::Package,
    HeroPathExport::PackageCancelable,
    HeroPathExport::Office,
    HeroPathExport::OfficeCancelable,
];

fn hero_path_packets() {
    for export in HERO_PATH_EXPORTS {
        for cap in [15, 16, 32] {
            reset(Mode::Success);
            let mut out = [0xa5; 40];
            // SAFETY: Path is a live static string; output is an exclusive initialized slice.
            let status = unsafe {
                export.call(
                    HINT.as_ptr(),
                    HINT.len(),
                    out[4..4 + cap].as_mut_ptr(),
                    cap,
                    Some(never_cancel),
                )
            };
            assert_eq!(status, if cap < 16 { -16 } else { 16 });
            if cap < 16 {
                assert_eq!(out, [0xa5; 40]);
            } else {
                assert_eq!(
                    &out[4..20],
                    &[2, 0, 0, 0, 1, 0, 0, 0, 3, 2, 1, 255, 30, 20, 10, 128]
                );
                assert!(out[..4]
                    .iter()
                    .chain(out[20..].iter())
                    .all(|byte| *byte == 0xa5));
            }
            HERO_CALLS.with(|calls| {
                let calls = calls.borrow();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].kind, export.kind());
                assert_eq!(calls[0].name, HINT);
                if matches!(
                    export,
                    HeroPathExport::PackageCancelable | HeroPathExport::OfficeCancelable
                ) {
                    assert!(std::ptr::fn_addr_eq(
                        calls[0].callback.unwrap(),
                        never_cancel as CancelCallback
                    ));
                } else {
                    assert!(calls[0].callback.is_none());
                }
            });
            assert_eq!(REOPENS.get(), 0);
        }
    }
}

#[allow(clippy::undocumented_unsafe_blocks)]
fn hero_path_failures() {
    let oversized = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    for export in HERO_PATH_EXPORTS {
        for (path, length) in [
            (null(), 0),
            (null(), 1),
            ([0xff].as_ptr(), 1),
            (oversized.as_ptr(), oversized.len()),
        ] {
            reset(Mode::Success);
            let mut out = [0xa5; 32];
            // SAFETY: Null/oversized probes must reject before dereference; other inputs are valid.
            assert_eq!(
                unsafe { export.call(path, length, out.as_mut_ptr(), out.len(), None) },
                -1
            );
            assert_eq!(out, [0xa5; 32]);
            HERO_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
        }
        for (mode, expected) in [(Mode::Empty, -2), (Mode::Panic, QL_ERROR_INTERNAL)] {
            reset(mode);
            let mut out = [0xa5; 32];
            // SAFETY: Static path and exclusive output remain live across the contained boundary.
            assert_eq!(
                unsafe {
                    export.call(HINT.as_ptr(), HINT.len(), out.as_mut_ptr(), out.len(), None)
                },
                expected
            );
            assert_eq!(out, [0xa5; 32]);
            HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 1));
        }
        reset(Mode::Success);
        assert_eq!(
            // SAFETY: Invalid null output is rejected by the checked legacy packet writer.
            unsafe { export.call(HINT.as_ptr(), HINT.len(), null_mut(), 16, None) },
            -16
        );
    }
    for export in [
        HeroPathExport::PackageCancelable,
        HeroPathExport::OfficeCancelable,
    ] {
        for callback in [
            pre_cancel as CancelCallback,
            cancel_on_second_poll as CancelCallback,
            after_hero as CancelCallback,
        ] {
            reset(Mode::Success);
            let mut out = [0xa5; 32];
            // SAFETY: Callback is static/non-unwinding and all ranges remain live.
            assert_eq!(
                unsafe {
                    export.call(
                        HINT.as_ptr(),
                        HINT.len(),
                        out.as_mut_ptr(),
                        out.len(),
                        Some(callback),
                    )
                },
                -3
            );
            assert_eq!(out, [0xa5; 32]);
            HERO_CALLS.with(|calls| {
                assert_eq!(
                    calls.borrow().len(),
                    usize::from(!std::ptr::fn_addr_eq(
                        callback,
                        pre_cancel as CancelCallback
                    ))
                )
            });
        }
    }
}

#[derive(Clone, Copy)]
enum HeroHandleExport {
    Office,
    Package,
    Layout,
}

impl HeroHandleExport {
    /// # Safety
    /// Caller retains source ownership; non-null output/scalar ranges are live/disjoint.
    #[allow(clippy::too_many_arguments, clippy::undocumented_unsafe_blocks)]
    unsafe fn call(
        self,
        input: Input,
        image_ref: Option<&[u8]>,
        target: (u32, u32),
        out: *mut u8,
        cap: usize,
        required: *mut usize,
        cb: Option<CancelCallback>,
    ) -> i32 {
        let (image_ptr, image_len) =
            image_ref.map_or((null(), 0), |value| (value.as_ptr(), value.len()));
        // SAFETY: All borrowed input slices and disjoint outputs remain valid for the call.
        unsafe {
            match self {
                Self::Office => ffi::hero_image::ql_extract_office_image_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Package => ffi::hero_image::ql_extract_package_icon_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    out,
                    cap,
                    required,
                    cb,
                ),
                Self::Layout => ffi::hero_image::ql_extract_office_layout_image_handle(
                    input.raw,
                    input.length,
                    input.name,
                    input.name_len,
                    image_ptr,
                    image_len,
                    target.0,
                    target.1,
                    out,
                    cap,
                    required,
                    cb,
                ),
            }
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Office => "office-handle",
            Self::Package => "package-handle",
            Self::Layout => "layout-handle",
        }
    }
}

const HERO_HANDLE_EXPORTS: [HeroHandleExport; 3] = [
    HeroHandleExport::Office,
    HeroHandleExport::Package,
    HeroHandleExport::Layout,
];

fn expected_hero_status(error: ReaderPreviewError) -> i32 {
    match error {
        ReaderPreviewError::Cancelled => QL_ERROR_CANCELLED,
        ReaderPreviewError::Io => QL_ERROR_IO,
        ReaderPreviewError::Malformed => QL_ERROR_MALFORMED,
        ReaderPreviewError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        ReaderPreviewError::LimitExceeded => QL_ERROR_LIMIT_EXCEEDED,
    }
}

#[allow(clippy::undocumented_unsafe_blocks)]
fn hero_handle_packets() {
    let mut source = Source::new(7);
    let baseline = handles();
    for export in HERO_HANDLE_EXPORTS {
        for cap in [0, 15, 16, 32] {
            reset(Mode::Success);
            let mut out = [0xa5; 40];
            let mut required = usize::MAX;
            let input = if matches!(export, HeroHandleExport::Layout) {
                source.input(LAYOUT_HINT.as_bytes())
            } else {
                source.input(HINT.as_bytes())
            };
            let pointer = if cap == 0 {
                null_mut()
            } else {
                out[4..].as_mut_ptr()
            };
            // SAFETY: Valid borrowed HANDLE/name and exclusive output/required storage; cap zero is a size probe.
            assert_eq!(
                unsafe {
                    export.call(
                        input,
                        Some(HERO_REF.as_bytes()),
                        (320, 240),
                        pointer,
                        cap,
                        &mut required,
                        Some(never_cancel),
                    )
                },
                if cap < 16 {
                    QL_ERROR_BUFFER_TOO_SMALL
                } else {
                    QL_OK
                }
            );
            assert_eq!(required, 16);
            if cap < 16 {
                assert_eq!(out, [0xa5; 40]);
            } else {
                assert_eq!(
                    &out[4..20],
                    &[2, 0, 0, 0, 1, 0, 0, 0, 3, 2, 1, 255, 30, 20, 10, 128]
                );
                assert!(out[..4]
                    .iter()
                    .chain(out[20..].iter())
                    .all(|byte| *byte == 0xa5));
            }
            assert_eq!(REOPENS.get(), 1);
            HERO_CALLS.with(|calls| {
                let calls = calls.borrow();
                assert_eq!(calls.len(), 1);
                let call = &calls[0];
                assert_eq!(call.kind, export.kind());
                assert_eq!(
                    call.name,
                    if matches!(export, HeroHandleExport::Layout) {
                        "report.docx"
                    } else {
                        BASENAME
                    }
                );
                assert_eq!(call.bytes, DATA);
                assert_eq!(call.length, DATA.len() as u64);
                assert!(std::ptr::fn_addr_eq(
                    call.callback.unwrap(),
                    never_cancel as CancelCallback
                ));
                if matches!(export, HeroHandleExport::Layout) {
                    assert_eq!(call.image_ref.as_deref(), Some(HERO_REF));
                    assert_eq!(call.target, Some((320, 240)));
                }
            });
            if matches!(export, HeroHandleExport::Layout) {
                HERO_VALIDATIONS.with(|calls| {
                    assert_eq!(
                        calls.borrow().last(),
                        Some(&("report.docx".to_owned(), HERO_REF.to_owned()))
                    )
                });
            }
            source.unchanged();
            assert_eq!(handles(), baseline);
        }
    }
}

#[allow(clippy::type_complexity, clippy::undocumented_unsafe_blocks)]
fn hero_handle_arguments() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let layout_input = source.input(LAYOUT_HINT.as_bytes());
    let oversized = vec![b'a'; MAX_OFFICE_IMAGE_REF_BYTES + 1];
    let invalid_utf8 = [0xff];
    for export in HERO_HANDLE_EXPORTS {
        let invalid_inputs: Vec<(Input, Option<&[u8]>, (u32, u32), i32)> =
            if matches!(export, HeroHandleExport::Layout) {
                vec![
                    (input, None, (320, 240), QL_ERROR_INVALID_ARGUMENT),
                    (input, Some(&[]), (320, 240), QL_ERROR_INVALID_ARGUMENT),
                    (
                        input,
                        Some(&invalid_utf8),
                        (320, 240),
                        QL_ERROR_INVALID_ARGUMENT,
                    ),
                    (
                        input,
                        Some(&oversized),
                        (320, 240),
                        QL_ERROR_INVALID_ARGUMENT,
                    ),
                    (
                        Input {
                            name: null(),
                            name_len: 0,
                            ..input
                        },
                        Some(HERO_REF.as_bytes()),
                        (320, 240),
                        QL_ERROR_INVALID_ARGUMENT,
                    ),
                    (
                        Input {
                            name: b"report.odt".as_ptr(),
                            name_len: 10,
                            ..input
                        },
                        Some(HERO_REF.as_bytes()),
                        (320, 240),
                        QL_ERROR_INVALID_ARGUMENT,
                    ),
                ]
            } else {
                Vec::new()
            };
        for (input, image_ref, target, expected) in invalid_inputs {
            reset(Mode::Success);
            let baseline = handles();
            let mut out = [0xa5; 32];
            let mut required = usize::MAX;
            // SAFETY: Each supplied name/reference is live and bounded; malformed input must fail closed.
            assert_eq!(
                unsafe {
                    export.call(
                        input,
                        image_ref,
                        target,
                        out.as_mut_ptr(),
                        out.len(),
                        &mut required,
                        None,
                    )
                },
                expected
            );
            assert_eq!(required, 0);
            assert_eq!(out, [0xa5; 32]);
            HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 0));
            assert_eq!(handles(), baseline);
        }
        if matches!(export, HeroHandleExport::Layout) {
            for target in [
                (0, 1),
                (1, 0),
                (preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION + 1, 1),
                (1, preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION + 1),
            ] {
                reset(Mode::Success);
                let baseline = handles();
                let mut out = [0xa5; 32];
                let mut required = usize::MAX;
                // SAFETY: Valid bounded name/reference and outputs; bad dimensions must reject before reopening.
                let selected = if matches!(export, HeroHandleExport::Layout) {
                    layout_input
                } else {
                    input
                };
                assert_eq!(
                    unsafe {
                        export.call(
                            selected,
                            Some(HERO_REF.as_bytes()),
                            target,
                            out.as_mut_ptr(),
                            out.len(),
                            &mut required,
                            None,
                        )
                    },
                    QL_ERROR_INVALID_ARGUMENT
                );
                assert_eq!(required, 0);
                assert_eq!(REOPENS.get(), 0);
                assert_eq!(out, [0xa5; 32]);
                assert_eq!(handles(), baseline);
            }
        }
    }
    let mut required = usize::MAX;
    reset(Mode::Success);
    assert_eq!(
        // SAFETY: Each export checks the required-size scalar before reading HANDLE or buffers.
        unsafe {
            HeroHandleExport::Office.call(
                input,
                None,
                (1, 1),
                null_mut(),
                0,
                null_mut(),
                Some(pre_cancel),
            )
        },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        // SAFETY: Null output plus nonzero capacity must be rejected before reopening.
        unsafe {
            HeroHandleExport::Package.call(
                input,
                None,
                (1, 1),
                null_mut(),
                1,
                &mut required,
                Some(pre_cancel),
            )
        },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(required, 0);
    assert_eq!(REOPENS.get(), 0);
    source.unchanged();
}

#[allow(clippy::undocumented_unsafe_blocks)]
fn hero_layout_validation() {
    let mut source = Source::new(7);
    let layout_input = source.input(LAYOUT_HINT.as_bytes());
    for image_ref in [
        b"ppt/media/image.png".as_slice(),
        b"word/media/../escape.png",
        b"word\\media\\image.png",
        b"word/media/readme.txt",
    ] {
        reset(Mode::Success);
        let baseline = handles();
        let mut required = usize::MAX;
        let mut out = [0xa5; 32];
        // SAFETY: Invalid but bounded references reject before provider access.
        assert_eq!(
            unsafe {
                HeroHandleExport::Layout.call(
                    layout_input,
                    Some(image_ref),
                    (1, 1),
                    out.as_mut_ptr(),
                    out.len(),
                    &mut required,
                    None,
                )
            },
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, 0);
        HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 0));
        assert_eq!(out, [0xa5; 32]);
        assert_eq!(handles(), baseline);
    }
    for error in [
        ReaderPreviewError::Cancelled,
        ReaderPreviewError::Io,
        ReaderPreviewError::Malformed,
        ReaderPreviewError::LengthMismatch,
        ReaderPreviewError::LimitExceeded,
    ] {
        reset(Mode::Error(error));
        let baseline = handles();
        let mut required = usize::MAX;
        let mut out = [0xa5; 32];
        assert_eq!(
            // SAFETY: Provider status must map through the actual exported FFI adapter.
            unsafe {
                HeroHandleExport::Layout.call(
                    layout_input,
                    Some(HERO_REF.as_bytes()),
                    (1, 1),
                    out.as_mut_ptr(),
                    out.len(),
                    &mut required,
                    None,
                )
            },
            expected_hero_status(error)
        );
        assert_eq!(required, 0);
        HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 1));
        assert_eq!(out, [0xa5; 32]);
        assert_eq!(handles(), baseline);
    }
    source.unchanged();
}

#[allow(clippy::undocumented_unsafe_blocks)]
fn hero_handle_failures() {
    let mut source = Source::new(7);
    let input = source.input(HINT.as_bytes());
    let layout_input = source.input(LAYOUT_HINT.as_bytes());
    let non_disk = File::open("NUL").unwrap();
    let mut exclusive = Source::new(0);
    for export in HERO_HANDLE_EXPORTS {
        let mut cases = vec![
            (Input { raw: 0, ..input }, QL_ERROR_INVALID_HANDLE, 0),
            (Input { raw: -1, ..input }, QL_ERROR_INVALID_HANDLE, 0),
            (
                Input {
                    raw: non_disk.as_raw_handle() as isize,
                    ..input
                },
                QL_ERROR_INVALID_HANDLE,
                0,
            ),
            (
                Input {
                    length: input.length + 1,
                    ..input
                },
                QL_ERROR_LENGTH_MISMATCH,
                0,
            ),
            (exclusive.input(HINT.as_bytes()), QL_ERROR_IO, 0),
        ];
        if !matches!(export, HeroHandleExport::Layout) {
            // These providers also preserve the same shared native HANDLE errors.
        }
        for (bad_input, expected, reopens) in cases.drain(..) {
            reset(Mode::Success);
            let baseline = handles();
            let mut out = [0xa5; 32];
            let mut required = usize::MAX;
            // SAFETY: Invalid HANDLE/length probes and valid exclusive ranges remain alive.
            let selected = if matches!(export, HeroHandleExport::Layout) {
                Input {
                    name: layout_input.name,
                    name_len: layout_input.name_len,
                    ..bad_input
                }
            } else {
                bad_input
            };
            assert_eq!(
                unsafe {
                    export.call(
                        selected,
                        Some(HERO_REF.as_bytes()),
                        (1, 1),
                        out.as_mut_ptr(),
                        out.len(),
                        &mut required,
                        None,
                    )
                },
                expected
            );
            assert_eq!(required, 0);
            HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 0));
            assert_eq!(REOPENS.get(), if reopens == 0 { 1 } else { reopens });
            assert_eq!(out, [0xa5; 32]);
            assert_eq!(handles(), baseline);
        }
        for mode in [
            Mode::Empty,
            Mode::Panic,
            Mode::Error(ReaderPreviewError::Cancelled),
            Mode::Error(ReaderPreviewError::Io),
            Mode::Error(ReaderPreviewError::Malformed),
            Mode::Error(ReaderPreviewError::LengthMismatch),
            Mode::Error(ReaderPreviewError::LimitExceeded),
        ] {
            reset(mode);
            let baseline = handles();
            let mut out = [0xa5; 32];
            let mut required = usize::MAX;
            let expected = if let Mode::Error(error) = mode {
                expected_hero_status(error)
            } else if matches!(mode, Mode::Empty) {
                QL_ERROR_MALFORMED
            } else {
                QL_ERROR_INTERNAL
            };
            // SAFETY: Valid borrowed inputs and output storage remain live across provider failures and panics.
            let selected = if matches!(export, HeroHandleExport::Layout) {
                layout_input
            } else {
                input
            };
            assert_eq!(
                unsafe {
                    export.call(
                        selected,
                        Some(HERO_REF.as_bytes()),
                        (1, 1),
                        out.as_mut_ptr(),
                        out.len(),
                        &mut required,
                        None,
                    )
                },
                expected
            );
            assert_eq!(required, 0);
            HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), 1));
            assert_eq!(REOPENS.get(), 1);
            assert_eq!(out, [0xa5; 32]);
            assert_eq!(handles(), baseline);
        }
        for (callback, reopens, readers) in [
            (pre_cancel as CancelCallback, 0, 0),
            (cancel_on_second_poll as CancelCallback, 1, 0),
            (after_reopen as CancelCallback, 1, 0),
            (after_hero as CancelCallback, 1, 1),
        ] {
            reset(Mode::Success);
            let baseline = handles();
            let mut out = [0xa5; 32];
            let mut required = usize::MAX;
            // SAFETY: Static callback and borrowed input/output remain valid and disjoint.
            let selected = if matches!(export, HeroHandleExport::Layout) {
                layout_input
            } else {
                input
            };
            assert_eq!(
                unsafe {
                    export.call(
                        selected,
                        Some(HERO_REF.as_bytes()),
                        (1, 1),
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
            HERO_CALLS.with(|calls| assert_eq!(calls.borrow().len(), readers));
            assert_eq!(out, [0xa5; 32]);
            assert_eq!(handles(), baseline);
        }
    }
    source.unchanged();
    exclusive.unchanged();
}

extern "C" fn after_hero() -> bool {
    HERO_CALLS.with(|calls| !calls.borrow().is_empty())
}
