// Included by the real Win32 HANDLE harness. Providers observe adapter contracts,
// not SQLite parsing: the production SQLite/WAL tests remain independently required.
const WAL_DATA: &[u8] = b"WAL committed pages";
const SHM_DATA: &[u8] = b"SHM diagnostics only";
const DATABASE_JSON: &str = "{\"database\":\"é\"}";

struct DatabasePathCall {
    path: String,
    size: i64,
    modified: i64,
    callback: Option<CancelCallback>,
}

struct DatabaseCall {
    main: Vec<u8>,
    wal: Option<Vec<u8>>,
    wal_length: u64,
    shm: Option<Vec<u8>>,
    shm_length: u64,
}

thread_local! {
    static DATABASE_PATH_CALLS: RefCell<Vec<DatabasePathCall>> = const { RefCell::new(Vec::new()) };
    static DATABASE_CALLS: RefCell<Vec<DatabaseCall>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn database_path(
    path: &str,
    size: i64,
    modified: i64,
    callback: Option<CancelCallback>,
) -> String {
    DATABASE_PATH_CALLS.with(|calls| {
        calls.borrow_mut().push(DatabasePathCall {
            path: path.to_owned(),
            size,
            modified,
            callback,
        })
    });
    match MODE.get() {
        Mode::Success => DATABASE_JSON.to_owned(),
        Mode::Empty => String::new(),
        Mode::Panic => panic!("database path panic"),
        Mode::Error(_) => unreachable!("legacy path provider has no typed errors"),
    }
}

fn read_database_prefix(reader: &mut dyn Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    reader
        .take(32)
        .read_to_end(&mut bytes)
        .expect("bounded database fixture read");
    bytes
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn database_reader(
    main: &mut File,
    length: u64,
    wal: preview::DatabaseCompanionReader<'_>,
    shm: preview::DatabaseCompanionReader<'_>,
    name: &str,
    modified: i64,
    callback: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    observe(main, "database", name, length, Some(modified), callback);
    DATABASE_CALLS.with(|calls| {
        calls.borrow_mut().push(DatabaseCall {
            main: read_database_prefix(main),
            wal: wal.reader.map(read_database_prefix),
            wal_length: wal.length,
            shm: shm.reader.map(read_database_prefix),
            shm_length: shm.length,
        })
    });
    match MODE.get() {
        Mode::Success => Ok(DATABASE_JSON.to_owned()),
        Mode::Empty => Ok(String::new()),
        Mode::Panic => panic!("database reader panic"),
        Mode::Error(error) => Err(error),
    }
}

extern "C" fn after_database_path() -> bool {
    DATABASE_PATH_CALLS.with(|calls| !calls.borrow().is_empty())
}

// Safe facade owns the range checks for every path invocation, including invalid UTF-8.
fn database_path_call(
    path: Option<&[u8]>,
    size: i64,
    out: Option<&mut [u8]>,
    cb: Option<CancelCallback>,
) -> i32 {
    let (path_ptr, path_len) = path.map_or((null(), 0), |path| (path.as_ptr(), path.len()));
    let (out_ptr, out_cap) = out.map_or((null_mut(), 0), |out| (out.as_mut_ptr(), out.len()));
    // SAFETY: Non-null buffers are live slices and disjoint; callbacks are static/non-unwinding.
    unsafe {
        ffi::database::ql_preview_database_cancelable(
            path_ptr, path_len, size, -77, out_ptr, out_cap, cb,
        )
    }
}

fn database_path_outputs() {
    for size in [-1, 0, i64::MAX] {
        for cap in [DATABASE_JSON.len() - 1, DATABASE_JSON.len(), 96] {
            reset(Mode::Success);
            let mut out = [0xa5; 104];
            let status = database_path_call(
                Some(HINT.as_bytes()),
                size,
                Some(&mut out[4..4 + cap]),
                Some(never_cancel),
            );
            let needed = DATABASE_JSON.len();
            assert_eq!(
                status,
                if cap < needed {
                    -(needed as i32)
                } else {
                    needed as i32
                }
            );
            if cap < needed {
                assert_eq!(out, [0xa5; 104]);
            } else {
                assert_eq!(&out[4..4 + needed], DATABASE_JSON.as_bytes());
                assert!(out[..4]
                    .iter()
                    .chain(out[4 + needed..].iter())
                    .all(|byte| *byte == 0xa5));
            }
            DATABASE_PATH_CALLS.with(|calls| {
                let calls = calls.borrow();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].path, HINT);
                assert_eq!(calls[0].size, size);
                assert_eq!(calls[0].modified, -77);
                assert!(std::ptr::fn_addr_eq(
                    calls[0].callback.unwrap(),
                    never_cancel as CancelCallback
                ));
            });
            assert_eq!(REOPENS.get(), 0);
        }
    }
    let path = vec![b'x'; MAX_FFI_STRING_BYTES];
    reset(Mode::Success);
    let mut out = [0xa5; 96];
    assert_eq!(
        database_path_call(Some(&path), 0, Some(&mut out), None),
        DATABASE_JSON.len() as i32
    );
    DATABASE_PATH_CALLS
        .with(|calls| assert_eq!(calls.borrow()[0].path.len(), MAX_FFI_STRING_BYTES));
}

