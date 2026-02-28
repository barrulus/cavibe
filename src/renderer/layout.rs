//! Bar layout computation shared by all styles.

use crate::config::TextPosition;
use super::RenderParams;

/// Pre-computed bar layout used by every style renderer.
pub struct BarLayout {
    pub bars_y_start: usize,
    pub bars_height: usize,
    pub start_x: usize,
    pub slot_width: usize,
    pub displayable: usize,
    pub render_frequencies: Vec<f32>,
}

pub fn compute_bar_layout(
    width: usize,
    height: usize,
    frequencies: &[f32],
    params: &RenderParams,
) -> Option<BarLayout> {
    if frequencies.is_empty() {
        return None;
    }

    let bar_count = frequencies.len().min(width);
    let text_height = if params.text_config.show_title || params.text_config.show_artist {
        60 + params.text_config.margin_top as usize + params.text_config.margin_bottom as usize
    } else {
        0
    };

    let (bars_y_start, bars_height) = match params.text_config.position {
        TextPosition::Top => (text_height, height.saturating_sub(text_height)),
        TextPosition::Bottom => (0, height.saturating_sub(text_height)),
        TextPosition::Center | TextPosition::Coordinates { .. } => (0, height),
    };

    if bars_height == 0 {
        return None;
    }

    let slot_width = params.bar_width + params.bar_spacing;
    let max_bars = width / slot_width.max(1);
    let displayable = max_bars.min(bar_count);

    if displayable == 0 {
        return None;
    }

    let total_width = displayable * slot_width;
    let start_x = (width.saturating_sub(total_width)) / 2;

    let render_frequencies: Vec<f32> = match (params.mirror, params.reverse_mirror) {
        (true, true) => {
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            for i in 0..half {
                let freq_idx = ((half - 1 - i) * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = (i * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (true, false) => {
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            for i in 0..half {
                let freq_idx = (i * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = ((displayable - half - 1 - i) * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (false, true) => {
            (0..displayable)
                .map(|i| {
                    let freq_idx = ((displayable - 1 - i) * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
        (false, false) => {
            (0..displayable)
                .map(|i| {
                    let freq_idx = (i * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
    };

    Some(BarLayout { bars_y_start, bars_height, start_x, slot_width, displayable, render_frequencies })
}
