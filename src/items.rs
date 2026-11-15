//! Finds the menu bar items of other apps with the Accessibility API, presses them, and moves
//! them with mouse events that are aimed at an item's own window (the way Ice does it), so the
//! cursor never has to travel. Needs the Accessibility permission. Screen Recording is not used.

use std::ffi::c_void;

use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use objc2_foundation::NSString;
use rune::Image;

type CFTypeRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFTypeRef) -> i32;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: *mut CFTypeRef) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
}

/// One icon in someone's menu bar. `pid` and `index` find it again when we press it.
#[derive(Clone)]
pub struct Item {
    pub pid: i32,
    pub index: usize,
    pub name: String,
    /// Left edge and width of the item in the menu bar, in points.
    pub x: f64,
    pub width: f64,
    /// True when the item sits left of our divider, so it is (or can be) hidden.
    pub hidden: bool,
    /// Path of the app, used to load its icon.
    pub path: String,
    /// Filled in on the UI thread, where the image cache lives.
    pub icon: Option<Image>,
}

impl Item {
    /// A name for this item that survives its app restarting: the app plus the item's number in it.
    pub fn key(&self) -> String {
        format!("{}#{}", self.path, self.index)
    }
}

/// Whether a process is still running.
pub fn is_running(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // Signal 0 only checks that the process exists.
    unsafe { kill(pid, 0) == 0 }
}

pub fn has_permission() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Reads one attribute. The caller must `CFRelease` the result.
unsafe fn attribute(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
    let name = NSString::from_str(name);
    let mut value: CFTypeRef = std::ptr::null();
    // An NSString is a CFString, so it can be passed straight in.
    let status = unsafe { AXUIElementCopyAttributeValue(element, &*name as *const NSString as CFTypeRef, &mut value) };
    (status == 0 && !value.is_null()).then_some(value)
}

/// Reads a text attribute.
unsafe fn text(element: CFTypeRef, name: &str) -> String {
    let Some(value) = (unsafe { attribute(element, name) }) else { return String::new() };
    let text = unsafe { &*(value as *const NSString) }.to_string();
    unsafe { CFRelease(value) };
    text
}

/// The left edge and width of an Accessibility element, in points.
unsafe fn frame(element: CFTypeRef) -> (f64, f64) {
    let mut point = [0.0f64; 2];
    let mut size = [0.0f64; 2];
    unsafe {
        if let Some(position) = attribute(element, "AXPosition") {
            AXValueGetValue(position, 1, point.as_mut_ptr() as *mut c_void); // 1 = a point
            CFRelease(position);
        }
        if let Some(value) = attribute(element, "AXSize") {
            AXValueGetValue(value, 2, size.as_mut_ptr() as *mut c_void); // 2 = a size
            CFRelease(value);
        }
    }
    (point[0], size[0])
}

/// The menu bar items of one app, as (index, x, width, title, description). Items that are not
/// on screen have x = 0.
unsafe fn extras(pid: i32, mut each: impl FnMut(usize, f64, f64, String, String)) {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        // A stuck app must not stall the whole read, so give up on it quickly.
        AXUIElementSetMessagingTimeout(app, 0.05);
        if let Some(bar) = attribute(app, "AXExtrasMenuBar") {
            if let Some(children) = attribute(bar, "AXChildren") {
                for index in 0..CFArrayGetCount(children) {
                    let child = CFArrayGetValueAtIndex(children, index);
                    let (x, width) = frame(child);
                    each(index as usize, x, width, text(child, "AXTitle"), text(child, "AXDescription"));
                }
                CFRelease(children);
            }
            CFRelease(bar);
        }
        CFRelease(app);
    }
}

/// Where our two buttons are, read through Accessibility like any other item: the divider and
/// the toggle, both as left edges. Only call this when the main thread is free, because the
/// system asks our own main thread for the answer.
pub fn own_markers() -> Option<(f64, f64)> {
    let (mut divider, mut toggle) = (None, None);
    unsafe {
        extras(std::process::id() as i32, |_, x, _, title, _| match title.as_str() {
            crate::status_item::DIVIDER_TITLE => divider = Some(x),
            _ => toggle = Some(x),
        });
    }
    divider.zip(toggle)
}

