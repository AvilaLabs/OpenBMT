//! The JNI glue: the one exported symbol of the library and the only
//! `unsafe` of the crate.
//!
//! ```text
//! Java_org_openbmt_owner_Native_call(env, class, request: jbyteArray)
//!     -> jbyteArray
//! ```
//!
//! It copies the request out of the Java array, runs
//! [`dispatch`](crate::dispatch) on the process's registry and returns the
//! response as a new Java array. Unlike the Key app's glue it calls no
//! method of a Java object: the owner node keeps its files itself, in the data
//! folder the app names.
//!
//! The function may be called from more than one thread at once (the app's
//! pump thread and its screen thread). It keeps nothing between calls but
//! the registry: there are no global references, no cached `JNIEnv` and no
//! threads of its own, and each call uses only the environment it was given.
//! After every JNI call that can leave an exception pending, the exception
//! is cleared and becomes an error response. A null from any JNI function
//! is handled and never dereferenced. Every `unsafe` block is one JNI call
//! (or one read of the function table) with its reason on the line before.

#![allow(unsafe_code)]

use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;
use std::sync::OnceLock;

use jni_sys::{jbyte, jbyteArray, jclass, jsize, JNIEnv, JNINativeInterface_, JNI_TRUE};

use crate::call::{dispatch, error_response, Registry};
use crate::codec::MAX_REQUEST;
use crate::error::{refused_by, AppError};

/// The registry of the process, used by the exported function.
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

/// The JNI function table of one call, copied out of the `JNIEnv`.
#[derive(Clone, Copy)]
struct Env {
    env: *mut JNIEnv,
    table: JNINativeInterface_,
}

impl Env {
    /// The table behind `env`, or `None` when `env` or the table is null.
    fn new(env: *mut JNIEnv) -> Option<Self> {
        if env.is_null() {
            return None;
        }
        // SAFETY: `env` is the non-null pointer the JVM passed to the native
        // method; it points at the pointer to the function table, which the
        // JVM keeps valid and unchanged while the call runs.
        let table_ptr = unsafe { *env };
        if table_ptr.is_null() {
            return None;
        }
        // SAFETY: `table_ptr` is non-null and is the JVM's function table
        // (see above); copying it reads `size_of::<JNINativeInterface_>()`
        // bytes that the JVM filled in.
        let table = unsafe { *table_ptr };
        Some(Self { env, table })
    }

    fn array_length(&self, array: jbyteArray) -> Option<usize> {
        let f = self.table.GetArrayLength?;
        // SAFETY: `array` is a live reference to a Java array (the argument
        // of the call) and `self.env` is this thread's environment.
        let n = unsafe { f(self.env, array) };
        usize::try_from(n).ok()
    }

    /// Copy the `len` bytes of `array` out, or `None` if the call fails.
    fn read_bytes(&self, array: jbyteArray, len: usize) -> Option<Vec<u8>> {
        let f = self.table.GetByteArrayRegion?;
        let count = jsize::try_from(len).ok()?;
        let mut buf = vec![0u8; len];
        // SAFETY: `array` is a live byte array of at least `len` elements
        // (its length was just read) and `buf` has room for `len` bytes.
        unsafe { f(self.env, array, 0, count, buf.as_mut_ptr().cast::<jbyte>()) };
        Some(buf)
    }

    fn new_bytes(&self, bytes: &[u8]) -> Option<jbyteArray> {
        let new = self.table.NewByteArray?;
        let set = self.table.SetByteArrayRegion?;
        let count = jsize::try_from(bytes.len()).ok()?;
        // SAFETY: `self.env` is this thread's environment; the length is
        // non-negative. The result may be null (out of memory), checked next.
        let array = unsafe { new(self.env, count) };
        if array.is_null() {
            return None;
        }
        // SAFETY: `array` is the live array of `count` elements just made,
        // and `bytes` holds `count` bytes.
        unsafe { set(self.env, array, 0, count, bytes.as_ptr().cast::<jbyte>()) };
        Some(array)
    }

