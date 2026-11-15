//! The calls the page makes into the module, with no glue library:
//!
//! - `demo_alloc(len) -> ptr` and `demo_free(ptr, len)`: room in the module's
//!   memory for a request, and giving back a reply once the page has read it;
//! - `owner_set_now(secs)`: the time, before each call;
//! - `owner_call(ptr, len) -> u64`: one framed request of the owner's
//!   protocol, answered as `ptr << 32 | len`;
//! - `owner_shop(part) -> u64`: what `start` is given for the shop, as bytes:
//!   part 0 the shop wallet's description text, 1 the approver's root, 2 Key
//!   1's account key;
//! - `owner_address(index) -> u64`: the regtest address of the shop wallet
//!   at that receive index; text, `ok:` and the address, or `err:` and why;
//! - `chain_pay(ptr, len, sats) -> u64`: a customer pays the address written
//!   at `ptr` on the pretend chain; the reply is text, `ok:` and the id, or
//!   `err:` and why;
//! - `chain_mine() -> u64`: a block is mined; text, `ok:` and the height;
//! - `chain_tip() -> u64`: the tip's hash; text, `ok:` and the hash.
//!
//! The one thing the module asks of the page is `openbmt_random(ptr, len)`,
//! imported from `env`: it fills the bytes with the page's random numbers.
//!
//! The module is built only for the 32-bit WebAssembly target, where a pointer
//! and a length fit in 32 bits each. This is the one module of the crate with
//! `unsafe` blocks: each hands a buffer that this module allocated to the page
//! and takes it back.

#[link(wasm_import_module = "env")]
extern "C" {
    fn openbmt_random(ptr: *mut u8, len: usize);
}

fn page_random(out: &mut [u8]) -> bool {
    // SAFETY: the pointer and length are those of a slice this function
    // borrows exclusively; the page writes inside it and nowhere else.
    unsafe { openbmt_random(out.as_mut_ptr(), out.len()) };
    true
}

/// A result as the page reads it: `ok:` and the value, or `err:` and why.
fn text_of(result: Result<String, String>) -> Vec<u8> {
    match result {
        Ok(value) => format!("ok:{value}"),
        Err(e) => format!("err:{e}"),
    }
    .into_bytes()
}

/// Hand `bytes` to the page as `ptr << 32 | len`.
fn give(bytes: Vec<u8>) -> u64 {
    let out = bytes.into_boxed_slice();
    let len = out.len();
    let ptr = Box::into_raw(out).cast::<u8>();
    ((ptr as usize as u64) << 32) | len as u64
}

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

/// Free `len` bytes at `ptr` that `demo_alloc` or a call gave.
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

/// The time, in seconds since 1970.
#[no_mangle]
pub extern "C" fn owner_set_now(secs: u64) {
    openbmt_kernel_random::set_host_fill(page_random);
    crate::set_now(secs);
}

/// One framed request of `len` bytes at `ptr` (freed here); the response is
/// returned as `ptr << 32 | len`.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer from `demo_alloc`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn owner_call(ptr: *mut u8, len: usize) -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    // SAFETY: the contract is the caller's, as for `take`.
    let request = unsafe { take(ptr, len) };
    give(crate::call(&request))
}

/// What the owner's `start` is given for the shop: `part` 0 is the wallet's description text, 1 the approver's root
/// (32 bytes), 2 Key 1's account key (78 bytes); empty when it could not be made.
#[no_mangle]
pub extern "C" fn owner_shop(part: u32) -> u64 {
    let made = match part {
        0 => crate::shop::wallet_text().map(String::into_bytes),
        1 => crate::shop::approver_root(),
        2 => crate::shop::key1(),
        _ => Err("no such part".to_string()),
    };
    give(made.unwrap_or_default())
}

/// The address of the shop wallet at receive `index`.
#[no_mangle]
pub extern "C" fn owner_address(index: u32) -> u64 {
    give(text_of(crate::shop::address(index)))
}

/// A customer pays `sats` to the address of `len` bytes at `ptr` (freed here).
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer from `demo_alloc`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn chain_pay(ptr: *mut u8, len: usize, sats: u64) -> u64 {
    // SAFETY: the contract is the caller's, as for `take`.
    let address = unsafe { take(ptr, len) };
    let text = match std::str::from_utf8(&address) {
        Ok(address) => match crate::world::pay(address, sats) {
            Ok(txid) => format!("ok:{txid}"),
            Err(e) => format!("err:{e}"),
        },
        Err(_) => "err:the address is not text".to_string(),
    };
    give(text.into_bytes())
}

/// The hash of the chain's tip.
#[no_mangle]
pub extern "C" fn chain_tip() -> u64 {
    let text = match crate::world::tip() {
        Ok(hash) => format!("ok:{hash}"),
        Err(e) => format!("err:{e}"),
    };
    give(text.into_bytes())
}

/// A block is mined of everything in the pool.
#[no_mangle]
pub extern "C" fn chain_mine() -> u64 {
    let text = match crate::world::mine() {
        Ok(height) => format!("ok:{height}"),
        Err(e) => format!("err:{e}"),
    };
    give(text.into_bytes())
}
