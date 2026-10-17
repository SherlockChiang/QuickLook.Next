//! Thread-affine ownership of one successful COM initialization.

use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

pub(crate) struct StaApartment {
    // COM initialization belongs to this thread; the guard must never be Send or Sync.
    _thread: PhantomData<Rc<()>>,
}

impl StaApartment {
    pub(crate) fn initialize() -> windows::core::Result<Self> {
        // SAFETY: No reserved pointer is supplied. Each successful S_OK/S_FALSE initialization
        // is balanced by this non-Send guard's Drop on the initializing thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self {
            _thread: PhantomData,
        })
    }
}

impl Drop for StaApartment {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after successful initialization, cannot move across
        // threads, and uniquely owns the obligation to uninitialize once.
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::StaApartment;
    use windows::Win32::Foundation::{CO_E_NOTINITIALIZED, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{
        CoGetApartmentType, CoInitializeEx, CoUninitialize, APTTYPEQUALIFIER_NONE, APTTYPE_CURRENT,
        APTTYPE_MTA, COINIT_MULTITHREADED,
    };

    // Avoid these two tests creating an implicit process MTA for one another's fresh threads.
    static COM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn apartment_type() -> windows::core::Result<windows::Win32::System::Com::APTTYPE> {
        let mut kind = APTTYPE_CURRENT;
        let mut qualifier = APTTYPEQUALIFIER_NONE;
        // SAFETY: The typed local output slots are aligned, writable and disjoint.
        unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }?;
        Ok(kind)
    }

    #[test]
    fn nested_sta_scopes_balance_success_and_s_false() {
        let _test_lock = COM_TEST_LOCK.lock().unwrap();
        std::thread::spawn(|| {
            assert_eq!(apartment_type().unwrap_err().code(), CO_E_NOTINITIALIZED);
            let first = StaApartment::initialize().unwrap();
            let second = StaApartment::initialize().unwrap();
            assert!(apartment_type().is_ok());
            drop(first);
            assert!(
                apartment_type().is_ok(),
                "nested initialization must remain active"
            );
            drop(second);
            assert_eq!(apartment_type().unwrap_err().code(), CO_E_NOTINITIALIZED);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn rejected_sta_initialization_does_not_uninitialize_existing_mta() {
        let _test_lock = COM_TEST_LOCK.lock().unwrap();
        std::thread::spawn(|| {
            // SAFETY: Initialize this fresh thread; the successful call is balanced below.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .unwrap();
            let error = match StaApartment::initialize() {
                Ok(_) => panic!("STA must not replace an initialized MTA"),
                Err(error) => error,
            };
            assert_eq!(error.code(), RPC_E_CHANGED_MODE);
            assert_eq!(apartment_type().unwrap(), APTTYPE_MTA);
            // SAFETY: Balance this thread's one successful MTA initialization only.
            unsafe { CoUninitialize() };
            assert_eq!(apartment_type().unwrap_err().code(), CO_E_NOTINITIALIZED);
        })
        .join()
        .unwrap();
    }
}