    /// Whether an exception is pending. A table without the function counts
    /// as one, so that nothing goes on after a failure that cannot be seen.
    fn exception_pending(&self) -> bool {
        let Some(f) = self.table.ExceptionCheck else {
            return true;
        };
        // SAFETY: `self.env` is this thread's environment.
        unsafe { f(self.env) == JNI_TRUE }
    }

    fn clear_exception(&self) {
        if let Some(f) = self.table.ExceptionClear {
            // SAFETY: `self.env` is this thread's environment.
            unsafe { f(self.env) };
        }
    }

    /// Check for a pending exception and clear it.
    fn threw(&self) -> bool {
        if self.exception_pending() {
            self.clear_exception();
            return true;
        }
        false
    }
}

/// The call: `request` is the Java array. The result is the response as a
/// new Java array, or null when not even that could be made (an
/// out-of-memory exception is then pending).
fn entry(registry: &Registry, env: Env, request: jbyteArray) -> jbyteArray {
    let respond = |e: AppError| env.new_bytes(&error_response(&e)).unwrap_or(null_mut());
    if request.is_null() {
        return respond(refused_by("request-null", "the request is null"));
    }
    let Some(len) = env.array_length(request) else {
        return respond(refused_by("request-length", "the request has no length"));
    };
    if len > MAX_REQUEST {
        return respond(refused_by(
            "request-too-long",
            format!("a request is at most {MAX_REQUEST} bytes"),
        ));
    }
    let bytes = env.read_bytes(request, len);
    if env.threw() {
        return respond(refused_by("request-read", "the request could not be read"));
    }
    let Some(mut bytes) = bytes else {
        return respond(refused_by("request-read", "the request could not be read"));
    };
    let response = dispatch(registry, &bytes);
    // The request may hold the owner secret and Key 1's account key.
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
    env.new_bytes(&response).unwrap_or(null_mut())
}

/// The one exported function of the library: `Native.call(request)` of
/// `org.openbmt.owner`. See the module documentation.
#[no_mangle]
pub extern "system" fn Java_org_openbmt_owner_Native_call(
    env: *mut JNIEnv,
    _class: jclass,
    request: jbyteArray,
) -> jbyteArray {
    let Some(env) = Env::new(env) else {
        return null_mut();
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| entry(registry(), env, request)))
        .unwrap_or(null_mut())
}

#[cfg(test)]
mod tests {
    //! The glue against a fake `JNIEnv`: a function table whose entries are
    //! `extern "system"` functions over state kept per test thread.

    use std::cell::RefCell;

    use jni_sys::{_jobject, jboolean, JNI_FALSE};

    use super::*;
    use crate::tests::kit::{start_request, Rig};
    use crate::tests::wire::frame;

    const ARRAY_BASE: usize = 0x1000;

