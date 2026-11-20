//! Turns a parsed screen into pixels: window, cell backgrounds, text and text decorations.

mod canvas;
mod text;
mod window;

use anyhow::Result;

pub use self::canvas::Canvas;
use self::canvas::{Bounds, OPAQUE};
use self::text::Typesetter;
use crate::ansi::{Screen, Style, TermColor};
use crate::color::{Backdrop, Rgb};
use crate::settings::Settings;
use crate::theme::Theme;

/// Share of the background mixed into the foreground of faint text.
const DIM_AMOUNT: f32 = 0.5;

/// Distance of the underline below the baseline, as a fraction of the font size.
const UNDERLINE_POSITION: f32 = 0.12;

/// Height of the strikethrough above the baseline, as a fraction of the font size.
const STRIKETHROUGH_POSITION: f32 = 0.28;

/// Thickness of underline and strikethrough, as a fraction of the font size.
const DECORATION_THICKNESS: f32 = 0.06;

/// Thinnest decoration line in pixels, so small fonts still show one.
const MIN_DECORATION_THICKNESS: f32 = 1.0;

/// Space between the title and the buttons in points.
const TITLE_GAP: f32 = 12.0;

/// The colors a cell is drawn with after the theme and the inverse and dim attributes apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellColors {
    /// Text color.
    foreground: Rgb,
    /// Fill behind the cell; `None` shows the window background.
    background: Option<Rgb>,
}

/// Resolves a cell style against a theme.
fn cell_colors(style: &Style, theme: &Theme) -> CellColors {
    let resolve = |color: TermColor| match color {
        TermColor::Default => None,
        TermColor::Indexed(index) => Some(theme.indexed(index)),
        TermColor::Rgb(rgb) => Some(rgb),
    };
    let mut foreground = resolve(style.foreground).unwrap_or(theme.foreground);
    let mut background = resolve(style.background);
    if style.inverse {
        (foreground, background) = (background.unwrap_or(theme.background), Some(foreground));
    }
    if style.dim {
        foreground = foreground.mix(background.unwrap_or(theme.background), DIM_AMOUNT);
    }
    CellColors {
        foreground,
        background,
    }
}

/// Renders a screen with the given settings.
///
/// # Errors
///
/// Fails when the configured font is not installed or the image would be too large.
pub fn render(screen: &Screen, settings: &Settings) -> Result<Canvas> {
    let scale = settings.scale;
    let font_px = settings.font_size * scale;
    let line_px = (font_px * settings.line_height).round();
    let mut typesetter = Typesetter::new(settings.font.as_deref(), font_px, line_px)?;
    let cell_width = typesetter.cell_width();
    let padding = settings.padding * scale;
    let theme = &settings.theme;

    let text_width = screen.width() as f32 * cell_width;
    let text_height = screen.lines.len() as f32 * line_px;
    let title_width = settings
        .title
        .as_deref()
        .map_or(0.0, |title| typesetter.measure(title));

    let (margin, title_bar) = if settings.window {
        (window::MARGIN * scale, window::TITLE_BAR_HEIGHT * scale)
    } else {
        (0.0, 0.0)
    };
    let mut window_width = text_width + 2.0 * padding;
    if settings.window {
        let title_space = if title_width > 0.0 {
            title_width + 2.0 * TITLE_GAP * scale
        } else {
            0.0
        };
        window_width = window_width.max(window::title_bar_reserved_width() * scale + title_space);
    }
    let window_width = window_width.ceil();
    let window_height = (title_bar + text_height + 2.0 * padding).ceil();
    let frame = Bounds {
        x: margin,
        y: margin,
        width: window_width,
        height: window_height,
    };

    let mut canvas = Canvas::new(
        (window_width + 2.0 * margin) as u32,
        (window_height + 2.0 * margin) as u32,
    )?;
    if let Backdrop::Solid(color) = settings.backdrop {
        canvas.fill(color);
    }
    if settings.window {
        if settings.shadow {
            window::draw_shadow(&mut canvas, frame, scale);
        }
        window::draw_frame(&mut canvas, frame, theme, scale);
        if let Some(title) = settings.title.as_deref() {
            let title_x = frame.x + (frame.width - title_width) / 2.0;
            let title_y = frame.y + (title_bar - line_px) / 2.0;
            typesetter.draw_text(
                &mut canvas,
                title,
                title_x,
                title_y,
                window::title_color(theme),
            );
        }
    } else {
        canvas.fill_rect(frame, theme.background, OPAQUE);
    }

    let grid = Grid {
        x: frame.x + padding,
        y: frame.y + title_bar + padding,
        cell_width,
        line_height: line_px,
    };
    draw_cell_backgrounds(&mut canvas, screen, theme, grid);
    typesetter.draw_screen(&mut canvas, screen, grid.x, grid.y, |line, column| {
        cell_colors(&screen.lines[line][column].style, theme).foreground
    });
    let baseline = typesetter.baseline();
    draw_decorations(&mut canvas, screen, theme, grid, baseline, font_px);
    Ok(canvas)
}

/// Where the cell grid sits on the canvas and how large a cell is, all in pixels.
#[derive(Clone, Copy, Debug)]
struct Grid {
    /// Left edge of the first column.
    x: f32,
    /// Top edge of the first line.
    y: f32,
    /// Width of a column.
    cell_width: f32,
    /// Height of a line.
    line_height: f32,
}

