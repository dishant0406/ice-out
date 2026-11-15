//! Presets: a name and the icons that are hidden. Applying one moves only the icons that differ.
use rune::prefs::Prefs;

use crate::State;

/// A preset: its name, and the keys of the icons that are hidden in it.
pub type Preset = (String, Vec<String>);

pub fn load() -> Vec<Preset> {
    Prefs::open(crate::APP_ID).get::<Vec<Preset>>("presets").unwrap_or_default()
}

/// Saves on another thread, so a click never waits for the disk.
pub fn save(presets: Vec<Preset>) {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || {
        Prefs::open(crate::APP_ID).set("presets", &presets).ok();
    });
}

/// The keys of the icons that are hidden right now.
pub fn hidden_now(s: &State) -> Vec<String> {
    s.items.iter().filter(|item| item.hidden).map(|item| item.key()).collect()
}

/// The moves that turn the current layout into the preset: (process, index, hide).
pub fn moves(s: &State, preset: &Preset) -> Vec<(i32, usize, bool)> {
    s.items
        .iter()
        .filter(|item| item.hidden != preset.1.contains(&item.key()))
        .map(|item| (item.pid, item.index, !item.hidden))
        .collect()
}

/// A preset is active when applying it would move nothing.
pub fn is_active(s: &State, preset: &Preset) -> bool {
    moves(s, preset).is_empty()
}
