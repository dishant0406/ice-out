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

1. Right-click the icon and choose Search, or use the chord. `open(Mode::Search)` in `src/status_item.rs` clears the box, makes Ice Out the front app and focuses the box (`focus_when_shown`). The window opens under the Iceout icon.
2. Each key changes `State.query`. `search_results` in `src/main.rs` keeps the icons that match (`matches`). Six rows show at most, and `fit` resizes the window to the rows.
3. Enter (`on_submit`) or a click calls `open_item`: the window closes, then a hidden icon is revealed with `press_hidden`, or a visible one is pressed.
4. Escape in the box closes the window (`on_input_key` in `search_view`: the box would otherwise take Escape and only lose focus).
5. Nothing matches: the list shows "No matching icons". Enter does nothing. Click away or Done closes it as always.
6. Busy (another icon is moving): `open_item` does nothing, so a second request cannot collide.

## Keyboard shortcuts

1. To learn the keys: Settings shows them in a small line under the switch (only while it is on).
2. At start `hotkeys::install` remembers the app handle. When Accessibility is allowed, `apply` calls `hotkeys::set_enabled(true)`, which makes the event tap the first time.
3. Every key press goes to `on_key` in `src/hotkeys.rs`. Pressing I with Option and Command held marks I as held and swallows it. While I is held, Space (strip) or F (search) with Option and Command runs the action (`open_from_shortcut`) and is swallowed. Both open under the Iceout icon, like a click on it. Releasing I clears the mark.
4. Everything else is passed on untouched.
5. The Settings switch calls `set_enabled` and saves `hotkeys` on another thread. Off means the tap is paused.
6. No Accessibility: no tap, the keys do nothing, and the switch still shows on. Allowing Accessibility fixes it on the next read.
7. The system turns the tap off if it is slow: `on_key` sees that and turns it back on.

## Presets

1. Settings, then the "Presets" row (`presets_row`), opens `Mode::Presets` in the same window and fits it (`presets_height`).
2. Type a name in the box and press Return: `save_preset` stores the hidden icons under that name (`add_preset`), replacing a preset with the same name. An empty or blank name does nothing.
3. Press a name (or pick it in the right-click menu): `apply_preset` works out the moves (`presets::moves`) and `status_item::move_items` does them one by one on a background thread, then reads the bar again. The window closes.
4. Nothing differs: nothing moves. Busy (an icon is moving or out for a peek): the request is ignored.
5. An icon in the preset is gone: it is not in the list, so there is nothing to move for it. A new icon: shown (not in the hidden list), so it is moved out of hiding if it was hidden.
6. The x deletes a preset. The list is saved on another thread (`presets::save`) and the menu names are updated (`set_preset_names`).
7. "Active" appears on a preset that matches the bar now.

## Triggers

1. Settings, then the "Triggers" row, opens `Mode::Triggers` (`triggers_view`). With no trigger it says "No triggers yet". With no preset it says to save a preset first.
2. "New trigger" opens the form (`State.draft`): pick WHEN (a dot), type a time range like `09:00-18:00`, or tick one or more of the open apps (the list is read when you choose "An app is running"), pick the preset to APPLY. "Add trigger" is grey until the form is valid (`triggers::is_valid`): a time range must parse, at least one app must be ticked, and a preset must be chosen.
3. Add: the trigger is saved (`triggers::save`) and checked at once (`triggers::evaluate`). Back leaves the form first, then the list.
4. Every 30 seconds `triggers::start` calls `evaluate` on the main thread. The last true trigger in the list wins. If the winner is different from last time, its preset is applied (`apply_preset`), and `last_trigger` remembers it. No winner clears `last_trigger`, so the same trigger fires again next time it becomes true.
5. No Accessibility: nothing is checked. Icons moving: `apply_preset` says no, and the check is repeated in 30 seconds.
6. Delete a trigger with the x. Deleting a preset removes the triggers that use it.
7. A time range that crosses midnight (`22:00-06:00`) works. An app trigger is true when any ticked app is open (whole name, ignoring case).

## Closing

Done, a click on the Iceout icon, or a click anywhere else (global mouse monitor) hides the window.

## Where things live

- `src/main.rs`: state, the two views, reading and merging the menu bar (`reload`, `apply`).
- `src/items.rs`: Accessibility reads, pressing an item, the synthetic drag.
- `src/status_item.rs`: the two buttons, the right-click menu, window placement, hide and reveal jobs.
