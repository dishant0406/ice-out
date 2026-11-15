//! Our two menu bar buttons. The toggle (the Iceout mark) opens the window. The divider (|) hides things:
//! when it is made very wide, macOS pushes every icon on its left off the screen.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::{thread::sleep, time::Duration};

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSEventType, NSMenu, NSMenuItem, NSScreen, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_app_kit::NSImage;
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};
use rune::{AppHandle, DisplayTarget, OverlayPlacement};

use crate::{items, Mode, State};

pub const DIVIDER_TITLE: &str = "\u{2009}";
const DIVIDER_WIDTH: f64 = 10.0;

/// Set when the user has clicked or pressed a key in another app, which means they are done
/// with the icon we revealed.
static DONE_WITH_ICON: AtomicBool = AtomicBool::new(false);

pub const HIDING_WIDTH: f64 = 10000.0;

/// True while an icon is out for a peek, so nothing else rearranges the bar meanwhile.
pub static PEEK_ACTIVE: AtomicBool = AtomicBool::new(false);
/// True while the first-launch setup is open: it must stay up while the user works in System
/// Settings, so clicking in another app does not close it.
pub static KEEP_OPEN: AtomicBool = AtomicBool::new(false);
/// True while icons are being put back in view after their app restarted.
static RESTORING: AtomicBool = AtomicBool::new(false);

/// Where the free part of the menu bar starts on a Mac with a notch: the notch's right edge, in
/// points (the bits of an f64). 0 without a notch.
static NOTCH_RIGHT: AtomicU64 = AtomicU64::new(0);

thread_local! {
    // Where the user last clicked our button, in screen points. The window opens under it, on that display.
    static LAST_CLICK: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
    // Menu bar buttons only live on the main thread, so a thread-local is enough.
    static TOGGLE: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    static DIVIDER: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
}

#[link(name = "System")]
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

extern "C" fn run_boxed(context: *mut c_void) {
    // SAFETY: `context` is the box that `on_main` leaked, and this runs once.
    let job = unsafe { Box::from_raw(context as *mut Box<dyn FnOnce()>) };
    job();
}

/// Runs `job` on the main thread through the system's own queue. Unlike `AppHandle::update`, it
/// does not wait for Rune's event loop, which can be busy for a moment after the window closes.
fn on_main(job: impl FnOnce() + Send + 'static) {
    let boxed: Box<Box<dyn FnOnce()>> = Box::new(Box::new(job));
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, Box::into_raw(boxed) as *mut c_void, run_boxed) };
}

/// `set_divider_width` for use from any thread.
pub fn set_divider_width_anywhere(width: f64) {
    on_main(move || set_divider_width(width));
}

struct Ivars {
    handle: AppHandle<State>,
}

declare_class!(
    struct Clicker;

    unsafe impl ClassType for Clicker {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "IceoutClicker";
    }

    impl DeclaredClass for Clicker {
        type Ivars = Ivars;
    }

    unsafe impl Clicker {
        // Left click: the hidden icons. Right click: a small menu with Settings and Quit.
        #[method(clicked:)]
        fn clicked(&self, _sender: &AnyObject) {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let mouse = unsafe { NSEvent::mouseLocation() };
            LAST_CLICK.set((mouse.x, mouse.y));
            let event = NSApplication::sharedApplication(mtm).currentEvent();
            if event.is_some_and(|e| unsafe { e.r#type() } == NSEventType::RightMouseUp) {
                self.show_menu(mtm);
            } else if self.ivars().handle.is_visible() {
                self.ivars().handle.hide();
            } else {
                open(&self.ivars().handle, Mode::Strip);
            }
        }

        #[method(settings:)]
        fn settings(&self, _sender: &AnyObject) {
            open(&self.ivars().handle, Mode::Settings);
        }

        #[method(search:)]
        fn search(&self, _sender: &AnyObject) {
            open(&self.ivars().handle, Mode::Search);
        }

        #[method(quit:)]
        fn quit(&self, _sender: &AnyObject) {
            // Bring every hidden icon back first, so quitting never leaves the bar changed.
            set_hiding(false);
            self.ivars().handle.quit();
        }
    }
);

impl Clicker {
    /// Pops up the right-click menu. A status item only shows a menu while one is attached, so
    /// attach it, click, and take it off again so left clicks keep working.
    fn show_menu(&self, mtm: MainThreadMarker) {
        let menu = NSMenu::new(mtm);
        for (title, action) in [("Search…", sel!(search:)), ("Settings…", sel!(settings:)), ("Quit", sel!(quit:))] {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(), &NSString::from_str(title), Some(action), &NSString::from_str(""),
                )
            };
            unsafe { item.setTarget(Some(self)) };
            menu.addItem(&item);
        }
        TOGGLE.with(|t| {
            if let Some(item) = t.borrow().as_ref() {
                unsafe {
                    item.setMenu(Some(&menu));
                    if let Some(button) = item.button(mtm) {
                        button.performClick(None);
                    }
                    item.setMenu(None);
                }
            }
        });
    }
}