/// Where our divider is. See `own_markers`.
pub fn own_divider_x() -> Option<f64> {
    own_markers().map(|(divider, _)| divider)
}

/// What we need to know about a running app before asking it for its menu bar items.
struct App {
    pid: i32,
    name: String,
    path: String,
}

/// All menu bar items of other apps, left to right. Every app is asked on its own thread,
/// so one slow app does not hold up the rest.
pub fn list(divider_x: Option<f64>) -> Vec<Item> {
    let me = std::process::id() as i32;
    let apps: Vec<App> = unsafe { NSWorkspace::sharedWorkspace().runningApplications() }
        .iter()
        // Background-only processes have no menu bar items.
        .filter(|app| unsafe { app.activationPolicy() } != NSApplicationActivationPolicy::Prohibited)
        // macOS's own icons (Wi-Fi, Battery, Search, Control Centre, Clock...) belong to the system,
        // not to an app, so they are left out of the list for good.
        .filter(|app| !unsafe { app.bundleIdentifier() }.is_some_and(|id| id.to_string().starts_with("com.apple.")))
        .map(|app| App {
            pid: unsafe { app.processIdentifier() },
            name: unsafe { app.localizedName() }.map(|n| n.to_string()).unwrap_or_default(),
            path: unsafe { app.bundleURL() }.and_then(|url| unsafe { url.path() }).map(|p| p.to_string()).unwrap_or_default(),
        })
        .filter(|app| app.pid != me)
        .collect();

    let mut found: Vec<(f64, Item)> = Vec::new();
    std::thread::scope(|scope| {
        let jobs: Vec<_> = apps
            .iter()
            .map(|app| {
                scope.spawn(move || {
                    let mut items = Vec::new();
                    unsafe {
                        extras(app.pid, |index, x, width, _title, description| {
                            // Items with x = 0 are placeholders that are not real icons. Hidden
                            // icons are real: they sit far off the screen, at a negative x.
                            if x == 0.0 {
                                return;
                            }
                            // For Apple's own items the description (Wi-Fi, Battery...) is the better name.
                            let name = if description.is_empty() { app.name.clone() } else { description };
                            let hidden = divider_x.is_some_and(|d| x < d);
                            let item = Item { pid: app.pid, index, name, x, width, hidden, path: app.path.clone(), icon: None };
                            items.push((x, item));
                        });
                    }
                    items
                })
            })
            .collect();
        for job in jobs {
            found.extend(job.join().unwrap_or_default());
        }
    });
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, item)| item).collect()
}

/// Where one item is on screen, asking only its own app. None while the item is off the screen.
pub fn position(pid: i32, index: usize) -> Option<f64> {
    let mut found = None;
    unsafe {
        extras(pid, |i, x, _, _, _| {
            if i == index && x > 0.0 {
                found = Some(x);
            }
        });
    }
    found
}

/// Clicks the item, so its menu opens. The item must be on screen.
pub fn press(pid: i32, index: usize) {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if let Some(bar) = attribute(app, "AXExtrasMenuBar") {
            if let Some(children) = attribute(bar, "AXChildren") {
                if (index as isize) < CFArrayGetCount(children) {
                    let child = CFArrayGetValueAtIndex(children, index as isize);
                    AXUIElementPerformAction(child, &*NSString::from_str("AXPress") as *const NSString as CFTypeRef);
                }
                CFRelease(children);
            }
            CFRelease(bar);
        }
        CFRelease(app);
    }
}

// ---- Menu bar windows ----
//
// Every menu bar icon is a window on layer 25, and the window server lists them (with an id and a
// frame) even while they are pushed off the screen. Moving an icon means sending mouse events
// that name one of these windows.

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
}

/// A menu bar icon's window.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub id: u32,
    /// The process that owns the window. On recent macOS this is Control Centre for every icon.
    pub pid: i32,
    pub x: f64,
    pub width: f64,
}

