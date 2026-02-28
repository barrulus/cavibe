# Styles & Themes

## Visualizer Styles

| Style | Description |
|-------|-------------|
| **Classic Bars** | Traditional vertical frequency bars |
| **Mirrored** | Bars grow from center, mirrored top/bottom |
| **Wave** | Continuous wave form visualization |
| **Dots** | Floating dots with trailing effect |
| **Blocks** | Unicode block characters for smooth gradients |
| **Oscilloscope** | Raw audio waveform display (time-domain) |
| **Spectrogram** | Scrolling 2D heatmap (frequency vs time) |
| **Radial** | Frequency bars radiating outward from a circle |

| Radial | Classic Bars | Oscilloscope |
|--------|--------------|--------------|
| ![Radial](images/new-image-3.png) | ![Classic](images/new-image-6.png) | ![Oscilloscope](images/new-image-5.png) |

| Dots | Spectrogram | Monochrome Bars |
|------|-------------|-----------------|
| ![Dots](images/new-image-8.png) | ![Spectrogram](images/new-image-9.png) | ![Monochrome](images/new-image-7.png) |

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

| Spectrum | Purple | Fire |
|----------|--------|------|
| ![Spectrum](images/new-image-2.png) | ![Purple](images/new-image-1.png) | ![Fire](images/new-image-8.png) |

Cycle colors with `c` in terminal mode or `cavibe ctl color next` in wallpaper mode.

## Font Styles

| Style | Description |
|-------|-------------|
| **Normal** | Standard size text |
| **Bold** | Larger text with thicker strokes |
| **Ascii** | Smaller, compact text |
| **Figlet** | Large banner-style text with outline effect |

Text scales proportionally with the surface size — smaller wallpaper surfaces get smaller text, larger surfaces get larger text.

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
