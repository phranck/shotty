//! The command line interface: flags, their help texts, and the subcommands.

use std::path::PathBuf;

use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, ValueEnum};

use crate::theme;

/// Text shown after the option list of `shotty --help`.
const AFTER_LONG_HELP: &str = "\
EXAMPLES:
    ls -la --color=always | shotty -o listing.png
    git log --oneline --color=always | shotty --theme dracula --title \"git log\"
    cargo build 2>&1 | shotty -o build.jpg --no-window

Many programs switch colors off when their output goes into a pipe. Ask them for color, usually
with --color=always.

CONFIGURATION:
    Defaults for every rendering option can be set in $XDG_CONFIG_HOME/shotty/config.toml, which
    is ~/.config/shotty/config.toml when XDG_CONFIG_HOME is not set. Keys are the long option names
    without the leading dashes, for example:

        theme = \"nord\"
        font-size = 16
        window = false
        background = \"#1e1e2e\"

    Options given on the command line win over the file.";

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = "shotty",
    version,
    about = "Turn the output of a command into a PNG or JPG image",
    long_about = "Reads terminal output, ANSI colors included, from standard input and renders it as \
                  a PNG or JPG that looks like a terminal window.",
    after_long_help = AFTER_LONG_HELP,
)]
pub struct Cli {
    /// What to render and how.
    #[command(flatten)]
    pub render: RenderArgs,
}

/// Image formats shotty writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// PNG, lossless and with transparency.
    Png,
    /// JPEG, smaller files, without transparency.
    #[value(alias = "jpeg")]
    #[serde(alias = "jpeg")]
    Jpg,
}

impl ImageFormat {
    /// The file extension written for this format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
        }
    }

    /// Picks the format that a file extension names, case insensitive; `None` for any other.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpg),
            _ => None,
        }
    }
}

/// Options for rendering. Every one of them is optional so that the config file can fill the gaps.
#[derive(Debug, Default, Args)]
pub struct RenderArgs {
    /// Where to write the image [default: shotty-<date>-<time>.<format> in the current directory]
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Image format [default: from the output's file extension, else png]
    #[arg(short, long, value_enum)]
    pub format: Option<ImageFormat>,

    /// Color theme [default: one-dark]
    #[arg(short, long, value_parser = PossibleValuesParser::new(theme::names()))]
    pub theme: Option<String>,

    /// Font family installed on this system [default: the bundled JetBrains Mono]
    #[arg(long, value_name = "FAMILY")]
    pub font: Option<String>,

    /// Font size in points [default: 14]
    #[arg(long, value_name = "POINTS")]
    pub font_size: Option<f32>,

    /// Line height as a multiple of the font size [default: 1.4]
    #[arg(long, value_name = "FACTOR")]
    pub line_height: Option<f32>,

    /// Space between the text and the window edge, in points [default: 20]
    #[arg(long, value_name = "POINTS")]
    pub padding: Option<f32>,

    /// Pixels per point; 2 gives sharp images on high resolution screens [default: 2]
    #[arg(long, value_name = "FACTOR")]
    pub scale: Option<f32>,

    /// Text in the title bar of the window
    #[arg(long)]
    pub title: Option<String>,

    /// Leave out the window frame and render only the terminal area
    #[arg(long)]
    pub no_window: bool,

    /// Leave out the shadow under the window
    #[arg(long)]
    pub no_shadow: bool,

    /// Color around the window: `transparent` or #rrggbb; a JPG shows white for transparent [default: transparent]
    #[arg(long, value_name = "COLOR")]
    pub background: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn format_follows_the_extension() {
        assert_eq!(ImageFormat::from_extension("JPEG"), Some(ImageFormat::Jpg));
        assert_eq!(ImageFormat::from_extension("png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("gif"), None);
    }
}
