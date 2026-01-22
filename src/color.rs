use palette::{Hsl, IntoColor, Srgb};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub enum ColorScheme {
    #[default]
    Spectrum,
    Rainbow,
    Fire,
    Ocean,
    Forest,
    Purple,
    Monochrome,
}

impl FromStr for ColorScheme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "spectrum" => Ok(Self::Spectrum),
            "rainbow" => Ok(Self::Rainbow),
            "fire" => Ok(Self::Fire),
            "ocean" => Ok(Self::Ocean),
            "forest" => Ok(Self::Forest),
            "purple" => Ok(Self::Purple),
            "mono" | "monochrome" => Ok(Self::Monochrome),
            _ => Err(format!("Unknown color scheme: {}", s)),
        }
    }
}

impl ColorScheme {
    /// Get color for a given position (0.0 to 1.0) and intensity (0.0 to 1.0)
    pub fn get_color(&self, position: f32, intensity: f32) -> (u8, u8, u8) {
        let (h, s, l) = match self {
            ColorScheme::Spectrum => {
                // Classic spectrum: purple -> blue -> cyan -> green -> yellow -> red
                let hue = 270.0 - (position * 270.0);
                (hue, 0.9, 0.4 + intensity * 0.3)
            }
            ColorScheme::Rainbow => {
                let hue = position * 360.0;
                (hue, 0.85, 0.5 + intensity * 0.2)
            }
            ColorScheme::Fire => {
                // Red -> orange -> yellow
                let hue = position * 60.0;
                (hue, 0.95, 0.3 + intensity * 0.4)
            }
            ColorScheme::Ocean => {
                // Deep blue -> cyan -> teal
                let hue = 180.0 + position * 60.0;
                (hue, 0.8, 0.3 + intensity * 0.35)
            }
            ColorScheme::Forest => {
                // Deep green -> lime -> yellow-green
                let hue = 80.0 + position * 60.0;
                (hue, 0.75, 0.25 + intensity * 0.35)
            }
            ColorScheme::Purple => {
                // Deep purple -> magenta -> pink
                let hue = 270.0 + position * 60.0;
                (hue, 0.8, 0.35 + intensity * 0.3)
            }
            ColorScheme::Monochrome => {
                // White/gray based on intensity
                (0.0, 0.0, intensity * 0.8)
            }
        };

        let hsl = Hsl::new(h, s, l);
        let rgb: Srgb = hsl.into_color();

        (
            (rgb.red * 255.0) as u8,
            (rgb.green * 255.0) as u8,
            (rgb.blue * 255.0) as u8,
        )
    }

    /// Get a pulsing color for text based on audio intensity
    pub fn get_text_color(&self, base_position: f32, intensity: f32, time: f32) -> (u8, u8, u8) {
        // Add time-based shimmer effect
        let shimmer = (time * 2.0).sin() * 0.1;
        let adjusted_intensity = (intensity + shimmer).clamp(0.0, 1.0);
        self.get_color(base_position, adjusted_intensity)
    }

    /// Get gradient colors for text characters
    pub fn get_text_gradient(&self, text_len: usize, intensity: f32, time: f32) -> Vec<(u8, u8, u8)> {
        (0..text_len)
            .map(|i| {
                let pos = i as f32 / text_len.max(1) as f32;
                // Add wave effect across text
                let wave = ((pos * std::f32::consts::PI * 2.0) + time).sin() * 0.15;
                let adjusted_intensity = (intensity + wave).clamp(0.0, 1.0);
                self.get_color(pos, adjusted_intensity)
            })
            .collect()
    }

    pub fn all() -> &'static [ColorScheme] {
        &[
            ColorScheme::Spectrum,
            ColorScheme::Rainbow,
            ColorScheme::Fire,
            ColorScheme::Ocean,
            ColorScheme::Forest,
            ColorScheme::Purple,
            ColorScheme::Monochrome,
        ]
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let current = all.iter().position(|c| c == self).unwrap_or(0);
        all[(current + 1) % all.len()]
    }
}

/// Interpolate between two colors
pub fn lerp_color(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}
