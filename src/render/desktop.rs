//! The desktop behind the window: a drawn wallpaper and the top bar of macOS or GNOME.
//!
//! Wallpapers are drawn from color blends rather than bundled pictures, so nothing needs a
//! license. All sizes are in points and are multiplied by the render scale.

use jiff::civil::DateTime;

use super::Insets;
use super::canvas::{Bounds, Canvas, OPAQUE};
use super::text::{Typesetter, UiFont, UiStyle};
use crate::cli::Desktop;
use crate::color::Rgb;

/// Space between the window and the left and right edges of the picture.
const SIDE_MARGIN: f32 = 96.0;

/// Space between the window and the bottom edge of the picture.
const BOTTOM_MARGIN: f32 = 96.0;

/// Space between the top bar and the window.
const GAP_BELOW_BAR: f32 = 48.0;

/// Space between the bar's edges and its outermost items.
const BAR_INSET: f32 = 16.0;

/// Height of the macOS menu bar.
const MENU_BAR_HEIGHT: f32 = 24.0;

/// Text size in the macOS menu bar.
const MENU_TEXT_SIZE: f32 = 13.0;

/// Space between two menus in the macOS menu bar.
const MENU_GAP: f32 = 20.0;

/// Opacity of the macOS menu bar's dark tint over the wallpaper, out of `255`.
const MENU_BAR_ALPHA: u8 = 70;

/// App name and menus of the macOS menu bar, left to right; the first one is set in bold.
const MENUS: [&str; 6] = ["Terminal", "Shell", "Edit", "View", "Window", "Help"];

/// The Apple logo, which the macOS system font holds at this private use code point.
const APPLE_LOGO: &str = "\u{F8FF}";

/// Size of the Apple logo in the macOS menu bar.
const APPLE_LOGO_SIZE: f32 = 15.0;

/// Clock text of the macOS menu bar, the time Apple shows in its own product pictures.
const MACOS_CLOCK_TEXT: &str = "9:41";

/// Height of the GNOME top bar.
const GNOME_BAR_HEIGHT: f32 = 32.0;

/// Text size in the GNOME top bar.
const GNOME_TEXT_SIZE: f32 = 14.0;

/// Clock format of the GNOME top bar, for example `Nov 20 13:42`.
const GNOME_CLOCK_FORMAT: &str = "%b %d %H:%M";

/// Width of the active workspace's pill in the GNOME top bar.
const WORKSPACE_ACTIVE_WIDTH: f32 = 28.0;

/// Diameter of a workspace dot, and the height of the active pill.
const WORKSPACE_DOT: f32 = 8.0;

/// Space between workspace indicators.
const WORKSPACE_GAP: f32 = 6.0;

/// Number of inactive workspace dots after the active pill.
const INACTIVE_WORKSPACES: usize = 2;

/// Opacity of an inactive workspace dot, out of `255`.
const INACTIVE_WORKSPACE_ALPHA: u8 = 110;

/// One step of a wallpaper's base gradient: position from `0.0` at the top left to `1.0` at the
/// bottom right, and its color.
type GradientStop = (f32, Rgb);

/// A soft spot of light on a wallpaper.
struct Glow {
    /// Horizontal center as a share of the width, `0.0` left to `1.0` right.
    x: f32,
    /// Vertical center as a share of the height, `0.0` top to `1.0` bottom.
    y: f32,
    /// Radius as a share of the longer side.
    radius: f32,
    /// Color at the center.
    color: Rgb,
    /// Opacity at the center, out of `255`; it fades to nothing at the radius.
    alpha: u8,
}

/// A drawn wallpaper: a diagonal base gradient with glows on top.
struct Wallpaper {
    /// The base gradient from the top left to the bottom right corner.
    gradient: &'static [GradientStop],
    /// Glows laid over the gradient in order.
    glows: &'static [Glow],
}

/// Deep blue to violet with orange, cyan and pink light, in the manner of recent macOS wallpapers.
const MACOS_WALLPAPER: Wallpaper = Wallpaper {
    gradient: &[
        (0.0, Rgb::new(0x1f, 0x1c, 0x5c)),
        (0.55, Rgb::new(0x4b, 0x2a, 0x8a)),
        (1.0, Rgb::new(0xa8, 0x32, 0x7e)),
    ],
    glows: &[
        Glow {
            x: 0.85,
            y: 0.1,
            radius: 0.6,
            color: Rgb::new(0xff, 0x9a, 0x5a),
            alpha: 190,
        },
        Glow {
            x: 0.08,
            y: 0.85,
            radius: 0.55,
            color: Rgb::new(0x3f, 0xb8, 0xff),
            alpha: 160,
        },
        Glow {
            x: 0.55,
            y: 1.05,
            radius: 0.5,
            color: Rgb::new(0xff, 0x5f, 0xa2),
            alpha: 150,
        },
    ],
};

