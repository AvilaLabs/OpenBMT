//! The calls the page makes into the module, with no glue library:
//!
//! - `demo_alloc(len) -> ptr` and `demo_free(ptr, len)`: room in the module's
//!   memory for a request, and giving back a reply once the page has read it;
//! - `counter_set_now(secs)`: the time, before each call;
//! - `counter_call(ptr, len) -> u64`: one framed request of the counter's
//!   protocol, answered as `ptr << 32 | len`;
//! - `counter_record() -> u64`: the pairing record the demo counter starts
//!   from, in the same form;
//! - `counter_record_with_box() -> u64`: the same record for a shop with a
//!   Box, which is the sender of the till's requests;
//! - `counter_till_request(amount_minor) -> u64`: the Box tells the running
//!   counter that the till asks for a sale of that amount; text, `ok:` and
//!   the till's sale as hex, or `err:` and why;
//! - `chain_pay(ptr, len, sats) -> u64`: a customer pays the address written
//!   at `ptr` on the pretend chain; the reply is text, `ok:` and the id, or
//!   `err:` and why;
//! - `chain_replace(ptr, len, sats) -> u64`: the customer takes the payment
//!   whose id is written at `ptr` back, by spending its coin again to an
//!   address of their own; text, `ok:` and the new id, or `err:` and why;
//! - `chain_mine() -> u64`: a block is mined; text, `ok:` and the height;
//! - `chain_tip() -> u64`: the tip's hash; text, `ok:` and the hash;
//! - `chain_heartbeat() -> u64`: the heartbeat the owner's phone would send
//!   the counter now, signed, as bytes (none when no node is running).
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
pub extern "C" fn counter_set_now(secs: u64) {
    openbmt_kernel_random::set_host_fill(page_random);
    crate::set_now(secs);
}

/// One framed request of `len` bytes at `ptr` (freed here); the response is
/// returned as `ptr << 32 | len`.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer from `demo_alloc`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn counter_call(ptr: *mut u8, len: usize) -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    // SAFETY: the contract is the caller's, as for `take`.
    let request = unsafe { take(ptr, len) };
    give(crate::call(&request))
}

/// The pairing record of the demo counter; empty when it could not be made.
#[no_mangle]
pub extern "C" fn counter_record() -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    give(crate::record::demo_record().unwrap_or_default())
}

/// The pairing record of the demo counter whose settings name a Box.
#[no_mangle]
pub extern "C" fn counter_record_with_box() -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    give(crate::record::demo_record_with_box().unwrap_or_default())
}

/// The Box tells the running counter that the till asks for a sale of
/// `amount_minor`.
#[no_mangle]
pub extern "C" fn counter_till_request(amount_minor: u64) -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    let text = match crate::till::request(amount_minor) {
        Ok(sale) => format!(
            "ok:{}",
            sale.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ),
        Err(e) => format!("err:{e}"),
    };
    give(text.into_bytes())
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

/// The customer replaces the payment whose id is the text of `len` bytes at
/// `ptr` (freed here) by one of `sats` to an address of their own.
///
/// # Safety
/// `ptr` and `len` must be exactly a buffer from `demo_alloc`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn chain_replace(ptr: *mut u8, len: usize, sats: u64) -> u64 {
    // SAFETY: the contract is the caller's, as for `take`.
    let id = unsafe { take(ptr, len) };
    let text = match std::str::from_utf8(&id) {
        Ok(id) => match crate::world::replace(id, sats) {
            Ok(txid) => format!("ok:{txid}"),
            Err(e) => format!("err:{e}"),
        },
        Err(_) => "err:the id is not text".to_string(),
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

/// The owner's heartbeat for the counter, at the time the page last gave.
#[no_mangle]
pub extern "C" fn chain_heartbeat() -> u64 {
    openbmt_kernel_random::set_host_fill(page_random);
    give(crate::world::heartbeat(crate::now()).unwrap_or_default())
}
