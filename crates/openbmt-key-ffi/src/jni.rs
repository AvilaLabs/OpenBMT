//! The JNI glue: the one exported symbol of the library and the only
//! `unsafe` of the crate.
//!
//! ```text
//! Java_org_openbmt_key_Native_call(env, class, store: jobject,
//!                                  request: jbyteArray) -> jbyteArray
//! ```
//!
//! It copies the request out of the Java array, wraps `store` in a
//! [`JniStore`] that implements [`KeyStore`] by calling the Java object's
//! `load(): ByteArray?`, `save(ByteArray)`, `random(Int): ByteArray` and
//! `now(): Long` through the JNI function table, runs
//! [`dispatch`](crate::dispatch) and returns the response as a new Java
//! array.
//!
//! The store object is used only during the call and on the calling thread:
//! there are no global references, no cached `JNIEnv` and no threads. After
//! every JNI call that can leave an exception pending, the exception is
//! cleared and becomes a [`StoreError`]. A null from any JNI function is
//! handled and never dereferenced. Every `unsafe` block is one JNI call (or
//! one read of the function table) with its reason on the line before.

#![allow(unsafe_code)]

use std::ffi::CStr;
use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;

use jni_sys::{
    jbyte, jbyteArray, jclass, jint, jmethodID, jobject, jsize, jvalue, JNIEnv,
    JNINativeInterface_, JNI_TRUE,
};

use crate::call::{dispatch, error_response, Registry};
use crate::codec::MAX_REQUEST;
use crate::error::{refused_by, KeyError, StoreError};
use crate::store::KeyStore;

/// The most bytes read back from the store object in one call: far more
/// than the state or any random draw, and a bound on what a wrong object
/// can make this side allocate.
const MAX_STORE_BYTES: usize = 16 << 20;

/// The registry of the process, used by the exported function.
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