/// A number stored under `key` in a window-info dictionary.
unsafe fn number(dictionary: *mut objc2::runtime::AnyObject, key: &str) -> f64 {
    use objc2::msg_send;
    unsafe {
        let value: *mut objc2::runtime::AnyObject = msg_send![dictionary, objectForKey: &*NSString::from_str(key)];
        if value.is_null() { 0.0 } else { msg_send![value, doubleValue] }
    }
}

/// Every menu bar icon window on the main display, on the screen or not.
pub fn windows() -> Vec<Window> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let mut found = Vec::new();
    unsafe {
        let list = CGWindowListCopyWindowInfo(0, 0) as *mut AnyObject; // 0 = every window
        if list.is_null() {
            return found;
        }
        let total: usize = msg_send![list, count];
        for i in 0..total {
            let info: *mut AnyObject = msg_send![list, objectAtIndex: i];
            if number(info, "kCGWindowLayer") as i64 != 25 {
                continue;
            }
            let bounds: *mut AnyObject = msg_send![info, objectForKey: &*NSString::from_str("kCGWindowBounds")];
            if bounds.is_null() {
                continue;
            }
            // The main display's menu bar starts at y = 0. Other displays have their own copies.
            if number(bounds, "Y").abs() > 60.0 {
                continue;
            }
            found.push(Window {
                id: number(info, "kCGWindowNumber") as u32,
                pid: number(info, "kCGWindowOwnerPID") as i32,
                x: number(bounds, "X"),
                width: number(bounds, "Width"),
            });
        }
        CFRelease(list as CFTypeRef);
    }
    found
}

/// Finds the window that belongs to an item whose accessibility frame is (`x`, `width`). The two
/// differ by a few points of padding, and by different amounts for different kinds of item, so
/// pick the window that is closest.
fn window_for(windows: &[Window], x: f64, width: f64) -> Option<Window> {
    windows
        .iter()
        .map(|w| (((w.x - x).abs() + (w.width - width).abs()), *w))
        .filter(|(_, w)| (w.x - x).abs() < 12.0 && (w.width - width).abs() < 20.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, w)| w)
}

/// The window of one item, found through its accessibility frame.
pub fn window_of(pid: i32, index: usize) -> Option<Window> {
    let mut frame_of = None;
    unsafe {
        extras(pid, |i, x, width, _, _| {
            if i == index {
                frame_of = Some((x, width));
            }
        });
    }
    let (x, width) = frame_of?;
    window_for(&windows(), x, width)
}

/// The windows of our own divider and toggle. Only call this when the main thread is free, because
/// the system asks our own main thread for the answer.
pub fn own_windows() -> Option<(Window, Window)> {
    let all = windows();
    let (mut divider, mut toggle) = (None, None);
    unsafe {
        extras(std::process::id() as i32, |_, x, width, title, _| match title.as_str() {
            crate::status_item::DIVIDER_TITLE => divider = window_for(&all, x, width),
            // Our other button, the one with the Iceout mark, is the toggle.
            _ => toggle = window_for(&all, x, width),
        });
    }
    divider.zip(toggle)
}

/// Where a window is now, or None if it is gone.
fn window_x(id: u32) -> Option<f64> {
    windows().into_iter().find(|w| w.id == id).map(|w| w.x)
}

// ---- Moving an item ----
//
// This is Ice's method. A Command-drag is two mouse events. Each is given a made-up position and
// is tagged with the window it is for, and posted straight to the process that owns that window,
// so the system carries out the drag on that window without any real mouse travel. The cursor
// does jump to the place where the drag ends, so it is hidden while that happens and put back.

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> Point;
    fn CGEventCreateMouseEvent(source: CFTypeRef, kind: u32, position: Point, button: u32) -> CFTypeRef;
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventSetIntegerValueField(event: CFTypeRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: CFTypeRef);
    fn CGEventPostToPid(pid: i32, event: CFTypeRef);
    fn CGEventSourceCreate(state: i32) -> CFTypeRef;
    fn CGEventSourceSetLocalEventsFilterDuringSuppressionState(source: CFTypeRef, filter: u32, state: u32);
    fn CGEventSourceSetLocalEventsSuppressionInterval(source: CFTypeRef, seconds: f64);
    fn CGWarpMouseCursorPosition(position: Point) -> i32;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    // Private, and used the same way by Ice: lets us hide the cursor while another app is in front.
    fn CGSMainConnectionID() -> i32;
    fn CGSSetConnectionProperty(connection: i32, target: i32, key: CFTypeRef, value: CFTypeRef) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
}

