//! Iceout: hide menu bar icons, and open them from one click.
//! - Click the Iceout icon: a glass strip with the hidden icons. Click one and it opens, as in the menu bar.
//! - Right-click it, then Settings: every menu bar icon with a switch. On means hidden.
//!
//! `cargo run -- --snap out/demo.png --scene settings --size 300x260` renders a window without a display.

mod items;
mod hotkeys;
mod login;
mod status_item;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use items::Item;
use rune::prefs::Prefs;
use rune::tok;
use rune::{
    text_input, App, AppHandle, AppSpec, Cx, Drag, El, ImageCache, MacWindowOptions, Phase, ScrollSpec, SystemMaterial, TextStyle, Tx, Waker, WindowOptions, col, el,
    oklcha, row, text,
};

const SETTINGS_WIDTH: f32 = 320.0;
const PAD: f32 = 20.0;
const ROW: f32 = 40.0;
const HEADER: f32 = 36.0;
const LOGO: f32 = 26.0;
/// The corner radius of the Settings and welcome windows, in points.
const WINDOW_RADIUS: f32 = 8.0;
const APP_ID: &str = "dev.iceout";
const WELCOME_WIDTH: f32 = 360.0;
const SEARCH_WIDTH: f32 = 360.0;
/// The most results the search window shows at once.
const SEARCH_ROWS: usize = 6;
const WELCOME_HEIGHT: f32 = 404.0;
const SECTION_LABEL: f32 = 26.0;
const SECTION_GAP: f32 = 14.0;
const PREVIEW_HEIGHT: f32 = 96.0;
const SEARCH_BOX: f32 = 40.0;
/// The search window keeps one size, so it does not jump while you type.
const SEARCH_HEIGHT: f32 = 2.0 * PAD + SEARCH_BOX + SEARCH_ROWS as f32 * ROW;
const SLIDER_BLOCK: f32 = 46.0;
/// The width of a slider: the window, less its padding and the padding of the card it sits in.
const SLIDER_WIDTH: f32 = SETTINGS_WIDTH - 2.0 * PAD;
const FIELDS: [Field; 4] = [Field::Icon, Field::Padding, Field::Spacing, Field::Corners];

/// How the strip looks. Changed in Settings, saved between runs.
#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    pub icon: f32,
    pub padding: f32,
    pub spacing: f32,
    pub corners: f32,
}

impl Default for Look {
    fn default() -> Self {
        Look::DEFAULT
    }
}

impl Look {
    const DEFAULT: Look = Look { icon: 28.0, padding: 10.0, spacing: 8.0, corners: 22.0 };

    /// Each icon sits in a square a little bigger than the icon.
    fn cell(&self) -> f32 {
        self.icon + 12.0
    }
}

/// One adjustable number of the look.
#[derive(Clone, Copy)]
enum Field {
    Icon,
    Padding,
    Spacing,
    Corners,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Field::Icon => "Icon size",
            Field::Padding => "Padding",
            Field::Spacing => "Spacing",
            Field::Corners => "Corner radius",
        }
    }

    fn value(self, look: &Look) -> f32 {
        match self {
            Field::Icon => look.icon,
            Field::Padding => look.padding,
            Field::Spacing => look.spacing,
            Field::Corners => look.corners,
        }
    }

    /// The smallest and largest value this field can take.
    fn range(self) -> (f32, f32) {
        match self {
            Field::Icon => (16.0, 48.0),
            Field::Padding => (4.0, 28.0),
            Field::Spacing => (0.0, 24.0),
            Field::Corners => (8.0, 32.0),
        }
    }

    /// Sets the value, rounded to a whole number and kept inside the range.
    fn set(self, look: &mut Look, value: f32) {
        let (low, high) = self.range();
        let slot = match self {
            Field::Icon => &mut look.icon,
            Field::Padding => &mut look.padding,
            Field::Spacing => &mut look.spacing,
            Field::Corners => &mut look.corners,
        };
        *slot = value.round().clamp(low, high);
    }
}

/// A copy of the look that `status_item` can read to size the window before it opens.
static LOOK: Mutex<Look> = Mutex::new(Look::DEFAULT);

