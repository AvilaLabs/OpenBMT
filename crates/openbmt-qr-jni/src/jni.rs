//! The JNI glue: the two exported symbols of the library and the only
//! `unsafe` of the crate.
//!
//! ```text
//! Java_org_openbmt_common_QrNative_decode(env, class, width: jint,
//!     height: jint, stride: jint, luma: jbyteArray) -> jbyteArray
//! Java_org_openbmt_common_UrNative_call(env, class, request: jbyteArray)
//!     -> jbyteArray
//! ```
//!
//! Each copies its array out of Java, runs [`reply`](crate::reply) or
//! [`call`](crate::call::call) and returns the framed reply as a new Java
//! array. It is written by hand
//! like the counter's glue: the function table is read slot by slot (the
//! slot numbers are those of the JNI specification, checked by a test
//! against a fake table), so no crate of the JNI types is used.
//!
//! The function keeps no global reference, no cached environment and no
//! thread of its own, and may be called from any thread. After every JNI
//! call that can leave an exception pending, the exception is cleared and
//! the call answers with a null array. A null from any JNI function is
//! handled and never dereferenced. Every `unsafe` block is one JNI call
//! (or one read of the function table) with its reason on the line before.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;

use crate::call::{call, MAX_CALL};
use crate::{bad_call, reply, MAX_FRAME};

type JInt = i32;
type JObject = *mut c_void;
type JByteArray = JObject;
type JClass = JObject;

/// The JNI function table, as the JVM lays it out: an array of function
/// pointers, indexed by the numbers of the JNI specification.
type Table = *const c_void;
/// A `JNIEnv`: a pointer to a pointer to the table.
type RawEnv = *mut *const Table;

const EXCEPTION_CLEAR: usize = 17;
const GET_ARRAY_LENGTH: usize = 171;
const NEW_BYTE_ARRAY: usize = 176;
const GET_BYTE_ARRAY_REGION: usize = 200;
const SET_BYTE_ARRAY_REGION: usize = 208;
const EXCEPTION_CHECK: usize = 228;

type GetArrayLength = unsafe extern "system" fn(RawEnv, JObject) -> JInt;
type NewByteArray = unsafe extern "system" fn(RawEnv, JInt) -> JByteArray;
type GetByteArrayRegion = unsafe extern "system" fn(RawEnv, JByteArray, JInt, JInt, *mut i8);
type SetByteArrayRegion = unsafe extern "system" fn(RawEnv, JByteArray, JInt, JInt, *const i8);
type ExceptionCheck = unsafe extern "system" fn(RawEnv) -> u8;
type ExceptionClear = unsafe extern "system" fn(RawEnv);

/// The environment of one call.
#[derive(Clone, Copy)]
struct Env {
    env: RawEnv,
    table: *const Table,
}

impl Env {
    /// The table behind `env`, or `None` when `env` or the table is null.
    fn new(env: RawEnv) -> Option<Self> {
        if env.is_null() {
            return None;
        }
        // SAFETY: `env` is the non-null pointer the JVM passed to the native
        // method; it points at the pointer to the function table, which the
        // JVM keeps valid and unchanged while the call runs.
        let table = unsafe { *env };
        if table.is_null() {
            return None;
        }
        Some(Self { env, table })
    }

    /// The function in slot `index`, or `None` when the slot is null.
    ///
    /// # Safety
    ///
    /// `F` must be the function type the JNI specification gives for slot
    /// `index`.
    unsafe fn slot<F: Copy>(&self, index: usize) -> Option<F> {
        // SAFETY: the JVM's table has an entry for every slot of the
        // specification, and `index` is one of them; each entry is a
        // function pointer or null, and `Option<F>` of a function pointer
        // has the same layout.
        unsafe { *self.table.add(index).cast::<Option<F>>() }
    }

    fn array_length(&self, array: JByteArray) -> Option<usize> {
        // SAFETY: slot 171 is `GetArrayLength`.
        let f: GetArrayLength = unsafe { self.slot(GET_ARRAY_LENGTH) }?;
        // SAFETY: `array` is a live reference to a Java array (the argument
        // of the call) and `self.env` is this thread's environment.
        let n = unsafe { f(self.env, array) };
        usize::try_from(n).ok()
    }