fn failed(text: impl Into<String>) -> StoreError {
    StoreError::Failed { text: text.into() }
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
        // of the call, or one returned by Java in this call) and `self.env`
        // is this thread's environment.
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

    fn object_class(&self, obj: jobject) -> Option<jclass> {
        let f = self.table.GetObjectClass?;
        // SAFETY: `obj` is a live reference passed by the JVM and `self.env`
        // is this thread's environment.
        let class = unsafe { f(self.env, obj) };
        (!class.is_null()).then_some(class)
    }

    fn method_id(&self, class: jclass, name: &CStr, sig: &CStr) -> Option<jmethodID> {
        let f = self.table.GetMethodID?;
        // SAFETY: `class` is a live class reference and `name` and `sig`
        // are NUL-terminated strings that outlive the call.
        let id = unsafe { f(self.env, class, name.as_ptr(), sig.as_ptr()) };
        (!id.is_null()).then_some(id)
    }

    fn call_object(&self, obj: jobject, id: jmethodID, args: &[jvalue]) -> Option<jobject> {
        let f = self.table.CallObjectMethodA?;
        // SAFETY: `id` was found on the class of `obj` with a signature that
        // takes exactly the values in `args`, in the types they hold.
        let result = unsafe { f(self.env, obj, id, args.as_ptr()) };
        Some(result)
    }

    fn call_void(&self, obj: jobject, id: jmethodID, args: &[jvalue]) -> Option<()> {
        let f = self.table.CallVoidMethodA?;
        // SAFETY: as `call_object`: `id` takes exactly the values in `args`.
        unsafe { f(self.env, obj, id, args.as_ptr()) };
        Some(())
    }

    fn call_long(&self, obj: jobject, id: jmethodID) -> Option<i64> {
        let f = self.table.CallLongMethodA?;
        // SAFETY: `id` is a method of `obj` that takes no arguments and
        // returns a long; the argument pointer is not read for no arguments.
        let result = unsafe { f(self.env, obj, id, std::ptr::null()) };
        Some(result)
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

    fn delete_local(&self, obj: jobject) {
        if obj.is_null() {
            return;
        }
        if let Some(f) = self.table.DeleteLocalRef {
            // SAFETY: `obj` is a local reference made in this call and is not
            // used after this.
            unsafe { f(self.env, obj) };
        }
    }
}

/// A Java object that implements the app's `KeyStore`, for the length of one
/// call on one thread.
struct JniStore {
    env: Env,
    obj: jobject,
    class: jclass,
    load: jmethodID,
    save: jmethodID,
    random: jmethodID,
    now: jmethodID,
    thread: ThreadId,
    closed: AtomicBool,
}

// SAFETY: a `JniStore` is created, used and closed inside one call of the
// exported function, on the thread of that call. Every method checks the
// thread and the closed flag before it touches the environment or the
// object, so no other thread reaches them and nothing is used after the call
// has returned. The bounds only let the value sit in an `Arc<dyn KeyStore>`.
unsafe impl Send for JniStore {}
// SAFETY: see the `Send` impl above.
unsafe impl Sync for JniStore {}

impl JniStore {
    fn new(env: Env, obj: jobject) -> Result<Self, KeyError> {
        let missing = |what: &str| KeyError::Store {
            rule: "store".to_string(),
            text: format!("the app's store object has no usable {what}"),
        };
        if obj.is_null() {
            return Err(missing("object (it is null)"));
        }
        let class = env
            .object_class(obj)
            .ok_or_else(|| missing("class (GetObjectClass failed)"))?;
        let find = |name: &CStr, sig: &CStr, what: &str| {
            let id = env.method_id(class, name, sig);
            // A method that is not there leaves NoSuchMethodError pending.
            env.clear_exception();
            id.ok_or_else(|| missing(what))
        };
        let ids = (
            find(c"load", c"()[B", "load(): ByteArray?"),
            find(c"save", c"([B)V", "save(ByteArray)"),
            find(c"random", c"(I)[B", "random(Int): ByteArray"),
            find(c"now", c"()J", "now(): Long"),
        );
        match ids {
            (Ok(load), Ok(save), Ok(random), Ok(now)) => Ok(Self {
                env,
                obj,
                class,
                load,
                save,
                random,
                now,
                thread: std::thread::current().id(),
                closed: AtomicBool::new(false),
            }),
            (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
                env.delete_local(class);
                Err(e)
            }
        }
    }

    /// After this no method touches the environment or the object.
    fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) && std::thread::current().id() == self.thread {
            self.env.delete_local(self.class);
        }
    }

    fn usable(&self) -> Result<(), StoreError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(failed("the store object is only usable during its call"));
        }
        if std::thread::current().id() != self.thread {
            return Err(failed(
                "the store object is only usable on the calling thread",
            ));
        }
        Ok(())
    }

    /// The bytes of a Java array that a method returned; the local
    /// reference is released.
    fn take_array(&self, array: jbyteArray, what: &str) -> Result<Vec<u8>, StoreError> {
        let len = self.env.array_length(array);
        let bytes = len
            .filter(|n| *n <= MAX_STORE_BYTES)
            .and_then(|n| self.env.read_bytes(array, n));
        let threw = self.env.threw();
        self.env.delete_local(array);
        match bytes {
            Some(b) if !threw => Ok(b),
            _ => Err(failed(format!(
                "the bytes {what} returned could not be read"
            ))),
        }
    }

    fn threw(&self, what: &str) -> StoreError {
        failed(format!("the app's store threw an exception in {what}"))
    }
}

