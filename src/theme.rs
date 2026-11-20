//! The bundled color themes and the 256 color palette built on top of them.
//!
//! Each theme is a Ghostty theme file from the iTerm2-Color-Schemes collection, kept unchanged in
//! `assets/themes/` so it can be compared with its source.

use anyhow::{Context, Result, anyhow, bail};

use crate::color::Rgb;

/// Number of colors a theme defines for the ANSI palette: eight normal and eight bright ones.
pub const ANSI_COLOR_COUNT: usize = 16;

/// First index of the 6×6×6 color cube in the 256 color palette.
const CUBE_START: u8 = 16;

/// Number of steps per channel in the color cube.
const CUBE_STEPS: u8 = 6;

/// First index of the gray ramp in the 256 color palette, which runs to `255`.
const GRAY_START: u8 = 232;

/// Channel value of the darkest gray in the ramp.
const GRAY_BASE: u8 = 8;

/// Distance between two neighboring grays in the ramp.
const GRAY_STEP: u8 = 10;

/// Base of the nonzero cube levels: step `n` from `1` to `5` has the channel value
/// `CUBE_LEVEL_BASE + n × CUBE_STEP_SIZE`, so the levels are `0, 95, 135, 175, 215, 255`.
const CUBE_LEVEL_BASE: u8 = 55;

/// Distance between two nonzero cube steps.
const CUBE_STEP_SIZE: u8 = 40;

/// Name of the theme used when neither a flag nor the config file picks one.
pub const DEFAULT_THEME: &str = "one-dark";

/// Every bundled theme as its name and the content of its Ghostty theme file.
const BUNDLED: &[(&str, &str)] = &[
    ("one-dark", include_str!("../assets/themes/one-dark.conf")),
    ("one-light", include_str!("../assets/themes/one-light.conf")),
    ("dracula", include_str!("../assets/themes/dracula.conf")),
    ("nord", include_str!("../assets/themes/nord.conf")),
    (
        "catppuccin-mocha",
        include_str!("../assets/themes/catppuccin-mocha.conf"),
    ),
    (
        "catppuccin-latte",
        include_str!("../assets/themes/catppuccin-latte.conf"),
    ),
    (
        "gruvbox-dark",
        include_str!("../assets/themes/gruvbox-dark.conf"),
    ),
    (
        "github-dark",
        include_str!("../assets/themes/github-dark.conf"),
    ),
    (
        "github-light",
        include_str!("../assets/themes/github-light.conf"),
    ),
    (
        "tokyo-night",
        include_str!("../assets/themes/tokyo-night.conf"),
    ),
];

/// The colors a terminal draws with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Color behind text that sets no background, and the color of the window.
    pub background: Rgb,
    /// Color of text that sets no foreground.
    pub foreground: Rgb,
    /// ANSI colors `0` to `15`.
    pub palette: [Rgb; ANSI_COLOR_COUNT],
}

impl Theme {
    /// Loads a bundled theme by name.
    ///
    /// # Errors
    ///
    /// Fails when no bundled theme has that name, listing the names that exist.
    pub fn bundled(name: &str) -> Result<Self> {
        let (_, source) = BUNDLED
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .ok_or_else(|| {
                anyhow!(
                    "unknown theme `{name}`; available: {}",
                    names().collect::<Vec<_>>().join(", ")
                )
            })?;
        parse(source).with_context(|| format!("bundled theme `{name}` is malformed"))
    }

    /// Returns the color for an index of the xterm 256 color palette.
    ///
    /// Indices `0` to `15` come from the theme. `16` to `231` are the 6×6×6 cube and `232` to `255`
    /// the gray ramp, both fixed by xterm and the same in every theme.
    pub fn indexed(&self, index: u8) -> Rgb {
        if usize::from(index) < ANSI_COLOR_COUNT {
            return self.palette[usize::from(index)];
        }
        if index >= GRAY_START {
            let level = GRAY_BASE + (index - GRAY_START) * GRAY_STEP;
            return Rgb::new(level, level, level);
        }
        let cube = index - CUBE_START;
        let level = |step: u8| {
            if step == 0 {
                0
            } else {
                CUBE_LEVEL_BASE + step * CUBE_STEP_SIZE
            }
        };
        Rgb::new(
            level(cube / (CUBE_STEPS * CUBE_STEPS)),
            level(cube / CUBE_STEPS % CUBE_STEPS),
            level(cube % CUBE_STEPS),
        )
    }
}

/// Names of all bundled themes in the order `--help` lists them.
pub fn names() -> impl Iterator<Item = &'static str> {
    BUNDLED.iter().map(|(name, _)| *name)
}

/// Parses a Ghostty theme file.
///
/// Reads `background`, `foreground` and `palette = N=#rrggbb` for `N` from `0` to `15`, and skips
/// every other key, blank lines and `#` comments.
///
/// # Errors
///
/// Fails on a malformed line, a palette index above `15`, an invalid color, or when one of the
/// required colors is missing.
fn parse(source: &str) -> Result<Theme> {
    let mut background = None;
    let mut foreground = None;
    let mut palette: [Option<Rgb>; ANSI_COLOR_COUNT] = [None; ANSI_COLOR_COUNT];

    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("line `{line}` has no `=`"))?;
        match key.trim() {
            "background" => background = Some(Rgb::parse_hex(value)?),
            "foreground" => foreground = Some(Rgb::parse_hex(value)?),
            "palette" => {
                let (index, color) = value
                    .split_once('=')
                    .ok_or_else(|| anyhow!("palette line `{line}` has no index"))?;
                let index: usize = index
                    .trim()
                    .parse()
                    .with_context(|| format!("palette index in `{line}`"))?;
                if index >= ANSI_COLOR_COUNT {
                    bail!("palette index {index} is above {}", ANSI_COLOR_COUNT - 1);
                }
                palette[index] = Some(Rgb::parse_hex(color)?);
            }
            _ => {}
        }
    }

    let mut resolved = [Rgb::BLACK; ANSI_COLOR_COUNT];
    for (index, color) in palette.iter().enumerate() {
        resolved[index] = color.ok_or_else(|| anyhow!("palette color {index} is missing"))?;
    }
    Ok(Theme {
        background: background.ok_or_else(|| anyhow!("background is missing"))?,
        foreground: foreground.ok_or_else(|| anyhow!("foreground is missing"))?,
        palette: resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_parses() {
        for name in names() {
            Theme::bundled(name).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        }
    }

    #[test]
    fn default_theme_is_bundled() {
        assert!(names().any(|name| name == DEFAULT_THEME));
    }

    #[test]
    fn unknown_theme_lists_the_known_ones() {
        let message = format!("{:#}", Theme::bundled("nope").unwrap_err());
        assert!(message.contains(DEFAULT_THEME), "{message}");
    }

    #[test]
    fn missing_palette_entry_is_an_error() {
        assert!(parse("background = #000000\nforeground = #ffffff\n").is_err());
    }

    #[test]
    fn indexed_colors_follow_xterm() {
        let theme = Theme::bundled(DEFAULT_THEME).unwrap();
        assert_eq!(theme.indexed(1), theme.palette[1]);
        assert_eq!(theme.indexed(16), Rgb::BLACK);
        assert_eq!(theme.indexed(196), Rgb::new(255, 0, 0));
        assert_eq!(theme.indexed(231), Rgb::new(255, 255, 255));
        assert_eq!(theme.indexed(232), Rgb::new(8, 8, 8));
        assert_eq!(theme.indexed(255), Rgb::new(238, 238, 238));
    }
}