fn database_path_failures() {
    let oversized = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    for path in [None, Some(&[0xff][..]), Some(&oversized[..])] {
        reset(Mode::Success);
        let mut out = [0xa5; 96];
        assert_eq!(database_path_call(path, 0, Some(&mut out), None), 0);
        assert_eq!(out, [0xa5; 96]);
        DATABASE_PATH_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    }
    for out in [None, Some(&mut [][..])] {
        reset(Mode::Success);
        assert_eq!(database_path_call(Some(HINT.as_bytes()), 0, out, None), 0);
        DATABASE_PATH_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    }
    reset(Mode::Success);
    assert_eq!(
        // SAFETY: Null output with nonzero capacity must reject before attempting a write.
        unsafe {
            ffi::database::ql_preview_database_cancelable(
                HINT.as_ptr(),
                HINT.len(),
                0,
                0,
                null_mut(),
                1,
                None,
            )
        },
        0
    );
    DATABASE_PATH_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    for (mode, cb, expected_calls, expected) in [
        (Mode::Success, Some(pre_cancel as CancelCallback), 0, 0),
        (
            Mode::Success,
            Some(after_database_path as CancelCallback),
            1,
            0,
        ),
        (Mode::Empty, None, 1, 0),
        (Mode::Panic, None, 1, QL_ERROR_INTERNAL),
    ] {
        reset(mode);
        let mut out = [0xa5; 96];
        assert_eq!(
            database_path_call(Some(HINT.as_bytes()), 0, Some(&mut out), cb),
            expected
        );
        DATABASE_PATH_CALLS.with(|calls| assert_eq!(calls.borrow().len(), expected_calls));
        assert_eq!(out, [0xa5; 96]);
        assert_eq!(REOPENS.get(), 0);
    }
    reset(Mode::Success);
    let mut out = [0xa5; 96];
    assert_eq!(
        database_path_call(Some(b""), 0, Some(&mut out), None),
        DATABASE_JSON.len() as i32
    );
}

#[derive(Clone, Copy)]
struct DatabaseInput {
    main: Input,
    wal: (isize, u64),
    shm: (isize, u64),
}

impl DatabaseInput {
    fn new(main: &Source, wal: Option<&Source>, shm: Option<&Source>) -> Self {
        let companion = |source: Option<&Source>| {
            source.map_or((0, 0), |source| {
                (source.file.as_raw_handle() as isize, source.length)
            })
        };
        Self {
            main: main.input(HINT.as_bytes()),
            wal: companion(wal),
            shm: companion(shm),
        }
    }

    /// # Safety
    /// Non-null input/output ranges must be live/disjoint; HANDLEs are borrowed for this call.
    unsafe fn call(
        self,
        out: *mut u8,
        cap: usize,
        required: *mut usize,
        cb: Option<CancelCallback>,
    ) -> i32 {
        // SAFETY: The caller supplies borrowed HANDLEs and live/disjoint buffers, forwarded unchanged.
        unsafe {
            ffi::database::ql_preview_sqlite_handles(
                self.main.raw,
                self.main.length,
                self.wal.0,
                self.wal.1,
                self.shm.0,
                self.shm.1,
                self.main.name,
                self.main.name_len,
                out,
                cap,
                required,
                cb,
            )
        }
    }
}

