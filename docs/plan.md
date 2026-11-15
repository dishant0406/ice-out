# Iceout: research and plan

Name: Iceout. Target: macOS only.

## What we want

A small Mac app that sits in the menu bar. You click it, and a dropdown shows the menu bar icons. It must use as few permissions as possible. It must also use very little memory.

Note: you wrote "icons in the dock". I read this as "icons in the menu bar", because that is what Ice and Bartender manage. Tell me if you meant the Dock.

## What Ice and Bartender do

Bartender 5 (paid, closed source, 4-week trial):
- Hides menu bar items. Reveals them by click, hover, or scroll.
- "Bartender Bar": a strip under the menu bar that shows the hidden items.
- Quick search by name, with a hotkey per item.
- Groups, triggers (battery, Wi-Fi, location, time), presets, Focus mode support.
- Styling (tint, border, shadow, rounded bar), item spacing, notch handling.
- Permissions: Screen Recording (to read item images) and Accessibility (to move items).

Ice (free, open source, GPL-3.0, macOS 14+):
- Hide, always-hidden section, hover/click/scroll reveal, auto rehide.
- "Ice Bar" (separate bar for hidden items, good for notch Macs), search, profiles, groups, spacers.
- Styling, hotkeys, launch at login.
- Uses the Accessibility API. Screen Recording was needed for item images. Release 0.11.12 added a "limited mode" without it. The release notes do not say what the limited mode loses.

