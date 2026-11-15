//! Global keyboard shortcuts. Hold I (for Ice) and Option + Command, then press:
//!   Space: show or hide the hidden icons.
//!   F: search.
//! Holding I with those two keys is something other apps never use, so nothing clashes. The
//! keys are read with an event tap, which can also swallow them, so the front app never sees
//! them. An event tap needs Accessibility, which Ice Out already has.
use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use rune::AppHandle;

use crate::{status_item, Mode, State};

const KEY_I: i64 = 34;
const KEY_SPACE: i64 = 49;
const KEY_F: i64 = 3;
const COMMAND: u64 = 0x100000;
const OPTION: u64 = 0x80000;
const OTHER_MODIFIERS: u64 = 0x40000 | 0x20000; // control, shift
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const TAP_DISABLED: [u32; 2] = [0xFFFF_FFFE, 0xFFFF_FFFF]; // too slow, or switched off by the system
const KEYCODE_FIELD: u32 = 9;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void, data: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
}

thread_local! {
    // The tap runs on the main thread, so thread-locals are enough.
    static HANDLE: RefCell<Option<AppHandle<State>>> = const { RefCell::new(None) };
    static TAP: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static I_HELD: Cell<bool> = const { Cell::new(false) };
}

/// Called for every key press and release. Returns null to swallow the key.
extern "C" fn on_key(_proxy: *mut c_void, kind: u32, event: *mut c_void, _data: *mut c_void) -> *mut c_void {
    if TAP_DISABLED.contains(&kind) {
        TAP.with(|t| unsafe { CGEventTapEnable(t.get(), true) });
        return event;
    }
    let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
    let flags = unsafe { CGEventGetFlags(event) };
    let chord_keys = flags & (COMMAND | OPTION | OTHER_MODIFIERS) == COMMAND | OPTION;
    let down = kind == KEY_DOWN;
    if code == KEY_I {
        // While I is held, its press and release (and repeats) are ours. Releasing I always counts,
        // even if Option or Command were let go first.
        if down && chord_keys || !down && I_HELD.get() {
            I_HELD.set(down);
            return std::ptr::null_mut();
        }
    } else if I_HELD.get() && (code == KEY_SPACE || code == KEY_F) && (chord_keys || !down) {
        if down {
            run(code == KEY_SPACE);
        }
        return std::ptr::null_mut();
    }
    event
}

fn run(show_icons: bool) {
    HANDLE.with(|h| {
        let Some(handle) = h.borrow().clone() else { return };
        match show_icons {
            true if handle.is_visible() => handle.hide(),
            true => status_item::open_from_shortcut(&handle, Mode::Strip),
            false => status_item::open_from_shortcut(&handle, Mode::Search),
        }
    });
}

/// Remembers the app handle. Call once, on the main thread.
pub fn install(handle: AppHandle<State>) {
    HANDLE.with(|h| *h.borrow_mut() = Some(handle));
}

/// Turns the shortcuts on or off. Call on the main thread. The tap is made the first time it is
/// needed. Without Accessibility the system refuses to make it, so this is called again once
/// Accessibility is allowed (see `reload`).
pub fn set_enabled(on: bool) {
    TAP.with(|tap| {
        if tap.get().is_null() && on {
            let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP);
            // 1 = tap the user's session, 0 = in front of other apps, 0 = may change or swallow events.
            let port = unsafe { CGEventTapCreate(1, 0, 0, mask, on_key, std::ptr::null_mut()) };
            if port.is_null() {
                return;
            }
            unsafe {
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
            }
            tap.set(port);
        }
        if !tap.get().is_null() {
            I_HELD.set(false);
            unsafe { CGEventTapEnable(tap.get(), on) };
        }
    });
}
