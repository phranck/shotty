//! Shapes text with `cosmic-text` and draws it onto a fixed cell grid, the way a terminal does.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, bail};
use cosmic_text::fontdb::{self, Database, Family, Source};
use cosmic_text::{
    Attrs, Buffer, FontSystem, Metrics, Shaping, Style as FontStyle, SwashCache, Weight, Wrap,
};

use super::canvas::Canvas;
use crate::ansi::Screen;
use crate::color::Rgb;
use crate::settings::FontChoice;

/// Family name inside the bundled font files.
pub const BUNDLED_FAMILY: &str = "JetBrains Mono NL";

/// The bundled font files: JetBrains Mono NL regular, bold, italic and bold italic for the
/// terminal text, and Inter regular and bold for the desktop bars.
const BUNDLED_FONTS: [&[u8]; 6] = [
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMonoNL-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
];

/// Family name inside the bundled Inter files.
pub const UI_FAMILY: &str = "Inter";

/// The macOS system font, San Francisco, as every Mac installs it. It is read from the running
/// system and never bundled, because its license does not allow passing it on.
const MACOS_SYSTEM_FONT_PATH: &str = "/System/Library/Fonts/SFNS.ttf";

/// Line height of interface text as a multiple of its size.
const UI_LINE_HEIGHT: f32 = 1.2;

/// Typeface of interface text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiFont {
    /// The macOS system font at `MACOS_SYSTEM_FONT_PATH`; Inter where it is not installed.
    MacosSystem,
    /// The bundled Inter.
    Inter,
}

/// Typeface, size and weight of interface text, such as the clock in a desktop bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiStyle {
    /// Typeface.
    pub font: UiFont,
    /// Font size in pixels.
    pub size_px: f32,
    /// `true` sets the text in the bold weight, `false` in the regular one.
    pub bold: bool,
}

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
    /// Family of the macOS system font, `None` where it is not installed.
    macos_system_family: Option<String>,
    /// Font size in pixels.
    font_px: f32,
    /// Distance between two baselines in pixels.
    line_px: f32,
    /// Width of one terminal column in pixels.
    cell_width: f32,
}