impl KeyStore for JniStore {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.usable()?;
        let result = self.env.call_object(self.obj, self.load, &[]);
        if self.env.threw() {
            self.env.delete_local(result.unwrap_or(null_mut()));
            return Err(self.threw("load"));
        }
        match result {
            None => Err(failed("the JNI environment cannot call load")),
            Some(array) if array.is_null() => Ok(None),
            Some(array) => self.take_array(array, "load").map(Some),
        }
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        self.usable()?;
        let array = self
            .env
            .new_bytes(&bytes)
            .ok_or_else(|| failed("a Java array for the bytes could not be made"))?;
        let arg = jvalue { l: array };
        let called = self.env.call_void(self.obj, self.save, &[arg]);
        let threw = self.env.threw();
        self.env.delete_local(array);
        match called {
            None => Err(failed("the JNI environment cannot call save")),
            Some(()) if threw => Err(self.threw("save")),
            Some(()) => Ok(()),
        }
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        self.usable()?;
        let count = jint::try_from(n).map_err(|_| failed("too many random bytes asked for"))?;
        let result = self
            .env
            .call_object(self.obj, self.random, &[jvalue { i: count }]);
        if self.env.threw() {
            self.env.delete_local(result.unwrap_or(null_mut()));
            return Err(self.threw("random"));
        }
        match result {
            None => Err(failed("the JNI environment cannot call random")),
            Some(array) if array.is_null() => Err(failed("random returned null")),
            Some(array) => self.take_array(array, "random"),
        }
    }

    fn now(&self) -> u64 {
        if self.usable().is_err() {
            return 0;
        }
        let seconds = self.env.call_long(self.obj, self.now);
        if self.env.threw() {
            return 0;
        }
        seconds.and_then(|s| u64::try_from(s).ok()).unwrap_or(0)
    }
}

/// The call: `request` is the Java array, `store` the Java store object.
/// The result is the response as a new Java array, or null when not even
/// that could be made (an out-of-memory exception is then pending).
fn entry(registry: &Registry, env: Env, store: jobject, request: jbyteArray) -> jbyteArray {
    let respond = |e: KeyError| env.new_bytes(&error_response(&e)).unwrap_or(null_mut());
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
    let jni_store = match JniStore::new(env, store) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            bytes.fill(0);
            return respond(e);
        }
    };
    let response = dispatch(registry, jni_store.clone(), &bytes);
    // The request may hold typed words.
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
    jni_store.close();
    env.new_bytes(&response).unwrap_or(null_mut())
}

/// The one exported function of the library: `Native.call(store, request)`
/// of `org.openbmt.key`. See the module documentation.
#[no_mangle]
pub extern "system" fn Java_org_openbmt_key_Native_call(
    env: *mut JNIEnv,
    _class: jclass,
    store: jobject,
    request: jbyteArray,
) -> jbyteArray {
    let Some(env) = Env::new(env) else {
        return null_mut();
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| entry(registry(), env, store, request)))
        .unwrap_or(null_mut())
}

#[cfg(test)]
mod tests {
    //! The glue against a fake `JNIEnv`: a function table whose entries are
    //! `extern "system"` functions over state kept per test thread.

    use std::cell::RefCell;
    use std::ffi::c_char;

    use jni_sys::{_jmethodID, _jobject, jboolean, JNI_FALSE};

    use super::*;
    use crate::envelope;
    use crate::tests::wire::{frame, W};
    use crate::{EntropySource, KeyNetwork};

    const STORE_OBJ: usize = 1;
    const CLASS: usize = 2;
    const ARRAY_BASE: usize = 0x1000;

