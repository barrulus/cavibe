//! Bar layout computation shared by all styles.

use crate::config::TextPosition;
use super::RenderParams;

/// Pre-computed bar layout used by every style renderer.
///
/// `render_frequencies` borrows from a caller-owned scratch buffer (usually
/// `Canvas::layout_scratch`) so per-frame rendering avoids allocating.
pub struct BarLayout<'a> {
    pub bars_y_start: usize,
    pub bars_height: usize,
    pub start_x: usize,
    pub slot_width: usize,
    pub displayable: usize,
    pub render_frequencies: &'a [f32],
}

pub fn compute_bar_layout<'a>(
    width: usize,
    height: usize,
    frequencies: &[f32],
    params: &RenderParams,
    scratch: &'a mut Vec<f32>,
) -> Option<BarLayout<'a>> {
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

    scratch.clear();
    scratch.reserve(displayable);
    let freq_len = frequencies.len();

    match (params.mirror, params.reverse_mirror) {
        (true, true) => {
            let half = displayable / 2;
            for i in 0..half {
                let freq_idx = ((half - 1 - i) * freq_len) / half.max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = (i * freq_len) / (displayable - half).max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
        }
        (true, false) => {
            let half = displayable / 2;
            for i in 0..half {
                let freq_idx = (i * freq_len) / half.max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = ((displayable - half - 1 - i) * freq_len) / (displayable - half).max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
        }
        (false, true) => {
            for i in 0..displayable {
                let freq_idx = ((displayable - 1 - i) * freq_len) / displayable.max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
        }
        (false, false) => {
            for i in 0..displayable {
                let freq_idx = (i * freq_len) / displayable.max(1);
                scratch.push(frequencies[freq_idx.min(freq_len - 1)]);
            }
        }
    };

    Some(BarLayout {
        bars_y_start,
        bars_height,
        start_x,
        slot_width,
        displayable,
        render_frequencies: scratch.as_slice(),
    })
}
