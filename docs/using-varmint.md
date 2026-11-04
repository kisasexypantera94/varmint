# Working inside and outside a Varmint VM

## Mouse and keyboard focus

- Click inside the VM to capture the mouse.
- Press **Control + Command** (`⌃⌘`) to release or capture it again.
- Switching to another macOS app releases the mouse automatically.
- Use the Varmint **View** menu to enter or leave full screen.

The **Command** key belongs to macOS and is not sent to Linux. Inside the VM,
use Linux shortcuts based on **Control** and **Alt**. For example, use
**Control+C** in a Linux application instead of **Command+C**. The Mac
**Option** key acts as Linux **Alt**.

### Keyboard shortcuts

Use these shortcuts while the VM window is focused:

| Shortcut | Action |
| --- | --- |
| `⌃⌘` | Capture or release the mouse |
| `⌘⇧S` | Show or hide Varmint performance statistics |
| `Shift+F9` | Show or hide the Metal HUD |
| `⌘Q` | Ask the guest to shut down cleanly |
| `Option+Tab` | Switch to the next Linux application |
| `Option+Shift+Tab` | Switch to the previous Linux application |
| `Option+Backtick` | Switch between windows of the current application |
| `Option+F4` | Close the active Linux window |
| `Control+F12` | Show the Linux desktop |

By default on Apple keyboards, hold **Fn** (or the Globe key) for shortcuts
that use F-keys. If macOS is set to use F1, F2, and so on as standard function
keys, omit it.

If a game stops responding, use `Control+F12` to show the Linux desktop. If the
whole VM is frozen, press `⌃⌘` to release the mouse.

Pressing `⌘Q` or closing the window a second time skips the guest shutdown
wait. Use that only when the VM is stuck, because running applications may lose
data.

## Copy and paste

Varmint automatically synchronizes text between the macOS and Linux
clipboards.

Copy and paste with `⌘C` and `⌘V` on macOS, or `Ctrl+C` and `Ctrl+V` in Linux.
Konsole uses `Ctrl+Shift+C` and `Ctrl+Shift+V` instead.

To move text between macOS and Linux, copy it on one side, switch focus, then
paste it on the other.

The shared clipboard carries text, not files or images. Its maximum size is
16 MiB. If the previous clipboard value appears, wait briefly and copy again.

## Copying commands in Konsole

Terminal shortcuts differ from normal desktop applications:

| Action | Konsole shortcut |
| --- | --- |
| Copy selected text | `Ctrl+Shift+C` |
| Paste | `Ctrl+Shift+V` |
| Stop the running command | `Ctrl+C` |

Selecting terminal text is not enough to place it on the shared clipboard;
press `Ctrl+Shift+C` after selecting it.

Passwords normally produce no visible characters in a terminal. This is
expected; type or paste the password and press Enter.