    const LOAD: usize = 1;
    const SAVE: usize = 2;
    const RANDOM: usize = 3;
    const NOW: usize = 4;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Normal,
        Throw,
        Null,
        Short,
        Long,
    }

    struct Fake {
        arrays: Vec<Vec<u8>>,
        pending: bool,
        /// References the glue made and has not released.
        live: i32,
        clears: usize,
        stored: Option<Vec<u8>>,
        drawn: u8,
        load: Mode,
        save: Mode,
        random: Mode,
        now: Mode,
        now_value: i64,
        new_array_fails: bool,
        no_class: bool,
        missing_method: Option<&'static str>,
        region_throws: bool,
        calls: Vec<&'static str>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                arrays: Vec::new(),
                pending: false,
                live: 0,
                clears: 0,
                stored: None,
                drawn: 0,
                load: Mode::Normal,
                save: Mode::Normal,
                random: Mode::Normal,
                now: Mode::Normal,
                now_value: 1_700_000_000,
                new_array_fails: false,
                no_class: false,
                missing_method: None,
                region_throws: false,
                calls: Vec::new(),
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

    /// A new array. `by_glue` arrays count as references to release.
    fn new_array(bytes: &[u8], by_glue: bool) -> jbyteArray {
        fake(|f| {
            f.arrays.push(bytes.to_vec());
            if by_glue {
                f.live += 1;
            }
            array_handle(f.arrays.len() - 1)
        })
    }

    unsafe extern "system" fn get_object_class(_: *mut JNIEnv, _: jobject) -> jclass {
        fake(|f| {
            if f.no_class {
                null_mut()
            } else {
                f.live += 1;
                CLASS as *mut _jobject
            }
        })
    }

    unsafe extern "system" fn get_method_id(
        _: *mut JNIEnv,
        _: jclass,
        name: *const c_char,
        sig: *const c_char,
    ) -> jmethodID {
        let name = CStr::from_ptr(name).to_str().unwrap_or("");
        let sig = CStr::from_ptr(sig).to_str().unwrap_or("");
        fake(|f| {
            let id = match (name, sig) {
                ("load", "()[B") => LOAD,
                ("save", "([B)V") => SAVE,
                ("random", "(I)[B") => RANDOM,
                ("now", "()J") => NOW,
                _ => 0,
            };
            if id == 0 || f.missing_method == Some(name) {
                f.pending = true;
                return null_mut();
            }
            id as *mut _jmethodID
        })
    }

    unsafe extern "system" fn call_object(
        _: *mut JNIEnv,
        obj: jobject,
        id: jmethodID,
        args: *const jvalue,
    ) -> jobject {
        assert_eq!(obj as usize, STORE_OBJ);
        let id = id as usize;
        let count = if id == RANDOM { (*args).i } else { 0 };
        fake(|f| {
            f.calls.push(if id == LOAD { "load" } else { "random" });
            let mode = if id == LOAD { f.load } else { f.random };
            let bytes: Vec<u8> = match mode {
                Mode::Throw => {
                    f.pending = true;
                    return null_mut();
                }
                Mode::Null => return null_mut(),
                _ if id == LOAD => match &f.stored {
                    None => return null_mut(),
                    Some(b) => b.clone(),
                },
                Mode::Normal | Mode::Short | Mode::Long => {
                    let n = usize::try_from(count).unwrap_or(0);
                    let n = match mode {
                        Mode::Short => n - 1,
                        Mode::Long => n + 1,
                        _ => n,
                    };
                    (0..n)
                        .map(|_| {
                            f.drawn = f.drawn.wrapping_add(1);
                            f.drawn
                        })
                        .collect()
                }
            };
            f.arrays.push(bytes);
            f.live += 1;
            array_handle(f.arrays.len() - 1)
        })
    }

    unsafe extern "system" fn call_void(
        _: *mut JNIEnv,
        _: jobject,
        id: jmethodID,
        args: *const jvalue,
    ) {
        assert_eq!(id as usize, SAVE);
        let array = (*args).l;
        fake(|f| {
            f.calls.push("save");
            if f.save == Mode::Throw {
                f.pending = true;
                return;
            }
            let bytes = array_index(array).and_then(|i| f.arrays.get(i)).cloned();
            f.stored = bytes;
        })
    }

    unsafe extern "system" fn call_long(
        _: *mut JNIEnv,
        _: jobject,
        id: jmethodID,
        _: *const jvalue,
    ) -> i64 {
        assert_eq!(id as usize, NOW);
        fake(|f| {
            f.calls.push("now");
            if f.now == Mode::Throw {
                f.pending = true;
                return 0;
            }
            f.now_value
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

    unsafe extern "system" fn delete_local_ref(_: *mut JNIEnv, _: jobject) {
        fake(|f| f.live -= 1)
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
            f.live += 1;
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
            table.GetObjectClass = Some(get_object_class);
            table.GetMethodID = Some(get_method_id);
            table.CallObjectMethodA = Some(call_object);
            table.CallVoidMethodA = Some(call_void);
            table.CallLongMethodA = Some(call_long);
            table.ExceptionCheck = Some(exception_check);
            table.ExceptionClear = Some(exception_clear);
            table.DeleteLocalRef = Some(delete_local_ref);
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
            let array = new_array(request, false);
            let env = self.env();
            let result = entry(registry, env, STORE_OBJ as jobject, array);
            self.read(result)
        }

        fn read(&self, array: jbyteArray) -> Option<Vec<u8>> {
            if array.is_null() {
                return None;
            }
            fake(|f| array_index(array).and_then(|i| f.arrays.get(i)).cloned())
        }
    }

    /// The status, and for an error the rule and text.
    fn reply(response: &[u8]) -> (u8, String, String) {
        assert_eq!(response.get(..4), Some(b"OBM9".as_slice()));
        let status = response[5];
        if status == 0 {
            return (0, String::new(), String::new());
        }
        let n = usize::from(u16::from_le_bytes([response[6], response[7]]));
        let rule = String::from_utf8(response[8..8 + n].to_vec()).expect("UTF-8");
        let rest = &response[8 + n..];
        let m = usize::from(u16::from_le_bytes([rest[0], rest[1]]));
        let text = String::from_utf8(rest[2..2 + m].to_vec()).expect("UTF-8");
        (status, rule, text)
    }

    fn open() -> Vec<u8> {
        frame(1, 0, &[])
    }

    fn create(app: u64) -> Vec<u8> {
        frame(
            11,
            app,
            &W::new()
                .source(&EntropySource::Platform)
                .network(KeyNetwork::Regtest)
                .0,
        )
    }

    fn state(app: u64) -> Vec<u8> {
        frame(10, app, &[])
    }

    fn set_pin(app: u64) -> Vec<u8> {
        frame(91, app, &W::new().string("482913").0)
    }

    fn unlock(app: u64) -> Vec<u8> {
        frame(92, app, &W::new().string("482913").0)
    }

    #[test]
    fn a_call_goes_through_the_store_and_back() {
        let mut h = Harness::new();
        let reg = Registry::new();
        let response = h.call(&reg, &open()).expect("a response");
        assert_eq!(reply(&response).0, 0);
        assert_eq!(response.get(6..), Some(1u64.to_le_bytes().as_slice()));
        assert_eq!(fake(|f| f.calls.clone()), vec!["load"]);

        // The salt of the PIN's key, nothing stored.
        let response = h.call(&reg, &set_pin(1)).expect("a response");
        assert_eq!(reply(&response).0, 0);
        assert_eq!(fake(|f| f.calls.clone()), vec!["load", "random"]);

        let response = h.call(&reg, &create(1)).expect("a response");
        assert_eq!(reply(&response).0, 0);
        // 32 bytes for the identity and 32 for the words, the nonce, then
        // the save.
        assert_eq!(
            fake(|f| f.calls.clone()),
            vec!["load", "random", "random", "random", "random", "save"]
        );
        let stored = fake(|f| f.stored.clone()).expect("stored");
        crate::lock::decode(&stored).expect("the locked frame decodes");
        assert!(envelope::decode(&stored).is_err());

        // The state is read back, and a restart from the stored bytes gives
        // the same.
        let response = h.call(&reg, &state(1)).expect("a response");
        assert_eq!(reply(&response).0, 0);
        assert_eq!(response.get(6..), Some([1u8, 1, 1].as_slice()));
        let again = Registry::new();
        let response = h.call(&again, &open()).expect("a response");
        assert_eq!(reply(&response).0, 0);
        // Locked until the PIN is given.
        let response = h.call(&again, &state(1)).expect("a response");
        assert_eq!(response.get(6..), Some([4u8].as_slice()));
        let response = h.call(&again, &unlock(1)).expect("a response");
        assert_eq!(reply(&response).0, 0);
        let response = h.call(&again, &state(1)).expect("a response");
        assert_eq!(response.get(6..), Some([1u8, 1, 0].as_slice()));

        assert!(!fake(|f| f.pending));
    }

    #[test]
    fn the_references_the_glue_makes_are_released() {
        let mut h = Harness::new();
        let reg = Registry::new();
        // Each response is one reference that the caller owns.
        let mut responses = 0;
        for request in [open(), set_pin(1), create(1), state(1)] {
            h.call(&reg, &request).expect("a response");
            responses += 1;
        }
        assert_eq!(fake(|f| f.live), responses);
    }

    #[test]
    fn the_time_comes_from_the_store() {
        let mut h = Harness::new();
        let store = JniStore::new(h.env(), STORE_OBJ as jobject).expect("a store");
        assert_eq!(store.now(), 1_700_000_000);
        fake(|f| f.now_value = -5);
        assert_eq!(store.now(), 0);
        store.close();
    }

    #[test]
    fn a_java_exception_in_each_method_becomes_a_store_error() {
        // load
        let mut h = Harness::new();
        fake(|f| f.load = Mode::Throw);
        let response = h.call(&Registry::new(), &open()).expect("a response");
        let (status, rule, text) = reply(&response);
        assert_eq!((status, rule.as_str()), (2, "load"));
        assert!(text.contains("exception in load"));
        assert!(!fake(|f| f.pending));
        assert!(fake(|f| f.clears) >= 1);

        // random
        let mut h = Harness::new();
        let reg = Registry::new();
        h.call(&reg, &open()).expect("a response");
        h.call(&reg, &set_pin(1)).expect("a response");
        fake(|f| f.random = Mode::Throw);
        let response = h.call(&reg, &create(1)).expect("a response");
        let (status, rule, text) = reply(&response);
        assert_eq!((status, rule.as_str()), (2, "AS-34"));
        assert!(text.contains("exception in random"));
        assert!(!fake(|f| f.pending));
        assert!(fake(|f| f.stored.is_none()));

        // save: nothing is stored and the app stays empty.
        let mut h = Harness::new();
        let reg = Registry::new();
        h.call(&reg, &open()).expect("a response");
        h.call(&reg, &set_pin(1)).expect("a response");
        fake(|f| f.save = Mode::Throw);
        let response = h.call(&reg, &create(1)).expect("a response");
        let (status, rule, text) = reply(&response);
        assert_eq!((status, rule.as_str()), (2, "AS-34"));
        assert!(text.contains("exception in save"));
        assert!(!fake(|f| f.pending));
        assert!(fake(|f| f.stored.is_none()));
        let response = h.call(&reg, &state(1)).expect("a response");
        assert_eq!(response.get(6..), Some([0u8].as_slice()));
        // The same call works once the store does.
        fake(|f| f.save = Mode::Normal);
        let response = h.call(&reg, &create(1)).expect("a response");
        assert_eq!(reply(&response).0, 0);

        // now: the exception is cleared and the clock reads 0.
        let mut h = Harness::new();
        let store = JniStore::new(h.env(), STORE_OBJ as jobject).expect("a store");
        fake(|f| f.now = Mode::Throw);
        assert_eq!(store.now(), 0);
        assert!(!fake(|f| f.pending));
        store.close();
    }

    #[test]
    fn nulls_and_wrong_lengths_are_handled() {
        // load returns null: an empty app.
        let mut h = Harness::new();
        fake(|f| f.load = Mode::Null);
        let reg = Registry::new();
        assert_eq!(reply(&h.call(&reg, &open()).expect("a response")).0, 0);
        assert_eq!(
            h.call(&reg, &state(1)).expect("a response").get(6..),
            Some([0u8].as_slice())
        );

        // random returns null, too few and too many bytes.
        for mode in [Mode::Null, Mode::Short, Mode::Long] {
            let mut h = Harness::new();
            let reg = Registry::new();
            h.call(&reg, &open()).expect("a response");
            h.call(&reg, &set_pin(1)).expect("a response");
            fake(|f| f.random = mode);
            let (status, rule, _) = reply(&h.call(&reg, &create(1)).expect("a response"));
            assert_eq!(status, 2);
            if mode == Mode::Null {
                assert_eq!(rule, "AS-34");
            } else {
                assert_eq!(rule, "random");
            }
            assert!(fake(|f| f.stored.is_none()));
        }

        // A null for the request, and a request that is too long.
        let mut h = Harness::new();
        let reg = Registry::new();
        let env = h.env();
        let response = h
            .read(entry(&reg, env, STORE_OBJ as jobject, null_mut()))
            .expect("a response");
        assert_eq!(reply(&response).1, "request-null");
        let big = vec![0u8; MAX_REQUEST + 1];
        let response = h.call(&reg, &big).expect("a response");
        assert_eq!(reply(&response).1, "request-too-long");

        // A request that cannot be read.
        fake(|f| f.region_throws = true);
        let response = h.call(&reg, &open()).expect("a response");
        assert_eq!(reply(&response).1, "request-read");
        assert!(!fake(|f| f.pending));
    }

    #[test]
    fn a_store_object_without_the_methods_is_refused() {
        // No class.
        let mut h = Harness::new();
        fake(|f| f.no_class = true);
        let response = h.call(&Registry::new(), &open()).expect("a response");
        assert_eq!(reply(&response).0, 2);
        assert_eq!(reply(&response).1, "store");

        // Each method missing: the pending exception is cleared and the
        // class reference released.
        for name in ["load", "save", "random", "now"] {
            let mut h = Harness::new();
            fake(|f| f.missing_method = Some(name));
            let response = h.call(&Registry::new(), &open()).expect("a response");
            let (status, rule, text) = reply(&response);
            assert_eq!((status, rule.as_str()), (2, "store"));
            assert!(text.contains(name), "{text}");
            assert!(!fake(|f| f.pending));
            // Only the response is left.
            assert_eq!(fake(|f| f.live), 1);
        }

        // A null store object.
        let mut h = Harness::new();
        let env = h.env();
        let array = new_array(&open(), false);
        let response = h
            .read(entry(&Registry::new(), env, null_mut(), array))
            .expect("a response");
        assert_eq!(reply(&response).0, 2);
    }

    #[test]
    fn no_array_for_the_response_gives_null_and_no_array_for_save_fails() {
        let mut h = Harness::new();
        let reg = Registry::new();
        h.call(&reg, &open()).expect("a response");
        fake(|f| f.new_array_fails = true);
        // The response cannot be made: null, with nothing cleared.
        assert!(h.call(&reg, &state(1)).is_none());
        // The save cannot make its array: nothing is stored.
        fake(|f| f.new_array_fails = false);
        let store = JniStore::new(h.env(), STORE_OBJ as jobject).expect("a store");
        fake(|f| f.new_array_fails = true);
        assert!(store.save(vec![1, 2, 3]).is_err());
        assert!(fake(|f| f.stored.is_none()));
        store.close();
    }

    #[test]
    fn a_closed_store_and_another_thread_touch_nothing() {
        let mut h = Harness::new();
        let store = Arc::new(JniStore::new(h.env(), STORE_OBJ as jobject).expect("a store"));
        // Another thread: refused, and the fake is not reached.
        let other = store.clone();
        let result = std::thread::spawn(move || other.save(vec![1]).is_err() && other.now() == 0)
            .join()
            .expect("joined");
        assert!(result);
        store.close();
        assert!(store.load().is_err());
        assert!(store.random(4).is_err());
        assert_eq!(store.now(), 0);
        // Closing twice releases the class once.
        store.close();
        assert_eq!(fake(|f| f.live), 0);
    }

    #[test]
    fn the_exported_symbol_runs_the_same_path() {
        let mut h = Harness::new();
        let array = new_array(&open(), false);
        let result =
            Java_org_openbmt_key_Native_call(h.env_ptr(), null_mut(), STORE_OBJ as jobject, array);
        let response = h.read(result).expect("a response");
        assert_eq!(reply(&response).0, 0);
        // The process registry holds that app; close it again.
        let handle = u64::from_le_bytes(response[6..14].try_into().expect("8"));
        let array = new_array(&frame(2, handle, &[]), false);
        let result =
            Java_org_openbmt_key_Native_call(h.env_ptr(), null_mut(), STORE_OBJ as jobject, array);
        assert_eq!(reply(&h.read(result).expect("a response")).0, 0);
        // A null environment gives null.
        let result =
            Java_org_openbmt_key_Native_call(null_mut(), null_mut(), STORE_OBJ as jobject, array);
        assert!(result.is_null());
    }
}