impl Grid {
    /// Left edge of `column`, rounded to a whole pixel so neighboring fills leave no seams.
    fn left(&self, column: usize) -> f32 {
        (self.x + column as f32 * self.cell_width).round()
    }

    /// Top edge of `row`.
    fn top(&self, row: usize) -> f32 {
        self.y + row as f32 * self.line_height
    }

    /// The area of one cell, with its side edges on whole pixels.
    fn cell(&self, row: usize, column: usize) -> Bounds {
        let left = self.left(column);
        Bounds {
            x: left,
            y: self.top(row),
            width: self.left(column + 1) - left,
            height: self.line_height,
        }
    }
}

/// Fills the background of every cell that sets one.
fn draw_cell_backgrounds(canvas: &mut Canvas, screen: &Screen, theme: &Theme, grid: Grid) {
    for (row, line) in screen.lines.iter().enumerate() {
        for (column, cell) in line.iter().enumerate() {
            if let Some(background) = cell_colors(&cell.style, theme).background {
                canvas.fill_rect(grid.cell(row, column), background, OPAQUE);
            }
        }
    }
}

/// Draws underlines and strikethroughs. `baseline` is the distance from the top of a line to its
/// baseline and `font_px` the font size, both in pixels.
fn draw_decorations(
    canvas: &mut Canvas,
    screen: &Screen,
    theme: &Theme,
    grid: Grid,
    baseline: f32,
    font_px: f32,
) {
    let thickness = (font_px * DECORATION_THICKNESS)
        .max(MIN_DECORATION_THICKNESS)
        .round();
    for (row, line) in screen.lines.iter().enumerate() {
        let baseline_y = grid.top(row) + baseline;
        for (column, cell) in line.iter().enumerate() {
            let style = &cell.style;
            if !style.underline && !style.strikethrough {
                continue;
            }
            let color = cell_colors(style, theme).foreground;
            let cell = grid.cell(row, column);
            let line_at = |y: f32| Bounds {
                y: y.round(),
                height: thickness,
                ..cell
            };
            if style.underline {
                canvas.fill_rect(
                    line_at(baseline_y + font_px * UNDERLINE_POSITION),
                    color,
                    OPAQUE,
                );
            }
            if style.strikethrough {
                canvas.fill_rect(
                    line_at(baseline_y - font_px * STRIKETHROUGH_POSITION),
                    color,
                    OPAQUE,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::RenderArgs;
    use crate::settings::ConfigFile;

    fn theme() -> Theme {
        Theme::bundled(crate::theme::DEFAULT_THEME).unwrap()
    }

    #[test]
    fn default_style_uses_theme_foreground_and_no_fill() {
        let theme = theme();
        assert_eq!(
            cell_colors(&Style::default(), &theme),
            CellColors {
                foreground: theme.foreground,
                background: None
            }
        );
    }

    #[test]
    fn inverse_swaps_including_defaults() {
        let theme = theme();
        let style = Style {
            inverse: true,
            ..Style::default()
        };
        assert_eq!(
            cell_colors(&style, &theme),
            CellColors {
                foreground: theme.background,
                background: Some(theme.foreground)
            }
        );
    }

    #[test]
    fn dim_moves_toward_the_background() {
        let theme = theme();
        let style = Style {
            dim: true,
            ..Style::default()
        };
        assert_eq!(
            cell_colors(&style, &theme).foreground,
            theme.foreground.mix(theme.background, DIM_AMOUNT)
        );
    }

    fn settings(args: RenderArgs) -> Settings {
        Settings::resolve(args, ConfigFile::default(), "test").unwrap()
    }

    #[test]
    fn plain_render_is_text_plus_padding() {
        let settings = settings(RenderArgs {
            no_window: true,
            scale: Some(1.0),
            padding: Some(10.0),
            font_size: Some(10.0),
            line_height: Some(2.0),
            ..RenderArgs::default()
        });
        let screen = Screen::parse(b"abcd\nef", None);
        let canvas = render(&screen, &settings).unwrap();
        // Four cells of 6 px (0.6 em at 10 px) plus 2 × 10 px padding; two lines of 20 px plus padding.
        assert_eq!((canvas.width(), canvas.height()), (44, 60));
        let theme = &settings.theme;
        assert_eq!(
            canvas.pixel(0, 0),
            [
                theme.background.red,
                theme.background.green,
                theme.background.blue,
                OPAQUE
            ]
        );
    }

    #[test]
    fn cell_background_fills_its_cell() {
        let settings = settings(RenderArgs {
            no_window: true,
            scale: Some(1.0),
            padding: Some(0.0),
            font_size: Some(10.0),
            line_height: Some(2.0),
            ..RenderArgs::default()
        });
        let screen = Screen::parse(b"\x1b[41m \x1b[0mx", None);
        let canvas = render(&screen, &settings).unwrap();
        let red = settings.theme.palette[1];
        assert_eq!(canvas.pixel(3, 10), [red.red, red.green, red.blue, OPAQUE]);
    }

    #[test]
    fn window_has_transparent_margin_and_room_for_buttons() {
        let settings = settings(RenderArgs {
            scale: Some(1.0),
            no_shadow: true,
            ..RenderArgs::default()
        });
        let canvas = render(&Screen::parse(b"x", None), &settings).unwrap();
        assert_eq!(canvas.pixel(0, 0)[3], 0);
        assert!(canvas.width() as f32 >= window::title_bar_reserved_width() + 2.0 * window::MARGIN);
    }
}
