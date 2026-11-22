//! Tiny RGBA raster used to export plots as PNG without a plotting library.

use std::io;
use std::path::Path;

use crate::engine::plot::Sample;

/// An RGBA colour.
pub type Color = [u8; 4];

/// A fixed-size RGBA image buffer with a minimal line rasteriser.
#[derive(Debug, Clone)]
pub struct Raster {
    /// Image width in pixels.
    pub width: usize,
    /// Image height in pixels.
    pub height: usize,
    pixels: Vec<u8>,
}

impl Raster {
    /// Fill a new image with one colour.
    #[must_use]
    pub fn new(width: usize, height: usize, background: Color) -> Self {
        let mut pixels = Vec::with_capacity(width.saturating_mul(height).saturating_mul(4));
        for _ in 0..width.saturating_mul(height) {
            pixels.extend_from_slice(&background);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Set one pixel, ignoring out-of-bounds coordinates.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) * 4;
        self.pixels[offset..offset + 4].copy_from_slice(&color);
    }

    /// Draw a line with Bresenham's algorithm, clipping to the image.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            if x >= 0 && y >= 0 {
                self.set_pixel(x as usize, y as usize, color);
            }
            if x == x1 && y == y1 {
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

/// Render sampled curves with axes and a grid into a PNG-ready raster.
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
    let mut raster = Raster::new(width, height, [16, 18, 24, 255]);
    let span_x = (x_max - x_min).max(1e-9);
    let span_y = (y_max - y_min).max(1e-9);
    let to_pixel = |x: f64, y: f64| -> (i32, i32) {
        (
            ((x - x_min) / span_x * (width.saturating_sub(1)) as f64).round() as i32,
            ((y_max - y) / span_y * (height.saturating_sub(1)) as f64).round() as i32,
        )
    };
    for tick in 0..=8 {
        let (px, _) = to_pixel(x_min + span_x * tick as f64 / 8.0, y_min);
        raster.draw_line(px, 0, px, height as i32 - 1, [40, 44, 54, 255]);
    }
    for tick in 0..=4 {
        let (_, py) = to_pixel(x_min, y_min + span_y * tick as f64 / 4.0);
        raster.draw_line(0, py, width as i32 - 1, py, [40, 44, 54, 255]);
    }
    if (x_min..=x_max).contains(&0.0) {
        let (px, _) = to_pixel(0.0, y_min);
        raster.draw_line(px, 0, px, height as i32 - 1, [72, 80, 96, 255]);
    }
    if (y_min..=y_max).contains(&0.0) {
        let (_, py) = to_pixel(x_min, 0.0);
        raster.draw_line(0, py, width as i32 - 1, py, [72, 80, 96, 255]);
    }
    let palette: [Color; 6] = [
        [106, 168, 245, 255],
        [84, 209, 132, 255],
        [238, 180, 86, 255],
        [232, 87, 63, 255],
        [178, 132, 245, 255],
        [96, 214, 200, 255],
    ];
    for (index, points) in curves.iter().enumerate() {
        let color = palette[index % palette.len()];
        for pair in points.windows(2) {
            if pair[0].y.is_finite() && pair[1].y.is_finite() {
                let (x0, y0) = to_pixel(pair[0].x, pair[0].y);
                let (x1, y1) = to_pixel(pair[1].x, pair[1].y);
                raster.draw_line(x0, y0, x1, y1, color);
            }
        }
    }
    raster
}

/// Render sampled curves with axes and a grid into a PNG-ready raster.
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
    let mut raster = Raster::new(width.max(2), height.max(2), [16, 18, 24, 255]);
    let width = raster.width;
    let height = raster.height;
    let span_x = (x_max - x_min).max(1e-9);
    let span_y = (y_max - y_min).max(1e-9);
    let to_pixel = |x: f64, y: f64| -> (i32, i32) {
        (
            ((x - x_min) / span_x * (width - 1) as f64).round() as i32,
            ((y_max - y) / span_y * (height - 1) as f64).round() as i32,
        )
    };
    for tick in 0..=8 {
        let (px, _) = to_pixel(x_min + span_x * tick as f64 / 8.0, y_min);
        raster.draw_line(px, 0, px, height as i32 - 1, [40, 44, 54, 255]);
    }
    for tick in 0..=4 {
        let (_, py) = to_pixel(x_min, y_min + span_y * tick as f64 / 4.0);
        raster.draw_line(0, py, width as i32 - 1, py, [40, 44, 54, 255]);
    }
    if (x_min..=x_max).contains(&0.0) {
        let (px, _) = to_pixel(0.0, y_min);
        raster.draw_line(px, 0, px, height as i32 - 1, [72, 80, 96, 255]);
    }
    if (y_min..=y_max).contains(&0.0) {
        let (_, py) = to_pixel(x_min, 0.0);
        raster.draw_line(0, py, width as i32 - 1, py, [72, 80, 96, 255]);
    }
    let palette: [Color; 6] = [
        [106, 168, 245, 255],
        [84, 209, 132, 255],
        [238, 180, 86, 255],
        [232, 87, 63, 255],
        [178, 132, 245, 255],
        [96, 214, 200, 255],
    ];
    for (index, points) in curves.iter().enumerate() {
        let color = palette[index % palette.len()];
        for pair in points.windows(2) {
            if pair[0].y.is_finite() && pair[1].y.is_finite() {
                let (x0, y0) = to_pixel(pair[0].x, pair[0].y);
                let (x1, y1) = to_pixel(pair[1].x, pair[1].y);
                raster.draw_line(x0, y0, x1, y1, color);
            }
        }
    }
    raster
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
}
