//! Shapes text with `cosmic-text` and draws it onto a fixed cell grid, the way a terminal does.

use std::sync::Arc;

use anyhow::{Result, bail};
use cosmic_text::fontdb::{self, Database, Family, Source};
use cosmic_text::{
    Attrs, Buffer, FontSystem, Metrics, Shaping, Style as FontStyle, SwashCache, Weight, Wrap,
};

use super::canvas::Canvas;
use crate::ansi::Screen;
use crate::color::Rgb;

/// Family name inside the bundled font files.
pub const BUNDLED_FAMILY: &str = "JetBrains Mono NL";

/// The bundled font files: regular, bold, italic and bold italic.
const BUNDLED_FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-BoldItalic.ttf"),
];

/// Locale for shaping decisions that depend on language. Terminal output carries no language, so
/// a fixed one keeps the result the same on every machine.
const SHAPING_LOCALE: &str = "en-US";

/// Character whose advance sets the width of a cell.
const CELL_REFERENCE: &str = "0";

/// Lays out and draws text in one font family at one size.
pub struct Typesetter {
    /// Fonts: the bundled ones and every font installed on the system, for fallback.
    font_system: FontSystem,
    /// Rasterized glyphs.
    cache: SwashCache,
    /// Family used for all text.
    family: String,
    /// Font size in pixels.
    font_px: f32,
    /// Distance between two baselines in pixels.
    line_px: f32,
    /// Width of one terminal column in pixels.
    cell_width: f32,
}

impl Typesetter {
    /// Loads the fonts and measures the cell width.
    ///
    /// `family` names an installed font family, `None` selects the bundled font. Sizes are in
    /// pixels.
    ///
    /// # Errors
    ///
    /// Fails when `family` is not installed.
    pub fn new(family: Option<&str>, font_px: f32, line_px: f32) -> Result<Self> {
        let mut database = Database::new();
        database.load_system_fonts();
        for font in BUNDLED_FONTS {
            database.load_font_source(Source::Binary(Arc::new(font)));
        }
        let family = match family {
            None => BUNDLED_FAMILY.to_owned(),
            Some(requested) => installed_family(&database, requested)?,
        };
        database.set_monospace_family(family.clone());

        let mut typesetter = Self {
            font_system: FontSystem::new_with_locale_and_db(SHAPING_LOCALE.to_owned(), database),
            cache: SwashCache::new(),
            family,
            font_px,
            line_px,
            cell_width: 0.0,
        };
        typesetter.cell_width = typesetter.measure(CELL_REFERENCE);
        Ok(typesetter)
    }

