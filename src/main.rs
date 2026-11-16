//! Iceout: hide menu bar icons, and open them from one click.
//! - Click the Iceout icon: a glass strip with the hidden icons. Click one and it opens, as in the menu bar.
//! - Right-click it, then Settings: every menu bar icon with a switch. On means hidden.
//!
//! `cargo run -- --snap out/demo.png --scene settings --size 300x260` renders a window without a display.

mod items;
mod hotkeys;
mod presets;
mod triggers;
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
/// A trigger being made: what it waits for, the text for that (a time range or an app name), and the preset it applies.
#[derive(Default)]
pub struct Draft {
    pub kind: String,
    pub text: rune::TextEdit,
    pub preset: Option<String>,
    /// For "an app is open": every installed app to choose from, a box to narrow them, and the ones chosen.
    pub apps: Vec<String>,
    pub filter: rune::TextEdit,
    pub chosen: Vec<String>,
}

pub const APP_ID: &str = "dev.iceout";
const WELCOME_WIDTH: f32 = 360.0;
const SEARCH_WIDTH: f32 = 360.0;
/// The most results the search window shows at once.
const SEARCH_ROWS: usize = 6;
const WELCOME_HEIGHT: f32 = 440.0;
const SECTION_LABEL: f32 = 26.0;
const SECTION_GAP: f32 = 14.0;
const PREVIEW_HEIGHT: f32 = 96.0;
/// How many presets the new-trigger form lists.
const FORM_PRESETS: usize = 4;
/// How many apps show at once in the new-trigger form; the rest scroll.
const APP_ROWS: usize = 4;
const SEARCH_BOX: f32 = 40.0;
const SEARCH_PAD: f32 = 12.0;
/// The space between the search box and the results.
const SEARCH_GAP: f32 = 8.0;
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
    // Tests drag the sliders; they must not write the real settings file.
    if cfg!(test) {
        return;
    }
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
static PRESET_COUNT: AtomicUsize = AtomicUsize::new(0);
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
    /// Saved layouts: pick one to apply it, or save the current one.
    Presets,
    /// Rules that apply a preset on their own.
    Triggers,
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
    /// Saved layouts, and the name being typed for a new one.
    pub presets: Vec<presets::Preset>,
    pub preset_name: rune::TextEdit,
    /// The rules, which one is in force, and the new rule being made (if any).
    pub triggers: Vec<triggers::Trigger>,
    pub last_trigger: Option<usize>,
    pub draft: Option<Draft>,
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
        Mode::Search | Mode::Presets | Mode::Triggers => SEARCH_WIDTH,
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
    let body = SECTION_LABEL + rows * ROW + SECTION_GAP + 5.0 * ROW + appearance;
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

/// Opens an icon's menu: a hidden one is brought out first, then put back. With `right` it is a
/// right click, for the apps that keep their menu there.
fn open_item(s: &mut State, tx: &mut Tx, item: &Item, right: bool) {
    tx.close_window();
    let (false, Some(handle)) = (s.busy, s.handle.clone()) else { return };
    s.busy = true;
    if item.hidden {
        status_item::press_hidden(handle, item.pid, item.index, item.key(), right);
    } else {
        let (pid, index) = (item.pid, item.index);
        std::thread::spawn(move || {
            items::press(pid, index, right);
            handle.update(|s, _| s.busy = false);
        });
    }
}

/// The search window is as tall as its results (at least one row, for the "no match" line).
fn search_height(results: usize) -> f32 {
    2.0 * SEARCH_PAD + SEARCH_BOX + 0.5 + SEARCH_GAP + results.clamp(1, SEARCH_ROWS) as f32 * ROW
}

/// The presets window: a title, the presets (at least one row, for the empty line), a thin line
/// and the box for a new name.
fn presets_height(count: usize) -> f32 {
    2.0 * SEARCH_PAD + HEADER + count.clamp(1, SEARCH_ROWS) as f32 * ROW + 0.5 + SEARCH_GAP + SEARCH_BOX
}

