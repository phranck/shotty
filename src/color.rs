//! sRGB colors and the parsing of the color notations shotty accepts.

use anyhow::{Context, Result, bail};

/// Number of hexadecimal digits in a `#rrggbb` color.
const HEX_COLOR_DIGITS: usize = 6;

/// Largest value of an 8 bit color channel.
const CHANNEL_MAX: f32 = 255.0;

/// A color in sRGB with 8 bits per channel and no alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel, `0` to `255`.
    pub red: u8,
    /// Green channel, `0` to `255`.
    pub green: u8,
    /// Blue channel, `0` to `255`.
    pub blue: u8,
}

impl Rgb {
    /// Plain white, used under a JPG where the picture has no background of its own.
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    /// Creates a color from its three channels, each from `0` to `255`.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Parses `#rrggbb` or `rrggbb`, case insensitive.
    ///
    /// # Errors
    ///
    /// Fails when the text is not exactly six hexadecimal digits after the optional `#`.
    pub fn parse_hex(text: &str) -> Result<Self> {
        let digits = text.trim().trim_start_matches('#');
        if digits.len() != HEX_COLOR_DIGITS
            || !digits.chars().all(|digit| digit.is_ascii_hexdigit())
        {
            bail!("`{text}` is not a color in the form #rrggbb");
        }
        let channel = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&digits[range], 16)
                .with_context(|| format!("`{text}` is not a color in the form #rrggbb"))
        };
        Ok(Self::new(channel(0..2)?, channel(2..4)?, channel(4..6)?))
    }

    /// Mixes `other` into this color.
    ///
    /// `amount` is the share of `other`, from `0.0`, which returns `self`, to `1.0`, which returns
    /// `other`; values outside that range are clamped. Each channel is rounded to the nearest
    /// integer.
    pub fn mix(self, other: Rgb, amount: f32) -> Rgb {
        let amount = amount.clamp(0.0, 1.0);
        let blend = |from: u8, to: u8| {
            (f32::from(from) + (f32::from(to) - f32::from(from)) * amount)
                .round()
                .clamp(0.0, CHANNEL_MAX) as u8
        };
        Rgb::new(
            blend(self.red, other.red),
            blend(self.green, other.green),
            blend(self.blue, other.blue),
        )
    }
}

/// The color around the window, as `--background` and the `background` config key accept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Fully transparent. A PNG keeps the transparency; a JPG gets `Rgb::WHITE` instead.
    Transparent,
    /// An opaque color.
    Solid(Rgb),
}

impl Backdrop {
    /// Keyword that selects `Backdrop::Transparent`.
    pub const TRANSPARENT_KEYWORD: &'static str = "transparent";

    /// Parses `transparent` or a `#rrggbb` color.
    ///
    /// # Errors
    ///
    /// Fails when the text is neither the keyword nor a valid hex color.
    pub fn parse(text: &str) -> Result<Self> {
        if text.trim().eq_ignore_ascii_case(Self::TRANSPARENT_KEYWORD) {
            return Ok(Self::Transparent);
        }
        Rgb::parse_hex(text).map(Self::Solid).with_context(|| {
            format!(
                "background must be `{}` or #rrggbb",
                Self::TRANSPARENT_KEYWORD
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_hash() {
        assert_eq!(
            Rgb::parse_hex("#21252b").unwrap(),
            Rgb::new(0x21, 0x25, 0x2b)
        );
        assert_eq!(
            Rgb::parse_hex("ABCDEF").unwrap(),
            Rgb::new(0xab, 0xcd, 0xef)
        );
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(Rgb::parse_hex("#12345").is_err());
        assert!(Rgb::parse_hex("#12345g").is_err());
        assert!(Rgb::parse_hex("#1234567").is_err());
    }

    #[test]
    fn mixes_at_both_ends_and_between() {
        let black = Rgb::new(0, 0, 0);
        assert_eq!(black.mix(Rgb::WHITE, 0.0), black);
        assert_eq!(black.mix(Rgb::WHITE, 1.0), Rgb::WHITE);
        assert_eq!(black.mix(Rgb::WHITE, 0.5), Rgb::new(128, 128, 128));
    }

    #[test]
    fn parses_backdrop() {
        assert_eq!(
            Backdrop::parse("Transparent").unwrap(),
            Backdrop::Transparent
        );
        assert_eq!(
            Backdrop::parse("#ffffff").unwrap(),
            Backdrop::Solid(Rgb::WHITE)
        );
        assert!(Backdrop::parse("blue").is_err());
    }
}