    /// Width of one terminal column in pixels.
    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    /// Width of `text` in pixels when set in the regular style.
    pub fn measure(&mut self, text: &str) -> f32 {
        let buffer = self.shape_regular(text);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    /// Draws `text` in the regular style with its left edge at `x` and the top of its line at `y`.
    pub fn draw_text(&mut self, canvas: &mut Canvas, text: &str, x: f32, y: f32, color: Rgb) {
        let buffer = self.shape_regular(text);
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((x, y + run.line_y), 1.0);
                draw_glyph(
                    &mut self.font_system,
                    &mut self.cache,
                    canvas,
                    &physical,
                    color,
                );
            }
        }
    }

    /// Draws every character of `screen` at its cell, with the top left cell at `(x, y)`.
    ///
    /// `color_of(line, column)` returns the color for the character in that cell. Each glyph is
    /// placed at the start of its cell rather than where shaping would put it, so a fallback font
    /// with other proportions cannot push the columns out of line.
    pub fn draw_screen(
        &mut self,
        canvas: &mut Canvas,
        screen: &Screen,
        x: f32,
        y: f32,
        color_of: impl Fn(usize, usize) -> Rgb,
    ) {
        let mut spans: Vec<(&str, Attrs<'_>)> = Vec::new();
        let mut columns_by_line: Vec<Vec<(usize, usize)>> = Vec::with_capacity(screen.lines.len());
        let family = self.family.clone();
        let attrs = |bold: bool, italic: bool| attrs_for(&family, bold, italic);

        for (index, line) in screen.lines.iter().enumerate() {
            if index > 0 {
                spans.push(("\n", attrs(false, false)));
            }
            let mut byte = 0;
            let mut columns = Vec::with_capacity(line.len());
            for (column, cell) in line
                .iter()
                .enumerate()
                .filter(|(_, cell)| !cell.is_continuation())
            {
                columns.push((byte, column));
                byte += cell.text.len();
                spans.push((
                    cell.text.as_str(),
                    attrs(cell.style.bold, cell.style.italic),
                ));
            }
            columns_by_line.push(columns);
        }

        let buffer = self.shape(&spans);
        for run in buffer.layout_runs() {
            let columns = &columns_by_line[run.line_i];
            for glyph in run.glyphs {
                let column = match columns.binary_search_by_key(&glyph.start, |&(byte, _)| byte) {
                    Ok(found) => columns[found].1,
                    Err(next) => columns[next.saturating_sub(1)].1,
                };
                let cell_x = x + column as f32 * self.cell_width - glyph.x;
                let physical = glyph.physical((cell_x, y + run.line_y), 1.0);
                draw_glyph(
                    &mut self.font_system,
                    &mut self.cache,
                    canvas,
                    &physical,
                    color_of(run.line_i, column),
                );
            }
        }
    }

    /// Distance from the top of a line to its baseline in pixels.
    pub fn baseline(&mut self) -> f32 {
        let buffer = self.shape_regular(CELL_REFERENCE);
        buffer
            .layout_runs()
            .next()
            .map_or(self.font_px, |run| run.line_y)
    }

    /// Shapes `text` in the regular style of the configured family.
    fn shape_regular(&mut self, text: &str) -> Buffer {
        let family = self.family.clone();
        self.shape(&[(text, attrs_for(&family, false, false))])
    }

    /// Shapes spans of text into a buffer without wrapping.
    fn shape(&mut self, spans: &[(&str, Attrs<'_>)]) -> Buffer {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(self.font_px, self.line_px),
        );
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        let default_attrs = attrs_for(&self.family, false, false);
        buffer.set_rich_text(
            spans.iter().map(|(text, attrs)| (*text, attrs.clone())),
            &default_attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }
}

/// Attributes for `family` in the given weight and slant.
fn attrs_for(family: &str, bold: bool, italic: bool) -> Attrs<'_> {
    Attrs::new()
        .family(Family::Name(family))
        .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        })
}

/// Rasterizes one glyph and blends it onto the canvas.
fn draw_glyph(
    font_system: &mut FontSystem,
    cache: &mut SwashCache,
    canvas: &mut Canvas,
    physical: &cosmic_text::PhysicalGlyph,
    color: Rgb,
) {
    let base = cosmic_text::Color::rgb(color.red, color.green, color.blue);
    cache.with_pixels(font_system, physical.cache_key, base, |x, y, pixel| {
        canvas.blend_pixel(
            physical.x + x,
            physical.y + y,
            Rgb::new(pixel.r(), pixel.g(), pixel.b()),
            pixel.a(),
        );
    });
}

/// Finds the installed family whose name matches `requested`, ignoring case.
///
/// # Errors
///
/// Fails when no installed font has that family.
fn installed_family(database: &Database, requested: &str) -> Result<String> {
    let found = database
        .faces()
        .flat_map(|face: &fontdb::FaceInfo| face.families.iter().map(|(name, _)| name))
        .find(|name| name.eq_ignore_ascii_case(requested))
        .cloned();
    match found {
        Some(name) => Ok(name),
        None => bail!("the font family `{requested}` is not installed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_is_found_by_its_family_name() {
        let mut database = Database::new();
        for font in BUNDLED_FONTS {
            database.load_font_source(Source::Binary(Arc::new(font)));
        }
        assert_eq!(
            installed_family(&database, &BUNDLED_FAMILY.to_lowercase()).unwrap(),
            BUNDLED_FAMILY
        );
    }

    #[test]
    fn unknown_family_is_an_error() {
        assert!(installed_family(&Database::new(), "No Such Font").is_err());
    }

    #[test]
    fn cell_width_matches_the_bundled_advance() {
        const FONT_PX: f32 = 20.0;
        // JetBrains Mono advances 600 of 1000 units per em.
        const EXPECTED_ADVANCE: f32 = 0.6 * FONT_PX;
        let typesetter = Typesetter::new(None, FONT_PX, FONT_PX * 1.4).unwrap();
        assert!(
            (typesetter.cell_width() - EXPECTED_ADVANCE).abs() < 0.01,
            "{}",
            typesetter.cell_width()
        );
    }
}
