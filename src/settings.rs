//! Resolves the rendering settings from the command line, the config file and the defaults, in that
//! order of precedence.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::{ImageFormat, RenderArgs};
use crate::color::Backdrop;
use crate::terminal_font::TerminalFont;
use crate::theme::{self, Theme};

/// Directory below the config home that holds shotty's files.
pub const APP_DIRECTORY: &str = "shotty";

/// Name of the config file inside `APP_DIRECTORY`.
const CONFIG_FILE: &str = "config.toml";

/// Environment variable that moves the config home away from `~/.config`.
const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";

/// Config home below the user's home directory when `XDG_CONFIG_HOME` is not set.
const DEFAULT_CONFIG_HOME: &str = ".config";

/// Font size in points when nothing else sets it.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// Line height as a multiple of the font size when nothing else sets it.
pub const DEFAULT_LINE_HEIGHT: f32 = 1.4;

/// Space between the text and the window edge in points when nothing else sets it.
pub const DEFAULT_PADDING: f32 = 20.0;

/// Pixels per point when nothing else sets it. `2` matches high resolution screens.
pub const DEFAULT_SCALE: f32 = 2.0;

/// Terminal width for `--execute` when nothing else sets it, the classic terminal width.
pub const DEFAULT_EXECUTE_COLUMNS: u16 = 80;

/// Widest accepted terminal. The parser drops columns beyond `ansi::MAX_COLUMNS` anyway.
const MAX_COLUMNS: u16 = crate::ansi::MAX_COLUMNS as u16;

/// Value of `--font` and of the `font` key that selects the bundled font instead of the
/// terminal's.
pub const BUNDLED_FONT_KEYWORD: &str = "bundled";

/// Smallest accepted font size in points. Below it glyphs turn into noise.
const MIN_FONT_SIZE: f32 = 4.0;

/// Largest accepted font size in points.
const MAX_FONT_SIZE: f32 = 200.0;

/// Smallest accepted scale.
const MIN_SCALE: f32 = 0.5;

/// Largest accepted scale. Above it even short output makes images of many megapixels.
const MAX_SCALE: f32 = 8.0;

/// Smallest accepted line height factor; below it lines overlap.
const MIN_LINE_HEIGHT: f32 = 1.0;

/// Largest accepted line height factor.
const MAX_LINE_HEIGHT: f32 = 4.0;

/// Largest accepted padding in points.
const MAX_PADDING: f32 = 400.0;

/// The keys `config.toml` accepts, named like the long command line options.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ConfigFile {
    /// Image format used when the output path does not decide it.
    pub format: Option<ImageFormat>,
    /// Name of a bundled theme.
    pub theme: Option<String>,
    /// Installed font family.
    pub font: Option<String>,
    /// Font size in points.
    pub font_size: Option<f32>,
    /// Line height as a multiple of the font size.
    pub line_height: Option<f32>,
    /// Space between text and window edge in points.
    pub padding: Option<f32>,
    /// Pixels per point.
    pub scale: Option<f32>,
    /// `false` leaves out the window frame, like `--no-window`.
    pub window: Option<bool>,
    /// `false` leaves out the shadow, like `--no-shadow`.
    pub shadow: Option<bool>,
    /// `transparent` or `#rrggbb`.
    pub background: Option<String>,
    /// Terminal width in columns.
    pub columns: Option<u16>,
}

impl ConfigFile {
    /// Path of the config file: `$XDG_CONFIG_HOME/shotty/config.toml`, or
    /// `~/.config/shotty/config.toml` when that variable is unset or empty. `None` when neither
    /// the variable nor a home directory is available.
    pub fn path() -> Option<PathBuf> {
        config_directory().map(|directory| directory.join(CONFIG_FILE))
    }

    /// Reads the config file. A missing file is an empty config.
    ///
    /// # Errors
    ///
    /// Fails when the file exists but cannot be read or is not valid for these keys.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(source) => toml::from_str(&source)
                .with_context(|| format!("invalid config file {}", path.display())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => {
                Err(error).with_context(|| format!("cannot read config file {}", path.display()))
            }
        }
    }
}