fn look() -> Look {
    *LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

fn load_look() -> Look {
    let prefs = Prefs::open(APP_ID);
    let get = |key: &str, default: f32| prefs.get::<f32>(key).unwrap_or(default);
    let d = Look::DEFAULT;
    Look { icon: get("icon", d.icon), padding: get("padding", d.padding), spacing: get("spacing", d.spacing), corners: get("corners", d.corners) }
}

/// Saves on another thread, so a click never waits for the disk.
fn save_look(look: Look) {
    std::thread::spawn(move || {
        let mut prefs = Prefs::open(APP_ID);
        prefs.set("icon", &look.icon).ok();
        prefs.set("padding", &look.padding).ok();
        prefs.set("spacing", &look.spacing).ok();
        prefs.set("corners", &look.corners).ok();
    });
}

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// With `ICEOUT_TRACE=1` set, prints how long after launch something happened. For measuring startup.
pub fn trace(label: &str) {
    if std::env::var_os("ICEOUT_TRACE").is_some() {
        let ms = START.get().map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
        eprintln!("TRACE {ms:7.0} ms  {label}");
    }
}

/// How many icons are hidden, how many there are, and whether Appearance is open: what
/// `status_item` needs to size the window before it opens.
pub static HIDDEN_COUNT: AtomicUsize = AtomicUsize::new(0);
static ITEM_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADVANCED_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Mode {
    /// The hidden icons in a row.
    #[default]
    Strip,
    /// Every icon, with a switch.
    Settings,
    /// The first-launch setup.
    Welcome,
    /// A text box and the icons whose names match it.
    Search,
}

#[derive(Default)]
pub struct State {
    pub mode: Mode,
    pub look: Look,
    /// Whether the Advanced section of Settings is open.
    pub advanced: bool,
    pub items: Vec<Item>,
    pub permission: bool,
    pub cache: Option<ImageCache>,
    /// True while an icon is being moved or revealed, so a second click waits.
    pub busy: bool,
    pub handle: Option<AppHandle<State>>,
    /// The key of an icon that is shown for a moment so its menu can be used. It still counts as
    /// hidden while it is out.
    pub peeking: Option<String>,
    /// What is typed in the search box.
    pub query: rune::TextEdit,
    /// The Iceout mark, shown at the top of Settings and the welcome window.
    pub logo: Option<rune::Image>,
    /// Keys of icons the user has had in view, so an app that restarts (its new icon always starts
    /// hidden) can be put back where it was.
    pub seen_visible: HashSet<String>,
    /// The process each icon belonged to the last time we read the bar, to notice a restart.
    pub last_pid: HashMap<String, i32>,
    /// How many reads in a row each missing icon has been absent (a slow app can miss one).
    pub misses: HashMap<String, u8>,
    /// Whether the first-launch setup has been finished.
    pub setup_done: bool,
    /// Whether the program starts at login.
    pub login: bool,
    /// Whether the keyboard shortcuts are on.
    pub hotkeys: bool,
}

fn hidden(s: &State) -> impl Iterator<Item = &Item> {
    s.items.iter().filter(|i| i.hidden)
}

/// Width of the window in a mode. The strip grows with the number of hidden icons.
pub fn window_width(mode: Mode) -> f32 {
    let n = HIDDEN_COUNT.load(Ordering::Relaxed);
    let look = look();
    match mode {
        Mode::Welcome => WELCOME_WIDTH,
        Mode::Search => SEARCH_WIDTH,
        Mode::Settings => SETTINGS_WIDTH,
        Mode::Strip if n == 0 => 320.0,
        Mode::Strip => 2.0 * look.padding + n as f32 * look.cell() + (n as f32 - 1.0) * look.spacing,
    }
}

/// Height of Settings for a number of icons. The list of icons scrolls; the rows below it never
/// do. With Appearance open the list gets less room, so the sliders always fit on screen.
fn settings_height(icons: usize, advanced: bool) -> f32 {
    let rows = (icons.max(1) as f32).min(if advanced { 4.0 } else { 8.0 });
    let appearance = if advanced { PREVIEW_HEIGHT + 4.0 * SLIDER_BLOCK + 8.0 } else { 0.0 };
    let body = SECTION_LABEL + rows * ROW + SECTION_GAP + 3.0 * ROW + appearance;
    (2.0 * PAD + HEADER + SECTION_GAP + body).min(760.0)
}

/// Whether the letters of `query` appear in `name` in order, ignoring case: "mt" finds "Moneta".
fn matches(name: &str, query: &str) -> bool {
    let mut name = name.chars().flat_map(char::to_lowercase);
    query.chars().flat_map(char::to_lowercase).filter(|c| !c.is_whitespace()).all(|c| name.any(|n| n == c))
}

/// The icons that match what is typed. An empty box matches every icon.
fn search_results(s: &State) -> Vec<&Item> {
    s.items.iter().filter(|item| matches(&item.name, &s.query.text)).collect()
}

/// Opens an icon's menu: a hidden one is brought out first, then put back.
fn open_item(s: &mut State, tx: &mut Tx, item: &Item) {
    tx.close_window();
    let (false, Some(handle)) = (s.busy, s.handle.clone()) else { return };
    s.busy = true;
    if item.hidden {
        status_item::press_hidden(handle, item.pid, item.index, item.key());
    } else {
        let (pid, index) = (item.pid, item.index);
        std::thread::spawn(move || {
            items::press(pid, index);
            let _ = handle.update(|s, _| s.busy = false);
        });
    }
}

fn window_height(s: &State) -> f32 {
    match s.mode {
        Mode::Welcome => WELCOME_HEIGHT,
        Mode::Search => SEARCH_HEIGHT,
        Mode::Settings => settings_height(s.items.len(), s.advanced),
        Mode::Strip => s.look.cell() + 2.0 * s.look.padding,
    }
}

/// The size of the window in a mode, from the numbers kept in statics, for use before the window opens.
pub fn window_size(mode: Mode) -> (f32, f32) {
    let height = match mode {
        Mode::Welcome => WELCOME_HEIGHT,
        Mode::Search => SEARCH_HEIGHT,
        Mode::Settings => settings_height(ITEM_COUNT.load(Ordering::Relaxed), ADVANCED_OPEN.load(Ordering::Relaxed)),
        Mode::Strip => look().cell() + 2.0 * look().padding,
    };
    (window_width(mode), height)
}

/// Fits the window to the current mode.
pub fn fit(s: &State, tx: &mut Tx) {
    tx.resize_window((window_width(s.mode), window_height(s)));
}

/// Reads the menu bar on a background thread, then updates the state. The main thread never
/// waits for the read, so the window stays responsive.
pub fn reload(handle: AppHandle<State>) {
    std::thread::spawn(move || {
        let permission = items::has_permission();
        if permission {
            // Icons that macOS has no room for are moved to the hidden side first.
            status_item::fix_overflow();
        }
        let found = if permission { items::list(items::own_divider_x()) } else { Vec::new() };
        handle.update(move |s, tx| apply(s, tx, permission, found));
    });
}

/// Puts a fresh read of the menu bar into the state. Which icons are hidden is read from where
/// they are: an icon left of the divider is hidden, whether it was hidden by us earlier, hidden
/// by hand, or is a brand new icon, which macOS always puts at the far left.
pub(crate) fn apply(s: &mut State, tx: &mut Tx, permission: bool, mut found: Vec<Item>) {
    trace(&format!("bar read applied ({} icons)", found.len()));
    s.permission = permission;

    // A slow app can miss one read. Keep an icon that vanished if its app is still running,
    // for up to two reads, so the list does not flicker.
    let mut kept = Vec::new();
    for old in &s.items {
        let key = old.key();
        if found.iter().any(|i| i.key() == key) {
            s.misses.remove(&key);
            continue;
        }
        let misses = s.misses.entry(key.clone()).or_insert(0);
        if permission && items::is_running(old.pid) && *misses < 2 {
            *misses += 1;
            kept.push(old.clone());
        } else {
            s.misses.remove(&key);
        }
    }
    found.extend(kept);
    found.sort_by(|a, b| a.x.total_cmp(&b.x));

    // Work out which icons belong to an app that restarted since the last read. The new icon is at
    // the far left, hidden, but if the user had the old one in view it should be in view again.
    let mut restore = Vec::new();
    for item in &mut found {
        let key = item.key();
        if s.peeking.as_deref() == Some(key.as_str()) {
            item.hidden = true;
        }
        let before = s.items.iter().find(|old| old.key() == key);
        if !item.hidden {
            s.seen_visible.insert(key.clone());
        } else if before.is_some_and(|old| !old.hidden && old.pid == item.pid) {
            // Moved to the hidden side while the same app kept running: the user hid it.
            s.seen_visible.remove(&key);
        }
        let restarted = s.last_pid.get(&key).is_some_and(|&pid| pid != item.pid);
        if item.hidden && restarted && s.seen_visible.contains(&key) {
            restore.push((item.pid, item.index));
        }
        s.last_pid.insert(key, item.pid);
    }

    if let Some(cache) = &s.cache {
        for item in found.iter_mut() {
            item.icon = Some(cache.app_icon(&item.path, 28.0));
        }
    }
    s.items = found;
    HIDDEN_COUNT.store(hidden(s).count(), Ordering::Relaxed);
    ITEM_COUNT.store(s.items.len(), Ordering::Relaxed);
    // If anything sits left of the divider it should be out of sight, so make sure the divider is
    // wide. This also hides a new icon that arrived while the divider was thin.
    if s.items.iter().any(|i| i.hidden) {
        status_item::set_divider_width_anywhere(status_item::HIDING_WIDTH);
    }
    if let (false, true, Some(handle)) = (restore.is_empty() || s.busy, s.peeking.is_none(), s.handle.clone()) {
        status_item::restore_visible(handle, restore);
    }
    fit(s, tx);
}

/// Watches for icons that come and go. Every two seconds it asks the window server for the list
/// of menu bar icon windows, which is cheap, and reads the whole bar again only when that list has
/// changed. A new icon shows up in Settings and in the strip within a couple of seconds.
fn watch(handle: AppHandle<State>) {
    std::thread::spawn(move || {
        // Start from what is there now: the first read has just happened, so nothing is new yet.
        let mut known: Vec<u32> = items::windows().iter().map(|w| w.id).collect();
        known.sort_unstable();
        let mut allowed = items::has_permission();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // The first-launch setup waits for the user to turn Accessibility on.
            let now_allowed = items::has_permission();
            if now_allowed != allowed {
                allowed = now_allowed;
                reload(handle.clone());
            }
            if items::SENDING_EVENTS.load(Ordering::Relaxed) {
                continue;
            }
            let mut now: Vec<u32> = items::windows().iter().map(|w| w.id).collect();
            now.sort_unstable();
            if now != known {
                known = now;
                // An app's icon can appear a moment before its accessibility entry does.
                std::thread::sleep(std::time::Duration::from_millis(400));
                reload(handle.clone());
            }
        }
    });
}