    /// Copy the `len` bytes of `array` out, or `None` if the call fails.
    fn read_bytes(&self, array: JByteArray, len: usize) -> Option<Vec<u8>> {
        // SAFETY: slot 200 is `GetByteArrayRegion`.
        let f: GetByteArrayRegion = unsafe { self.slot(GET_BYTE_ARRAY_REGION) }?;
        let count = JInt::try_from(len).ok()?;
        let mut buf = vec![0u8; len];
        // SAFETY: `array` is a live byte array of at least `len` elements
        // (its length was just read) and `buf` has room for `len` bytes.
        unsafe { f(self.env, array, 0, count, buf.as_mut_ptr().cast::<i8>()) };
        Some(buf)
    }

    fn new_bytes(&self, bytes: &[u8]) -> Option<JByteArray> {
        // SAFETY: slots 176 and 208 are `NewByteArray` and
        // `SetByteArrayRegion`.
        let new: NewByteArray = unsafe { self.slot(NEW_BYTE_ARRAY) }?;
        let set: SetByteArrayRegion = unsafe { self.slot(SET_BYTE_ARRAY_REGION) }?;
        let count = JInt::try_from(bytes.len()).ok()?;
        // SAFETY: `self.env` is this thread's environment; the length is
        // non-negative. The result may be null (out of memory), checked next.
        let array = unsafe { new(self.env, count) };
        if array.is_null() {
            return None;
        }
        // SAFETY: `array` is the live array of `count` elements just made,
        // and `bytes` holds `count` bytes.
        unsafe { set(self.env, array, 0, count, bytes.as_ptr().cast::<i8>()) };
        Some(array)
    }

    /// Check for a pending exception and clear it. A table without the
    /// function counts as one, so that nothing goes on after a failure
    /// that cannot be seen.
    fn threw(&self) -> bool {
        // SAFETY: slot 228 is `ExceptionCheck`.
        let Some(check): Option<ExceptionCheck> = (unsafe { self.slot(EXCEPTION_CHECK) }) else {
            return true;
        };
        // SAFETY: `self.env` is this thread's environment.
        if unsafe { check(self.env) } == 0 {
            return false;
        }
        // SAFETY: slot 17 is `ExceptionClear`.
        if let Some(clear) = unsafe { self.slot::<ExceptionClear>(EXCEPTION_CLEAR) } {
            // SAFETY: `self.env` is this thread's environment.
            unsafe { clear(self.env) };
        }
        true
    }
}

/// The call. The result is the framed reply as a new Java array, or null
/// when not even that could be made (an exception may then be pending).
fn entry(env: Env, width: JInt, height: JInt, stride: JInt, luma: JByteArray) -> JByteArray {
    let answer = |bytes: &[u8]| env.new_bytes(bytes).unwrap_or(null_mut());
    if luma.is_null() {
        return answer(&bad_call());
    }
    let Some(len) = env.array_length(luma) else {
        return answer(&bad_call());
    };
    if len > MAX_FRAME {
        return answer(&bad_call());
    }
    let frame = env.read_bytes(luma, len);
    if env.threw() {
        return null_mut();
    }
    let Some(mut frame) = frame else {
        return answer(&bad_call());
    };
    let response = reply(width, height, stride, &frame);
    // Nothing of the frame is kept.
    frame.fill(0);
    std::hint::black_box(&mut frame);
    answer(&response)
}

/// The request of [`call`](crate::call::call) as a new reply array. A null
/// or oversized array is answered with the reply to an empty request, which
/// is a refusal for framing.
fn entry_call(env: Env, request: JByteArray) -> JByteArray {
    let answer = |bytes: &[u8]| env.new_bytes(bytes).unwrap_or(null_mut());
    if request.is_null() {
        return answer(&call(&[]));
    }
    let Some(len) = env.array_length(request) else {
        return answer(&call(&[]));
    };
    if len > MAX_CALL {
        return answer(&call(&[]));
    }
    let bytes = env.read_bytes(request, len);
    if env.threw() {
        return null_mut();
    }
    let Some(mut bytes) = bytes else {
        return answer(&call(&[]));
    };
    let response = call(&bytes);
    // Nothing of the request is kept.
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
    answer(&response)
}

/// The second exported function: `UrNative.call` of `org.openbmt.common`.
/// See the module documentation and [`call`](crate::call).
#[no_mangle]
pub extern "system" fn Java_org_openbmt_common_UrNative_call(
    env: RawEnv,
    _class: JClass,
    request: JByteArray,
) -> JByteArray {
    let Some(env) = Env::new(env) else {
        return null_mut();
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| entry_call(env, request))).unwrap_or(null_mut())
}

