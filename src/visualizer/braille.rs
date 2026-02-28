use ratatui::prelude::*;

/// Braille dot positions within a 2x4 cell:
/// (0,0)=0x01 (1,0)=0x08
/// (0,1)=0x02 (1,1)=0x10
/// (0,2)=0x04 (1,2)=0x20
/// (0,3)=0x40 (1,3)=0x80
pub const DOT_MAP: [[u8; 4]; 2] = [
    [0x01, 0x02, 0x04, 0x40],
    [0x08, 0x10, 0x20, 0x80],
];

/// A canvas for sub-character braille rendering.
/// Each terminal character cell maps to a 2x4 grid of braille dots.
pub struct BrailleCanvas {
    pub grid: Vec<bool>,
    pub grid_w: usize,
    pub grid_h: usize,
    char_w: usize,
    char_h: usize,
}

impl BrailleCanvas {
    /// Create a new braille canvas for the given character dimensions.
    pub fn new(char_w: usize, char_h: usize) -> Self {
        let grid_w = char_w * 2;
        let grid_h = char_h * 4;
        Self {
            grid: vec![false; grid_w * grid_h],
            grid_w,
            grid_h,
            char_w,
            char_h,
        }
    }

    /// Set a single dot on the braille grid (bounds-checked).
    #[inline]
    pub fn set(&mut self, gx: usize, gy: usize) {
        if gx < self.grid_w && gy < self.grid_h {
            self.grid[gy * self.grid_w + gx] = true;
        }
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
        bresenham_line(&mut self.grid, self.grid_w, self.grid_h, x0, y0, x1, y1);
    }

    /// Encode braille grid to characters and write to the frame buffer.
    /// `color_fn(cx, cy)` returns an optional RGB color for the character cell at (cx, cy).
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        color_fn: impl Fn(usize, usize) -> Option<(u8, u8, u8)>,
    ) {
        for cy in 0..self.char_h {
            for cx in 0..self.char_w {
                let mut braille: u8 = 0;
                let mut has_dots = false;

                for (dx, col) in DOT_MAP.iter().enumerate() {
                    for (dy, &bit) in col.iter().enumerate() {
                        let gx = cx * 2 + dx;
                        let gy = cy * 4 + dy;
                        if gx < self.grid_w && gy < self.grid_h && self.grid[gy * self.grid_w + gx]
                        {
                            braille |= bit;
                            has_dots = true;
                        }
                    }
                }

                if has_dots {
                    if let Some((r, g, b)) = color_fn(cx, cy) {
                        let ch = char::from_u32(0x2800 + braille as u32).unwrap_or(' ');
                        let cell = frame
                            .buffer_mut()
                            .cell_mut((area.x + cx as u16, area.y + cy as u16));
                        if let Some(cell) = cell {
                            cell.set_char(ch);
                            cell.set_fg(Color::Rgb(r, g, b));
                        }
                    }
                }
            }
        }
    }
}

/// Draw a line on a boolean grid using Bresenham's algorithm.
pub fn bresenham_line(
    grid: &mut [bool],
    grid_w: usize,
    grid_h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) {
    let mut x0 = x0 as isize;
    let mut y0 = y0 as isize;
    let x1 = x1 as isize;
    let y1 = y1 as isize;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: isize = if x0 < x1 { 1 } else { -1 };
    let sy: isize = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 >= 0 && x0 < grid_w as isize && y0 >= 0 && y0 < grid_h as isize {
            grid[y0 as usize * grid_w + x0 as usize] = true;
        }

        if x0 == x1 && y0 == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}