/// A small on/off switch. The knob slides over a track that turns accent colour when it is on.
fn switch(on: bool, dark: bool) -> El {
    let t = tok::theme(dark);
    let knob = el().size(16.0, 16.0).radius(8.0).fill(t.knob).shadow(1.0, 2.0, t.knob_shadow).absolute(if on { 16.0 } else { 2.0 }, 2.0);
    el().size(34.0, 20.0).radius(10.0).fill(if on { tok::accent(0, dark) } else { line_fill(dark) }).child(knob)
}

/// The picture for an item: its app icon, or a letter tile for Apple's own items.
fn picture(item: &Item, size: f32, dark: bool) -> El {
    let t = tok::theme(dark);
    match &item.icon {
        Some(icon) => el().size(size, size).pixels(icon.pixels()),
        None => col().size(size, size).radius(size * 0.28).fill(t.control_off).center()
            .child(text(item.name.chars().next().unwrap_or('?').to_string()).font(TextStyle::new(size * 0.5, 600.0)).color(t.text)),
    }
}

fn glass(radius: f32) -> El {
    col().w_full().h_full().radius(radius).system_material(SystemMaterial::Glass)
}

/// A faint card behind a group of rows, and the line between rows.
fn card_fill(dark: bool) -> rune::Color {
    if dark { oklcha(1.0, 0.0, 0.0, 0.07) } else { oklcha(0.0, 0.0, 0.0, 0.05) }
}

