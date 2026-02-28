/// Convert polar coordinates to cartesian grid coordinates.
/// `cx`, `cy`: center point; `angle`: radians; `radius`: distance from center.
#[inline]
pub fn polar_to_grid(cx: f32, cy: f32, angle: f32, radius: f32) -> (f32, f32) {
    (cx + angle.cos() * radius, cy + angle.sin() * radius)
}

/// Compute a circle that fits within the given area, accounting for aspect ratio.
/// `aspect_ratio`: width/height of a single unit (e.g. 2.0 for terminal chars that are ~2x tall).
/// Returns (center_x, center_y, max_radius) in the grid's coordinate system.
pub fn fit_circle(area_w: usize, area_h: usize, aspect_ratio: f32) -> (f32, f32, f32) {
    let cx = area_w as f32 / 2.0;
    let cy = area_h as f32 / 2.0;
    // The effective visual width is area_w / aspect_ratio when chars are taller than wide.
    // We pick the smaller of effective width and height to fit the circle.
    let effective_w = area_w as f32 / aspect_ratio;
    let max_radius = (effective_w.min(area_h as f32) / 2.0) * 0.95;
    (cx, cy, max_radius)
}