impl Typesetter {
    /// Loads the fonts, picks the family `font` asks for and measures the cell width. Sizes are in
    /// pixels.
    ///
    /// # Errors
    ///
    /// Fails when `font` names a family with `--font` or the config file that is not installed.
    pub fn new(font: &FontChoice, font_px: f32, line_px: f32) -> Result<Self> {
        let mut database = Database::new();
        database.load_system_fonts();
        for font in BUNDLED_FONTS {
            database.load_font_source(Source::Binary(Arc::new(font)));
        }
        let family = choose_family(&database, font)?;
        database.set_monospace_family(family.clone());
        let macos_system_family = family_of_file(&database, Path::new(MACOS_SYSTEM_FONT_PATH));

        let mut typesetter = Self {
            font_system: FontSystem::new_with_locale_and_db(SHAPING_LOCALE.to_owned(), database),
            cache: SwashCache::new(),
            family,
            macos_system_family,
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
        self.draw_buffer(canvas, &buffer, x, y, color);
    }

    /// Whether the macOS system font is installed, so `UiFont::MacosSystem` sets text in it
    /// rather than in Inter.
    pub fn has_macos_system_font(&self) -> bool {
        self.macos_system_family.is_some()
    }

    /// Width of `text` in pixels when set in the interface font in `style`.
    pub fn measure_ui(&mut self, text: &str, style: UiStyle) -> f32 {
        let buffer = self.shape_ui(text, style);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    /// Draws `text` in the interface font in `style`, with its left edge at `x` and centered
    /// vertically on `center_y`.
    pub fn draw_ui_text(
        &mut self,
        canvas: &mut Canvas,
        text: &str,
        x: f32,
        center_y: f32,
        style: UiStyle,
        color: Rgb,
    ) {
        let buffer = self.shape_ui(text, style);
        let top = center_y - style.size_px * UI_LINE_HEIGHT / 2.0;
        self.draw_buffer(canvas, &buffer, x, top, color);
    }

    /// Draws every glyph of a shaped buffer where shaping placed it, with the buffer's top left
    /// at `(x, y)`.
    fn draw_buffer(&mut self, canvas: &mut Canvas, buffer: &Buffer, x: f32, y: f32, color: Rgb) {
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

    /// Shapes spans of text at the terminal font size into a buffer without wrapping.
    fn shape(&mut self, spans: &[(&str, Attrs<'_>)]) -> Buffer {
        self.shape_with(spans, Metrics::new(self.font_px, self.line_px))
    }

    /// Shapes `text` in the interface font in `style`.
    fn shape_ui(&mut self, text: &str, style: UiStyle) -> Buffer {
        let family = match (style.font, &self.macos_system_family) {
            (UiFont::MacosSystem, Some(family)) => family.clone(),
            (UiFont::MacosSystem, None) | (UiFont::Inter, _) => UI_FAMILY.to_owned(),
        };
        self.shape_with(
            &[(text, attrs_for(&family, style.bold, false))],
            Metrics::new(style.size_px, style.size_px * UI_LINE_HEIGHT),
        )
    }

    /// Shapes spans of text with the given metrics into a buffer without wrapping.
    fn shape_with(&mut self, spans: &[(&str, Attrs<'_>)], metrics: Metrics) -> Buffer {
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
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

/// Decides the family for `font` among the fonts in `database`.
///
/// A terminal font that is not installed, as over SSH, falls back to the bundled font with a
/// note on standard error, because the user did not ask for it by name.
///
/// # Errors
///
/// Fails when `font` is a `FontChoice::Family` that is not installed.
fn choose_family(database: &Database, font: &FontChoice) -> Result<String> {
    match font {
        FontChoice::Bundled => Ok(BUNDLED_FAMILY.to_owned()),
        FontChoice::Family(requested) => installed_family(database, requested),
        FontChoice::Terminal(postscript_name) => Ok(family_of_face(database, postscript_name).unwrap_or_else(|| {
            eprintln!("shotty: the terminal's font {postscript_name} is not installed here, so {BUNDLED_FAMILY} is used");
            BUNDLED_FAMILY.to_owned()
        })),
    }
}

/// Returns the family of the face whose PostScript name is `postscript_name`, or `None` when no
/// installed face has it.
fn family_of_face(database: &Database, postscript_name: &str) -> Option<String> {
    database
        .faces()
        .find(|face| face.post_script_name == postscript_name)
        .and_then(|face| face.families.first())
        .map(|(family, _)| family.clone())
}

/// Returns the family of the first face loaded from the font file at `path`, or `None` when no
/// face in `database` came from it.
fn family_of_file(database: &Database, path: &Path) -> Option<String> {
    database
        .faces()
        .find(|face| match &face.source {
            Source::File(source) | Source::SharedFile(source, _) => source == path,
            Source::Binary(_) => false,
        })
        .and_then(|face| face.families.first())
        .map(|(family, _)| family.clone())
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

    /// PostScript name of the bundled regular face.
    const BUNDLED_REGULAR_FACE: &str = "JetBrainsMonoNL-Regular";

    /// A database holding only the bundled fonts, so the result does not depend on the machine.
    fn bundled_database() -> Database {
        let mut database = Database::new();
        for font in BUNDLED_FONTS {
            database.load_font_source(Source::Binary(Arc::new(font)));
        }
        database
    }

    #[test]
    fn bundled_font_is_found_by_its_family_name() {
        assert_eq!(
            installed_family(&bundled_database(), &BUNDLED_FAMILY.to_lowercase()).unwrap(),
            BUNDLED_FAMILY
        );
    }

    #[test]
    fn unknown_family_is_an_error() {
        assert!(installed_family(&Database::new(), "No Such Font").is_err());
        assert!(
            choose_family(
                &Database::new(),
                &FontChoice::Family("No Such Font".to_owned())
            )
            .is_err()
        );
    }

    #[test]
    fn terminal_font_maps_its_postscript_name_to_the_family() {
        let font = FontChoice::Terminal(BUNDLED_REGULAR_FACE.to_owned());
        assert_eq!(
            choose_family(&bundled_database(), &font).unwrap(),
            BUNDLED_FAMILY
        );
    }

    #[test]
    fn missing_terminal_font_falls_back_to_the_bundled_one() {
        let font = FontChoice::Terminal("NotInstalled-Regular".to_owned());
        assert_eq!(
            choose_family(&Database::new(), &font).unwrap(),
            BUNDLED_FAMILY
        );
    }

    #[test]
    fn cell_width_matches_the_bundled_advance() {
        const FONT_PX: f32 = 20.0;
        // JetBrains Mono advances 600 of 1000 units per em.
        const EXPECTED_ADVANCE: f32 = 0.6 * FONT_PX;
        let typesetter = Typesetter::new(&FontChoice::Bundled, FONT_PX, FONT_PX * 1.4).unwrap();
        assert!(
            (typesetter.cell_width() - EXPECTED_ADVANCE).abs() < 0.01,
            "{}",
            typesetter.cell_width()
        );
    }
}
