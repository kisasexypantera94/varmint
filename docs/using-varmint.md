# Working inside and outside a Varmint VM

Varmint puts a Linux desktop inside a macOS window. Most confusion comes from
forgetting which system currently owns the mouse, keyboard, or clipboard.

## Mouse and keyboard focus

- Click inside the VM to capture the mouse.
- Press **Control + Command** (`⌃⌘`) to release or capture it again.
- Switching to another macOS app releases the mouse automatically.
- Use the Varmint **View** menu to enter or leave full screen.

The **Command** key belongs to macOS and is not sent to Linux. Inside the VM,
use Linux shortcuts based on **Control** and **Alt**. For example, use
**Control+C** in a Linux application instead of **Command+C**. The Mac
**Option** key acts as Linux **Alt**.

### Switching Linux applications

These shortcuts work inside the VM:

| Shortcut | Action |
| --- | --- |
| `Option+Tab` | Switch to the next Linux application |
| `Option+Shift+Tab` | Switch to the previous Linux application |
| `Option+Backtick` | Switch between windows of the current application |
| `Option+F4` | Close the active Linux window |
| `Control+F9` | Show the windows on the current Linux desktop |
| `Control+F10` | Show the windows on all Linux desktops |
| `Control+F12` | Show the Linux desktop |

On a Mac keyboard, the F-key shortcuts may also require **Fn**, depending on
the macOS keyboard setting.

`Control+F12` is useful when a game window stops responding. It is handled by
Plasma and should still reveal the desktop while the rest of the guest is
responsive. If the whole Linux desktop is frozen, press `⌃⌘` to release the
mouse and return to macOS instead.

Useful host shortcuts:

| Shortcut | Action |
| --- | --- |
| `⌃⌘` | Capture or release the mouse |
| `⌘⇧S` | Show or hide Varmint performance statistics |
| `⌘Q` | Ask the guest to shut down cleanly |
| `⌘Tab` | Switch to another macOS application |

Pressing `⌘Q` or closing the window a second time skips the guest shutdown
wait. Use that only when the VM is stuck, because running applications may lose
data.

## Copy and paste

Varmint automatically synchronizes text between the macOS and Linux
clipboards. The desktop session must be running, and synchronization may take a
fraction of a second.

### From macOS to Linux

1. Copy on macOS with `⌘C`.
2. Focus the VM.
3. Paste with `Ctrl+V` in a normal Linux application, or `Ctrl+Shift+V` in
   Konsole.

### From Linux to macOS

1. Copy with `Ctrl+C` in a normal Linux application, or `Ctrl+Shift+C` in
   Konsole.
2. Switch to the macOS application.
3. Paste with `⌘V`.

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
press `Ctrl+Shift+C` after selecting it. You can also use the terminal's
right-click menu.

Be careful when pasting several lines: the terminal may run them immediately.
Read unfamiliar commands before pasting them. Setup guides opened from Vet
provide a **Copy** button for each command block.

Passwords normally produce no visible characters in a terminal. This is
expected; type or paste the password and press Enter.

## If input seems stuck

- Press `⌃⌘` once to release the mouse, then click the VM again if needed.
- Make sure the intended macOS or Linux window has focus.
- In Konsole, use `Ctrl+Shift+V`, not `⌘V` or `Ctrl+V`.
- If clipboard synchronization has not started, log in to the Linux desktop and
  try again.
