# Design decisions

Each entry: what we chose, why, what we did not choose, and the trade-off. New entries go at the bottom.

## Rust and Rune, not Swift

Chosen: Rust with Rune (a GPU UI framework), because you asked for Rune. Rejected: plain AppKit in Swift. Trade-off: Rune draws with the GPU, so the app costs about 27 MB at idle. A pure AppKit app would likely be a bit lighter. We have not built one to compare.

## Accessibility only, no Screen Recording

Chosen: read menu bar items through the Accessibility API. It gives each item's app name and position. We checked on macOS 26 that without Accessibility we only see positions, and with it we also get names. Rejected: Screen Recording, which Ice and Bartender use to copy the real icon pictures. Trade-off: we show each app's own icon, not the exact menu bar glyph. Apple's own items (Wi-Fi, Battery) all belong to Control Centre, so they get a letter tile.

## Hiding with a wide divider

Chosen: two menu bar buttons of ours. The toggle (the Iceout mark) opens the UI. The divider is made very wide to hide things: macOS lays icons out from the right, so a very wide item pushes every icon on its left off the screen. This needs no permission.

## Moving icons: window-targeted events, as Ice does

First version: a synthetic Command-drag along the menu bar. It moved your real cursor, and it only worked on icons that were on screen, so every hide or peek flashed all the hidden icons. Rejected.

Now: the method from Ice's `MenuBarItemManager`. Each icon is a window (layer 25) that the window server lists even while it is off the screen. A Command-drag is two mouse events. Each is given a made-up position (the drag starts at 20000, 20000) and tagged with an icon's window id (event fields 91, 92, 0x33), and with the owner process (field 89). It is posted to that process and to the session event tap. The system then runs the drag on that window with no real mouse travel, so it works on off-screen icons and the divider never has to shrink. The cursor still jumps to where the drop happens, so it is hidden (`CGDisplayHideCursor`, allowed in the background by the private `SetsCursorInBackground` connection property, like Ice) and put back. Our own global mouse monitors ignore events while we send these.

Icon windows are matched to accessibility items by frame. The window frame is a few points wider than the accessibility frame, and by different amounts for third-party and Apple items, so the closest window wins.

## Showing one icon: move it, do not reveal everything

Like Ice's `tempShowItem`: move the icon to the visible side, click it, move it back later. The divider stays wide, so no other hidden icon ever shows. This replaced a long attempt to widen the divider to just the right width, which depended on the notch, the app menus and which icon was nearest.

## Quit brings icons back

Quit makes the divider thin first. Even if the app crashes, our divider disappears with it, so icons return on their own.

## Reading the menu bar off the main thread

Reading all apps' items took 2.5 s because one helper process answered after 1.5 s. Chosen: ask every app on its own thread, with a 0.15 s timeout per app. The whole read now takes about 175 ms and runs in the background. Why it matters: the bar can only re-layout when our main thread is free, so blocking it also froze hiding.

## Click away closes the window

Chosen: a global mouse monitor. It only hears clicks that go to other apps, so our own buttons and window do not trigger it, and it uses no CPU while nobody clicks. Rejected: a polling loop. This needed the `block2` crate (already in the build through objc2).

## One window, two modes

The strip and Settings share one transparent glass window. Only its size and content change. One window means one GPU surface and less memory. Rejected: two windows.

## Release profile

`lto`, `codegen-units = 1`, `strip` and `panic = "abort"`: the binary dropped from 26 MB to 16.8 MB. Trade-off: slower release builds (about a minute).

## Hidden means left of the divider, always read from positions

An icon is hidden when it sits left of our divider. That is read fresh on every read of the bar, as Ice does with its control items. It was briefly a saved set of names, which went wrong: a new app's icon (macOS puts new icons at the far left, so it is hidden at once) was not in the set, and the read skipped off-screen icons, so the icon was hidden but missing from the list. Now the read keeps off-screen icons (they have a negative x), and a new icon shows in the list as hidden, with its switch on. While an icon is out for a peek, it still counts as hidden.

## Keeping the list fresh

Like Ice, the list is not read on a timer. A small thread asks the window server every two seconds for the ids of the menu bar icon windows (about a millisecond), and the whole bar is read again only when that set changed. It also waits 400 ms after a change, because an icon can appear before its accessibility entry. The bar is also read again after every move, and when Settings opens. Ice does the same with a 5 second check plus a 0.25 s delay after the list of running apps changes. Idle cost measured: 0% CPU, 28 MB.

## Sliders, preview and an Advanced section

Look settings (icon size, padding, spacing, corner radius) are sliders in an Appearance card pinned at the bottom of Settings, with a live preview. Only the icon list scrolls, so the sliders are never cut off. The preview is a plain filled shape, not glass, because nested glass is limited. Values live in one `Look` struct in the state, with a copy in a static so the window can be sized before it opens. They are saved on another thread when a drag ends, not on every move.

## Start at login with a launch agent

`SMAppService` needs a signed app bundle, and this program is a plain binary for now. A launch agent file in `~/Library/LaunchAgents` runs the current program at login and works for both. Trade-off: if the program moves, the file points at the old path.

## Name, icon and installer

The app is called Iceout. Icon: a rounded square in a bright gradient (electric cyan, blue, violet, hot pink, with a soft light at the top left), a solid white circle, and a wedge cut out of the circle like a crack in ice, which shows the gradient through. It keeps the shape language of the Akidev icon (one circle, one cut) with colours that excite. The first version used flat ice blue on near-black and looked dull. The source is `assets/iceout-icon.svg`, and `scripts/make-icon.swift` draws the same shapes on the standard macOS icon grid.