/// Ubuntu's aubergine running into its orange.
const LINUX_WALLPAPER: Wallpaper = Wallpaper {
    gradient: &[
        (0.0, Rgb::new(0x2c, 0x00, 0x1e)),
        (0.55, Rgb::new(0x5e, 0x27, 0x50)),
        (1.0, Rgb::new(0xe9, 0x54, 0x20)),
    ],
    glows: &[Glow {
        x: 0.9,
        y: 0.95,
        radius: 0.55,
        color: Rgb::new(0xff, 0x7a, 0x3d),
        alpha: 140,
    }],
};

/// Space `desktop` needs around the window at `scale` pixels per point; the top one holds the bar.
pub fn insets(desktop: Desktop, scale: f32) -> Insets {
    Insets {
        top: (bar_height(desktop) + GAP_BELOW_BAR) * scale,
        side: SIDE_MARGIN * scale,
        bottom: BOTTOM_MARGIN * scale,
    }
}

/// Narrowest picture in pixels in which the bar's items do not overlap.
pub fn min_width(
    typesetter: &mut Typesetter,
    desktop: Desktop,
    time: &DateTime,
    scale: f32,
) -> f32 {
    let clock = typesetter.measure_ui(&clock(desktop, time), clock_style(desktop, scale));
    let left = match desktop {
        Desktop::Macos => {
            let items = menu_items(typesetter.has_macos_system_font(), scale);
            items
                .iter()
                .map(|&(text, style)| typesetter.measure_ui(text, style))
                .sum::<f32>()
                + MENU_GAP * scale * (items.len() - 1) as f32
        }
        Desktop::Linux => workspaces_width() * scale,
    };
    // Both ends need room for the left items, so the centered GNOME clock stays clear of them.
    2.0 * (BAR_INSET * scale + left + MENU_GAP * scale) + clock
}

/// Items on the left of the macOS menu bar with their styles, left to right: the Apple logo when
/// `with_logo` is `true`, then the app name in bold and the menus.
///
/// Only the macOS system font has the logo, so the caller passes `true` only where it is
/// installed.
fn menu_items(with_logo: bool, scale: f32) -> Vec<(&'static str, UiStyle)> {
    let logo = with_logo.then_some((
        APPLE_LOGO,
        UiStyle {
            font: UiFont::MacosSystem,
            size_px: APPLE_LOGO_SIZE * scale,
            bold: false,
        },
    ));
    let menus = MENUS.iter().enumerate().map(|(index, menu)| {
        (
            *menu,
            UiStyle {
                font: UiFont::MacosSystem,
                size_px: MENU_TEXT_SIZE * scale,
                bold: index == 0,
            },
        )
    });
    logo.into_iter().chain(menus).collect()
}

/// Style of the clock of `desktop`: the macOS system font in regular, or Inter in bold on GNOME.
fn clock_style(desktop: Desktop, scale: f32) -> UiStyle {
    match desktop {
        Desktop::Macos => UiStyle {
            font: UiFont::MacosSystem,
            size_px: MENU_TEXT_SIZE * scale,
            bold: false,
        },
        Desktop::Linux => UiStyle {
            font: UiFont::Inter,
            size_px: GNOME_TEXT_SIZE * scale,
            bold: true,
        },
    }
}

/// Draws wallpaper and bar of `desktop` over the whole canvas, with `time` on the clock.
pub fn draw(
    canvas: &mut Canvas,
    typesetter: &mut Typesetter,
    desktop: Desktop,
    time: &DateTime,
    scale: f32,
) {
    let wallpaper = match desktop {
        Desktop::Macos => &MACOS_WALLPAPER,
        Desktop::Linux => &LINUX_WALLPAPER,
    };
    draw_wallpaper(canvas, wallpaper);
    match desktop {
        Desktop::Macos => draw_menu_bar(canvas, typesetter, time, scale),
        Desktop::Linux => draw_gnome_bar(canvas, typesetter, time, scale),
    }
}

/// Height of the bar of `desktop` in points.
fn bar_height(desktop: Desktop) -> f32 {
    match desktop {
        Desktop::Macos => MENU_BAR_HEIGHT,
        Desktop::Linux => GNOME_BAR_HEIGHT,
    }
}

/// The clock text of `desktop`: always `MACOS_CLOCK_TEXT` on macOS, and `time` on GNOME.
fn clock(desktop: Desktop, time: &DateTime) -> String {
    match desktop {
        Desktop::Macos => MACOS_CLOCK_TEXT.to_owned(),
        Desktop::Linux => time.strftime(GNOME_CLOCK_FORMAT).to_string(),
    }
}

