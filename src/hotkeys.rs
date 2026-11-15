//! Global keyboard shortcuts, through the system's hot key calls (Carbon). They need no extra
//! permission and cost nothing while no key is pressed.
//!   Option + Command + I (I for Ice): show or hide the hidden icons.
//!   Option + Command + F: search.
use std::cell::RefCell;
use std::ffi::c_void;

use rune::AppHandle;

use crate::{status_item, Mode, State};

const COMMAND_OPTION: u32 = 0x0100 | 0x0800;
const KEY_I: u32 = 34;
const KEY_F: u32 = 3;
const STRIP_ID: u32 = 1;
const SEARCH_ID: u32 = 2;

#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(target: *mut c_void, handler: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32, count: u32, types: *const EventType, data: *mut c_void, out: *mut c_void) -> i32;
    fn GetEventParameter(event: *mut c_void, name: u32, kind: u32, actual_kind: *mut u32, size: usize, actual_size: *mut usize, data: *mut c_void) -> i32;
    fn RegisterEventHotKey(code: u32, modifiers: u32, id: HotKeyId, target: *mut c_void, options: u32, out: *mut *mut c_void) -> i32;
    fn UnregisterEventHotKey(key: *mut c_void) -> i32;
}

thread_local! {
    // Hot keys only work on the main thread, so a thread-local is enough.
    static HANDLE: RefCell<Option<AppHandle<State>>> = const { RefCell::new(None) };
    static KEYS: RefCell<Vec<*mut c_void>> = const { RefCell::new(Vec::new()) };
}

/// Called by the system when one of our keys is pressed (on the main thread).
extern "C" fn pressed(_next: *mut c_void, event: *mut c_void, _data: *mut c_void) -> i32 {
    let mut key = HotKeyId { signature: 0, id: 0 };
    // '----' is the direct object of the event, 'hkid' is the type of a hot key id.
    let status = unsafe { GetEventParameter(event, 0x2D2D2D2D, 0x686B6964, std::ptr::null_mut(), size_of::<HotKeyId>(), std::ptr::null_mut(), &raw mut key as *mut c_void) };
    if status != 0 {
        return status;
    }
    HANDLE.with(|h| {
        let Some(handle) = h.borrow().clone() else { return };
        match key.id {
            STRIP_ID if handle.is_visible() => handle.hide(),
            STRIP_ID => status_item::open_at_mouse(&handle, Mode::Strip),
            SEARCH_ID => status_item::open_at_mouse(&handle, Mode::Search),
            _ => {}
        }
    });
    0
}

/// Starts listening. Call once, on the main thread.
pub fn install(handle: AppHandle<State>) {
    HANDLE.with(|h| *h.borrow_mut() = Some(handle));
    // 'keyb' events of kind 5 are "a hot key was pressed".
    let types = EventType { class: 0x6B657962, kind: 5 };
    unsafe { InstallEventHandler(GetApplicationEventTarget(), pressed, 1, &types, std::ptr::null_mut(), std::ptr::null_mut()) };
}

/// Turns the shortcuts on or off. Call on the main thread. A key another app already owns just
/// stays off: the call fails and nothing is stored.
pub fn set_enabled(on: bool) {
    KEYS.with(|keys| {
        let mut keys = keys.borrow_mut();
        for key in keys.drain(..) {
            unsafe { UnregisterEventHotKey(key) };
        }
        if on {
            for (code, id) in [(KEY_I, STRIP_ID), (KEY_F, SEARCH_ID)] {
                let mut key = std::ptr::null_mut();
                // 'ICEO' is our signature; it only has to be the same every time.
                let status = unsafe { RegisterEventHotKey(code, COMMAND_OPTION, HotKeyId { signature: 0x4943454F, id }, GetApplicationEventTarget(), 0, &mut key) };
                if status == 0 {
                    keys.push(key);
                }
            }
        }
    });
}