/// Shows the window in the given mode. The strip and Settings open under the place that was
/// clicked, on the display where the click was. The welcome window opens in the middle of the
/// main display.
pub fn open(handle: &AppHandle<State>, mode: Mode) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.iter().next() else { return };
    let primary_height = primary.frame().size.height;
    let (width, height) = crate::window_size(mode);
    let (width, height) = (width as f64, height as f64);

    let (x, y) = if mode == Mode::Welcome {
        ((primary.frame().size.width - width) / 2.0, 140.0)
    } else {
        let (mouse_x, mouse_y) = LAST_CLICK.get();
        let screen = screens
            .iter()
            .find(|screen| {
                let f = screen.frame();
                mouse_x >= f.origin.x && mouse_x < f.origin.x + f.size.width && mouse_y >= f.origin.y && mouse_y < f.origin.y + f.size.height
            })
            .unwrap_or(primary);
        let frame = screen.frame();
        // The bottom edge of the menu bar, in Cocoa coordinates (y counts up from the bottom of the main display).
        let bar_bottom = screen.visibleFrame().origin.y + screen.visibleFrame().size.height;
        // Centred under the click, but never past the screen edge.
        let left = (mouse_x - width / 2.0).clamp(frame.origin.x + 8.0, (frame.origin.x + frame.size.width - width - 8.0).max(frame.origin.x + 8.0));
        // Rune wants points counted from the top left of the main display.
        (left, primary_height - bar_bottom + 6.0)
    };

    KEEP_OPEN.store(mode == Mode::Welcome, Ordering::Relaxed);
    handle.update(move |s, tx| {
        s.mode = mode;
        s.query = rune::TextEdit::new("");
        crate::fit(s, tx);
    });
    if mode != Mode::Welcome {
        crate::reload(handle.clone());
    }
    handle.place(
        DisplayTarget::Primary,
        OverlayPlacement::Rect { x: x as f32, y: y as f32, width: width as f32, height: height as f32 },
    );
    handle.show();
}

/// `open` for a shortcut: the window opens under the mouse, on the display it is on.
pub fn open_at_mouse(handle: &AppHandle<State>, mode: Mode) {
    let mouse = unsafe { NSEvent::mouseLocation() };
    LAST_CLICK.set((mouse.x, mouse.y));
    open(handle, mode);
}

/// `open` for use from any thread.
pub fn open_anywhere(handle: AppHandle<State>, mode: Mode) {
    on_main(move || open(&handle, mode));
}

/// Remembers where the notch ends, if the main screen has one.
fn read_notch(mtm: MainThreadMarker) {
    if let Some(screen) = NSScreen::mainScreen(mtm) {
        let free = unsafe { screen.auxiliaryTopRightArea() };
        NOTCH_RIGHT.store(free.origin.x.to_bits(), Ordering::Relaxed);
    }
}

