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
