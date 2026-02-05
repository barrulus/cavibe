# Wallpaper Mode

Cavibe can run as a desktop wallpaper/background layer on Wayland compositors that support the `wlr-layer-shell` protocol.

```bash
cavibe --mode wallpaper
```

## Niri

Niri has native support for layer-shell surfaces. To start cavibe as a wallpaper on login, add to your Niri config (`~/.config/niri/config.kdl`):

```kdl
// If cavibe is in your PATH:
spawn-at-startup "sh" "-c" "cavibe --mode wallpaper"

// Or with full path:
spawn-at-startup "sh" "-c" "/home/user/dev/cavibe/target/release/cavibe --mode wallpaper"
```

Note: Wallpaper mode uses wlr-layer-shell and won't appear in `niri msg windows` - it renders directly on the background layer. Cavibe will wait up to 30 seconds for outputs to become available at startup.

## Sway

Add to your Sway config (`~/.config/sway/config`):

```
exec cavibe --mode wallpaper
```

## Hyprland

Add to your Hyprland config (`~/.config/hypr/hyprland.conf`):

```
exec-once = cavibe --mode wallpaper
```

## X11 (with xwinwrap)

On X11, use `xwinwrap` to display cavibe as a wallpaper:

```bash
xwinwrap -fs -fdt -ni -b -nf -un -o 1.0 -st -- cavibe --mode wallpaper
```

## Systemd Service

Create a systemd user service (`~/.config/systemd/user/cavibe.service`):

```ini
[Unit]
Description=Cavibe Audio Visualizer Wallpaper
After=graphical-session.target
Wants=pipewire.service

[Service]
ExecStart=/path/to/cavibe --mode wallpaper
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
```

Then enable it:

```bash
systemctl --user enable --now cavibe.service
```

## Transparent Terminal Alternative

If layer-shell isn't working, you can use a transparent terminal positioned as a background.

**Niri + Kitty** (`~/.config/niri/config.kdl`):

```kdl
window-rule {
    match app-id="cavibe-term"
    open-floating true
    default-floating-position x=0 y=0 relative-to="top-left"
}

spawn-at-startup "kitty" "--class" "cavibe-term" "-o" "background_opacity=0.0" "-e" "cavibe"
```

**Niri + Ghostty** (`~/.config/niri/config.kdl`):

```kdl
window-rule {
    match app-id="cavibe-term"
    open-floating true
    default-floating-position x=0 y=0 relative-to="top-left"
}

spawn-at-startup "ghostty" "--class=cavibe-term" "--background-opacity=0" "-e" "cavibe"
```

Note: The `app-id` must match what the terminal sets. Use `niri msg windows` to check actual app-ids.
