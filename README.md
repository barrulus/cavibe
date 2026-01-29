# Cavibe

Audio visualizer with animated song display - terminal and wallpaper modes.

Cavibe captures system audio, performs frequency analysis, and displays colorful visualizations alongside animated track information from MPRIS-compatible music players.

## Features

- **Multiple visualizer styles**: Classic bars, mirrored, wave, dots, blocks
- **Color schemes**: Spectrum, rainbow, fire, ocean, forest, purple, monochrome
- **Animated song text**: Pulsing colors synced to audio intensity
- **Font styles**: Normal, bold, ASCII art, and Figlet banner text
- **MPRIS integration**: Displays current track from Spotify, MPD, VLC, etc.
- **Style rotation**: Automatically cycle through visualizer styles
- **Terminal mode**: Full TUI experience with keyboard controls
- **Wallpaper mode**: Native Wayland layer-shell support (Niri, Sway, Hyprland)

## Installation

### Using Nix

```bash
nix develop  # Enter development shell
cargo build --release
```

### Manual

Requires Rust 1.70+ and the following system dependencies:

- ALSA or PulseAudio/PipeWire for audio capture
- D-Bus for MPRIS metadata

```bash
cargo build --release
./target/release/cavibe
```

## Usage

```bash
# Basic usage (terminal mode)
cavibe

# With options
cavibe --bars 128 --colors fire

# Rotate styles every 15 seconds
cavibe --rotate --rotate-interval 15

# Wallpaper mode (Wayland)
cavibe --mode wallpaper

# With Figlet-style large text
cavibe --font-style figlet
```

### Keyboard Controls (Terminal Mode)

| Key | Action |
|-----|--------|
| `q` | Quit |
| `s` | Cycle visualizer style |
| `c` | Cycle color scheme |
| `Ctrl+C` | Quit |

## Wallpaper Mode

Cavibe can run as a desktop wallpaper/background layer on Wayland compositors that support the `wlr-layer-shell` protocol.

### Niri

Niri has native support for layer-shell surfaces. Simply run:

```bash
cavibe --mode wallpaper
```

To start cavibe as a wallpaper on login, add to your Niri config (`~/.config/niri/config.kdl`):

```kdl
spawn-at-startup "cavibe" "--mode" "wallpaper"
```

Or create a systemd user service (`~/.config/systemd/user/cavibe.service`):

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

### Sway

Add to your Sway config (`~/.config/sway/config`):

```
exec cavibe --mode wallpaper
```

### Hyprland

Add to your Hyprland config (`~/.config/hypr/hyprland.conf`):

```
exec-once = cavibe --mode wallpaper
```

### X11 (with xwinwrap)

On X11, use `xwinwrap` to display cavibe as a wallpaper:

```bash
xwinwrap -fs -fdt -ni -b -nf -un -o 1.0 -st -- cavibe --mode wallpaper
```

### Transparent Terminal Alternative

If layer-shell isn't working, you can use a transparent terminal positioned as a background:

**Niri example** (`~/.config/niri/config.kdl`):
```kdl
window-rule {
    match app-id="cavibe-term"
    open-floating true
    default-floating-position x=0 y=0 relative-to="screen"
}

spawn-at-startup "kitty" "--class" "cavibe-term" "-o" "background_opacity=0.0" "cavibe"
```

## Configuration

Cavibe looks for a config file at `~/.config/cavibe/config.toml`. Generate a default config with:

```bash
cavibe --init-config
```

Example configuration:

```toml
[display]
mode = "terminal"           # "terminal" or "wallpaper"
rotate_styles = false
rotation_interval_secs = 30

[audio]
sample_rate = 44100
buffer_size = 1024
smoothing = 0.7
sensitivity = 1.0           # 0.1-10.0, higher = more reactive

[visualizer]
bars = 64
color_scheme = "spectrum"   # spectrum, rainbow, fire, ocean, forest, purple, monochrome
bar_width = 2               # proportional width of bars
bar_spacing = 1             # proportional spacing between bars
mirror = false              # mirror visualization from center

[text]
show_title = true
show_artist = true
animation_speed = 1.0
pulse_intensity = 0.8
position = "bottom"         # top, bottom, center
font_style = "normal"       # normal, bold, ascii, figlet
alignment = "center"        # left, center, right
animation_style = "scroll"  # none, scroll, pulse, fade, wave
```

## Visualizer Styles

- **Classic Bars**: Traditional vertical frequency bars
- **Mirrored**: Bars grow from center, mirrored top/bottom
- **Wave**: Continuous wave form visualization
- **Dots**: Floating dots with trailing effect
- **Blocks**: Unicode block characters for smooth gradients

## Color Schemes

- **Spectrum**: Purple to red gradient (classic audio spectrum)
- **Rainbow**: Full hue rotation
- **Fire**: Red/orange/yellow
- **Ocean**: Blue/cyan/teal
- **Forest**: Green tones
- **Purple**: Magenta/pink
- **Monochrome**: Grayscale intensity

## Requirements

- Linux (uses ALSA/PulseAudio and MPRIS)
- A terminal with true color support (kitty, alacritty, wezterm, etc.)
- Music player with MPRIS support for track info

## Roadmap

- [x] Wallpaper mode (Wayland layer-shell)
- [x] Figlet/ASCII art text styles
- [x] Proportional bar width/spacing
- [ ] X11 native wallpaper mode (without xwinwrap)
- [ ] Album art display
- [ ] Custom color schemes from config
- [ ] More visualizer styles
- [ ] Audio device selection menu
- [ ] Multi-monitor support for wallpaper mode

## Troubleshooting

### Wallpaper mode shows error about layer-shell

Your compositor doesn't support the `wlr-layer-shell` protocol. This is required for native wallpaper mode. Use the transparent terminal alternative instead.

### No audio visualization

1. Ensure PipeWire or PulseAudio is running
2. Check that audio is playing through your default sink
3. Try increasing sensitivity: `cavibe --sensitivity 2.0`

### No track info displayed

Cavibe uses MPRIS to get track metadata. Ensure your music player supports MPRIS (most do: Spotify, Firefox, VLC, MPD with mpDris2, etc.).

## License

MIT
