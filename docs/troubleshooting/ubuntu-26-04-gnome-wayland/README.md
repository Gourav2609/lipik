# Lipik on Ubuntu 26.04 GNOME Wayland

Written on Ubuntu 26.04.1 LTS, GNOME Wayland, for the upstream app (0.9.8). Not yet tested with Lipik.

The fix is to use `ydotool` for typing and `lipik_keys` for shortcuts.

_Note: Run commands one by one in terminal_

## 1. Check Wayland and uinput

```bash
echo "$XDG_SESSION_TYPE"
id
ls -l /dev/uinput
grep -R 'uinput' /etc/udev/rules.d /usr/lib/udev/rules.d 2>/dev/null
```

You should be using `Wayland`, be in the `input` group, and have `/dev/uinput` owned by `root:input` with mode `0660`. Ubuntu provides the required udev rule in `80-uinput.rules`.

If you are not in `input`:

```bash
sudo usermod -aG input "$USER"
```

Log out and back in after adding the group.

## 2. Install and test ydotool

```bash
sudo apt install ydotool
systemctl --user start ydotool
systemctl --user status ydotool --no-pager
ydotool type "HELLO FROM YDOTOOL"
```

The service should show `active (running)` and the test should type into the focused application.

## 3. Configure Lipik

Edit Lipik's settings with:

```bash
sed -i 's/"typing_tool": "auto"/"typing_tool": "ydotool"/' ~/.local/share/com.gourav.lipik/settings_store.json
sed -i 's/"keyboard_implementation": "tauri"/"keyboard_implementation": "lipik_keys"/' ~/.local/share/com.gourav.lipik/settings_store.json
```

Restart Lipik:

```bash
pkill lipik
lipik --start-hidden &
```

Check the Lipik log for:

```text
lipik-keys manager thread started
lipik-keys shortcuts initialized
```

Then use your existing Lipik shortcut and test dictation.

## 4. Hide the overlay

On GNOME, the overlay is a regular window that can take the focus, so nothing is pasted. Set **Overlay** to **None**.

## 5. Install wl-clipboard

Lipik uses `wl-copy` on Wayland when it is installed:

```bash
sudo apt install wl-clipboard
```

## 6. Non-QWERTY keyboard layouts

`ydotool` sends physical keys, so the built-in paste methods fail on layouts such as bépo or Dvorak: Ctrl+V presses the QWERTY `V` key, and Shift+Insert breaks while a modifier of your shortcut is still held.

Use an external script instead: it presses Ctrl and the key that types `v` on your layout, `KEY_U` (22) on bépo or `KEY_DOT` (52) on Dvorak (codes in `/usr/include/linux/input-event-codes.h`).

Create `~/.local/bin/lipik-paste`, here for bépo:

```sh
#!/bin/sh
printf '%s' "$1" | wl-copy
sleep 0.1
ydotool key 29:1 22:1 22:0 29:0
```

```bash
chmod +x ~/.local/bin/lipik-paste
```

Set **Paste Method** to **External Script** with this path.