fn line_fill(dark: bool) -> rune::Color {
    if dark { oklcha(1.0, 0.0, 0.0, 0.1) } else { oklcha(0.0, 0.0, 0.0, 0.08) }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let t = tok::theme(dark);
    if s.mode == Mode::Welcome {
        return welcome_view(s, dark);
    }
    if !s.permission {
        return glass(WINDOW_RADIUS).padding(PAD).child(text("Turn on Accessibility for this app in System Settings, Privacy & Security.").color(t.text));
    }
    match s.mode {
        Mode::Strip => strip_view(s, dark),
        Mode::Settings => settings_view(s, dark),
        Mode::Welcome => welcome_view(s, dark),
        Mode::Search => search_view(s, dark),
    }
}

fn strip_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let hidden: Vec<&Item> = hidden(s).collect();
    if hidden.is_empty() {
        return glass(s.look.corners).padding(PAD).center().child(text("Nothing hidden yet. Right-click the Iceout icon, then Settings.").color(t.text));
    }
    let icons = hidden.into_iter().enumerate().map(|(i, item)| {
        let (pid, index, key) = (item.pid, item.index, item.key());
        col().key(format!("hidden-{i}")).role("button").label(item.name.as_str()).hit()
            .size(s.look.cell(), s.look.cell()).radius(10.0).center()
            .on_press_tx(move |s: &mut State, tx| {
                if let (false, Some(handle)) = (s.busy, s.handle.clone()) {
                    s.busy = true;
                    status_item::press_hidden(handle, pid, index, key.clone());
                    tx.close_window();
                }
            })
            .child(picture(item, s.look.icon, dark))
    });
    glass(s.look.corners).child(row().w_full().h_full().center_y().padding(s.look.padding).gap(s.look.spacing).children(icons))
}

