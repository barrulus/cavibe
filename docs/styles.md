# Styles & Themes

## Visualizer Styles

| Style | Description |
|-------|-------------|
| **Classic Bars** | Traditional vertical frequency bars |
| **Mirrored** | Bars grow from center, mirrored top/bottom |
| **Wave** | Continuous wave form visualization |
| **Dots** | Floating dots with trailing effect |
| **Blocks** | Unicode block characters for smooth gradients |

Cycle styles with `s` in terminal mode or `cavibe ctl style next` in wallpaper mode.

Auto-rotate through all styles:

```bash
cavibe --rotate --rotate-interval 15
```

## Color Schemes

| Scheme | Description |
|--------|-------------|
| **Spectrum** | Purple to red gradient (classic audio spectrum) |
| **Rainbow** | Full hue rotation |
| **Fire** | Red/orange/yellow |
| **Ocean** | Blue/cyan/teal |
| **Forest** | Green tones |
| **Purple** | Magenta/pink |
| **Monochrome** | Grayscale intensity |

Cycle colors with `c` in terminal mode or `cavibe ctl color next` in wallpaper mode.

## Font Styles

| Style | Description |
|-------|-------------|
| **Normal** | Standard size text (scale 3x in wallpaper mode) |
| **Bold** | Larger text with thicker strokes (scale 4x with multi-pass rendering) |
| **Ascii** | Smaller, compact text (scale 2x) |
| **Figlet** | Large banner-style text with outline effect (scale 5x) |

```bash
cavibe --font-style figlet
```

## Text Alignment

| Alignment | Description |
|-----------|-------------|
| **Left** | Text aligned to the left edge |
| **Center** | Text centered horizontally (default) |
| **Right** | Text aligned to the right edge |

```bash
cavibe --text-alignment right
```

## Text Animations

| Animation | Description |
|-----------|-------------|
| **None** | Static text, no animation |
| **Scroll** | Text scrolls horizontally when wider than display area (ping-pong) |
| **Pulse** | Text opacity pulses with audio intensity |
| **Fade** | Text fades in and out over time |
| **Wave** | Characters oscillate up and down in a wave pattern |

```bash
cavibe --text-animation wave --animation-speed 1.5
```