/// The first exported function: `QrNative.decode` of `org.openbmt.common`.
/// See the module documentation.
#[no_mangle]
pub extern "system" fn Java_org_openbmt_common_QrNative_decode(
    env: RawEnv,
    _class: JClass,
    width: JInt,
    height: JInt,
    stride: JInt,
    luma: JByteArray,
) -> JByteArray {
    let Some(env) = Env::new(env) else {
        return null_mut();
    };
    std::panic::catch_unwind(AssertUnwindSafe(|| entry(env, width, height, stride, luma)))
        .unwrap_or(null_mut())
}

#[cfg(test)]
mod tests {
    //! The glue against a fake `JNIEnv`: a function table whose entries are
    //! `extern "system"` functions over state kept per test thread, at the
    //! slot numbers of the JNI specification.

    use std::cell::RefCell;

    use super::*;
    use crate::{DECODED, REFUSED};

    const ARRAY_BASE: usize = 0x1000;
    const SLOTS: usize = 235;

    #[derive(Default)]
    struct Fake {
        arrays: Vec<Vec<u8>>,
        pending: bool,
        clears: usize,
        new_array_fails: bool,
        region_throws: bool,
    }

    thread_local! {
        static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
    }

    fn index_of(array: JObject) -> usize {
        (array as usize).wrapping_sub(ARRAY_BASE)
    }

    unsafe extern "system" fn get_array_length(_: RawEnv, array: JObject) -> JInt {
        FAKE.with(|f| {
            let f = f.borrow();
            f.arrays
                .get(index_of(array))
                .map_or(-1, |a| a.len() as JInt)
        })
    }

