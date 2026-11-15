//! Triggers: "when this is true, apply that preset". A trigger is (kind, text, preset name).
//! Every 30 seconds the triggers are checked. A preset is applied only when the winning trigger
//! changes, so a layout you set by hand stays until the next change. If two triggers are true,
//! the one lower in the list wins.
use std::ffi::c_void;
use std::time::Duration;

use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use rune::prefs::Prefs;
use rune::AppHandle;

use crate::State;

pub type Trigger = (String, String, String);

pub const BATTERY: &str = "battery";
pub const PLUGGED: &str = "plugged";
pub const TIME: &str = "time";
pub const APP: &str = "app";

pub fn load() -> Vec<Trigger> {
    Prefs::open(crate::APP_ID).get::<Vec<Trigger>>("triggers").unwrap_or_default()
}

/// Saves on another thread, so a click never waits for the disk.
pub fn save(triggers: Vec<Trigger>) {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || {
        Prefs::open(crate::APP_ID).set("triggers", &triggers).ok();
    });
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSGetProvidingPowerSourceType(info: *const c_void) -> *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetCString(string: *const c_void, buffer: *mut u8, size: isize, encoding: u32) -> bool;
    fn CFRelease(object: *const c_void);
}

#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    gmtoff: i64,
    zone: *const u8,
}

unsafe extern "C" {
    fn time(out: *mut i64) -> i64;
    fn localtime_r(seconds: *const i64, out: *mut Tm) -> *mut Tm;
}

/// True when the Mac runs on its battery. A desktop Mac is always on the mains.
fn on_battery() -> bool {
    unsafe {
        let info = IOPSCopyPowerSourcesInfo();
        if info.is_null() {
            return false;
        }
        let mut text = [0u8; 64];
        // 0x08000100 is UTF-8.
        let ok = CFStringGetCString(IOPSGetProvidingPowerSourceType(info), text.as_mut_ptr(), text.len() as isize, 0x0800_0100);
        CFRelease(info);
        ok && text.starts_with(b"Battery Power")
    }
}

/// The local time as minutes since midnight.
fn minutes_now() -> u32 {
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    unsafe {
        let now = time(std::ptr::null_mut());
        localtime_r(&now, &mut tm);
    }
    (tm.hour * 60 + tm.min) as u32
}

/// "9:30" or "09:30" as minutes since midnight.
fn parse_clock(text: &str) -> Option<u32> {
    let (hour, minute) = text.trim().split_once(':')?;
    let (hour, minute): (u32, u32) = (hour.trim().parse().ok()?, minute.trim().parse().ok()?);
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// "09:00-18:00" as (start, end). An end before the start means the range crosses midnight.
fn parse_range(text: &str) -> Option<(u32, u32)> {
    let (start, end) = text.split_once('-')?;
    Some((parse_clock(start)?, parse_clock(end)?))
}

fn in_range(now: u32, start: u32, end: u32) -> bool {
    if start <= end { now >= start && now < end } else { now >= start || now < end }
}

/// The names of the apps that are open and have a window or a Dock icon, sorted.
pub fn running_app_names() -> Vec<String> {
    let mut names: Vec<String> = unsafe {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter_map(|app| app.localizedName().map(|n| n.to_string()))
            .collect()
    };
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The app names in a trigger's text: one or more, separated by a bar.
pub fn app_names(text: &str) -> impl Iterator<Item = &str> {
    text.split('|').map(str::trim).filter(|n| !n.is_empty())
}

/// True when any of the named apps is open.
fn app_is_running(text: &str) -> bool {
    let running = running_app_names();
    app_names(text).any(|wanted| running.iter().any(|n| n.eq_ignore_ascii_case(wanted)))
}

/// Whether the text of a trigger makes sense for its kind.
pub fn is_valid(kind: &str, text: &str) -> bool {
    match kind {
        TIME => parse_range(text).is_some(),
        APP => app_names(text).next().is_some(),
        _ => true,
    }
}

/// The trigger in plain words.
pub fn describe(kind: &str, text: &str) -> String {
    match kind {
        BATTERY => "On battery".into(),
        PLUGGED => "Plugged in".into(),
        TIME => text.replace('-', " – "),
        _ => {
            let names: Vec<&str> = app_names(text).collect();
            format!("{} is open", names.join(" or "))
        }
    }
}

fn is_true(kind: &str, text: &str) -> bool {
    match kind {
        BATTERY => on_battery(),
        PLUGGED => !on_battery(),
        TIME => parse_range(text).is_some_and(|(start, end)| in_range(minutes_now(), start, end)),
        _ => app_is_running(text),
    }
}

/// Applies the preset of the winning trigger, once, when the winner changes. Runs on the main thread.
pub fn evaluate(s: &mut State) {
    if !s.permission {
        return;
    }
    let winner = s.triggers.iter().rposition(|t| is_true(&t.0, &t.1));
    if winner == s.last_trigger {
        return;
    }
    let Some(i) = winner else {
        s.last_trigger = None;
        return;
    };
    // If the app is busy moving icons, try again at the next check.
    let preset = s.triggers[i].2.clone();
    if crate::apply_preset(s, &preset) {
        s.last_trigger = winner;
    }
}

/// Checks the triggers now and then. Nothing runs between checks.
pub fn start(handle: AppHandle<State>) {
    std::thread::spawn(move || {
        // The first check waits for the first read of the bar.
        std::thread::sleep(Duration::from_secs(5));
        loop {
            handle.update(|s, _| evaluate(s));
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_range_can_cross_midnight() {
        assert_eq!(parse_range("09:00-18:30"), Some((540, 1110)));
        assert_eq!(parse_range("9:00 - 6:00"), Some((540, 360)));
        assert!(parse_range("nine-five").is_none());
        assert!(parse_range("25:00-26:00").is_none());
        assert!(in_range(600, 540, 1110));
        assert!(!in_range(1110, 540, 1110));
        assert!(in_range(60, 1320, 360)); // 01:00 is inside 22:00-06:00
        assert!(!in_range(720, 1320, 360));
    }

    #[test]
    fn the_system_calls_answer() {
        assert!(minutes_now() < 24 * 60);
        // Exactly one of "on battery" and "plugged in" is true.
        assert_ne!(is_true(BATTERY, ""), is_true(PLUGGED, ""));
        assert!(app_is_running("Finder"));
        assert!(app_is_running("no such app|finder"));
        assert!(!app_is_running("no such app anywhere"));
        assert!(running_app_names().iter().any(|n| n == "Finder"));
    }

    #[test]
    fn words_describe_a_trigger() {
        assert_eq!(describe(BATTERY, ""), "On battery");
        assert_eq!(describe(TIME, "09:00-18:00"), "09:00 – 18:00");
        assert_eq!(describe(APP, " Xcode "), "Xcode is open");
        assert_eq!(describe(APP, "Xcode|Slack"), "Xcode or Slack is open");
        assert!(is_valid(PLUGGED, ""));
        assert!(!is_valid(APP, "  "));
    }
}
