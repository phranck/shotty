//! A pixel surface with the few drawing operations the renderer needs.

use anyhow::{Result, anyhow};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Transform};

use crate::color::{CHANNEL_MAX, RGBA_BYTES, Rgb, blend_channel};

/// Fully opaque alpha.
pub const OPAQUE: u8 = CHANNEL_MAX;

/// Bézier handle length for a quarter circle, as a fraction of the radius.
const QUARTER_CIRCLE_KAPPA: f32 = 0.552_284_8;

/// An axis aligned rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width; zero or less makes the rectangle empty.
    pub width: f32,
    /// Height; zero or less makes the rectangle empty.
    pub height: f32,
}

impl Bounds {
    /// The same rectangle shrunk by `amount` on every side.
    pub fn inset(self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: self.width - 2.0 * amount,
            height: self.height - 2.0 * amount,
        }
    }

    /// The same rectangle moved down by `distance`.
    pub fn moved_down(self, distance: f32) -> Self {
        Self {
            y: self.y + distance,
            ..self
        }
    }
}

/// An RGBA surface. Pixels are stored premultiplied, as `tiny_skia` keeps them.
pub struct Canvas {
    /// The pixels.
    pixmap: Pixmap,
}

impl Canvas {
    /// Creates a fully transparent canvas.
    ///
    /// # Errors
    ///
    /// Fails when a side is zero or the image would exceed what `tiny_skia` can address.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let pixmap = Pixmap::new(width, height)
            .ok_or_else(|| anyhow!("an image of {width}×{height} pixels is too large to create"))?;
        Ok(Self { pixmap })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Fills the whole canvas with an opaque color.
    pub fn fill(&mut self, color: Rgb) {
        self.pixmap.fill(tiny_skia::Color::from_rgba8(
            color.red,
            color.green,
            color.blue,
            OPAQUE,
        ));
    }

    /// Fills an axis aligned rectangle. Nothing happens for an empty rectangle.
    pub fn fill_rect(&mut self, bounds: Bounds, color: Rgb, alpha: u8) {
        if let Some(rect) = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height) {
            self.pixmap
                .fill_rect(rect, &paint(color, alpha), Transform::identity(), None);
        }
    }

    /// Fills a rectangle with rounded corners. Nothing happens for an empty rectangle.
    pub fn fill_rounded_rect(&mut self, bounds: Bounds, radius: f32, color: Rgb, alpha: u8) {
        if let Some(path) = rounded_rect(bounds, radius) {
            self.pixmap.fill_path(
                &path,
                &paint(color, alpha),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Fills a circle around `(center_x, center_y)`.
    pub fn fill_circle(
        &mut self,
        center_x: f32,
        center_y: f32,
        radius: f32,
        color: Rgb,
        alpha: u8,
    ) {
        if let Some(path) = PathBuilder::from_circle(center_x, center_y, radius) {
            self.pixmap.fill_path(
                &path,
                &paint(color, alpha),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Draws one pixel of `color` with straight `alpha` over what is there. Pixels outside the
    /// canvas are ignored, so glyphs may overhang the edge.
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: Rgb, alpha: u8) {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if alpha == 0 || x >= self.width() || y >= self.height() {
            return;
        }
        let offset = (y as usize * self.width() as usize + x as usize) * RGBA_BYTES;
        let pixel = &mut self.pixmap.data_mut()[offset..offset + RGBA_BYTES];
        for (channel, value) in pixel
            .iter_mut()
            .zip([color.red, color.green, color.blue, OPAQUE])
        {
            *channel = blend_channel(value, *channel, alpha);
        }
    }

    /// Draws another canvas of the same size over this one.
    pub fn draw_over(&mut self, other: &Canvas) {
        self.pixmap.draw_pixmap(
            0,
            0,
            other.pixmap.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    /// Mutable premultiplied RGBA bytes, row by row.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.pixmap.data_mut()
    }

    /// Premultiplied RGBA bytes, row by row.
    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// The premultiplied RGBA value of one pixel, for tests that check what was drawn.
    #[cfg(test)]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; RGBA_BYTES] {
        let offset = (y * self.width() + x) as usize * RGBA_BYTES;
        self.data()[offset..offset + RGBA_BYTES]
            .try_into()
            .expect("slice has the length of a pixel")
    }

    /// Consumes the canvas and returns straight alpha RGBA bytes, row by row.
    pub fn into_straight_rgba(self) -> Vec<u8> {
        self.pixmap.take_demultiplied()
    }
}

/// A solid paint with anti aliasing.
fn paint(color: Rgb, alpha: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.red, color.green, color.blue, alpha);
    paint.anti_alias = true;
    paint
}

/// Builds the outline of a rectangle with circular corners.
///
/// `radius` is limited to half the shorter side. Returns `None` for an empty rectangle.
fn rounded_rect(bounds: Bounds, radius: f32) -> Option<Path> {
    let Bounds {
        x,
        y,
        width,
        height,
    } = bounds;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    let handle = radius * QUARTER_CIRCLE_KAPPA;
    let (right, bottom) = (x + width, y + height);
    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(
        right - radius + handle,
        y,
        right,
        y + radius - handle,
        right,
        y + radius,
    );
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - radius + handle,
        right - radius + handle,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(x + radius, bottom);
    builder.cubic_to(
        x + radius - handle,
        bottom,
        x,
        bottom - radius + handle,
        x,
        bottom - radius,
    );
    builder.line_to(x, y + radius);
    builder.cubic_to(
        x,
        y + radius - handle,
        x + radius - handle,
        y,
        x + radius,
        y,
    );
    builder.close();
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_onto_transparent_gives_premultiplied_color() {
        let mut canvas = Canvas::new(2, 2).unwrap();
        canvas.blend_pixel(1, 1, Rgb::new(200, 100, 0), 128);
        assert_eq!(canvas.pixel(1, 1), [100, 50, 0, 128]);
    }

    #[test]
    fn opaque_blend_replaces_and_outside_pixels_are_ignored() {
        let mut canvas = Canvas::new(2, 2).unwrap();
        canvas.fill(Rgb::new(10, 20, 30));
        canvas.blend_pixel(0, 0, Rgb::WHITE, OPAQUE);
        canvas.blend_pixel(-1, 5, Rgb::WHITE, OPAQUE);
        assert_eq!(canvas.pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(canvas.pixel(1, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn rounded_corners_stay_transparent() {
        let mut canvas = Canvas::new(40, 40).unwrap();
        let bounds = Bounds {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        canvas.fill_rounded_rect(bounds, 12.0, Rgb::WHITE, OPAQUE);
        assert_eq!(canvas.pixel(0, 0)[3], 0);
        assert_eq!(canvas.pixel(20, 20)[3], OPAQUE);
    }

    #[test]
    fn zero_sized_canvas_is_an_error() {
        assert!(Canvas::new(0, 10).is_err());
    }
}
