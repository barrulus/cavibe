# Runtime Control

When running in wallpaper mode, cavibe listens on a Unix socket for control commands. Use `cavibe ctl` to control a running instance.

## Commands

```bash
cavibe ctl style next       # Cycle to next visualizer style
cavibe ctl style prev       # Cycle to previous style
cavibe ctl color next       # Cycle to next color scheme
cavibe ctl color prev       # Cycle to previous color scheme
cavibe ctl toggle           # Show/hide the visualizer
cavibe ctl opacity 0.5      # Set opacity (0.0-1.0)
cavibe ctl reload           # Reload config file
cavibe ctl status           # Show current settings
cavibe ctl list styles      # List available visualizer styles
cavibe ctl list colors      # List available color schemes
cavibe ctl ping             # Check if cavibe is running

# Text controls
cavibe ctl text position top    # Move text to top/bottom/center
cavibe ctl text font figlet     # Set font: normal, bold, ascii, figlet
cavibe ctl text animation wave  # Set animation: scroll, pulse, fade, wave, none
cavibe ctl text toggle          # Show/hide song text
```

## Compositor Keybindings

### Niri

Add to `~/.config/niri/config.kdl`:

```kdl
binds {
    Mod+Shift+V { spawn "cavibe" "ctl" "style" "next"; }
    Mod+Shift+C { spawn "cavibe" "ctl" "color" "next"; }
    Mod+Shift+H { spawn "cavibe" "ctl" "toggle"; }
}
```

### Sway

Add to `~/.config/sway/config`:

```
bindsym $mod+Shift+v exec cavibe ctl style next
bindsym $mod+Shift+c exec cavibe ctl color next
bindsym $mod+Shift+h exec cavibe ctl toggle
```

### Hyprland

Add to `~/.config/hypr/hyprland.conf`:

```
bind = $mainMod SHIFT, V, exec, cavibe ctl style next
bind = $mainMod SHIFT, C, exec, cavibe ctl color next
bind = $mainMod SHIFT, H, exec, cavibe ctl toggle
```

## Socket Details

The IPC socket is created at `$XDG_RUNTIME_DIR/cavibe.sock` (fallback: `/tmp/cavibe.sock`).

- The socket is created when cavibe starts in wallpaper mode
- It is cleaned up automatically on exit
- Stale sockets from crashed processes are removed on startup

## Protocol

The socket uses a simple line-based text protocol. Each command is a single line terminated by `\n`, and the response is a single line prefixed with `ok:` or `err:`.

This means you can also interact with it directly using socat:

```bash
echo "style next" | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/cavibe.sock
```