/// The triggers window: the list (a title, the rules, a line, one row to add) or the form for a new rule.
fn triggers_height(s: &State) -> f32 {
    let top = 2.0 * SEARCH_PAD + HEADER;
    let Some(draft) = &s.draft else {
        return top + s.triggers.len().clamp(1, SEARCH_ROWS) as f32 * ROW + 0.5 + SEARCH_GAP + ROW;
    };
    let extra = match draft.kind.as_str() {
        triggers::TIME => SEARCH_BOX,
        triggers::APP => SEARCH_BOX + APP_ROWS as f32 * ROW,
        _ => 0.0,
    };
    let presets = s.presets.len().clamp(1, FORM_PRESETS) as f32 * ROW;
    top + SECTION_LABEL + 4.0 * ROW + extra + SECTION_GAP + SECTION_LABEL + presets + SEARCH_GAP + ROW
}

/// Saves the icons that are hidden now under the typed name, replacing a preset of that name.
fn save_preset(s: &mut State, tx: &mut Tx) {
    if add_preset(s) {
        presets_changed(s, tx);
    }
}

/// The state change of `save_preset`: false when the name is empty.
fn add_preset(s: &mut State) -> bool {
    let name = s.preset_name.text.trim().to_string();
    if name.is_empty() {
        return false;
    }
    let hidden = presets::hidden_now(s);
    match s.presets.iter_mut().find(|p| p.0 == name) {
        Some(existing) => existing.1 = hidden,
        None => s.presets.push((name, hidden)),
    }
    s.preset_name = rune::TextEdit::new("");
    true
}

fn delete_preset(s: &mut State, tx: &mut Tx, name: &str) {
    s.presets.retain(|p| p.0 != name);
    // A trigger for a preset that is gone has nothing to apply.
    s.triggers.retain(|t| t.2 != name);
    s.last_trigger = None;
    triggers::save(s.triggers.clone());
    presets_changed(s, tx);
}

fn presets_changed(s: &State, tx: &mut Tx) {
    PRESET_COUNT.store(s.presets.len(), Ordering::Relaxed);
    status_item::set_preset_names(s.presets.iter().map(|p| p.0.clone()).collect());
    presets::save(s.presets.clone());
    fit(s, tx);
}

/// Moves the icons that differ from the preset, one at a time, on a background thread.
/// Returns false when it could not start because an icon is being moved.
pub fn apply_preset(s: &mut State, name: &str) -> bool {
    let (false, Some(handle)) = (s.busy, s.handle.clone()) else { return false };
    let Some(preset) = s.presets.iter().find(|p| p.0 == name) else { return true };
    let moves = presets::moves(s, preset);
    if !moves.is_empty() {
        s.busy = true;
        status_item::move_items(handle, moves);
    }
    true
}

fn window_height(s: &State) -> f32 {
    match s.mode {
        Mode::Welcome => WELCOME_HEIGHT,
        Mode::Search => search_height(search_results(s).len()),
        Mode::Presets => presets_height(s.presets.len()),
        Mode::Triggers => triggers_height(s),
        Mode::Settings => settings_height(s.items.len(), s.advanced),
        Mode::Strip => s.look.cell() + 2.0 * s.look.padding,
    }
}

/// The size of the window in a mode, from the numbers kept in statics, for use before the window opens.
pub fn window_size(mode: Mode) -> (f32, f32) {
    let height = match mode {
        Mode::Welcome => WELCOME_HEIGHT,
        Mode::Search => search_height(ITEM_COUNT.load(Ordering::Relaxed)),
        Mode::Presets => presets_height(PRESET_COUNT.load(Ordering::Relaxed)),
        Mode::Triggers => presets_height(1),
        Mode::Settings => settings_height(ITEM_COUNT.load(Ordering::Relaxed), ADVANCED_OPEN.load(Ordering::Relaxed)),
        Mode::Strip => look().cell() + 2.0 * look().padding,
    };
    (window_width(mode), height)
}

/// Fits the window to the current mode.
pub fn fit(s: &State, tx: &mut Tx) {
    tx.resize_window((window_width(s.mode), window_height(s)));
}