    unsafe extern "system" fn new_byte_array(_: RawEnv, len: JInt) -> JByteArray {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            if f.new_array_fails {
                return null_mut();
            }
            f.arrays.push(vec![0; len as usize]);
            (ARRAY_BASE + f.arrays.len() - 1) as JObject
        })
    }

    unsafe extern "system" fn get_region(
        _: RawEnv,
        array: JByteArray,
        start: JInt,
        len: JInt,
        out: *mut i8,
    ) {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            if f.region_throws {
                f.pending = true;
                return;
            }
            let src = &f.arrays[index_of(array)];
            let (s, n) = (start as usize, len as usize);
            // SAFETY: the glue gave a buffer of `len` bytes.
            unsafe { std::ptr::copy_nonoverlapping(src[s..s + n].as_ptr().cast::<i8>(), out, n) };
        });
    }

    unsafe extern "system" fn set_region(
        _: RawEnv,
        array: JByteArray,
        start: JInt,
        len: JInt,
        from: *const i8,
    ) {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            let dst = &mut f.arrays[index_of(array)];
            let (s, n) = (start as usize, len as usize);
            // SAFETY: the glue gave a buffer of `len` bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(from.cast::<u8>(), dst[s..s + n].as_mut_ptr(), n)
            };
        });
    }

    unsafe extern "system" fn exception_check(_: RawEnv) -> u8 {
        FAKE.with(|f| u8::from(f.borrow().pending))
    }

    unsafe extern "system" fn exception_clear(_: RawEnv) {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.pending = false;
            f.clears += 1;
        });
    }

    struct Rig {
        table: Vec<*const c_void>,
    }

    impl Rig {
        fn new() -> Self {
            FAKE.with(|f| *f.borrow_mut() = Fake::default());
            let mut table: Vec<*const c_void> = vec![std::ptr::null(); SLOTS];
            table[EXCEPTION_CLEAR] =
                exception_clear as unsafe extern "system" fn(RawEnv) as *const c_void;
            table[GET_ARRAY_LENGTH] = get_array_length as GetArrayLength as *const c_void;
            table[NEW_BYTE_ARRAY] = new_byte_array as NewByteArray as *const c_void;
            table[GET_BYTE_ARRAY_REGION] = get_region as GetByteArrayRegion as *const c_void;
            table[SET_BYTE_ARRAY_REGION] = set_region as SetByteArrayRegion as *const c_void;
            table[EXCEPTION_CHECK] = exception_check as ExceptionCheck as *const c_void;
            Self { table }
        }

        fn env(&mut self) -> (RawEnv, Box<*const Table>) {
            let mut cell = Box::new(self.table.as_ptr());
            let raw: RawEnv = &mut *cell;
            (raw, cell)
        }

        fn array(&self, bytes: &[u8]) -> JByteArray {
            FAKE.with(|f| {
                let mut f = f.borrow_mut();
                f.arrays.push(bytes.to_vec());
                (ARRAY_BASE + f.arrays.len() - 1) as JObject
            })
        }

        fn read(&self, array: JByteArray) -> Vec<u8> {
            FAKE.with(|f| f.borrow().arrays[index_of(array)].clone())
        }
    }

    /// A frame with a code of version 1 at module size 4.
    fn frame() -> (usize, Vec<u8>, Vec<u8>) {
        let data = b"hello".to_vec();
        let code = openbmt_qr::encode_bytes(&data, openbmt_qr::Ecc::M).expect("fits");
        let n = code.size();
        let scale = 4;
        let side = (n + 8) * scale;
        let mut px = vec![255u8; side * side];
        for y in 0..n {
            for x in 0..n {
                if code.get(x, y) {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            px[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                        }
                    }
                }
            }
        }
        (side, px, data)
    }

    #[test]
    fn qr_jni_slots_are_those_of_the_jni_specification() {
        // The fake table puts each function at the slot the glue reads; a
        // call that works end to end proves the glue reads those slots.
        // The numbers themselves are those of jni.h: ExceptionClear 17,
        // GetArrayLength 171, NewByteArray 176, GetByteArrayRegion 200,
        // SetByteArrayRegion 208, ExceptionCheck 228.
        assert_eq!(
            [
                EXCEPTION_CLEAR,
                GET_ARRAY_LENGTH,
                NEW_BYTE_ARRAY,
                GET_BYTE_ARRAY_REGION,
                SET_BYTE_ARRAY_REGION,
                EXCEPTION_CHECK
            ],
            [17, 171, 176, 200, 208, 228]
        );
    }

    #[test]
    fn qr_jni_decodes_a_frame_through_the_table() {
        let (side, px, data) = frame();
        let mut rig = Rig::new();
        let array = rig.array(&px);
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_QrNative_decode(
            env,
            null_mut(),
            side as i32,
            side as i32,
            side as i32,
            array,
        );
        assert!(!out.is_null());
        let bytes = rig.read(out);
        assert_eq!(bytes[0], DECODED);
        assert_eq!(&bytes[1..], &data[..]);
    }

    #[test]
    fn qr_jni_refuses_with_a_rule_code() {
        let mut rig = Rig::new();
        let array = rig.array(&vec![255u8; 64 * 64]);
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_QrNative_decode(env, null_mut(), 64, 64, 64, array);
        assert_eq!(rig.read(out), [&[REFUSED][..], b"QR-12"].concat());
    }

    #[test]
    fn qr_jni_null_array_and_null_env_do_not_crash() {
        let mut rig = Rig::new();
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_QrNative_decode(env, null_mut(), 64, 64, 64, null_mut());
        assert_eq!(rig.read(out), [&[REFUSED][..], b"QR-10"].concat());
        assert!(Java_org_openbmt_common_QrNative_decode(
            null_mut(),
            null_mut(),
            64,
            64,
            64,
            null_mut()
        )
        .is_null());
        let mut null_table: *const Table = std::ptr::null();
        let raw: RawEnv = &mut null_table;
        assert!(
            Java_org_openbmt_common_QrNative_decode(raw, null_mut(), 64, 64, 64, null_mut())
                .is_null()
        );
    }

    #[test]
    fn qr_jni_a_read_that_throws_is_cleared_and_gives_null() {
        let mut rig = Rig::new();
        let array = rig.array(&vec![255u8; 64 * 64]);
        FAKE.with(|f| f.borrow_mut().region_throws = true);
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_QrNative_decode(env, null_mut(), 64, 64, 64, array);
        assert!(out.is_null());
        FAKE.with(|f| {
            let f = f.borrow();
            assert_eq!(f.clears, 1);
            assert!(!f.pending);
        });
    }

    #[test]
    fn qr_jni_out_of_memory_gives_null() {
        let mut rig = Rig::new();
        let array = rig.array(&vec![255u8; 64 * 64]);
        FAKE.with(|f| f.borrow_mut().new_array_fails = true);
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_QrNative_decode(env, null_mut(), 64, 64, 64, array);
        assert!(out.is_null());
    }

    #[test]
    fn qr_jni_a_table_without_the_functions_gives_null() {
        let mut rig = Rig::new();
        let array = rig.array(&vec![255u8; 64 * 64]);
        for slot in [
            GET_ARRAY_LENGTH,
            NEW_BYTE_ARRAY,
            GET_BYTE_ARRAY_REGION,
            EXCEPTION_CHECK,
        ] {
            let saved = rig.table[slot];
            rig.table[slot] = std::ptr::null();
            let (env, _cell) = rig.env();
            let out = Java_org_openbmt_common_QrNative_decode(env, null_mut(), 64, 64, 64, array);
            assert!(out.is_null() || !rig.read(out).is_empty(), "slot {slot}");
            rig.table[slot] = saved;
        }
    }

    fn call_request(op: u8, rest: &[u8]) -> Vec<u8> {
        let mut v = crate::call::REQUEST_MAGIC.to_vec();
        v.push(crate::call::VERSION);
        v.push(op);
        v.extend_from_slice(rest);
        v
    }

    #[test]
    fn qr_jni_call_goes_through_the_table() {
        let mut rig = Rig::new();
        // Draw a text: side 21 for "HI" at level L.
        let mut rest = vec![0u8];
        rest.extend_from_slice(&2u32.to_be_bytes());
        rest.extend_from_slice(b"HI");
        let array = rig.array(&call_request(crate::call::OP_QR_ENCODE, &rest));
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_UrNative_call(env, null_mut(), array);
        assert!(!out.is_null());
        let reply = rig.read(out);
        assert_eq!(&reply[..4], &crate::call::REPLY_MAGIC);
        assert_eq!(reply[5], crate::call::OK);
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 21);
        // The request array was zeroed after the call.
        FAKE.with(|f| {
            let f = f.borrow();
            // The copy made by the glue is zeroed, not the Java array; the
            // Java array is the caller's own and is left as it was.
            assert!(f.arrays[index_of(array)].iter().any(|&b| b != 0));
        });
    }

    #[test]
    fn qr_jni_call_refuses_a_null_array_and_survives_a_null_env() {
        let mut rig = Rig::new();
        let (env, _cell) = rig.env();
        let out = Java_org_openbmt_common_UrNative_call(env, null_mut(), null_mut());
        let reply = rig.read(out);
        assert_eq!(&reply[6..], crate::call::BAD_FRAMING.as_bytes());
        assert!(
            Java_org_openbmt_common_UrNative_call(null_mut(), null_mut(), null_mut()).is_null()
        );
        let mut null_table: *const Table = std::ptr::null();
        let raw: RawEnv = &mut null_table;
        assert!(Java_org_openbmt_common_UrNative_call(raw, null_mut(), null_mut()).is_null());
    }

    #[test]
    fn qr_jni_call_a_read_that_throws_is_cleared_and_gives_null() {
        let mut rig = Rig::new();
        let array = rig.array(&call_request(crate::call::OP_DECODER_NEW, &[]));
        FAKE.with(|f| f.borrow_mut().region_throws = true);
        let (env, _cell) = rig.env();
        assert!(Java_org_openbmt_common_UrNative_call(env, null_mut(), array).is_null());
        FAKE.with(|f| {
            let f = f.borrow();
            assert_eq!(f.clears, 1);
            assert!(!f.pending);
        });
    }

    #[test]
    fn qr_jni_call_out_of_memory_and_a_bare_table_give_null() {
        let mut rig = Rig::new();
        let array = rig.array(&call_request(crate::call::OP_DECODER_NEW, &[]));
        FAKE.with(|f| f.borrow_mut().new_array_fails = true);
        let (env, _cell) = rig.env();
        assert!(Java_org_openbmt_common_UrNative_call(env, null_mut(), array).is_null());
        FAKE.with(|f| f.borrow_mut().new_array_fails = false);
        for slot in [
            GET_ARRAY_LENGTH,
            NEW_BYTE_ARRAY,
            GET_BYTE_ARRAY_REGION,
            EXCEPTION_CHECK,
        ] {
            let saved = rig.table[slot];
            rig.table[slot] = std::ptr::null();
            let (env, _cell) = rig.env();
            let out = Java_org_openbmt_common_UrNative_call(env, null_mut(), array);
            assert!(out.is_null() || !rig.read(out).is_empty(), "slot {slot}");
            rig.table[slot] = saved;
        }
    }
}