    struct Fake {
        arrays: Vec<Vec<u8>>,
        pending: bool,
        clears: usize,
        new_array_fails: bool,
        region_throws: bool,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                arrays: Vec::new(),
                pending: false,
                clears: 0,
                new_array_fails: false,
                region_throws: false,
            }
        }
    }

    thread_local! {
        static FAKE: RefCell<Fake> = RefCell::new(Fake::new());
    }

    fn fake<T>(f: impl FnOnce(&mut Fake) -> T) -> T {
        FAKE.with(|s| f(&mut s.borrow_mut()))
    }

    fn array_handle(i: usize) -> jbyteArray {
        (ARRAY_BASE + i) as *mut _jobject
    }

    fn array_index(a: jbyteArray) -> Option<usize> {
        (a as usize).checked_sub(ARRAY_BASE)
    }

    fn new_array(bytes: &[u8]) -> jbyteArray {
        fake(|f| {
            f.arrays.push(bytes.to_vec());
            array_handle(f.arrays.len() - 1)
        })
    }

    unsafe extern "system" fn exception_check(_: *mut JNIEnv) -> jboolean {
        fake(|f| if f.pending { JNI_TRUE } else { JNI_FALSE })
    }

    unsafe extern "system" fn exception_clear(_: *mut JNIEnv) {
        fake(|f| {
            f.pending = false;
            f.clears += 1;
        })
    }

    unsafe extern "system" fn get_array_length(_: *mut JNIEnv, array: jbyteArray) -> jsize {
        fake(|f| {
            array_index(array)
                .and_then(|i| f.arrays.get(i))
                .map_or(-1, |a| jsize::try_from(a.len()).unwrap_or(-1))
        })
    }

    unsafe extern "system" fn get_byte_array_region(
        _: *mut JNIEnv,
        array: jbyteArray,
        start: jsize,
        len: jsize,
        buf: *mut jbyte,
    ) {
        let (data, throws) = fake(|f| {
            (
                array_index(array).and_then(|i| f.arrays.get(i)).cloned(),
                f.region_throws,
            )
        });
        let Some(data) = data.filter(|_| !throws) else {
            fake(|f| f.pending = true);
            return;
        };
        let start = usize::try_from(start).unwrap_or(0);
        let len = usize::try_from(len).unwrap_or(0);
        std::ptr::copy_nonoverlapping(data.as_ptr().add(start), buf.cast::<u8>(), len);
    }

    unsafe extern "system" fn new_byte_array(_: *mut JNIEnv, len: jsize) -> jbyteArray {
        fake(|f| {
            if f.new_array_fails {
                return null_mut();
            }
            f.arrays.push(vec![0; usize::try_from(len).unwrap_or(0)]);
            array_handle(f.arrays.len() - 1)
        })
    }

    unsafe extern "system" fn set_byte_array_region(
        _: *mut JNIEnv,
        array: jbyteArray,
        start: jsize,
        len: jsize,
        buf: *const jbyte,
    ) {
        let start = usize::try_from(start).unwrap_or(0);
        let len = usize::try_from(len).unwrap_or(0);
        let mut copy = vec![0u8; len];
        std::ptr::copy_nonoverlapping(buf.cast::<u8>(), copy.as_mut_ptr(), len);
        fake(|f| {
            if let Some(a) = array_index(array).and_then(|i| f.arrays.get_mut(i)) {
                a.splice(start..start + len, copy);
            }
        })
    }

    /// The fake environment: a table, and the pointer to it that a `JNIEnv*`
    /// is.
    struct Harness {
        _table: Box<JNINativeInterface_>,
        table_ptr: Box<*const JNINativeInterface_>,
    }

    impl Harness {
        fn new() -> Self {
            fake(|f| *f = Fake::new());
            // SAFETY: the table is a struct of raw pointers and `Option`s of
            // function pointers, for which all zeroes is the null value.
            let mut table: Box<JNINativeInterface_> = Box::new(unsafe { std::mem::zeroed() });
            table.ExceptionCheck = Some(exception_check);
            table.ExceptionClear = Some(exception_clear);
            table.GetArrayLength = Some(get_array_length);
            table.GetByteArrayRegion = Some(get_byte_array_region);
            table.NewByteArray = Some(new_byte_array);
            table.SetByteArrayRegion = Some(set_byte_array_region);
            let table_ptr: Box<*const JNINativeInterface_> = Box::new(&*table);
            Self {
                _table: table,
                table_ptr,
            }
        }

        fn env_ptr(&mut self) -> *mut JNIEnv {
            &mut *self.table_ptr
        }

        fn env(&mut self) -> Env {
            Env::new(self.env_ptr()).expect("a table")
        }

        /// Run the entry function on `request`; the response bytes, or
        /// `None` for a null result.
        fn call(&mut self, registry: &Registry, request: &[u8]) -> Option<Vec<u8>> {
            let array = new_array(request);
            let env = self.env();
            let result = entry(registry, env, array);
            self.read(result)
        }

        fn read(&self, array: jbyteArray) -> Option<Vec<u8>> {
            if array.is_null() {
                return None;
            }
            fake(|f| array_index(array).and_then(|i| f.arrays.get(i)).cloned())
        }
    }

    /// The status, and for an error the rule.
    fn reply(response: &[u8]) -> (u8, String) {
        assert_eq!(response.get(..4), Some(b"OBMl".as_slice()));
        let status = response[5];
        if status == 0 {
            return (0, String::new());
        }
        let n = usize::from(u16::from_le_bytes([response[6], response[7]]));
        let rule = String::from_utf8(response[8..8 + n].to_vec()).expect("UTF-8");
        (status, rule)
    }

    #[test]
    fn a_call_goes_through_the_glue_and_back() {
        let mut h = Harness::new();
        let rig = Rig::new();
        let registry = rig.registry();
        let response = h
            .call(registry, &start_request(&rig.dir()))
            .expect("a response");
        assert_eq!(reply(&response).0, 0);
        let handle = u64::from_le_bytes(response[6..14].try_into().expect("8"));
        let status = h
            .call(registry, &frame(20, handle, &[]))
            .expect("a response");
        assert_eq!(reply(&status).0, 0);
        let stop = h
            .call(registry, &frame(2, handle, &[]))
            .expect("a response");
        assert_eq!(reply(&stop).0, 0);
        // The node is gone.
        let after = h
            .call(registry, &frame(20, handle, &[]))
            .expect("a response");
        assert_eq!(reply(&after), (1, "handle-unknown".to_string()));
    }

    #[test]
    fn errors_of_the_glue_are_frames_with_their_rules() {
        let mut h = Harness::new();
        let registry = Registry::new();
        // A null request.
        let env = h.env();
        let result = entry(&registry, env, null_mut());
        let bytes = h.read(result).expect("a response");
        assert_eq!(reply(&bytes), (1, "request-null".to_string()));
        // An array the JVM cannot read.
        fake(|f| f.region_throws = true);
        let bytes = h.call(&registry, &frame(20, 1, &[])).expect("a response");
        assert_eq!(reply(&bytes), (1, "request-read".to_string()));
        assert!(fake(|f| f.clears) >= 1);
        assert!(!fake(|f| f.pending));
        fake(|f| f.region_throws = false);
        // An array that is not an array.
        let env = h.env();
        let result = entry(&registry, env, array_handle(9_999));
        let bytes = h.read(result).expect("a response");
        assert_eq!(reply(&bytes), (1, "request-length".to_string()));
        // Over 9 MiB.
        let big = vec![0u8; MAX_REQUEST + 1];
        let bytes = h.call(&registry, &big).expect("a response");
        assert_eq!(reply(&bytes), (1, "request-too-long".to_string()));
    }

    #[test]
    fn no_array_for_the_response_gives_null() {
        let mut h = Harness::new();
        let registry = Registry::new();
        fake(|f| f.new_array_fails = true);
        assert!(h.call(&registry, &frame(20, 1, &[])).is_none());
    }

    #[test]
    fn a_table_that_cannot_report_an_exception_stops_the_call() {
        let h = Harness::new();
        let registry = Registry::new();
        // Without ExceptionCheck the glue must treat the read as failed.
        let mut table = *h._table;
        table.ExceptionCheck = None;
        let mut env_ptr: *const JNINativeInterface_ = &table;
        let env = Env::new(&mut env_ptr).expect("a table");
        let array = new_array(&frame(20, 1, &[]));
        let result = entry(&registry, env, array);
        let bytes = h.read(result).expect("a response");
        assert_eq!(reply(&bytes), (1, "request-read".to_string()));
    }

    #[test]
    fn the_exported_symbol_runs_the_same_path() {
        let mut h = Harness::new();
        let array = new_array(&frame(20, 777, &[]));
        let result = Java_org_openbmt_owner_Native_call(h.env_ptr(), null_mut(), array);
        let response = h.read(result).expect("a response");
        assert_eq!(reply(&response), (1, "handle-unknown".to_string()));
        // A null environment gives null.
        let result = Java_org_openbmt_owner_Native_call(null_mut(), null_mut(), array);
        assert!(result.is_null());
    }
}