/// Width of the GNOME workspace indicator in points.
fn workspaces_width() -> f32 {
    WORKSPACE_ACTIVE_WIDTH + INACTIVE_WORKSPACES as f32 * (WORKSPACE_GAP + WORKSPACE_DOT)
}

/// Paints `wallpaper` over the whole canvas.
fn draw_wallpaper(canvas: &mut Canvas, wallpaper: &Wallpaper) {
    let (width, height) = (canvas.width() as f32, canvas.height() as f32);
    canvas.fill_linear_gradient((0.0, 0.0), (width, height), wallpaper.gradient);
    let longer = width.max(height);
    for glow in wallpaper.glows {
        canvas.fill_glow(
            (glow.x * width, glow.y * height),
            glow.radius * longer,
            glow.color,
            glow.alpha,
        );
    }
}

/// Draws the macOS menu bar: a dark tint, the menus on the left and the clock on the right.
fn draw_menu_bar(canvas: &mut Canvas, typesetter: &mut Typesetter, time: &DateTime, scale: f32) {
    let height = MENU_BAR_HEIGHT * scale;
    canvas.fill_rect(
        Bounds {
            x: 0.0,
            y: 0.0,
            width: canvas.width() as f32,
            height,
        },
        Rgb::BLACK,
        MENU_BAR_ALPHA,
    );
    let center = height / 2.0;
    let mut x = BAR_INSET * scale;
    for (text, style) in menu_items(typesetter.has_macos_system_font(), scale) {
        typesetter.draw_ui_text(canvas, text, x, center, style, Rgb::WHITE);
        x += typesetter.measure_ui(text, style) + MENU_GAP * scale;
    }
    let style = clock_style(Desktop::Macos, scale);
    let clock = clock(Desktop::Macos, time);
    let clock_x = canvas.width() as f32 - BAR_INSET * scale - typesetter.measure_ui(&clock, style);
    typesetter.draw_ui_text(canvas, &clock, clock_x, center, style, Rgb::WHITE);
}

/// Draws the GNOME top bar: black, the workspace indicator on the left and the clock centered.
fn draw_gnome_bar(canvas: &mut Canvas, typesetter: &mut Typesetter, time: &DateTime, scale: f32) {
    let height = GNOME_BAR_HEIGHT * scale;
    canvas.fill_rect(
        Bounds {
            x: 0.0,
            y: 0.0,
            width: canvas.width() as f32,
            height,
        },
        Rgb::BLACK,
        OPAQUE,
    );
    let center = height / 2.0;
    let dot = WORKSPACE_DOT * scale;
    let mut x = BAR_INSET * scale;
    let pill = Bounds {
        x,
        y: center - dot / 2.0,
        width: WORKSPACE_ACTIVE_WIDTH * scale,
        height: dot,
    };
    canvas.fill_rounded_rect(pill, dot / 2.0, Rgb::WHITE, OPAQUE);
    x += pill.width + WORKSPACE_GAP * scale;
    for _ in 0..INACTIVE_WORKSPACES {
        canvas.fill_circle(
            x + dot / 2.0,
            center,
            dot / 2.0,
            Rgb::WHITE,
            INACTIVE_WORKSPACE_ALPHA,
        );
        x += dot + WORKSPACE_GAP * scale;
    }
    let style = clock_style(Desktop::Linux, scale);
    let clock = clock(Desktop::Linux, time);
    let clock_x = (canvas.width() as f32 - typesetter.measure_ui(&clock, style)) / 2.0;
    typesetter.draw_ui_text(canvas, &clock, clock_x, center, style, Rgb::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The local time the tests run at.
    const TIME: DateTime = jiff::civil::date(2026, 11, 20).at(13, 42, 0, 0);

    #[test]
    fn clocks_follow_each_desktop() {
        assert_eq!(clock(Desktop::Macos, &TIME), "9:41");
        assert_eq!(clock(Desktop::Linux, &TIME), "Nov 20 13:42");
    }

    #[test]
    fn apple_logo_comes_first_only_with_the_system_font() {
        assert_eq!(menu_items(true, 1.0)[0].0, APPLE_LOGO);
        assert_eq!(menu_items(true, 1.0).len(), MENUS.len() + 1);
        let without = menu_items(false, 1.0);
        assert_eq!(without[0].0, MENUS[0]);
        assert!(without[0].1.bold);
    }

    #[test]
    fn insets_hold_the_bar_and_the_gap() {
        let insets = insets(Desktop::Linux, 2.0);
        assert_eq!(insets.top, (GNOME_BAR_HEIGHT + GAP_BELOW_BAR) * 2.0);
        assert_eq!(insets.side, SIDE_MARGIN * 2.0);
    }
}
