//! The three calls the page makes into the module, with no glue library:
//!
//! - `demo_alloc(len) -> ptr`: room in the module's memory for a request;
//! - `demo_call(ptr, len) -> u64`: runs the request at `ptr` (and frees it)
//!   and returns the reply as `ptr << 32 | len`;
//! - `demo_free(ptr, len)`: frees a reply once the page has read it.
//!
//! The module is built only for the 32-bit WebAssembly target, where a pointer
//! and a length fit in 32 bits each. This is the one module of the crate with `unsafe` blocks: each
//! hands a buffer that this module allocated to the page and takes it back.

use std::sync::Mutex;

use crate::api::handle;
use crate::shop::Shop;

/// The session of the page. The module runs on one thread; the lock is only
/// there so that the static is allowed.
static SESSION: Mutex<Option<Shop>> = Mutex::new(None);

/// Room for `len` bytes. The page writes a request into it.
#[no_mangle]
pub extern "C" fn demo_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast::<u8>()
}

/// Take back the buffer of `len` bytes at `ptr`.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer this module handed out, not yet
/// taken back.
unsafe fn take(ptr: *mut u8, len: usize) -> Box<[u8]> {
    // SAFETY: by the contract above, the pointer and length are those of a
    // boxed slice that this module leaked.
    unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) }
}

/// Free `len` bytes at `ptr` that `demo_alloc` or `demo_call` gave.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer this module handed out, not yet
/// freed.
#[no_mangle]
pub unsafe extern "C" fn demo_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: the contract is the caller's, as for `take`.
        drop(unsafe { take(ptr, len) });
    }
}

/// Run the request of `len` bytes at `ptr` (and free it); the reply is
/// returned as `ptr << 32 | len`, for the page to free with `demo_free`.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer from `demo_alloc`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn demo_call(ptr: *mut u8, len: usize) -> u64 {
    // SAFETY: the contract is the caller's, as for `take`.
    let request = unsafe { take(ptr, len) };
    let reply = match SESSION.lock() {
        Ok(mut session) => handle(&mut session, &request),
        Err(_) => r#"{"ok":false,"error":"the session is lost"}"#.to_string(),
    };
    let out = reply.into_bytes().into_boxed_slice();
    let out_len = out.len();
    let out_ptr = Box::into_raw(out).cast::<u8>();
    ((out_ptr as usize as u64) << 32) | out_len as u64
}