/// `fit` without the animation, for switching between two screens: the content would stretch while the window grows.
fn fit_instant(s: &State, tx: &mut Tx) {
    tx.resize_window_instant((window_width(s.mode), window_height(s)));
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
    if permission && s.hotkeys {
        hotkeys::set_enabled(true);
    }

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
    } else if !s.busy && s.peeking.is_none() {
        // Nothing is hidden (or Accessibility is off), so the divider has no job and leaves the bar.
        status_item::remove_divider_anywhere();
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
        return glass(WINDOW_RADIUS).padding(PAD).child(text("Turn on Accessibility for this app in System Settings, Privacy & Security.").wrap().color(t.text));
    }
    match s.mode {
        Mode::Strip => strip_view(s, dark),
        Mode::Settings => settings_view(s, dark),
        Mode::Welcome => welcome_view(s, dark),
        Mode::Search => search_view(s, dark),
        Mode::Presets => presets_view(s, dark),
        Mode::Triggers => triggers_view(s, dark),
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
        // The same open for both clicks. A right click opens the app's right-click menu.
        let open = move |s: &mut State, tx: &mut Tx, right: bool| {
            if let (false, Some(handle)) = (s.busy, s.handle.clone()) {
                s.busy = true;
                status_item::press_hidden(handle, pid, index, key.clone(), right);
                tx.close_window();
            }
        };
        let open_right = open.clone();
        col().key(format!("hidden-{i}")).role("button").label(item.name.as_str()).hit()
            .size(s.look.cell(), s.look.cell()).radius(10.0).center()
            .on_press_tx(move |s: &mut State, tx| open(s, tx, false))
            .on_secondary_press(move |s: &mut State, _, tx| open_right(s, tx, true))
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
    let field = text_input(&s.query, TextStyle::new(15.0, 450.0))
        .on_edit(|s: &mut State| &mut s.query)
        .on_submit(move |s: &mut State, tx| {
            if let Some(item) = &first {
                open_item(s, tx, item, false);
            }
        })
        .key("search-box")
        .on_input_key(|_: &mut State, key: &rune::KeyEvent, tx| {
            let escape = key.key == rune::Key::Escape;
            if escape {
                tx.close_window();
            }
            escape
        })
        .single_line()
        .placeholder("Search icons", t.text_secondary)
        .on_input_change(|s: &mut State, tx| fit(s, tx))
        .autofocus()
        .label("Search menu bar icons")
        .grow(1.0);
    // The box has no fill and no corners: just text on the glass, with a thin line under it.
    let input = row().w_full().h(SEARCH_BOX).px(6.0).center_y().child(field);
    let rows = found.into_iter().enumerate().map(|(i, item)| {
        let name = item.name.clone();
        let icon = picture(&item, 22.0, dark);
        let right_item = item.clone();
        row().key(format!("result-{i}")).role("button").label(name.as_str()).hit().w_full().h(ROW).center_y().gap(12.0).px(6.0)
            .on_press_tx(move |s: &mut State, tx| open_item(s, tx, &item, false))
            .on_secondary_press(move |s: &mut State, _, tx| open_item(s, tx, &right_item, true))
            .child(icon)
            .child(text(name).font(TextStyle::new(13.5, 450.0)).color(t.text))
    });
    let none = s.items.iter().all(|item| !matches(&item.name, &s.query.text)).then(|| {
        row().w_full().h(ROW).center_y().px(6.0).child(text("No matching icons").font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
    });
    glass(WINDOW_RADIUS).padding(SEARCH_PAD).child(input).child(hairline(dark)).child(el().h(SEARCH_GAP)).children(rows).children(none)
}

/// The presets: a title, one row per preset (press to apply, x to delete), and a box to save the
/// current layout under a new name. Plain text on the glass, like search.
fn presets_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let header = row().w_full().h(HEADER).center_y()
        .child(text("Presets").font(TextStyle::new(15.0, 600.0)).color(t.text).grow(1.0))
        .child(row().key("presets-back").role("button").label("Back").hit().h(HEADER).center_y()
            .on_press_tx(|s: &mut State, tx| {
                s.mode = Mode::Settings;
                fit_instant(s, tx);
            })
            .child(text("Back").font(TextStyle::new(13.5, 450.0)).color(tok::accent(0, dark))));
    let rows = s.presets.iter().take(SEARCH_ROWS).enumerate().map(|(i, preset)| {
        let (apply, remove) = (preset.0.clone(), preset.0.clone());
        let active = presets::is_active(s, preset).then(|| text("Active").font(TextStyle::new(11.0, 450.0)).color(t.text_secondary));
        row().w_full().h(ROW).center_y().px(6.0).gap(8.0)
            .child(row().key(format!("preset-{i}")).role("button").label(preset.0.as_str()).hit().grow(1.0).h(ROW).center_y().gap(8.0)
                .on_press_tx(move |s: &mut State, tx| {
                    apply_preset(s, &apply);
                    tx.close_window();
                })
                .child(text(preset.0.as_str()).font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
                .children(active))
            .child(row().key(format!("preset-delete-{i}")).role("button").label("Delete preset").hit().size(28.0, ROW).center()
                .on_press_tx(move |s: &mut State, tx| delete_preset(s, tx, &remove))
                .child(text("×").font(TextStyle::new(16.0, 450.0)).color(t.text_secondary)))
    });
    let none = s.presets.is_empty().then(|| {
        row().w_full().h(ROW).center_y().px(6.0).child(text("No presets yet").font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
    });
    let field = text_input(&s.preset_name, TextStyle::new(14.0, 450.0))
        .on_edit(|s: &mut State| &mut s.preset_name)
        .on_submit(|s: &mut State, tx| save_preset(s, tx))
        .key("preset-box")
        .single_line()
        .placeholder("Save the current layout as…", t.text_secondary)
        .label("Name for a new preset")
        .grow(1.0);
    let input = row().w_full().h(SEARCH_BOX).px(6.0).center_y().child(field);
    glass(WINDOW_RADIUS).key("presets-screen").padding(SEARCH_PAD).child(header).children(rows).children(none).child(hairline(dark)).child(el().h(SEARCH_GAP)).child(input)
}

/// A row with a dot (a square when several can be chosen): filled when chosen. Used in the new-trigger form.
fn choice(key: String, label: &str, chosen: bool, multi: bool, dark: bool, on_press: impl Fn(&mut State, &mut Tx) + 'static) -> El {
    let t = tok::theme(dark);
    let dot = if chosen { tok::accent(0, dark) } else { line_fill(dark) };
    row().key(key).role("radio").label(label).checked(chosen).hit().w_full().h(ROW).center_y().px(6.0).gap(12.0)
        .on_press_tx(on_press)
        .child(el().size(8.0, 8.0).radius(if multi { 2.0 } else { 4.0 }).fill(dot).a11y_hidden())
        .child(text(label).font(TextStyle::new(13.5, 450.0)).color(t.text))
}

/// The rules, or the form for a new one. Plain words and thin lines, like Presets.
fn triggers_view(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let title = if s.draft.is_some() { "New trigger" } else { "Triggers" };
    let header = row().w_full().h(HEADER).center_y()
        .child(text(title).font(TextStyle::new(15.0, 600.0)).color(t.text).grow(1.0))
        .child(row().key("triggers-back").role("button").label("Back").hit().h(HEADER).center_y()
            .on_press_tx(|s: &mut State, tx| {
                // Back leaves the form first, then the list.
                if s.draft.take().is_none() {
                    s.mode = Mode::Settings;
                }
                fit_instant(s, tx);
            })
            .child(text("Back").font(TextStyle::new(13.5, 450.0)).color(tok::accent(0, dark))));
    let body = match &s.draft {
        Some(draft) => trigger_form(s, draft, dark),
        None => trigger_list(s, dark),
    };
    glass(WINDOW_RADIUS).key("triggers-screen").padding(SEARCH_PAD).child(header).child(body)
}

fn trigger_list(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    let rows = s.triggers.iter().take(SEARCH_ROWS).enumerate().map(|(i, trigger)| {
        row().w_full().h(ROW).center_y().px(6.0).gap(8.0)
            .child(text(triggers::describe(&trigger.0, &trigger.1)).font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
            .child(text(trigger.2.as_str()).font(TextStyle::new(12.0, 450.0)).color(t.text_secondary))
            .child(row().key(format!("trigger-delete-{i}")).role("button").label("Delete trigger").hit().size(28.0, ROW).center()
                .on_press_tx(move |s: &mut State, tx| {
                    s.triggers.remove(i);
                    s.last_trigger = None;
                    triggers::save(s.triggers.clone());
                    fit(s, tx);
                })
                .child(text("×").font(TextStyle::new(16.0, 450.0)).color(t.text_secondary)))
    });
    let none = s.triggers.is_empty().then(|| {
        row().w_full().h(ROW).center_y().px(6.0).child(text("No triggers yet").font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
    });
    let add = if s.presets.is_empty() {
        row().w_full().h(ROW).center_y().px(6.0).child(text("Save a preset first, then add a trigger").font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
    } else {
        row().key("trigger-new").role("button").label("New trigger").hit().w_full().h(ROW).center_y().px(6.0)
            .on_press_tx(|s: &mut State, tx| {
                s.draft = Some(Draft { kind: triggers::BATTERY.into(), ..Draft::default() });
                fit_instant(s, tx);
            })
            .child(text("New trigger").font(TextStyle::new(13.5, 450.0)).color(tok::accent(0, dark)))
    };
    col().w_full().children(rows).children(none).child(hairline(dark)).child(el().h(SEARCH_GAP)).child(add)
}

/// The text a finished form stores: the two times, or the chosen apps joined by a bar.
fn trigger_text(draft: &Draft) -> String {
    match draft.kind.as_str() {
        triggers::TIME => draft.text.text.trim().to_string(),
        triggers::APP => draft.chosen.join("|"),
        _ => String::new(),
    }
}

fn trigger_form(s: &State, draft: &Draft, dark: bool) -> El {
    let t = tok::theme(dark);
    let kinds = [(triggers::BATTERY, "On battery"), (triggers::PLUGGED, "Plugged in"), (triggers::TIME, "Between two times"), (triggers::APP, "An app is running")];
    let when = kinds.into_iter().map(|(kind, label)| {
        choice(format!("kind-{kind}"), label, draft.kind == kind, false, dark, move |s: &mut State, tx| {
            if let Some(d) = &mut s.draft {
                d.kind = kind.into();
                if kind == triggers::APP && d.apps.is_empty() {
                    d.apps = triggers::installed_app_names();
                }
            }
            fit(s, tx);
        })
    });
    // A time range is typed. Apps are picked from the ones that are open, one or several.
    let time_box = (draft.kind == triggers::TIME).then(|| {
        let field = text_input(&draft.text, TextStyle::new(14.0, 450.0))
            .on_edit(|s: &mut State| &mut s.draft.as_mut().expect("a draft is open").text)
            .key("trigger-text")
            .single_line()
            .placeholder("09:00-18:00", t.text_secondary)
            .label("The two times")
            .grow(1.0);
        // Lined up with the choices above, with a thin line under it so it reads as a box.
        col().w_full().child(row().w_full().h(SEARCH_BOX - 0.5).pl(26.0).center_y().child(field)).child(hairline(dark))
    });
    let app_list = (draft.kind == triggers::APP).then(|| {
        let field = text_input(&draft.filter, TextStyle::new(14.0, 450.0))
            .on_edit(|s: &mut State| &mut s.draft.as_mut().expect("a draft is open").filter)
            .key("app-filter")
            .single_line()
            .placeholder("Search your apps", t.text_secondary)
            .label("Search your apps")
            .grow(1.0);
        let filter = col().w_full().child(row().w_full().h(SEARCH_BOX - 0.5).pl(26.0).center_y().child(field)).child(hairline(dark));
        let shown = draft.apps.iter().filter(|name| matches(name, &draft.filter.text));
        let rows = shown.enumerate().map(|(i, name)| {
            let toggled = name.clone();
            let chosen = draft.chosen.contains(name);
            choice(format!("app-{i}"), name, chosen, true, dark, move |s: &mut State, _| {
                if let Some(d) = &mut s.draft {
                    match d.chosen.iter().position(|n| *n == toggled) {
                        Some(at) => {
                            d.chosen.remove(at);
                        }
                        None => d.chosen.push(toggled.clone()),
                    }
                }
            })
        });
        let none = draft.apps.iter().all(|name| !matches(name, &draft.filter.text))
            .then(|| row().w_full().h(ROW).center_y().pl(26.0).child(text("No app found").font(TextStyle::new(13.0, 450.0)).color(t.text_secondary)));
        // The list keeps one height, so the window does not jump while you type.
        let list = col().key("apps").w_full().pl(20.0).h(APP_ROWS as f32 * ROW).scroll(ScrollSpec { stick_bottom: false }).children(rows).children(none);
        col().w_full().child(filter).child(list)
    });
    let presets = s.presets.iter().take(FORM_PRESETS).enumerate().map(|(i, preset)| {
        let name = preset.0.clone();
        choice(format!("pick-{i}"), &preset.0, draft.preset.as_deref() == Some(preset.0.as_str()), false, dark, move |s: &mut State, _| {
            if let Some(d) = &mut s.draft {
                d.preset = Some(name.clone());
            }
        })
    });
    let ready = draft.preset.is_some() && triggers::is_valid(&draft.kind, &trigger_text(draft));
    let save = if ready {
        row().key("trigger-save").role("button").label("Add trigger").hit().w_full().h(ROW).center_y().px(6.0)
            .on_press_tx(|s: &mut State, tx| {
                if let Some(d) = s.draft.take() {
                    let arg = trigger_text(&d);
                    s.triggers.push((d.kind, arg, d.preset.unwrap_or_default()));
                    s.last_trigger = None;
                    triggers::save(s.triggers.clone());
                    triggers::evaluate(s);
                }
                fit_instant(s, tx);
            })
            .child(text("Add trigger").font(TextStyle::new(13.5, 600.0)).color(tok::accent(0, dark)))
    } else {
        row().w_full().h(ROW).center_y().px(6.0).child(text("Add trigger").font(TextStyle::new(13.5, 450.0)).color(t.text_secondary))
    };
    col().w_full()
        .child(section_label("WHEN", dark))
        .children(when)
        .children(time_box)
        .children(app_list)
        .child(el().h(SECTION_GAP))
        .child(section_label("APPLY", dark))
        .children(presets)
        .child(el().h(SEARCH_GAP))
        .child(save)
}

/// One row that opens the triggers.
fn triggers_row(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    row().key("triggers").role("button").label("Triggers").hit().w_full().h(ROW).center_y()
        .on_press_tx(|s: &mut State, tx| {
            s.mode = Mode::Triggers;
            s.draft = None;
            fit_instant(s, tx);
        })
        .child(text("Triggers").font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
        .child(text(format!("{}  ›", s.triggers.len())).font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
}

/// One row that opens the presets.
fn presets_row(s: &State, dark: bool) -> El {
    let t = tok::theme(dark);
    row().key("presets").role("button").label("Presets").hit().w_full().h(ROW).center_y()
        .on_press_tx(|s: &mut State, tx| {
            s.mode = Mode::Presets;
            fit_instant(s, tx);
        })
        .child(text("Presets").font(TextStyle::new(13.5, 450.0)).color(t.text).grow(1.0))
        .child(text(format!("{}  ›", s.presets.len())).font(TextStyle::new(13.0, 450.0)).color(t.text_secondary))
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

    glass(WINDOW_RADIUS).key("settings-screen").padding(PAD).gap(SECTION_GAP)
        .child(header)
        .child(icons_section)
        .child(col().w_full().child(hairline(dark)).child(login_row(s, dark)).child(hairline(dark)).child(hotkeys_row(s, dark)).child(hairline(dark)).child(presets_row(s, dark)).child(hairline(dark)).child(triggers_row(s, dark)).child(hairline(dark)).child(advanced_section(s, dark)))
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

/// One row with a switch: the keyboard shortcuts. While on, a small line under the name shows the keys.
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
        .child(col().grow(1.0).gap(1.0)
            .child(text("Keyboard shortcuts").font(TextStyle::new(13.5, 450.0)).color(t.text))
            .children(s.hotkeys.then(|| text("I ⌥ ⌘ + Space  show icons   ·   I ⌥ ⌘ + F  search").font(TextStyle::new(9.0, 450.0)).color(t.text_secondary))))
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
    let presets = presets::load();
    PRESET_COUNT.store(presets.len(), Ordering::Relaxed);
    State { look, setup_done, login: login::enabled(), hotkeys, presets, triggers: triggers::load(), ..State::default() }
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
            ("presets", |s| { demo_items(s); s.presets = vec![("Work".into(), vec!["Wi-Fi#0".into()]), ("Focus".into(), vec![])]; s.mode = Mode::Presets; }),
            ("triggers", |s| {
                demo_items(s);
                s.presets = vec![("Work".into(), vec![]), ("Focus".into(), vec![])];
                s.triggers = vec![("battery".into(), String::new(), "Focus".into()), ("time".into(), "09:00-18:00".into(), "Work".into())];
                s.mode = Mode::Triggers;
            }),
            ("trigger-form", |s| {
                demo_items(s);
                s.presets = vec![("Work".into(), vec![]), ("Focus".into(), vec![])];
                s.draft = Some(Draft { kind: "time".into(), text: rune::TextEdit::new("09:00-18:00"), preset: Some("Work".into()), ..Draft::default() });
                s.mode = Mode::Triggers;
            }),
            ("trigger-apps", |s| {
                demo_items(s);
                s.presets = vec![("Work".into(), vec![]), ("Focus".into(), vec![])];
                let apps: Vec<String> = ["Calendar", "Finder", "Notes", "Slack", "Xcode", "Zoom"].into_iter().map(String::from).collect();
                s.draft = Some(Draft { kind: "app".into(), apps, chosen: vec!["Slack".into(), "Zoom".into()], preset: Some("Focus".into()), ..Draft::default() });
                s.mode = Mode::Triggers;
            }),
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
        status_item::set_preset_names(s.presets.iter().map(|p| p.0.clone()).collect());
        triggers::start(handle.clone());
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
    fn a_preset_moves_only_the_icons_that_differ() {
        let mut state = State::default();
        demo_items(&mut state); // Wi-Fi and Clock are hidden; Battery and Bluetooth are not
        // Wi-Fi stays hidden, Clock comes out, Battery goes in.
        let preset = ("Work".to_string(), vec![state.items[0].key(), state.items[1].key()]);
        let moves = presets::moves(&state, &preset);
        assert_eq!(moves, vec![(0, 1, true), (0, 2, false)]);
        assert!(!presets::is_active(&state, &preset));
        let same = ("Now".to_string(), presets::hidden_now(&state));
        assert!(presets::is_active(&state, &same));
    }

    #[test]
    fn saving_a_preset_keeps_the_hidden_icons_and_replaces_a_name() {
        let mut state = State::default();
        demo_items(&mut state);
        state.preset_name = rune::TextEdit::new("  Work ");
        assert!(add_preset(&mut state));
        assert_eq!(state.presets.len(), 1);
        assert_eq!(state.presets[0].0, "Work");
        assert_eq!(state.presets[0].1.len(), 2);
        state.preset_name = rune::TextEdit::new("Work");
        assert!(add_preset(&mut state));
        assert_eq!(state.presets.len(), 1);
        state.preset_name = rune::TextEdit::new("   ");
        assert!(!add_preset(&mut state));
    }

    #[test]
    fn the_trigger_form_lists_choices_and_asks_for_a_time() {
        let mut state = State::default();
        demo_items(&mut state);
        state.presets = vec![("Work".into(), vec![])];
        state.draft = Some(Draft { kind: "time".into(), ..Draft::default() });
        state.mode = Mode::Triggers;
        let size = (window_width(Mode::Triggers), window_height(&state));
        let mut rt = Runtime::new(state, view, size, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        for key in ["kind-battery", "kind-plugged", "kind-time", "kind-app", "trigger-text", "pick-0"] {
            assert!(rt.find(key).is_some(), "{key} is drawn");
        }
        // The form is not ready until a preset is chosen, so there is no Add button yet.
        assert!(rt.find("trigger-save").is_none());
        let (x, y) = rt.find("pick-0").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        // A preset is chosen, but "time" still needs a valid range.
        assert!(rt.find("trigger-save").is_none());
    }

    #[test]
    fn the_strip_shows_only_hidden_items() {
        let rt = runtime(Mode::Strip);
        assert!(rt.find("hidden-0").is_some() && rt.find("hidden-1").is_some());
        assert!(rt.find("hidden-2").is_none());
    }
}