/// Lets us hide the cursor even though we are never the front app. Call once at startup.
pub fn allow_hiding_cursor() {
    unsafe {
        let key = NSString::from_str("SetsCursorInBackground");
        let connection = CGSMainConnectionID();
        CGSSetConnectionProperty(connection, connection, &*key as *const NSString as CFTypeRef, kCFBooleanTrue);
    }
}

/// True while we are sending our own mouse events. Our global mouse monitors hear those events
/// too, and must ignore them.
pub static SENDING_EVENTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// Raw numbers for the event fields we need.
const TARGET_PID: u32 = 89;
const SOURCE_USER_DATA: u32 = 42;
const WINDOW_UNDER_POINTER: u32 = 91;
const WINDOW_UNDER_POINTER_CAN_HANDLE: u32 = 92;
const WINDOW_ID: u32 = 0x33;

/// One Command + mouse event, tagged for `window`. `x` is where it claims to happen.
unsafe fn tagged_event(source: CFTypeRef, kind: u32, x: f64, y: f64, window: Window) -> CFTypeRef {
    unsafe {
        let event = CGEventCreateMouseEvent(source, kind, Point { x, y }, 0);
        CGEventSetFlags(event, 0x100000); // Command
        CGEventSetIntegerValueField(event, TARGET_PID, i64::from(window.pid));
        CGEventSetIntegerValueField(event, SOURCE_USER_DATA, event as i64);
        CGEventSetIntegerValueField(event, WINDOW_UNDER_POINTER, i64::from(window.id));
        CGEventSetIntegerValueField(event, WINDOW_UNDER_POINTER_CAN_HANDLE, i64::from(window.id));
        CGEventSetIntegerValueField(event, WINDOW_ID, i64::from(window.id));
        event
    }
}

unsafe fn send(event: CFTypeRef, pid: i32) {
    unsafe {
        CGEventPostToPid(pid, event);
        CGEventPost(1, event); // 1 = the session event tap
        CFRelease(event);
    }
}

/// Moves `item` so that it ends up just left of `target` (a window of the divider or the toggle).
/// Returns whether the item moved.
pub fn move_left_of(item: Window, target: Window) -> bool {
    use std::{thread::sleep, time::Duration};
    let before = window_x(item.id);
    SENDING_EVENTS.store(true, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        let start = CGEventCreate(std::ptr::null());
        let cursor = CGEventGetLocation(start);
        CFRelease(start);
        let display = CGMainDisplayID();
        CGDisplayHideCursor(display);

        // Let our own events through even while the user's mouse is in use.
        let permit = CGEventSourceCreate(0); // 0 = the combined session state
        for state in [0u32, 1] {
            CGEventSourceSetLocalEventsFilterDuringSuppressionState(permit, 7, state);
        }
        CGEventSourceSetLocalEventsSuppressionInterval(permit, 0.0);
        CFRelease(permit);

        let source = CGEventSourceCreate(1); // 1 = the hardware state
        // The drag starts far off the screen and ends at the left edge of the target.
        let down = tagged_event(source, 1, 20_000.0, 20_000.0, item);
        let up = tagged_event(source, 2, target.x, 16.0, target);
        send(down, item.pid);
        sleep(Duration::from_millis(60));
        send(up, item.pid);
        CFRelease(source);

        // Wait for the item to land, then put the cursor back where the user had it.
        for _ in 0..20 {
            sleep(Duration::from_millis(15));
            if window_x(item.id) != before {
                break;
            }
        }
        CGWarpMouseCursorPosition(cursor);
        CGDisplayShowCursor(display);
    }
    // The system hands our events to the monitors a moment later, so keep ignoring for a bit.
    sleep(Duration::from_millis(200));
    SENDING_EVENTS.store(false, std::sync::atomic::Ordering::Relaxed);
    window_x(item.id) != before
}
