//! Tiny WASM frame-effect example (~30 lines of guest logic).
//!
//! Build (requires `wasm32-unknown-unknown` target):
//! ```
//! rustc --crate-type cdylib -O --target wasm32-unknown-unknown -o invert.wasm invert.rs
//! ```
//! Or regenerate via the host: `renderly_core::plugins::compile_invert_wasm()`.
//!
//! ABI: export `memory` and `process(ptr, len, width, height)` which inverts RGB in place.

#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn process(ptr: *mut u8, len: i32, _w: i32, _h: i32) {
    if ptr.is_null() || len <= 0 {
        return;
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) };
    let mut i = 0;
    while i < slice.len() {
        if i % 4 != 3 {
            slice[i] = 255 - slice[i];
        }
        i += 1;
    }
}
