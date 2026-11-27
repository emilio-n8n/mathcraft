//! Tiny RGBA raster used to export plots as PNG without a plotting library.

use std::io;
use std::path::Path;

use crate::engine::expr::format_value;
use crate::engine::plot::{Sample, axis_ticks, normalized};

/// An RGBA colour.
pub type Color = [u8; 4];

const PALETTE: [Color; 6] = [
    [106, 168, 245, 255],
    [84, 209, 132, 255],
    [238, 180, 86, 255],
    [232, 87, 63, 255],
    [178, 132, 245, 255],
    [96, 214, 200, 255],
];

/// A fixed-size RGBA image buffer with a minimal line rasteriser.
#[derive(Debug, Clone)]
pub struct Raster {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Raster {
    /// Fill a new image with one colour.
    ///
    /// If the dimensions cannot be represented or the allocation fails, an
    /// empty image is returned; PNG encoding then reports an error.
    #[must_use]
    pub fn new(width: usize, height: usize, background: Color) -> Self {
        if width > u32::MAX as usize || height > u32::MAX as usize {
            return Self {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            };
        }
        let Some(byte_len) = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return Self {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            };
        };
        let mut pixels = Vec::new();
        if pixels.try_reserve_exact(byte_len).is_err() {
            return Self {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            };
        }
        pixels.resize(byte_len, 0);
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&background);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Image width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Image height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Read one pixel, returning `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = y
            .checked_mul(self.width)
            .and_then(|index| index.checked_add(x))
            .and_then(|index| index.checked_mul(4))?;
        self.pixels
            .get(offset..offset + 4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// Set one pixel, ignoring out-of-bounds coordinates.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let Some(offset) = y
            .checked_mul(self.width)
            .and_then(|index| index.checked_add(x))
            .and_then(|index| index.checked_mul(4))
        else {
            return;
        };
        if let Some(pixel) = self.pixels.get_mut(offset..offset + 4) {
            pixel.copy_from_slice(&color);
        }
    }

    /// Draw a line with Bresenham's algorithm, clipping it to the image.
    ///
    /// Clipping happens before rasterisation, so even extreme `i32` endpoints
    /// cannot overflow the algorithm or turn into an unbounded drawing loop.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        self.draw_line_f64(
            f64::from(x0),
            f64::from(y0),
            f64::from(x1),
            f64::from(y1),
            color,
        );
    }

    fn draw_line_f64(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, color: Color) {
        if self.width == 0
            || self.height == 0
            || [x0, y0, x1, y1].iter().any(|value| value.is_nan())
        {
            return;
        }
        // A finite, saturating stand-in for infinity preserves which side of
        // the viewport an off-screen coordinate lies on.
        let limit = (self.width.max(self.height) as f64).mul_add(4.0, 1.0);
        let x0 = x0.clamp(-limit, limit);
        let y0 = y0.clamp(-limit, limit);
        let x1 = x1.clamp(-limit, limit);
        let y1 = y1.clamp(-limit, limit);
        let xmin = 0.0;
        let xmax = (self.width - 1) as f64;
        let ymin = 0.0;
        let ymax = (self.height - 1) as f64;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let mut start: f64 = 0.0;
        let mut end: f64 = 1.0;
        for (p, q) in [
            (-dx, x0 - xmin),
            (dx, xmax - x0),
            (-dy, y0 - ymin),
            (dy, ymax - y0),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return;
                }
                continue;
            }
            let ratio = q / p;
            if p < 0.0 {
                start = start.max(ratio);
            } else {
                end = end.min(ratio);
            }
            if start > end {
                return;
            }
        }
        let clipped = |x: f64, y: f64, fraction: f64| {
            (
                (x + fraction * dx).round().clamp(0.0, xmax) as i32,
                (y + fraction * dy).round().clamp(0.0, ymax) as i32,
            )
        };
        let (x0, y0) = clipped(x0, y0, start);
        let (x1, y1) = clipped(x1, y1, end);
        self.bresenham(x0, y0, x1, y1, color);
    }

    fn bresenham(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let mut x = i64::from(x0);
        let mut y = i64::from(y0);
        let dx = (i64::from(x1) - x).abs();
        let dy = -(i64::from(y1) - y).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            if x >= 0 && y >= 0 {
                self.set_pixel(x as usize, y as usize, color);
            }
            if x == i64::from(x1) && y == i64::from(y1) {
                break;
            }
            let doubled = 2 * error;
            if doubled >= dy {
                error += dy;
                x += sx;
            }
            if doubled <= dx {
                error += dx;
                y += sy;
            }
        }
    }

    /// Draw uppercase ASCII text with the built-in 5x7 bitmap font.
    ///
    /// Unsupported characters are shown as `?`. Coordinates and text are
    /// clipped to the image.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, color: Color, scale: usize) {
        let scale = scale.clamp(1, 16);
        let mut cursor = i64::from(x);
        for character in text.chars() {
            let glyph = glyph(character.to_ascii_uppercase() as u8);
            for (row, bits) in glyph.iter().copied().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) == 0 {
                        continue;
                    }
                    for offset_y in 0..scale {
                        for offset_x in 0..scale {
                            let px = cursor + (column * scale + offset_x) as i64;
                            let py = i64::from(y) + (row * scale + offset_y) as i64;
                            if px >= 0 && py >= 0 {
                                self.set_pixel(px as usize, py as usize, color);
                            }
                        }
                    }
                }
            }
            cursor += 6 * scale as i64;
        }
    }

    /// Encode the image as PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the encoder cannot write the image.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, self.width as u32, self.height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&self.pixels)?;
        }
        Ok(output)
    }

    /// Write the image to a PNG file.
    ///
    /// # Errors
    ///
    /// Returns an I/O or encoding error when the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), io::Error> {
        let bytes = self
            .to_png()
            .map_err(|error| io::Error::other(error.to_string()))?;
        std::fs::write(path, bytes)
    }
}

/// Render sampled curves with axes, labelled graduations and a grid.
#[must_use]
pub fn plot_raster(
    curves: &[&[Sample]],
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    width: usize,
    height: usize,
) -> Raster {
    render_plot(curves, x_min, x_max, y_min, y_max, width, height)
}

/// Render sampled curves with axes, labelled graduations and a grid into a
/// PNG-ready raster.
#[must_use]
pub fn render_plot(
    curves: &[&[Sample]],
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    width: usize,
    height: usize,
) -> Raster {
    render(curves, None, x_min, x_max, y_min, y_max, width, height)
}

/// Render named curves with a colour legend, axes and labelled graduations.
///
/// # Panics
///
/// This function does not panic. Invalid ranges fall back to `[-1, 1]`, and
/// non-finite samples are skipped.
#[must_use]
pub fn render_plot_named(
    curves: &[(String, &[Sample])],
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    width: usize,
    height: usize,
) -> Raster {
    let samples = curves.iter().map(|(_, points)| *points).collect::<Vec<_>>();
    render(
        &samples,
        Some(curves),
        x_min,
        x_max,
        y_min,
        y_max,
        width,
        height,
    )
}

#[allow(clippy::too_many_arguments)]
fn render(
    curves: &[&[Sample]],
    named: Option<&[(String, &[Sample])]>,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    width: usize,
    height: usize,
) -> Raster {
    let mut raster = Raster::new(width.max(2), height.max(2), [16, 18, 24, 255]);
    let width = raster.width;
    let height = raster.height;
    let (x_min, x_max) = finite_range(x_min, x_max);
    let (y_min, y_max) = finite_range(y_min, y_max);
    let roomy = width >= 240 && height >= 120;
    let left = if roomy { 64.0 } else { 0.0 };
    let right = if roomy {
        (width - 12) as f64
    } else {
        (width - 1) as f64
    };
    let legend_rows = named.map_or(0, |curves| curves.len().min(4));
    let top = if roomy && legend_rows > 0 {
        8.0 + legend_rows as f64 * 12.0
    } else {
        8.0
    };
    let bottom = if roomy {
        (height - 34) as f64
    } else {
        (height - 1) as f64
    };
    let to_pixel = |x: f64, y: f64| {
        (
            left + normalized(x, x_min, x_max) * (right - left),
            bottom - normalized(y, y_min, y_max) * (bottom - top),
        )
    };

    for tick in axis_ticks(x_min, x_max, 9) {
        let (x, _) = to_pixel(tick, y_min);
        raster.draw_line_f64(x, top, x, bottom, [40, 44, 54, 255]);
        if roomy {
            let label = format_value(tick);
            let label_x = x as i32 - text_width(&label, 1) as i32 / 2;
            raster.draw_text(label_x, bottom as i32 + 7, &label, [150, 158, 174, 255], 1);
        }
    }
    for tick in axis_ticks(y_min, y_max, 5) {
        let (_, y) = to_pixel(x_min, tick);
        raster.draw_line_f64(left, y, right, y, [40, 44, 54, 255]);
        if roomy {
            let label = format_value(tick);
            let label_x = left as i32 - text_width(&label, 1) as i32 - 5;
            raster.draw_text(label_x, y as i32 - 3, &label, [150, 158, 174, 255], 1);
        }
    }
    if (x_min..=x_max).contains(&0.0) {
        let (x, _) = to_pixel(0.0, y_min);
        raster.draw_line_f64(x, top, x, bottom, [72, 80, 96, 255]);
    }
    if (y_min..=y_max).contains(&0.0) {
        let (_, y) = to_pixel(x_min, 0.0);
        raster.draw_line_f64(left, y, right, y, [72, 80, 96, 255]);
    }
    for (index, points) in curves.iter().enumerate() {
        let color = PALETTE[index % PALETTE.len()];
        for pair in points.windows(2) {
            if [pair[0].x, pair[0].y, pair[1].x, pair[1].y]
                .iter()
                .all(|value| value.is_finite())
            {
                let (x0, y0) = to_pixel(pair[0].x, pair[0].y);
                let (x1, y1) = to_pixel(pair[1].x, pair[1].y);
                raster.draw_line_f64(x0, y0, x1, y1, color);
            }
        }
    }
    if let Some(named) = named {
        for (index, (name, _)) in named.iter().take(4).enumerate() {
            let color = PALETTE[index % PALETTE.len()];
            let y = 8 + index as i32 * 12;
            raster.draw_line(left as i32, y + 3, left as i32 + 12, y + 3, color);
            raster.draw_text(left as i32 + 17, y, name, color, 1);
        }
        if named.len() > 4 {
            let label = format!("+{} MORE", named.len() - 4);
            let y = 8 + 4 * 12;
            raster.draw_text(left as i32, y, &label, [150, 158, 174, 255], 1);
        }
    }
    raster
}

fn finite_range(minimum: f64, maximum: f64) -> (f64, f64) {
    if minimum.is_finite() && maximum.is_finite() && minimum < maximum {
        (minimum, maximum)
    } else {
        (-1.0, 1.0)
    }
}

fn text_width(text: &str, scale: usize) -> usize {
    text.chars().count().saturating_mul(6).saturating_mul(scale)
}

fn glyph(character: u8) -> [u8; 7] {
    match character {
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        b'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        b'6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        b'-' => [0, 0, 0, 0b11111, 0, 0, 0],
        b'+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        b'.' => [0, 0, 0, 0, 0, 0b00110, 0b00110],
        b',' => [0, 0, 0, 0, 0, 0b00110, 0b00100],
        b':' => [0, 0b00110, 0b00110, 0, 0b00110, 0b00110, 0],
        b'/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        b'\\' => [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
        b'(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        b')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        b'[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        b']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        b'=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 0b11111],
        b'*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        b'^' => [0b00100, 0b01010, 0b10001, 0, 0, 0, 0],
        b'%' => [0b11001, 0b11010, 0b00100, 0b01000, 0b10110, 0b00110, 0],
        b'?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100],
        b'\'' => [0b00100, 0b00100, 0b01000, 0, 0, 0, 0],
        b'"' => [0b01010, 0b01010, 0b10100, 0, 0, 0, 0],
        b' ' | 0..=31 => [0; 7],
        _ => glyph(b'?'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_pixels_and_lines_inside_the_image() {
        let mut raster = Raster::new(8, 4, [0, 0, 0, 255]);
        raster.set_pixel(1, 1, [255, 0, 0, 255]);
        raster.set_pixel(99, 99, [0, 255, 0, 255]);
        raster.draw_line(0, 0, 3, 3, [0, 0, 255, 255]);
        let png = raster.to_png().unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[16..24], &[0, 0, 0, 8, 0, 0, 0, 4]);
    }

    #[test]
    fn renders_curves_into_a_png_raster() {
        let points = [Sample { x: 0.0, y: 0.0 }, Sample { x: 1.0, y: 1.0 }];
        let raster = plot_raster(&[&points], 0.0, 1.0, 0.0, 1.0, 64, 32);
        let png = raster.to_png().unwrap();
        assert_eq!(&png[16..24], &[0, 0, 0, 64, 0, 0, 0, 32]);
    }

    #[test]
    fn extreme_and_offscreen_lines_are_clipped_without_overflow() {
        let mut raster = Raster::new(16, 16, [0, 0, 0, 255]);
        raster.draw_line(i32::MIN, 8, i32::MAX, 8, [255, 255, 255, 255]);
        assert_eq!(raster.pixel(0, 8), Some([255, 255, 255, 255]));
        assert_eq!(raster.pixel(15, 8), Some([255, 255, 255, 255]));

        let mut raster = Raster::new(16, 16, [0, 0, 0, 255]);
        raster.draw_line(-100, -100, 200, 200, [255, 0, 0, 255]);
        assert_eq!(raster.pixel(8, 8), Some([255, 0, 0, 255]));
    }

    #[test]
    fn bitmap_text_and_named_legends_are_drawn() {
        let mut raster = Raster::new(40, 12, [0, 0, 0, 255]);
        raster.draw_text(1, 1, "A1", [255, 255, 255, 255], 1);
        assert_eq!(raster.pixel(2, 1), Some([255, 255, 255, 255]));

        let points = [Sample { x: 0.0, y: 0.0 }, Sample { x: 1.0, y: 1.0 }];
        let named = [("A1".to_owned(), &points[..])];
        let raster = render_plot_named(&named, 0.0, 1.0, 0.0, 1.0, 960, 540);
        assert_eq!(raster.pixel(64, 11), Some(PALETTE[0]));
        assert!(raster.to_png().unwrap().len() > 1000);
    }

    #[test]
    fn tick_labels_use_data_values_instead_of_pixel_coordinates() {
        let points = [Sample { x: 0.0, y: 0.0 }, Sample { x: 1.0, y: 1.0 }];
        let unit = render_plot(&[&points], 0.0, 1.0, 0.0, 1.0, 960, 540);
        let hundreds = render_plot(&[&points], 0.0, 100.0, 0.0, 100.0, 960, 540);
        let row_bytes = 960 * 4;
        let label_start = 510 * row_bytes;
        assert_ne!(unit.pixels[label_start..], hundreds.pixels[label_start..]);
    }

    #[test]
    fn invalid_ranges_fall_back_and_non_finite_samples_are_skipped() {
        let points = [
            Sample {
                x: f64::NAN,
                y: 0.0,
            },
            Sample {
                x: 0.0,
                y: f64::INFINITY,
            },
        ];
        let raster = render_plot(&[&points], 1.0, 0.0, f64::NAN, f64::NAN, 32, 32);
        assert_eq!(raster.width(), 32);
        assert_eq!(raster.height(), 32);
        assert!(raster.to_png().is_ok());
    }
}