/// Adds our two buttons to the menu bar. Call it on the main thread, after the event loop starts.
pub fn add(handle: AppHandle<State>) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    items::allow_hiding_cursor();
    read_notch(mtm);
    let handle_for_monitor = handle.clone();
    let this = mtm.alloc::<Clicker>().set_ivars(Ivars { handle });
    let clicker: Retained<Clicker> = unsafe { msg_send_id![super(this), init] };
    // The toggle goes first. macOS puts the next item to its left, which is where the divider belongs.
    let toggle = unsafe { NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength) };
    if let Some(button) = unsafe { toggle.button(mtm) } {
        unsafe {
            // The Iceout mark, drawn in one colour so macOS can tint it for light and dark menu bars.
            let bytes = include_bytes!("../assets/menubar-icon.png");
            if let Some(image) = NSImage::initWithData(mtm.alloc(), &NSData::with_bytes(bytes)) {
                image.setSize(NSSize::new(18.0, 18.0));
                image.setTemplate(true);
                button.setImage(Some(&image));
            }
            button.setTarget(Some(&clicker));
            button.setAction(Some(sel!(clicked:)));
            button.sendActionOn(NSEventMask::LeftMouseUp.union(NSEventMask::RightMouseUp));
        }
    }
    let divider = unsafe { NSStatusBar::systemStatusBar().statusItemWithLength(DIVIDER_WIDTH) };
    if let Some(button) = unsafe { divider.button(mtm) } {
        unsafe { button.setTitle(&NSString::from_str(DIVIDER_TITLE)) };
    }
    // Close the window when the user clicks anywhere else. A global monitor only hears clicks that
    // go to other apps, so clicks on our own buttons and window do not trigger it, and it
    // costs nothing while nobody clicks.
    let close = handle_for_monitor;
    let on_click = RcBlock::new(move |_event: NonNull<NSEvent>| {
        if !items::SENDING_EVENTS.load(Ordering::Relaxed) && !KEEP_OPEN.load(Ordering::Relaxed) && close.is_visible() {
            close.hide();
        }
    });
    let clicks = NSEventMask::LeftMouseDown.union(NSEventMask::RightMouseDown);
    let monitor = unsafe { NSEvent::addGlobalMonitorForEventsMatchingMask_handler(clicks, &on_click) };
    // A second one tells `press_hidden` when the user has finished with a revealed icon.
    let on_finish = RcBlock::new(|_event: NonNull<NSEvent>| {
        if !items::SENDING_EVENTS.load(Ordering::Relaxed) {
            DONE_WITH_ICON.store(true, Ordering::Relaxed);
        }
    });
    let finish = NSEventMask::LeftMouseUp.union(NSEventMask::RightMouseUp).union(NSEventMask::KeyDown);
    let finish_monitor = unsafe { NSEvent::addGlobalMonitorForEventsMatchingMask_handler(finish, &on_finish) };
    // The monitors live as long as the app does.
    std::mem::forget(monitor);
    std::mem::forget(finish_monitor);
    crate::trace("buttons added");
    TOGGLE.with(|t| *t.borrow_mut() = Some(toggle));
    DIVIDER.with(|d| *d.borrow_mut() = Some(divider));
    // The clicker must live as long as the app does.
    std::mem::forget(clicker);
}

/// Sets how wide the divider is. Icons on its left are pushed that far to the left.
pub fn set_divider_width(width: f64) {
    DIVIDER.with(|d| {
        if let Some(item) = d.borrow().as_ref() {
            unsafe { item.setLength(width) };
        }
    });
}

/// Makes the divider very wide (everything on its left is pushed off the screen) or thin again.
pub fn set_hiding(on: bool) {
    set_divider_width(if on { HIDING_WIDTH } else { DIVIDER_WIDTH });
}

/// Waits until `check` is true, looking every 30 ms for up to `seconds`.
fn wait_until(seconds: f64, mut check: impl FnMut() -> bool) -> bool {
    let tries = (seconds * 1000.0 / 30.0) as usize;
    for _ in 0..tries {
        if check() {
            return true;
        }
        sleep(Duration::from_millis(30));
    }
    false
}

