# How it flows

## First launch

On the first run (or whenever Accessibility is off) a welcome window opens in the middle of the main display. It has "Allow Accessibility" (opens the right System Settings page; the app checks every 2 seconds and shows "Allowed ✓" when you turn it on), an "Open at login" switch, and "Get started", which turns on once Accessibility is allowed and saves `setup_done`. "Open at login" writes or removes `~/Library/LaunchAgents/dev.iceout.plist`, and is also in Settings.

## Start

1. `main` in `src/main.rs` builds the app: no Dock icon, no window yet, GPU released while closed.
2. `status_item::add` (`src/status_item.rs`) puts two buttons in the menu bar: the toggle (the Iceout mark) and, on its left, the divider `|`.
3. One second later a background thread reads the menu bar (`items::list`). Every icon left of the divider is marked hidden, and the divider is made wide, so those icons disappear.

If Accessibility is off, the window says "Turn on Accessibility for this app in System Settings, Privacy & Security." Nothing breaks. You turn it on and open the window again.

## Left-click the Iceout icon: the strip

`clicked:` calls `open(Mode::Strip)`. The window opens under the button in about 40 ms, wide enough for the hidden icons. With nothing hidden it says "Nothing hidden yet. Right-click the Iceout icon, then Settings."

Click an icon (`press_hidden`, background thread). Only that icon ever shows:
1. The window closes.
2. The icon is moved from the hidden side to the spot just left of our toggle, which is the first spot right of the divider (`items::move_left_of`). The divider stays wide the whole time, so every other hidden icon stays hidden.
3. The icon is pressed with Accessibility, so its menu opens. The cursor does not move.
4. The icon stays where it is until you click or press a key in another app, or 15 seconds pass.
5. The icon is moved back to the hidden side.

If the app that owns the icon quit, nothing is found and nothing happens.

## Right-click the Iceout icon: the menu

Settings… opens the window in Settings mode: a switch per icon, a Done button, and at the bottom an Advanced section (closed at first). Quit makes the divider thin, then quits.

## Settings: hide or show an icon

Turning a switch (`move_item`, background thread): the icon is moved to the left of the divider (hide) or to the left of the toggle (show), with the same window-targeted mouse events. No other icon moves or flashes. The divider is not changed, except that the very first hide widens it. Then the list is read again.

Edge cases: a failed move leaves the icon where it was. System-managed items (the camera and microphone indicators) cannot be moved, so their switch does nothing.

## Settings: Advanced

Click Advanced to open it. It shows a live preview of the strip and four sliders: icon size, padding, spacing and corner radius. Dragging a slider changes the preview at once. When you let go, the values are saved (`prefs.json` in `~/Library/Application Support/dev.iceout`, written on another thread). The strip uses them the next time it opens. Arrow keys also move a focused slider.

## When icons come and go

- **New app opens:** macOS puts its icon at the far left, which is the hidden side. Within about 2.5 seconds the watcher sees a new icon window and the bar is read again. The icon is in the strip and in Settings with its switch on. Turn the switch off to bring it into view.
- **App quits:** its icon window goes away, the watcher sees it, and the icon leaves both lists within about 2.5 seconds.
- **App restarts:** it gets a new process id, but icons are matched fresh on each read, so it simply shows up again as a new icon (at the far left, hidden).
- **Icon moved by hand (Command-drag):** the next read sees the new position.
- **While moving or peeking:** the watcher waits, so a half-moved icon is not read.

## Odd cases

- **Several displays:** the strip and Settings open under the place you clicked, on that display. The click point is remembered when you click (`LAST_CLICK`), the display that holds it is found, and Rune places the window with global points (`DisplayTarget::Primary`, `OverlayPlacement::Rect`).
- **A slow app:** if an app does not answer in time for one read, its icon stays in the list for up to two reads while the app is still running, so the list does not flicker.
- **An app restarts:** macOS remembers each icon's place, so most apps come back where they were. If an icon you had in view comes back hidden with a new process id, it is moved back into view. An icon you hid yourself stays hidden.
- **No room (notch):** icons right of the divider that sit left of the notch's right edge cannot be seen, so they are moved to the hidden side, and you can open them from the strip. Nothing happens on a Mac without a notch.

## Search

1. Right-click the icon, choose Search. `search:` in `src/status_item.rs` calls `open(Mode::Search)`. The box starts empty and has focus.
2. Each key changes `State.query`. `search_results` in `src/main.rs` keeps the icons that match (`matches`). Six rows show at most.
3. Enter (`on_submit`) or a click calls `open_item`: the window closes, then a hidden icon is revealed with `press_hidden`, or a visible one is pressed.
4. Nothing matches: the list is empty. Enter does nothing. Click away or Done closes it as always.
5. Busy (another icon is moving): `open_item` does nothing, so a second request cannot collide.

## Keyboard shortcuts

0. To learn the keys, right-click the icon: "Show Icons" and "Search…" show them.
1. At start, `hotkeys::install` hooks the system event, and `hotkeys::set_enabled` registers the two keys if the saved switch is on.
2. A key press calls `pressed` in `src/hotkeys.rs` on the main thread. Space hides the window if it is open, else opens the strip. F opens search. Both open under the mouse (`open_at_mouse`).
3. The Settings switch calls `set_enabled` again and saves `hotkeys` on another thread.
4. Another app owns the key: its register call fails and that key does nothing. The switch still shows on.

## Closing

Done, a click on the Iceout icon, or a click anywhere else (global mouse monitor) hides the window.

## Where things live

- `src/main.rs`: state, the two views, reading and merging the menu bar (`reload`, `apply`).
- `src/items.rs`: Accessibility reads, pressing an item, the synthetic drag.
- `src/status_item.rs`: the two buttons, the right-click menu, window placement, hide and reveal jobs.