/// Directory that holds shotty's config, as described at `ConfigFile::path`.
pub fn config_directory() -> Option<PathBuf> {
    let home = env::var_os(XDG_CONFIG_HOME)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(DEFAULT_CONFIG_HOME)))?;
    Some(home.join(APP_DIRECTORY))
}

/// The font the text is set in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontChoice {
    /// The bundled JetBrains Mono NL, chosen with `BUNDLED_FONT_KEYWORD` or when the terminal's
    /// font is unknown.
    Bundled,
    /// An installed family named with `--font` or the config file. It is an error when it is not
    /// installed.
    Family(String),
    /// The terminal's font, by the PostScript name of its regular face. The bundled font stands in
    /// when it is not installed.
    Terminal(String),
}

/// Everything the renderer needs, with every value decided.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Where the image goes.
    pub output: PathBuf,
    /// Format of the image.
    pub format: ImageFormat,
    /// Colors.
    pub theme: Theme,
    /// Font of the text.
    pub font: FontChoice,
    /// Font size in points.
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Space between text and window edge in points.
    pub padding: f32,
    /// Pixels per point.
    pub scale: f32,
    /// Title bar text; `None` leaves the title bar empty.
    pub title: Option<String>,
    /// Whether the window frame is drawn.
    pub window: bool,
    /// Whether the window casts a shadow. Only has an effect with `window`.
    pub shadow: bool,
    /// Color around the window.
    pub backdrop: Backdrop,
    /// Terminal width from `--columns` or the config file, from `1` to `MAX_COLUMNS`. `None`
    /// means it was not set; `main` then keeps piped lines whole and gives `--execute`
    /// `DEFAULT_EXECUTE_COLUMNS`.
    pub columns: Option<u16>,
}

impl Settings {
    /// Combines the command line and the config file, falling back to the defaults.
    ///
    /// `terminal` is the font of the terminal shotty runs in. It comes after the command line and
    /// the config file for the family and the size each, and before the defaults. `now` names the
    /// file when no output path is given, as `shotty-<now>.<extension>`.
    ///
    /// # Errors
    ///
    /// Fails on an output extension other than `.png`, `.jpg` or `.jpeg` without `--format`, an
    /// unknown theme, an unparsable background, or a number outside its accepted range.
    pub fn resolve(
        args: RenderArgs,
        config: ConfigFile,
        terminal: Option<TerminalFont>,
        now: &str,
    ) -> Result<Self> {
        let extension = args.output.as_deref().and_then(Path::extension);
        let format = match (args.format, extension) {
            (Some(format), _) => format,
            (None, Some(extension)) => {
                let extension = extension.to_string_lossy();
                ImageFormat::from_extension(&extension).with_context(|| {
                    format!("cannot tell the image format from `.{extension}`; use .png or .jpg, or name the format with --format")
                })?
            }
            (None, None) => config.format.unwrap_or(ImageFormat::Png),
        };
        let output = args
            .output
            .unwrap_or_else(|| PathBuf::from(format!("shotty-{now}.{}", format.extension())));
        let theme_name = args
            .theme
            .or(config.theme)
            .unwrap_or_else(|| theme::DEFAULT_THEME.to_owned());
        let backdrop = match args.background.or(config.background) {
            Some(text) => Backdrop::parse(&text)?,
            None => Backdrop::Transparent,
        };

        Ok(Self {
            output,
            format,
            theme: Theme::bundled(&theme_name)?,
            font: match args.font.or(config.font) {
                Some(name) if name.eq_ignore_ascii_case(BUNDLED_FONT_KEYWORD) => {
                    FontChoice::Bundled
                }
                Some(name) => FontChoice::Family(name),
                None => terminal.as_ref().map_or(FontChoice::Bundled, |font| {
                    FontChoice::Terminal(font.postscript_name.clone())
                }),
            },
            font_size: in_range(
                "font-size",
                args.font_size
                    .or(config.font_size)
                    .or(terminal.as_ref().map(|font| font.size))
                    .unwrap_or(DEFAULT_FONT_SIZE),
                MIN_FONT_SIZE,
                MAX_FONT_SIZE,
            )?,
            line_height: in_range(
                "line-height",
                args.line_height
                    .or(config.line_height)
                    .unwrap_or(DEFAULT_LINE_HEIGHT),
                MIN_LINE_HEIGHT,
                MAX_LINE_HEIGHT,
            )?,
            padding: in_range(
                "padding",
                args.padding.or(config.padding).unwrap_or(DEFAULT_PADDING),
                0.0,
                MAX_PADDING,
            )?,
            scale: in_range(
                "scale",
                args.scale.or(config.scale).unwrap_or(DEFAULT_SCALE),
                MIN_SCALE,
                MAX_SCALE,
            )?,
            title: args.title.or_else(|| args.execute.clone()),
            window: !args.no_window && config.window.unwrap_or(true),
            shadow: !args.no_shadow && config.shadow.unwrap_or(true),
            backdrop,
            columns: args
                .columns
                .or(config.columns)
                .map(|columns| in_range("columns", columns, 1, MAX_COLUMNS))
                .transpose()?,
        })
    }
}