Sources: [Ice README](https://github.com/jordanbaird/Ice), [Ice release notes](https://jordanbaird.github.io/ice-releases/ReleaseNotes.html), [Bartender permissions](https://macbartender.com/Bartender5/PermissionInfo), [Bartender 5](https://macbartender.com/Bartender5/).

No Ice or Bartender screenshots exist on this Mac. The only screenshots on your Desktop show Chrome and GitHub, so I did not use them.

## The key test: what can an app see with zero permissions?

I built a small signed test app with its own identity. Screen Recording was off and Accessibility was off. It read the window list.

Result, on macOS 26.6.2:
- It sees all 30 menu bar item windows, with their position and width.
- It sees no names and no owners. Every item is owned by "Control Centre", and the window names are empty.

So with zero permissions we know where each icon is, but not which app it belongs to. We cannot draw the real icon image either. (My first test looked better because the host app already had Screen Recording. That test was wrong.)

## Permission ladder

| Level | Permissions | What the dropdown can do |
| --- | --- | --- |
| 0 | None | Hide and show the icons with a spacer item (the Hidden Bar technique). Dropdown shows only a count and slots, no names. |
| 1 | Accessibility only (to verify) | Probably names and click-through for items. Needs a test on macOS 26. |
| 2 | Accessibility + Screen Recording | Real icon images. This is what Bartender and Ice do. You said you do not want this. |

**Chosen: level 1 (Accessibility only).**

Level 1 test result (macOS 26.6.2, `tools/axprobe`): with Accessibility on and Screen Recording off, the app listed every menu bar item with its owner app name, bundle ID and x position. Examples: Claude, Kivi, Moneta, Tinycast, Screendrop. Apple items (Wi-Fi, Battery, Clock) come with descriptions. Nine Control Centre entries had no data and x=0; probably hidden or off-screen, to check.

So the dropdown can show the real app name and the app's own icon (from its bundle, no Screen Recording). Not tested yet: pressing an item (AXPress) to open its menu.

## V0 scope

Done so far:
- Project from `examples/hello`, pinned to Rune `dev` rev `feac7b1`, with the toolchain file and the patches Rune asks apps to repeat.
- Dropdown view (title and item rows). Items come from a fake "demo" scene, kept apart from the view so the real source can plug in later.
- Headless checks pass: `cargo test`, `cargo run -- --snap out/demo.png --scene demo`, `cargo run -- --tree out/tree.json --scene demo`.

Not done yet:
- The status item in the menu bar (needs `objc2`, a new dependency; I need your OK).
- A window that opens under the status item and hides when you click away.
- The real item source (depends on your level choice).
- Idle memory measurement.

Headless checks cover the dropdown view only. They do not cover the status item or the real menu bar. Those need a real launch.

## Memory plan

Rune draws with the GPU. Use `.windowless()`, `.accessory()` and `.release_gpu_when_closed()` so the app holds almost nothing while the dropdown is closed. Measure the real idle number with `procwatch` before we claim anything. If plain AppKit is much lighter, I will tell you.

## Later (V1 and up)

Search, hotkeys, auto rehide, groups, triggers, styling, launch at login, notch support.

## Ice and Bartender look (from their public screenshots)

- Both show hidden items as a horizontal rounded strip just under the menu bar, not a vertical list. Ice calls it the Ice Bar. Bartender calls it the Bartender Bar.
- Both draw the real menu bar glyph of each item. That needs Screen Recording. We draw the app's own icon instead, so we need only Accessibility.
- Ice has three sections: visible, hidden and always-hidden. You arrange items with Command + drag in the menu bar.
- Bartender also has a search panel (type a name, press Enter to open the item).

## V0 as built

- Click the Iceout icon: a glass strip of the hidden icons. Click one and it opens. Right-click it: Settings and Quit.
- Settings lists every menu bar icon with a switch. On means hidden. Hiding moves the icon left of our `|` button, and `|` is made very wide so macOS pushes those icons off the screen.
- Permission: Accessibility only. Icons shown are the apps' own icons. Real menu bar glyphs would need Screen Recording (not built, not asked for).

## Speed and memory (release build, measured on this Mac)

| What | Number |
| --- | --- |
| Idle memory (`phys_footprint`) | 27 MB (RSS 60 MB, which counts shared system libraries) |
| Memory with the window open | 32 MB |
| Idle CPU | 0% |
| Window appears after a click | 36 to 40 ms |
| Reading all menu bar items | 175 ms, on a background thread (was 2.5 s on the main thread) |
| Click away closes the window | yes (about 0.5 s check) |
| Binary size | 16.8 MB (was 26 MB before LTO and strip) |

Debug builds are far slower (window takes about 25 s). Only judge speed from `cargo build --release`.

# Plan: search, hotkeys, presets, triggers

## What the research found

| Feature | Ice | Bartender |
| --- | --- | --- |
| Search | Built. A floating panel (`MenuBarSearchPanel.swift`), fuzzy matching on names, shows sections. Pick a row and Ice does "temp show, click, put back". Closes on Escape, a click outside, or a Space change. Screen Recording only adds the pictures. | Built (Quick Search). Finds, shows and opens any item from the keyboard. Its pages do not say how it opens or what Enter does. |
| Hotkeys | Built, and no extra permission. It uses Carbon `RegisterEventHotKey`. Six actions: toggle hidden section, toggle always-hidden section, search, Ice Bar on or off, show dividers, toggle app menus. A recorder refuses combinations macOS reserves. Saved as JSON in UserDefaults. | Built. A hotkey on any single item shows and opens it. AppleScript can do the same. |
| Triggers | **Not built.** Only a roadmap line. | Built. Battery (state or level), Wi-Fi (connected, or a named network), location, time and date. Conditions combine. A match applies a preset or shows items. |
| Presets | **Not built.** Ice has no named saved layouts (its roadmap calls them profiles). | Built. Unlimited. Applied by hand, by a trigger, or by a macOS Focus mode. |

So Bartender is the only reference for triggers and presets, and its pages are thin. Ice is the reference for search and hotkeys, with real code to learn from (we copy the ideas, not the code, because Ice is GPL-3.0).

## Order of work

Each step ships on its own, and each is checked before the next starts.

### 1. Search (small)

- **What:** a hotkey or a menu item opens a small glass panel with a text box and a list of every non-system icon (hidden and visible). Type to filter. Up and Down move. Enter opens the icon. Escape or a click away closes it.
- **How:** one more mode in the same window (`Mode::Search`), so no new window and no new memory. Matching is a simple "letters in order" match on the app name, ranked by earlier match. No new crate. Enter reuses `press_hidden` for a hidden icon, and `press` for a visible one.
- **Permission:** none new. The panel needs keyboard focus, so the app activates itself while the panel is open.
- **Edge cases:** empty list, no match, app quit while the panel is open, two icons with the same name (show the app path's last part), keyboard focus lost to another app (close).
- **Done when:** the panel opens in under 50 ms, 30 open and close cycles keep memory flat, and a headless test types "mo" and finds "Moneta".

### 2. Hotkeys (medium)

- **What:** global shortcuts you record in Settings. Actions to start with: open the strip, open search, and "open this icon" (one per icon you choose).
- **How:** Carbon `RegisterEventHotKey`, as Ice does. It needs no permission and costs nothing while idle. Check combinations against the ones macOS reserves (`CopySymbolicHotKeys`). A small recorder row in Settings. Save in `prefs.json`.
- **Permission:** none.
- **Edge cases:** two actions on one key, a key another app already took (registration fails: say so in the row), the app not running at login, keyboard layouts.
- **Done when:** a recorded key works from any app, a clash shows a message, and idle CPU stays 0%.

### 3. Presets (medium)

- **What:** a named layout is the set of icons that are hidden. Settings gets a small "Layouts" row: save the current layout, switch layout, delete. Switching moves only the icons that differ.
- **How:** store `name -> list of icon keys` in `prefs.json`. Applying one is a diff, then the same `move_item` calls we already use, one at a time. An icon that is not running is skipped and remembered.
- **Permission:** none.
- **Edge cases:** an icon in the layout is gone, a new icon not in the layout (leave it hidden, like any new icon), applying while an icon is out for a peek (wait), the first layout being "what it is now".
- **Done when:** switching between two layouts of 8 icons takes under a second with no flash, and nothing else on the bar moves.

### 4. Triggers (largest)

- **What:** a rule says "when X, apply layout Y". Start with the ones that need no permission.
- **Phase A, no permission:**
  - Time of day and days of the week. One timer set for the next boundary, so nothing wakes up while idle.
  - Power: on battery or charging, and a battery level. Event driven (IOKit power source notifications), so no polling.
  - An app is running or in front (we already read running apps).
- **Phase B, optional, asks first:**
  - Wi-Fi network name. On current macOS this needs the Location permission. It must be opt-in and say clearly why, because the whole point of Ice Out is as few permissions as possible.
  - macOS Focus mode. Needs its own permission too. Opt-in.
  - Place (GPS) is **not planned**. It needs Location, and the gain is small.
- **How:** a rule is `{conditions, layout}`. Conditions are all "and" at first (Bartender does not say which it uses). When the rule set changes, evaluate once and apply the winner. If two rules match, the one lower in the list wins, and the list is orderable.
- **Edge cases:** rules fighting each other (apply only on a change, never in a loop), the Mac waking from sleep (re-evaluate), a layout that does not exist any more, the user changing the layout by hand (their change stays until the next trigger fires).
- **Done when:** a time rule and a battery rule switch the layout on their own, idle CPU is still 0%, and no permission prompt appears unless the user adds a Wi-Fi or Focus rule.

## Cross-cutting rules

- No new permission without a clear opt-in and one sentence on why.
- No polling loop. Use events or one timer for the next change.
- Speed budget: window opens under 50 ms, idle CPU 0%, idle memory under 40 MB.
- Each feature gets: a headless unit test, a live test with a screenshot, a line in `docs/flow.md` (steps and edge cases), a `docs/design.md` entry (choice, reason, rejected options), and a README row.
- Do not send key presses in live tests (they stop the session). For search and hotkeys, test with the unit tests and with mouse-driven paths.

## Rough size

| Step | Size | New code | New permission |
| --- | --- | --- | --- |
| Search | small | one mode, one match function, one hotkey hook | none |
| Hotkeys | medium | Carbon registration, recorder row, storage | none |
| Presets | medium | storage, diff and apply, a small Settings row | none |
| Triggers A | large | rule storage, timer, power events, rule editor | none |
| Triggers B | medium | Wi-Fi and Focus readers, opt-in screens | Location, Focus (opt-in) |