/// The Iceout mark at a size, with rounded corners.
fn logo(s: &State, size: f32) -> El {
    match &s.logo {
        Some(image) => el().size(size, size).radius(size * 0.24).pixels(image.pixels()),
        None => el().size(size, size),
    }
}

/// A thin line across the window, used instead of boxes to separate things.
fn hairline(dark: bool) -> El {
    el().w_full().h(0.5).fill(line_fill(dark))
}

/// A small label over a group of rows.
fn section_label(label: &str, dark: bool) -> El {
    text(label).font(TextStyle::new(11.0, 600.0)).color(tok::theme(dark).text_secondary).h(SECTION_LABEL)
}

fn search_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let found: Vec<Item> = search_results(s).into_iter().take(SEARCH_ROWS).cloned().collect();
    let first = found.first().cloned();
    let input = text_input(&s.query, TextStyle::new(15.0, 450.0))
        .on_edit(|s: &mut State| &mut s.query)
        .on_submit(move |s: &mut State, tx| {
            if let Some(item) = &first {
                open_item(s, tx, item);
            }
        })
        .autofocus()
        .label("Search menu bar icons")
        .w_full()
        .h(SEARCH_BOX);
    let rows = found.into_iter().enumerate().map(|(i, item)| {
        let name = item.name.clone();
        let icon = picture(&item, 22.0, dark);
        row().key(format!("result-{i}")).role("button").label(name.as_str()).hit().w_full().h(ROW).center_y().gap(12.0)
            .on_press_tx(move |s: &mut State, tx| open_item(s, tx, &item))
            .child(icon)
            .child(text(name).font(TextStyle::new(13.5, 450.0)).color(t.text))
    });
    glass(WINDOW_RADIUS).padding(PAD).child(input).child(hairline(dark)).children(rows)
}

fn settings_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let header = row().w_full().h(HEADER).center_y()
        .child(logo(s, LOGO))
        .child(el().grow(1.0))
        .child(
            row().key("done").role("button").label("Done").hit().px(10.0).h(28.0).center_y().radius(6.0)
                .on_press_tx(|_: &mut State, tx| tx.close_window())
                .child(text("Done").font(TextStyle::new(13.0, 600.0)).color(tok::accent(0, dark))),
        );

    // Every icon is one flat row with a thin line under it, straight on the glass.
    let last = s.items.len().saturating_sub(1);
    let rows = s.items.iter().enumerate().map(|(i, item)| {
        let (pid, index, hide) = (item.pid, item.index, !item.hidden);
        col().w_full()
            .child(
                row().key(format!("item-{i}")).role("switch").label(item.name.as_str()).checked(item.hidden).hit()
                    .w_full().h(ROW - 0.5).center_y().gap(12.0)
                    .on_press_tx(move |s: &mut State, _| {
                        if let (false, Some(handle)) = (s.busy, s.handle.clone()) {
                            s.busy = true;
                            status_item::move_item(handle, pid, index, hide);
                        }
                    })
                    .child(picture(item, 22.0, dark))
                    .child(text(item.name.as_str()).font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
                    .child(switch(item.hidden, dark)),
            )
            .child(if i == last { el().w_full().h(0.5) } else { el().w_full().h(0.5).fill(line_fill(dark)) })
    });
    let list = col().key("body").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).children(rows);
    let icons_section = col().w_full().grow(1.0).min_h(0.0).child(section_label("HIDDEN ICONS", dark)).child(list);

    glass(WINDOW_RADIUS).padding(PAD).gap(SECTION_GAP)
        .child(header)
        .child(icons_section)
        .child(col().w_full().child(hairline(dark)).child(login_row(s, dark)).child(hairline(dark)).child(hotkeys_row(s, dark)).child(hairline(dark)).child(advanced_section(s, dark)))
}

