//! The window around the terminal area: frame, title bar buttons and drop shadow.

use image::{ImageBuffer, Rgba, imageops};

use super::canvas::{Bounds, Canvas, OPAQUE};
use crate::cli::Desktop;
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

/// Diameter of the GNOME close button in points.
const CLOSE_BUTTON_DIAMETER: f32 = 24.0;

/// Distance from the window's right edge to the GNOME close button in points.
const CLOSE_BUTTON_INSET: f32 = 6.0;

/// Opacity of the GNOME close button's round backing in the foreground color, out of `255`.
const CLOSE_BUTTON_ALPHA: u8 = 26;

/// Distance from the center of the close button's cross to the end of each arm along both axes,
/// in points.
const CLOSE_CROSS_ARM: f32 = 3.75;

/// Line width of the close button's cross in points.
const CLOSE_CROSS_WIDTH: f32 = 1.5;

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

/// How the window frame looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowStyle {
    /// macOS: close, minimize and zoom buttons on the left of the title bar.
    Macos,
    /// GNOME: a header bar with one round close button on the right.
    Gnome,
}

impl WindowStyle {
    /// The style that matches `desktop`: GNOME on the Linux desktop, macOS on the macOS desktop
    /// and without a desktop.
    pub fn for_desktop(desktop: Option<Desktop>) -> Self {
        match desktop {
            Some(Desktop::Linux) => Self::Gnome,
            Some(Desktop::Macos) | None => Self::Macos,
        }
    }
}

/// Width the title bar of `style` needs in points for its buttons on one side and the same space on
/// the other, so a centered title clears them.
pub fn title_bar_reserved_width(style: WindowStyle) -> f32 {
    let buttons = match style {
        WindowStyle::Macos => {
            BUTTON_INSET
                + BUTTON_RADIUS * 2.0 * BUTTON_COLORS.len() as f32
                + BUTTON_GAP * (BUTTON_COLORS.len() - 1) as f32
        }
        WindowStyle::Gnome => CLOSE_BUTTON_INSET + CLOSE_BUTTON_DIAMETER,
    };
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

/// Draws the window body with its outline and the title bar buttons of `style`.
pub fn draw_frame(
    canvas: &mut Canvas,
    frame: Bounds,
    theme: &Theme,
    style: WindowStyle,
    scale: f32,
) {
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
    match style {
        WindowStyle::Macos => draw_macos_buttons(canvas, frame.x, center_y, scale),
        WindowStyle::Gnome => {
            draw_close_button(canvas, frame.x + frame.width, center_y, theme, scale);
        }
    }
}

/// Draws the red, yellow and green buttons from the window's left edge `left`, centered
/// vertically on `center_y`.
fn draw_macos_buttons(canvas: &mut Canvas, left: f32, center_y: f32, scale: f32) {
    for (index, color) in BUTTON_COLORS.iter().enumerate() {
        let center_x = left
            + (BUTTON_INSET + BUTTON_RADIUS + index as f32 * (2.0 * BUTTON_RADIUS + BUTTON_GAP))
                * scale;
        canvas.fill_circle(center_x, center_y, BUTTON_RADIUS * scale, *color, OPAQUE);
    }
}

/// Draws GNOME's close button, a faint round backing with a cross in the theme's foreground, at
/// the window's right edge `right`, centered vertically on `center_y`.
fn draw_close_button(canvas: &mut Canvas, right: f32, center_y: f32, theme: &Theme, scale: f32) {
    let radius = CLOSE_BUTTON_DIAMETER * scale / 2.0;
    let center_x = right - CLOSE_BUTTON_INSET * scale - radius;
    canvas.fill_circle(
        center_x,
        center_y,
        radius,
        theme.foreground,
        CLOSE_BUTTON_ALPHA,
    );
    let arm = CLOSE_CROSS_ARM * scale;
    for slope in [1.0, -1.0] {
        canvas.stroke_line(
            (center_x - arm, center_y - arm * slope),
            (center_x + arm, center_y + arm * slope),
            CLOSE_CROSS_WIDTH * scale,
            theme.foreground,
            OPAQUE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws one window of `style` at scale 1 on a canvas that leaves no margin around it.
    fn window(style: WindowStyle) -> (Canvas, Theme, Bounds) {
        let theme = Theme::bundled(crate::theme::DEFAULT_THEME).unwrap();
        let frame = Bounds {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        let mut canvas = Canvas::new(200, 100).unwrap();
        draw_frame(&mut canvas, frame, &theme, style, 1.0);
        (canvas, theme, frame)
    }

    /// `color` as an opaque premultiplied pixel.
    fn opaque(color: Rgb) -> [u8; 4] {
        [color.red, color.green, color.blue, OPAQUE]
    }

    #[test]
    fn gnome_has_a_close_button_on_the_right_and_nothing_on_the_left() {
        let (canvas, theme, frame) = window(WindowStyle::Gnome);
        let center_y = (TITLE_BAR_HEIGHT / 2.0) as u32;
        let macos_close_x = (BUTTON_INSET + BUTTON_RADIUS) as u32;
        assert_eq!(
            canvas.pixel(macos_close_x, center_y),
            opaque(theme.background)
        );
        let cross_x = (frame.width - CLOSE_BUTTON_INSET - CLOSE_BUTTON_DIAMETER / 2.0) as u32;
        assert_eq!(canvas.pixel(cross_x, center_y), opaque(theme.foreground));
    }

    #[test]
    fn macos_has_its_buttons_on_the_left() {
        let (canvas, _, _) = window(WindowStyle::Macos);
        let center_y = (TITLE_BAR_HEIGHT / 2.0) as u32;
        let close_x = (BUTTON_INSET + BUTTON_RADIUS) as u32;
        assert_eq!(canvas.pixel(close_x, center_y), opaque(BUTTON_COLORS[0]));
    }

    #[test]
    fn style_follows_the_desktop() {
        assert_eq!(
            WindowStyle::for_desktop(Some(Desktop::Linux)),
            WindowStyle::Gnome
        );
        assert_eq!(
            WindowStyle::for_desktop(Some(Desktop::Macos)),
            WindowStyle::Macos
        );
        assert_eq!(WindowStyle::for_desktop(None), WindowStyle::Macos);
    }
}
