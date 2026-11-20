//! The window around the terminal area: frame, title bar buttons and drop shadow.

use image::{ImageBuffer, Rgba, imageops};

use super::canvas::{Bounds, Canvas, OPAQUE};
use crate::color::Rgb;
use crate::theme::Theme;

/// Corner radius of the window in points.
pub const CORNER_RADIUS: f32 = 10.0;

/// Height of the title bar in points.
pub const TITLE_BAR_HEIGHT: f32 = 36.0;

/// Radius of a title bar button in points.
const BUTTON_RADIUS: f32 = 6.0;

/// Gap between two title bar buttons in points.
const BUTTON_GAP: f32 = 8.0;

/// Distance from the window's left edge to the first button in points.
const BUTTON_INSET: f32 = 14.0;

/// Colors of the close, minimize and zoom buttons, left to right.
const BUTTON_COLORS: [Rgb; 3] = [
    Rgb::new(0xff, 0x5f, 0x57),
    Rgb::new(0xfe, 0xbc, 0x2e),
    Rgb::new(0x28, 0xc8, 0x40),
];

/// Width of the window outline in points.
const BORDER_WIDTH: f32 = 1.0;

/// Share of the foreground mixed into the background for the window outline.
const BORDER_CONTRAST: f32 = 0.18;

/// Share of the foreground mixed into the background for the title text.
const TITLE_CONTRAST: f32 = 0.6;

/// How far the shadow falls below the window in points.
const SHADOW_OFFSET: f32 = 12.0;

/// Blur radius of the shadow in points, as the standard deviation of the blur.
const SHADOW_SIGMA: f32 = 14.0;

/// Opacity of the shadow before blurring, out of `255`.
const SHADOW_ALPHA: u8 = 120;

/// Space around the window in points, so the shadow has room.
pub const MARGIN: f32 = 40.0;

/// Width the title bar needs in points for its buttons on the left and the same space on the right,
/// so a centered title clears them.
pub fn title_bar_reserved_width() -> f32 {
    let buttons = BUTTON_INSET
        + BUTTON_RADIUS * 2.0 * BUTTON_COLORS.len() as f32
        + BUTTON_GAP * (BUTTON_COLORS.len() - 1) as f32;
    buttons * 2.0
}

/// Color of the title text for a theme.
pub fn title_color(theme: &Theme) -> Rgb {
    theme.background.mix(theme.foreground, TITLE_CONTRAST)
}

/// Draws the blurred shadow of a window at `frame`, scaled by `scale` pixels per point.
pub fn draw_shadow(canvas: &mut Canvas, frame: Bounds, scale: f32) {
    let Ok(mut shadow) = Canvas::new(canvas.width(), canvas.height()) else {
        return;
    };
    shadow.fill_rounded_rect(
        frame.moved_down(SHADOW_OFFSET * scale),
        CORNER_RADIUS * scale,
        Rgb::BLACK,
        SHADOW_ALPHA,
    );
    let (width, height) = (shadow.width(), shadow.height());
    let Some(sharp) =
        ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, shadow.data().to_vec())
    else {
        return;
    };
    // `fast_blur` expects premultiplied pixels, which is what the canvas holds.
    let blurred = imageops::fast_blur(&sharp, SHADOW_SIGMA * scale);
    shadow.data_mut().copy_from_slice(blurred.as_raw());
    canvas.draw_over(&shadow);
}

/// Draws the window body with its outline and the three title bar buttons.
pub fn draw_frame(canvas: &mut Canvas, frame: Bounds, theme: &Theme, scale: f32) {
    let radius = CORNER_RADIUS * scale;
    let border = BORDER_WIDTH * scale;
    canvas.fill_rounded_rect(
        frame,
        radius,
        theme.background.mix(theme.foreground, BORDER_CONTRAST),
        OPAQUE,
    );
    canvas.fill_rounded_rect(
        frame.inset(border),
        radius - border,
        theme.background,
        OPAQUE,
    );

    let center_y = frame.y + TITLE_BAR_HEIGHT * scale / 2.0;
    for (index, color) in BUTTON_COLORS.iter().enumerate() {
        let center_x = frame.x
            + (BUTTON_INSET + BUTTON_RADIUS + index as f32 * (2.0 * BUTTON_RADIUS + BUTTON_GAP))
                * scale;
        canvas.fill_circle(center_x, center_y, BUTTON_RADIUS * scale, *color, OPAQUE);
    }
}