/// Checks that `value` lies within `min..=max`.
///
/// # Errors
///
/// Fails with a message naming the option when it does not, or when it is not a number.
fn in_range<T: PartialOrd + Copy + std::fmt::Display>(
    option: &str,
    value: T,
    min: T,
    max: T,
) -> Result<T> {
    if !(min..=max).contains(&value) {
        anyhow::bail!("{option} must be between {min} and {max}, not {value}");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "20261120-101500";

    #[test]
    fn defaults_apply_without_flags_or_config() {
        let settings =
            Settings::resolve(RenderArgs::default(), ConfigFile::default(), None, NOW).unwrap();
        assert_eq!(settings.output, PathBuf::from("shotty-20261120-101500.png"));
        assert_eq!(settings.format, ImageFormat::Png);
        assert_eq!(settings.font_size, DEFAULT_FONT_SIZE);
        assert!(settings.window && settings.shadow);
        assert_eq!(settings.backdrop, Backdrop::Transparent);
    }

    #[test]
    fn flags_win_over_config_and_config_over_defaults() {
        let args = RenderArgs {
            font_size: Some(20.0),
            ..RenderArgs::default()
        };
        let config: ConfigFile =
            toml::from_str("font-size = 16\npadding = 8\nwindow = false\ntheme = \"nord\"")
                .unwrap();
        let settings = Settings::resolve(args, config, None, NOW).unwrap();
        assert_eq!(settings.font_size, 20.0);
        assert_eq!(settings.padding, 8.0);
        assert!(!settings.window);
        assert_eq!(settings.theme, Theme::bundled("nord").unwrap());
    }

    #[test]
    fn output_extension_picks_the_format() {
        let args = RenderArgs {
            output: Some("shot.JPEG".into()),
            ..RenderArgs::default()
        };
        assert_eq!(
            Settings::resolve(args, ConfigFile::default(), None, NOW)
                .unwrap()
                .format,
            ImageFormat::Jpg
        );
    }

    #[test]
    fn format_flag_names_the_default_file() {
        let args = RenderArgs {
            format: Some(ImageFormat::Jpg),
            ..RenderArgs::default()
        };
        assert_eq!(
            Settings::resolve(args, ConfigFile::default(), None, NOW)
                .unwrap()
                .output,
            PathBuf::from("shotty-20261120-101500.jpg")
        );
    }

    #[test]
    fn execute_names_the_window() {
        let args = RenderArgs {
            execute: Some("git status".to_owned()),
            ..RenderArgs::default()
        };
        let settings = Settings::resolve(args, ConfigFile::default(), None, NOW).unwrap();
        assert_eq!(settings.title.as_deref(), Some("git status"));
    }

    /// The font of a terminal profile, as `terminal_font::detect` reports it.
    fn terminal() -> Option<TerminalFont> {
        Some(TerminalFont {
            postscript_name: "FiraCodeNF-Reg".to_owned(),
            size: 15.0,
        })
    }

    #[test]
    fn terminal_font_comes_after_flags_and_config() {
        let settings = Settings::resolve(
            RenderArgs::default(),
            ConfigFile::default(),
            terminal(),
            NOW,
        )
        .unwrap();
        assert_eq!(
            settings.font,
            FontChoice::Terminal("FiraCodeNF-Reg".to_owned())
        );
        assert_eq!(settings.font_size, 15.0);

        let config: ConfigFile = toml::from_str("font = \"Menlo\"\nfont-size = 12").unwrap();
        let settings = Settings::resolve(RenderArgs::default(), config, terminal(), NOW).unwrap();
        assert_eq!(settings.font, FontChoice::Family("Menlo".to_owned()));
        assert_eq!(settings.font_size, 12.0);

        let args = RenderArgs {
            font: Some("Monaco".to_owned()),
            ..RenderArgs::default()
        };
        let settings = Settings::resolve(args, ConfigFile::default(), terminal(), NOW).unwrap();
        assert_eq!(settings.font, FontChoice::Family("Monaco".to_owned()));
        assert_eq!(
            settings.font_size, 15.0,
            "the terminal's size stays when only the family is set"
        );
    }

    #[test]
    fn bundled_keyword_and_unknown_terminal_select_the_bundled_font() {
        let args = RenderArgs {
            font: Some("Bundled".to_owned()),
            ..RenderArgs::default()
        };
        assert_eq!(
            Settings::resolve(args, ConfigFile::default(), terminal(), NOW)
                .unwrap()
                .font,
            FontChoice::Bundled
        );
        let settings =
            Settings::resolve(RenderArgs::default(), ConfigFile::default(), None, NOW).unwrap();
        assert_eq!(
            (settings.font, settings.font_size),
            (FontChoice::Bundled, DEFAULT_FONT_SIZE)
        );
    }

    #[test]
    fn unknown_config_key_is_an_error() {
        assert!(toml::from_str::<ConfigFile>("colour = \"red\"").is_err());
    }

    #[test]
    fn out_of_range_numbers_are_errors() {
        let args = RenderArgs {
            scale: Some(100.0),
            ..RenderArgs::default()
        };
        assert!(Settings::resolve(args, ConfigFile::default(), None, NOW).is_err());
        let args = RenderArgs {
            font_size: Some(f32::NAN),
            ..RenderArgs::default()
        };
        assert!(Settings::resolve(args, ConfigFile::default(), None, NOW).is_err());
    }

    #[test]
    fn missing_config_file_is_empty() {
        let config = ConfigFile::load(Path::new("/nonexistent/shotty/config.toml")).unwrap();
        assert!(config.theme.is_none());
    }

    #[test]
    fn unknown_extension_needs_the_format_flag() {
        let args = RenderArgs {
            output: Some("shot.gif".into()),
            ..RenderArgs::default()
        };
        let error = Settings::resolve(args, ConfigFile::default(), None, NOW).unwrap_err();
        assert!(error.to_string().contains(".gif"), "{error}");

        let args = RenderArgs {
            output: Some("shot.gif".into()),
            format: Some(ImageFormat::Png),
            ..RenderArgs::default()
        };
        assert!(Settings::resolve(args, ConfigFile::default(), None, NOW).is_ok());
    }

    #[test]
    fn columns_from_the_config_are_checked() {
        let config: ConfigFile = toml::from_str("columns = 0").unwrap();
        assert!(Settings::resolve(RenderArgs::default(), config, None, NOW).is_err());
    }
}