/// One row with a switch: whether the program starts when you log in.
fn login_row(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    row().key("login").role("switch").label("Open at login").checked(s.login).hit()
        .w_full().h(ROW).center_y()
        .on_press_tx(|s: &mut State, _| {
            s.login = !s.login;
            let on = s.login;
            std::thread::spawn(move || {
                login::set(on);
            });
        })
        .child(text("Open at login").font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
        .child(switch(s.login, dark))
}

/// One row with a switch: the keyboard shortcuts (Control + Option + Space, and + F for search).
fn hotkeys_row(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    row().key("hotkeys").role("switch").label("Keyboard shortcuts").checked(s.hotkeys).hit()
        .w_full().h(ROW).center_y()
        .on_press_tx(|s: &mut State, _| {
            s.hotkeys = !s.hotkeys;
            hotkeys::set_enabled(s.hotkeys);
            let on = s.hotkeys;
            std::thread::spawn(move || {
                Prefs::open(APP_ID).set("hotkeys", &on).ok();
            });
        })
        .child(col().grow(1.0)
            .child(text("Keyboard shortcuts").font(TextStyle::new(13.5, 450.0)).color(t.text))
            .child(text("⌃⌥Space shows icons, ⌃⌥F searches").font(TextStyle::new(11.0, 450.0)).color(t.text_secondary)))
        .child(switch(s.hotkeys, dark))
}

/// The first-launch setup: the mark, a short welcome, two plain rows, and one button.
fn welcome_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let allowed = s.permission;
    let step = |number: &str, title: &str, detail: &str, action: El| {
        row().w_full().gap(14.0)
            .child(text(number.to_string()).font(TextStyle::new(12.0, 600.0)).color(t.text_secondary).w(14.0).mt(1.0))
            .child(
                col().grow(1.0).min_w(0.0).gap(2.0)
                    .child(text(title.to_string()).font(TextStyle::new(13.5, 600.0)).color(t.text))
                    .child(text(detail.to_string()).wrap().font(TextStyle::new(12.0, 450.0)).color(t.text_secondary))
                    .child(action.mt(6.0)),
            )
    };
    let allow = if allowed {
        row().h(28.0).center_y().child(text("Allowed ✓").font(TextStyle::new(12.5, 600.0)).color(tok::accent(0, dark)))
    } else {
        row().key("allow").role("button").label("Open System Settings").hit().h(28.0).center_y()
            .on_press_tx(|_: &mut State, _| {
                std::thread::spawn(|| {
                    std::process::Command::new("open")
                        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                        .status()
                        .ok();
                });
            })
            .child(text("Open System Settings ›").font(TextStyle::new(12.5, 600.0)).color(tok::accent(0, dark)))
    };
    let finish = row().key("start").role("button").label("Get started").hit().w_full().h(38.0).center().radius(8.0)
        .fill(if allowed { tok::accent(0, dark) } else { line_fill(dark) })
        .on_press_tx(move |s: &mut State, tx| {
            if s.permission {
                status_item::KEEP_OPEN.store(false, std::sync::atomic::Ordering::Relaxed);
                s.setup_done = true;
                std::thread::spawn(|| {
                    Prefs::open(APP_ID).set("setup_done", &true).ok();
                });
                tx.close_window();
            }
        })
        .child(text("Get started").font(TextStyle::new(13.5, 600.0)).color(if allowed { rune::Color::WHITE } else { t.text_secondary }));
    glass(WINDOW_RADIUS).padding(28.0).gap(18.0)
        .child(logo(s, 52.0))
        .child(
            col().gap(4.0)
                .child(text("Welcome to Ice Out").font(TextStyle::new(20.0, 700.0)).color(t.text))
                .child(text("Hide the menu bar icons you do not need. Open them from one click.").wrap().font(TextStyle::new(13.0, 450.0)).color(t.text_secondary)),
        )
        .child(
            col().w_full().gap(14.0).child(hairline(dark))
                .child(step("1", "Allow Accessibility", "Needed to see and move menu bar icons. Nothing else is read.", allow))
                .child(hairline(dark))
                .child(row().w_full().gap(14.0).center_y()
                    .child(text("2".to_string()).font(TextStyle::new(12.0, 600.0)).color(t.text_secondary).w(14.0))
                    .child(text("Open at login").font(TextStyle::new(13.5, 600.0)).color(t.text).grow(1.0))
                    .child(row().key("login-welcome").role("switch").label("Open at login").checked(s.login).hit().child(switch(s.login, dark))
                        .on_press_tx(|s: &mut State, _| {
                            s.login = !s.login;
                            let on = s.login;
                            std::thread::spawn(move || {
                                login::set(on);
                            });
                        }))),
        )
        .child(el().grow(1.0))
        .child(finish)
}