The menu bar button is the same mark, in one colour (`assets/menubar-icon.png`, 36 x 36 pixels shown at 18 points, drawn by `scripts/make-menubar-icon.swift`). It is a template image, so macOS tints it for light and dark menu bars. The button has no title now, so the app finds its own two buttons by the divider's thin-space title; the other button is the toggle.

`scripts/package.sh` builds `dist/Iceout.app` (no Dock icon, ad-hoc signature) and `dist/Iceout.dmg`. The installer window follows the Akidev one: a bright picture (`assets/dmg/background.html`, drawn by headless Chrome into one TIFF with a sharp 2x image) with a mock menu bar whose icons slide away, a glass strip that holds them (what the app does), a dotted arrow from the app to Applications, and a hint line. The middle of the picture stays a mid-tone blue so Finder's icon names, which are black on a light Mac and white on a dark one, read on both. `dmgbuild` (fetched when the script runs, not part of the app) writes the window layout without asking Finder, so no permission prompt appears. Trade-off: the signature is ad-hoc, so another Mac shows a Gatekeeper warning until the app is signed with a Developer ID and notarized. Accessibility must be allowed again after each rebuild, and a new build or bundle gets new menu bar buttons at the far left, so icons you hid before show again until you hide them once more.

## Flat windows, no cards

The first Settings and welcome windows put filled boxes inside the window, and boxes inside boxes, and looked generated. References on Dribbble (menu bar popovers such as Figma's, BlackSSL and Skud) share a few habits, which we follow: a small logo in the header instead of a text title; tiny upper-case section labels; rows that sit straight on the glass, separated by hairlines, not boxes; one accent colour; one filled button at most. Settings is now a logo and Done, a HIDDEN ICONS list, then plain rows for Open at login and Appearance. The welcome window is the logo, a title, two numbered rows and one Get started button. The logo is `assets/logo.png`, the app icon cropped to its body.

## Window corners

The Settings and welcome windows use 8-point corners, with 8-point cards and 6-point buttons inside them. The strip's corner radius is set by the user (slider, 8 to 32).

## Performance, measured

Release build, this Mac. `ICEOUT_TRACE=1` prints how long after launch things happen; `out/bench.sh` (not committed) launched the app 3 to 4 times per variant, waited 4 seconds, pressed the toggle and timed the window.

| Step | Before | After |
| --- | --- | --- |
| Buttons in the menu bar | 90 ms | 90 ms (unchanged) |
| First read of the bar applied (hidden icons hide) | about 1090 ms | about 295 ms |
| First window open after launch | 33 ms | 33 ms (unchanged) |
| Reads of the bar in the first 9 idle seconds | 2 | 1 |
| Idle memory and CPU | 28 MB, 0% | unchanged |

What did the work:
- The first read waited a fixed second for our buttons to settle. It now polls every 20 ms for our own button windows and reads as soon as they exist: 1090 ms to about 300 ms, the biggest gap found.
- The per-app Accessibility wait went from 0.15 s to 0.05 s. The gain is small (about 25 ms), because most apps answer in a few milliseconds and the read is already parallel. Slow apps are covered by the two-read tolerance.
- The watcher started with an empty list of icon windows, so it read the whole bar again two seconds after launch for nothing. It now starts from the windows that exist.

What was already fast and was left alone: opening a window (33 ms), scrolling (the Settings list is a plain scroll area of at most 8 rows), and reading after a change.

## Known gaps

- New menu bar items appear at the far left, which is the hidden side, so a newly installed app's icon may start hidden.
- Blocking calls left in the UI path: none known. Hiding one icon still takes a few seconds, because the bar needs time to re-layout between drag steps.
- System-managed items (the camera and microphone indicators) cannot be moved.

## No system window shadow

The system shadow of a transparent window is drawn from an old picture of the window, so after the window resized (opening Appearance) a square border appeared around the glass. The shadow is off. The glass has its own edge.

## Search

Right-click the Iceout icon, then Search. You type, and the icons whose names contain your letters in order are listed ("mt" finds "Moneta"). Enter opens the top one. A click opens any of them.

- **Plain letter match, not fuzzy scoring.** Ice uses a fuzzy search library. Here the list is about 40 names at most, so "letters in order" is enough, and it needs no new dependency.
- **It searches all icons, hidden or not.** A hidden icon is brought out and put back, the same way the strip does it (`open_item` calls the same `press_hidden`). A visible icon is just pressed.
- **Fixed window height.** Six rows tall, always. The window does not jump while you type, and we never have to resize it on each key.

## Keyboard shortcuts

Option + Command + I (I for Ice) shows or hides the strip. Option + Command + F opens search. Control + Option was the first choice, but other apps use it too; Option + Command + letter is rare. We did not use plain Command + F or Command + Space: they would steal find and Spotlight in every app. One switch in Settings turns both on or off. It is on by default and saved in `prefs.json`.
- **The keys are shown as one tiny line.** A first version listed the keys under the switch with key caps. It looked crowded. Now there is one small grey line under the switch label, only while the switch is on, and the row keeps its height so nothing moves. The right-click menu also shows the keys beside "Show Icons" and "Search…". Rejected: key rows with key caps (clutter).

- **System hot key calls (Carbon `RegisterEventHotKey`), not an event monitor.** They need no permission and cost nothing until a key is pressed. This is also what Ice does.
- **Fixed keys, one switch.** A key recorder needs a lot of code (key names, modifiers, clashes), and a first version does not need it. The trade-off: you cannot pick your own keys yet. If another app owns one of these keys, the register call just fails and that key stays off.
- **The window opens under the mouse.** There is no button click to place it under, so a shortcut uses where the mouse is, on the display it is on.