fn database_dispatch() {
    let mut main = Source::new(7);
    let mut wal = Source::with_bytes(7, WAL_DATA);
    let mut shm = Source::with_bytes(7, SHM_DATA);
    let mut empty = Source::with_bytes(7, b"");
    // The hint names a missing path; rename and replace all actual path entries as well.
    for source in [&main, &wal, &shm] {
        fs::rename(&source.path, source.path.with_extension("pinned")).unwrap();
        fs::write(&source.path, b"decoy").unwrap();
    }
    let baseline = handles();
    for wal_source in [None, Some(&wal), Some(&empty)] {
        for shm_source in [None, Some(&shm), Some(&empty)] {
            let input = DatabaseInput::new(&main, wal_source, shm_source);
            for cap in [0, DATABASE_JSON.len() - 1, DATABASE_JSON.len(), 96] {
                reset(Mode::Success);
                let mut out = [0xa5; 104];
                let mut required = usize::MAX;
                let out_ptr = if cap == 0 {
                    null_mut()
                } else {
                    out[4..].as_mut_ptr()
                };
                assert_eq!(
                    // SAFETY: All borrowed source objects/ranges are live; null/zero is a size probe.
                    unsafe { input.call(out_ptr, cap, &mut required, Some(never_cancel)) },
                    if cap < DATABASE_JSON.len() {
                        QL_ERROR_BUFFER_TOO_SMALL
                    } else {
                        QL_OK
                    }
                );
                assert_eq!(required, DATABASE_JSON.len());
                if cap < required {
                    assert_eq!(out, [0xa5; 104]);
                } else {
                    assert_eq!(&out[4..4 + required], DATABASE_JSON.as_bytes());
                    assert!(out[..4]
                        .iter()
                        .chain(out[4 + required..].iter())
                        .all(|byte| *byte == 0xa5));
                }
                assert_eq!(
                    REOPENS.get(),
                    1 + usize::from(wal_source.is_some()) + usize::from(shm_source.is_some())
                );
                CALLS.with(|calls| {
                    let calls = calls.borrow();
                    assert_eq!(calls.len(), 1);
                    let call = &calls[0];
                    assert_eq!(call.renderer, "database");
                    assert_eq!(call.name, BASENAME);
                    assert_eq!(call.length, main.length);
                    assert_eq!(call.position, 0);
                    assert_eq!(
                        call.modified,
                        Some(
                            main.file
                                .metadata()
                                .unwrap()
                                .modified()
                                .unwrap()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64
                        )
                    );
                    assert!(std::ptr::fn_addr_eq(
                        call.callback.unwrap(),
                        never_cancel as CancelCallback
                    ));
                });
                DATABASE_CALLS.with(|calls| {
                    let calls = calls.borrow();
                    assert_eq!(calls.len(), 1);
                    assert_eq!(calls[0].main, DATA);
                    assert_eq!(
                        calls[0].wal.as_deref(),
                        wal_source.map(|source| if source.length == 0 {
                            &b""[..]
                        } else {
                            WAL_DATA
                        })
                    );
                    assert_eq!(
                        calls[0].shm.as_deref(),
                        shm_source.map(|source| if source.length == 0 {
                            &b""[..]
                        } else {
                            SHM_DATA
                        })
                    );
                    assert_eq!(calls[0].wal_length, input.wal.1);
                    assert_eq!(calls[0].shm_length, input.shm.1);
                });
                assert_eq!(handles(), baseline);
            }
        }
    }
    for source in [&mut main, &mut wal, &mut shm, &mut empty] {
        source.unchanged();
    }
}

fn database_failure(input: DatabaseInput, expected: i32, reopens: usize) {
    reset(Mode::Success);
    let mut out = [0xa5; 96];
    let mut required = usize::MAX;
    let baseline = handles();
    assert_eq!(
        // SAFETY: Live disjoint buffers; invalid tuples/HANDLE values must be rejected by Win32/adapter.
        unsafe { input.call(out.as_mut_ptr(), out.len(), &mut required, None) },
        expected
    );
    assert_eq!(required, 0);
    assert_eq!(out, [0xa5; 96]);
    assert_eq!(calls(), 0);
    assert_eq!(REOPENS.get(), reopens);
    assert_eq!(handles(), baseline);
}

