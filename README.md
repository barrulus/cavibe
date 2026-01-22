# Cavibe

Audio visualizer with animated song display - terminal and wallpaper modes.

Cavibe captures system audio, performs frequency analysis, and displays colorful visualizations alongside animated track information from MPRIS-compatible music players.

## Features

- **Multiple visualizer styles**: Classic bars, mirrored, wave, dots, blocks
- **Color schemes**: Spectrum, rainbow, fire, ocean, forest, purple, monochrome
- **Animated song text**: Pulsing colors synced to audio intensity
- **MPRIS integration**: Displays current track from Spotify, MPD, VLC, etc.
- **Style rotation**: Automatically cycle through visualizer styles
- **Terminal mode**: Full TUI experience with keyboard controls
- **Wallpaper mode**: (Planned) Desktop background integration

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

# Wallpaper mode (coming soon)
cavibe --mode wallpaper
```

### Keyboard Controls

| Key | Action |
|-----|--------|
| `q` | Quit |
| `s` | Cycle visualizer style |
| `c` | Cycle color scheme |
| `Ctrl+C` | Quit |

## Configuration

Cavibe looks for a config file at `~/.config/cavibe/config.toml`:

```toml
[display]
mode = "terminal"
rotate_styles = false
rotation_interval_secs = 30

[audio]
sample_rate = 44100
buffer_size = 1024
smoothing = 0.7

[visualizer]
bars = 64
color_scheme = "spectrum"
bar_width = 2
bar_spacing = 1
mirror = false

[text]
show_title = true
show_artist = true
animation_speed = 1.0
pulse_intensity = 0.8
position = "bottom"
font_style = "normal"
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

- [ ] Wallpaper mode (X11 overlay / Wayland layer-shell)
- [ ] Album art display
- [ ] Custom color schemes from config
- [ ] More visualizer styles
- [ ] Audio device selection menu
- [ ] Figlet/ASCII art text

## License

MIT