/// The Appearance row: a header that opens it, and inside, a live preview of the strip and one
/// slider for each part of its look.
fn advanced_section(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let arrow = if s.advanced { "⌃" } else { "⌄" };
    let header = row().key("advanced").role("button").label("Appearance").hit().w_full().h(ROW).center_y()
        .on_press_tx(|s: &mut State, tx| {
            s.advanced = !s.advanced;
            ADVANCED_OPEN.store(s.advanced, Ordering::Relaxed);
            fit(s, tx);
        })
        .child(text("Appearance").font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
        .child(text(arrow).font(TextStyle::new(13.0, 600.0)).color(t.text_secondary));
    let section = col().w_full().child(header);
    if !s.advanced {
        return section;
    }
    section
        .child(preview(s, dark))
        .child(col().w_full().pb(8.0).children(FIELDS.into_iter().map(|field| slider(field, s.look, dark))))
}

/// The strip as it would look with the current sliders, drawn with a few of the real icons.
fn preview(s: &State, dark: bool) -> El {
    let look = s.look;
    let icons = s.items.iter().take(4).map(|item| {
        col().size(look.cell(), look.cell()).radius(10.0).center().child(picture(item, look.icon, dark))
    });
    row().key("preview").w_full().h(PREVIEW_HEIGHT).center()
        .child(row().center_y().padding(look.padding).gap(look.spacing).radius(look.corners).fill(card_fill(dark)).children(icons))
}

/// A name and its number on one line, and a slider under them. Dragging changes the look at
/// once, so the preview follows the thumb. The value is saved when the drag ends.
fn slider(field: Field, look: Look, dark: bool) -> El {
    let t = tok::theme(dark);
    let (w, h, k, bar) = (SLIDER_WIDTH, 20.0, 16.0, 3.0);
    let travel = w - k;
    let (low, high) = field.range();
    let value = field.value(&look);
    let v = (value - low) / (high - low);
    let to_value = move |x: f32| low + ((x - k / 2.0) / travel).clamp(0.0, 1.0) * (high - low);
    let set = move |s: &mut State, value: f32| {
        field.set(&mut s.look, value);
        *LOOK.lock().unwrap_or_else(|e| e.into_inner()) = s.look;
    };
    let label = row().w_full().h(20.0).center_y()
        .child(text(field.name()).font(TextStyle::new(13.0, 450.0)).color(t.text_secondary).grow(1.0))
        .child(text(format!("{value:.0}")).font(TextStyle::new(13.0, 550.0)).color(t.text));
    let track = el().key(field.name()).role("slider").label(field.name())
        .range_value(f64::from(value), f64::from(low), f64::from(high))
        .range_step(1.0)
        .a11y_value(format!("{value:.0}"))
        .on_step(move |s: &mut State, steps, _| {
            set(s, field.value(&s.look) + steps as f32);
            save_look(s.look);
        })
        .size(w, h)
        .on_drag(move |s: &mut State, d: &mut Drag, tx| match d.phase {
            Phase::Down | Phase::Move => {
                set(s, to_value(d.x));
                tx.instant();
            }
            Phase::Up => save_look(s.look),
        })
        .child(el().key("track").size(w, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(line_fill(dark)).a11y_hidden())
        .child(el().key("fill").size(k / 2.0 + v * travel, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(tok::accent(0, dark)).a11y_hidden())
        .child(el().key("knob").size(k, k).absolute(v * travel, (h - k) / 2.0).radius(k / 2.0).fill(t.knob).shadow(1.0, 3.0, t.knob_shadow).a11y_hidden());
    col().w_full().h(SLIDER_BLOCK).gap(6.0).py(8.0).child(label).child(track)
}

fn init() -> State {
    let look = load_look();
    *LOOK.lock().unwrap_or_else(|e| e.into_inner()) = look;
    let setup_done = Prefs::open(APP_ID).get::<bool>("setup_done").unwrap_or(false);
    let hotkeys = Prefs::open(APP_ID).get::<bool>("hotkeys").unwrap_or(true);
    State { look, setup_done, login: login::enabled(), hotkeys, ..State::default() }
}

fn setup(s: &mut State, waker: Waker) {
    let cache = ImageCache::new(waker);
    s.logo = Some(cache.load(include_bytes!("../assets/logo.png").as_slice()));
    s.cache = Some(cache);
}

fn demo_items(s: &mut State) {
    s.permission = true;
    for (i, name) in ["Wi-Fi", "Battery", "Clock", "Bluetooth"].into_iter().enumerate() {
        s.items.push(Item {
            pid: 0, index: i, name: name.to_string(), x: i as f64 * 30.0, width: 24.0, hidden: i % 2 == 0,
            path: String::new(), icon: None,
        });
    }
    HIDDEN_COUNT.store(2, Ordering::Relaxed);
}

fn main() {
    START.get_or_init(std::time::Instant::now);
    let spec = AppSpec::<State> {
        scenes: &[
            ("strip", |s| { demo_items(s); s.mode = Mode::Strip; }),
            ("settings", |s| { demo_items(s); s.mode = Mode::Settings; }),
            ("advanced", |s| { demo_items(s); s.mode = Mode::Settings; s.advanced = true; }),
            ("search", |s| { demo_items(s); s.query = rune::TextEdit::new("b"); s.mode = Mode::Search; }),
            ("welcome", |s| { demo_items(s); s.mode = Mode::Welcome; }),
            ("welcome-blocked", |s| { s.permission = false; s.mode = Mode::Welcome; }),
        ],
        setup: Some(setup),
        min_size: Some((40.0, 40.0)),
        ..AppSpec::new("Ice Out", (SETTINGS_WIDTH, 260.0), init, view)
    };
    // No Dock icon, no window at launch, and the process stays alive when the window hides.
    let app = App::new(spec)
        .accessory()
        .windowless()
        .keep_running()
        .release_gpu_when_closed()
        .window_options(WindowOptions {
            resizable: false,
            borderless: true,
            transparent: true,
            always_on_top: true,
            min_size: Some((40.0, 40.0)),
            ..Default::default()
        })
        // Round the window itself, so no square edge shows around the glass. No system shadow: it
        // is drawn from an old picture of the window and leaves a border behind when it resizes.
        .macos_window(MacWindowOptions::default().corner_radius(8.0).shadow(false));
    let handle = app.handle();
    app.handle().update({
        let handle = handle.clone();
        move |s, _| s.handle = Some(handle)
    });
    // Updates run on the main thread inside the event loop, which is where AppKit calls belong.
    app.handle().update(move |s, _| {
        status_item::add(handle.clone());
        hotkeys::install(handle);
        hotkeys::set_enabled(s.hotkeys);
    });
    // A second after launch the buttons have their place in the bar, so the first read waits for
    // that (the main thread must keep running meanwhile, so this waits on its own thread). Then
    // keep watching for icons that come and go.
    let startup = app.handle();
    std::thread::spawn(move || {
        // Read the bar as soon as our own buttons have their place in it, instead of after a fixed wait.
        for _ in 0..100 {
            if items::own_windows().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        reload(startup.clone());
        // First launch (or Accessibility is off): show the setup.
        startup.update(|s, _| {
            if let (true, Some(h)) = (!s.setup_done || !items::has_permission(), s.handle.clone()) {
                status_item::open_anywhere(h, Mode::Welcome);
            }
        });
        watch(startup);
    });
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn runtime(mode: Mode) -> Runtime<State> {
        let mut state = State::default();
        demo_items(&mut state);
        state.mode = mode;
        let size = (window_width(mode), window_height(&state));
        let mut rt = Runtime::new(state, view, size, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        rt
    }

    #[test]
    fn settings_has_a_row_for_every_item() {
        let rt = runtime(Mode::Settings);
        for i in 0..4 {
            assert!(rt.find(&format!("item-{i}")).is_some(), "item-{i} is drawn");
        }
    }

    #[test]
    fn dragging_a_slider_changes_the_look() {
        let mut state = State::default();
        demo_items(&mut state);
        state.mode = Mode::Settings;
        state.advanced = true;
        let size = (window_width(Mode::Settings), window_height(&state));
        let mut rt = Runtime::new(state, view, size, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("Icon size").unwrap();
        // The right end of the slider is the largest size.
        rt.pointer_down(x + 125.0, y);
        rt.pointer_up(x + 125.0, y);
        rt.frame(0.0);
        assert!(rt.state.look.icon >= 44.0, "icon size is {}", rt.state.look.icon);
        *LOOK.lock().unwrap() = Look::DEFAULT;
    }

    #[test]
    fn search_matches_letters_in_order() {
        assert!(matches("Moneta", "mt"));
        assert!(matches("Moneta", "MO"));
        assert!(!matches("Moneta", "tm"));
        assert!(matches("Wi-Fi", ""));
    }

    #[test]
    fn search_lists_only_matching_items() {
        let mut state = State::default();
        demo_items(&mut state);
        state.query = rune::TextEdit::new("bat");
        let names: Vec<_> = search_results(&state).into_iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Battery"]);
    }

    #[test]
    fn the_strip_shows_only_hidden_items() {
        let rt = runtime(Mode::Strip);
        assert!(rt.find("hidden-0").is_some() && rt.find("hidden-1").is_some());
        assert!(rt.find("hidden-2").is_none());
    }
}