fn database_arguments() {
    let mut main = Source::new(7);
    let mut wal = Source::with_bytes(7, WAL_DATA);
    let mut shm = Source::with_bytes(7, SHM_DATA);
    let mut exclusive = Source::new(0);
    let non_disk = File::open("NUL").unwrap();
    let input = DatabaseInput::new(&main, Some(&wal), Some(&shm));
    for raw in [0, -1, non_disk.as_raw_handle() as isize] {
        database_failure(
            DatabaseInput {
                main: Input { raw, ..input.main },
                ..input
            },
            QL_ERROR_INVALID_HANDLE,
            1,
        );
    }
    database_failure(
        DatabaseInput {
            main: Input {
                length: main.length + 1,
                ..input.main
            },
            ..input
        },
        QL_ERROR_LENGTH_MISMATCH,
        1,
    );
    database_failure(
        DatabaseInput {
            main: exclusive.input(HINT.as_bytes()),
            ..input
        },
        QL_ERROR_IO,
        1,
    );
    for is_shm in [false, true] {
        for (pair, expected, reopens) in [
            ((0, 1), QL_ERROR_INVALID_ARGUMENT, 1),
            (
                (
                    0,
                    if is_shm {
                        preview::MAX_SQLITE_SHM_BYTES + 1
                    } else {
                        preview::MAX_SQLITE_WAL_BYTES + 1
                    },
                ),
                QL_ERROR_INVALID_ARGUMENT,
                1,
            ),
            ((-1, 0), QL_ERROR_INVALID_HANDLE, if is_shm { 3 } else { 2 }),
            (
                (non_disk.as_raw_handle() as isize, 0),
                QL_ERROR_INVALID_HANDLE,
                if is_shm { 3 } else { 2 },
            ),
            (
                (exclusive.file.as_raw_handle() as isize, exclusive.length),
                QL_ERROR_IO,
                if is_shm { 3 } else { 2 },
            ),
            (
                (if is_shm { input.shm.0 } else { input.wal.0 }, 1),
                QL_ERROR_LENGTH_MISMATCH,
                if is_shm { 3 } else { 2 },
            ),
        ] {
            let invalid = if is_shm {
                DatabaseInput { shm: pair, ..input }
            } else {
                DatabaseInput { wal: pair, ..input }
            };
            database_failure(invalid, expected, reopens);
        }
    }
    for bad_name in [None, Some(&[0xff][..]), Some(&b".."[..])] {
        let (name, name_len) = bad_name.map_or((null(), 0), |bytes| (bytes.as_ptr(), bytes.len()));
        database_failure(
            DatabaseInput {
                main: Input {
                    name,
                    name_len,
                    ..input.main
                },
                ..input
            },
            QL_ERROR_INVALID_ARGUMENT,
            0,
        );
    }
    reset(Mode::Success);
    let baseline = handles();
    let mut required = usize::MAX;
    assert_eq!(
        // SAFETY: Null required scalar must be rejected before any input/output access.
        unsafe { input.call(null_mut(), 0, null_mut(), Some(pre_cancel)) },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        // SAFETY: Null output with nonzero capacity must be rejected before I/O, even if cancelled.
        unsafe { input.call(null_mut(), 1, &mut required, Some(pre_cancel)) },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(required, 0);
    assert_eq!(REOPENS.get(), 0);
    assert_eq!(handles(), baseline);
    for source in [&mut main, &mut wal, &mut shm, &mut exclusive] {
        source.unchanged();
    }
}

fn database_limits() {
    let mut main = Source::new(7);
    let mut wal = Source::with_bytes(7, WAL_DATA);
    let mut shm = Source::with_bytes(7, SHM_DATA);
    for (index, limit) in [
        preview::MAX_DATABASE_HANDLE_BYTES,
        preview::MAX_SQLITE_WAL_BYTES,
        preview::MAX_SQLITE_SHM_BYTES,
    ]
    .into_iter()
    .enumerate()
    {
        // Future production-limit changes must not silently grow this scratch-file budget.
        assert!(limit <= 256 * 1024 * 1024, "fixture logical-file budget");
        for extra in [0, 1] {
            let source = match index {
                0 => &mut main,
                1 => &mut wal,
                _ => &mut shm,
            };
            // Real file lengths are essential: main is validated by ReOpenFile before FFI limits.
            // At most 256 MiB + 1 logical bytes; the provider reads only a 32-byte prefix.
            source.resize(limit + extra);
            let input = DatabaseInput::new(&main, Some(&wal), Some(&shm));
            if extra == 1 {
                database_failure(input, QL_ERROR_LIMIT_EXCEEDED, 1);
            } else {
                reset(Mode::Success);
                let mut required = usize::MAX;
                let baseline = handles();
                assert_eq!(
                    // SAFETY: Real exact-length files remain borrowed; null/zero probes only the size.
                    unsafe { input.call(null_mut(), 0, &mut required, None) },
                    QL_ERROR_BUFFER_TOO_SMALL
                );
                assert_eq!(required, DATABASE_JSON.len());
                assert_eq!(REOPENS.get(), 3);
                assert_eq!(calls(), 1);
                assert_eq!(handles(), baseline);
            }
        }
        match index {
            0 => main.resize(DATA.len() as u64),
            1 => wal.resize(WAL_DATA.len() as u64),
            _ => shm.resize(SHM_DATA.len() as u64),
        }
    }
    for source in [&mut main, &mut wal, &mut shm] {
        source.unchanged();
    }
}

fn database_errors_cancellation() {
    let mut main = Source::new(7);
    let mut wal = Source::with_bytes(7, WAL_DATA);
    let mut shm = Source::with_bytes(7, SHM_DATA);
    let input = DatabaseInput::new(&main, Some(&wal), Some(&shm));
    let baseline = handles();
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
        let mut out = [0xa5; 96];
        let mut required = usize::MAX;
        let expected = if let Mode::Error(error) = mode {
            expected_status(error)
        } else {
            QL_ERROR_INTERNAL
        };
        assert_eq!(
            // SAFETY: All inputs/outputs live; actual FFI boundary must contain a provider unwind.
            unsafe { input.call(out.as_mut_ptr(), out.len(), &mut required, None) },
            expected
        );
        assert_eq!(required, 0);
        assert_eq!(out, [0xa5; 96]);
        assert_eq!(calls(), 1);
        assert_eq!(REOPENS.get(), 3);
        assert_eq!(handles(), baseline);
    }
    for (callback, reopens, readers) in [
        (pre_cancel as CancelCallback, 0, 0),
        (cancel_on_second_poll as CancelCallback, 0, 0),
        (after_reopen as CancelCallback, 1, 0),
        (after_reader as CancelCallback, 3, 1),
    ] {
        reset(Mode::Success);
        let mut out = [0xa5; 96];
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Static callbacks do not unwind; live buffers and borrowed HANDLEs are disjoint.
            unsafe { input.call(out.as_mut_ptr(), out.len(), &mut required, Some(callback)) },
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(out, [0xa5; 96]);
        assert_eq!(REOPENS.get(), reopens);
        assert_eq!(calls(), readers);
        assert_eq!(handles(), baseline);
    }
    // A post-render cancellation wins over every returned typed provider failure.
    for error in [
        ReaderPreviewError::Cancelled,
        ReaderPreviewError::Io,
        ReaderPreviewError::Malformed,
        ReaderPreviewError::LengthMismatch,
        ReaderPreviewError::LimitExceeded,
    ] {
        reset(Mode::Error(error));
        let mut out = [0xa5; 96];
        let mut required = usize::MAX;
        assert_eq!(
            // SAFETY: Borrowed inputs and outputs are live/disjoint; static callback does not unwind.
            unsafe {
                input.call(
                    out.as_mut_ptr(),
                    out.len(),
                    &mut required,
                    Some(after_reader),
                )
            },
            QL_ERROR_CANCELLED
        );
        assert_eq!(required, 0);
        assert_eq!(out, [0xa5; 96]);
        assert_eq!(REOPENS.get(), 3);
        assert_eq!(calls(), 1);
        assert_eq!(handles(), baseline);
    }
    reset(Mode::Success);
    let invalid = DatabaseInput {
        main: Input {
            raw: 0,
            name: null(),
            name_len: 0,
            ..input.main
        },
        wal: (0, 1),
        ..input
    };
    let mut required = usize::MAX;
    assert_eq!(
        // SAFETY: Pre-cancellation rejects invalid inputs before any access or reopen.
        unsafe { invalid.call(null_mut(), 0, &mut required, Some(pre_cancel)) },
        QL_ERROR_CANCELLED
    );
    assert_eq!(required, 0);
    assert_eq!(REOPENS.get(), 0);
    reset(Mode::Success);
    assert_eq!(
        // SAFETY: Fresh call uses still-live caller sources after all failures, cancellations and panic.
        unsafe { input.call(null_mut(), 0, &mut required, None) },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    for source in [&mut main, &mut wal, &mut shm] {
        source.unchanged();
    }
    assert_eq!(handles(), baseline);
}
