# Configuration

Cavibe looks for a config file at `~/.config/cavibe/config.toml`. Generate a default config with:

```bash
cavibe --init-config
```

CLI arguments take priority over config file values.

## Full Reference

```toml
[display]
mode = "terminal"           # "terminal" or "wallpaper"
rotate_styles = false       # auto-cycle visualizer styles
rotation_interval_secs = 30 # seconds between style changes

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
reverse_mirror = false      # with mirror: lows meet in middle, highs on outside
opacity = 1.0               # 0.0-1.0, bar transparency (wallpaper mode only)

[text]
show_title = true
show_artist = true
animation_speed = 1.0
pulse_intensity = 0.8
position = "bottom"         # top, bottom, center
font_style = "normal"       # normal, bold, ascii, figlet
alignment = "center"        # left, center, right
animation_style = "scroll"  # none, scroll, pulse, fade, wave
margin_top = 0              # pixels in wallpaper mode, characters in terminal
margin_bottom = 0
margin_horizontal = 2
use_color_scheme = true     # false to use custom colors below
# title_color = { r = 255, g = 255, b = 255 }
# artist_color = { r = 200, g = 200, b = 200 }
# background_color = { r = 0, g = 0, b = 0 }  # text background (wallpaper only)

[wallpaper]
anchor = "fullscreen"       # fullscreen, center, top, bottom, left, right,
                            # top-left, top-right, bottom-left, bottom-right
# width = "50%"             # pixels (e.g. "800") or percentage (e.g. "50%")
# height = "200"
# margin = 10               # uniform margin from all edges (pixels)
# margin_top = 0
# margin_right = 0
# margin_bottom = 0
# margin_left = 0
```