/// Moves one icon to the hidden side (left of the divider) or to the visible side (just left of
/// our toggle, which is the first spot right of the divider). Only that icon moves: the divider
/// never changes width, so no other icon appears. Runs on its own thread, because the system asks
/// our main thread questions while it works.
pub fn move_item(handle: AppHandle<State>, pid: i32, index: usize, hide: bool) {
    std::thread::spawn(move || {
        if let (Some(item), Some((divider, toggle))) = (items::window_of(pid, index), items::own_windows()) {
            items::move_left_of(item, if hide { divider } else { toggle });
        }
        // Make sure the divider is wide (it is thin only before the first icon is hidden).
        set_divider_width_anywhere(HIDING_WIDTH);
        handle.update(|s, _| s.busy = false);
        // Read the bar again, which shows where the icon really is now.
        crate::reload(handle);
    });
}

/// Opens a hidden icon's menu. Like Ice, this moves the icon next to the toggle, where it can be
/// seen, clicks it, and puts it back once the user is done with its menu. Every other hidden icon
/// stays hidden the whole time. The click is an accessibility press, so the cursor does not move.
pub fn press_hidden(handle: AppHandle<State>, pid: i32, index: usize, key: String) {
    std::thread::spawn(move || {
        PEEK_ACTIVE.store(true, Ordering::Relaxed);
        let shown = key.clone();
        handle.update(move |s, _| s.peeking = Some(shown));
        let moved = match (items::window_of(pid, index), items::own_windows()) {
            (Some(item), Some((_, toggle))) => items::move_left_of(item, toggle),
            _ => false,
        };
        if moved && wait_until(2.0, || items::position(pid, index).is_some()) {
            DONE_WITH_ICON.store(false, Ordering::Relaxed);
            items::press(pid, index);
            // Keep the icon where it is until the user clicks or presses a key in another app,
            // which is how a menu gets used, or until 15 seconds pass.
            for _ in 0..300 {
                if DONE_WITH_ICON.load(Ordering::Relaxed) {
                    break;
                }
                sleep(Duration::from_millis(50));
            }
        }
        // Put the icon back on the hidden side.
        if let (Some(item), Some((divider, _))) = (items::window_of(pid, index), items::own_windows()) {
            items::move_left_of(item, divider);
        }
        PEEK_ACTIVE.store(false, Ordering::Relaxed);
        handle.update(|s, _| {
            s.busy = false;
            s.peeking = None;
        });
        crate::reload(handle);
    });
}

/// Icons right of the divider that sit under the notch cannot be seen, because macOS has no room
/// for them. Move them to the hidden side, so the strip can still open them. Does nothing on a
/// Mac without a notch, or while an icon is out for a peek.
pub fn fix_overflow() {
    let notch = f64::from_bits(NOTCH_RIGHT.load(Ordering::Relaxed));
    if notch <= 0.0 || PEEK_ACTIVE.load(Ordering::Relaxed) || items::SENDING_EVENTS.load(Ordering::Relaxed) {
        return;
    }
    let Some((divider, _)) = items::own_windows() else { return };
    for window in items::windows() {
        let right_of_divider = window.x >= divider.x + divider.width - 2.0;
        let too_far_left = window.x + window.width <= notch + 2.0;
        if right_of_divider && too_far_left {
            items::move_left_of(window, divider);
        }
    }
}

/// Puts icons whose app restarted back in view. A restarted app gets a new icon at the far left,
/// which is the hidden side, even if the user had it in view.
pub fn restore_visible(handle: AppHandle<State>, targets: Vec<(i32, usize)>) {
    if RESTORING.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        for (pid, index) in targets {
            if let (Some(item), Some((_, toggle))) = (items::window_of(pid, index), items::own_windows()) {
                items::move_left_of(item, toggle);
            }
        }
        RESTORING.store(false, Ordering::Relaxed);
        crate::reload(handle);
    });
}
