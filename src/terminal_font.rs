//! Finds the font family and size of the terminal shotty runs in, so the picture can use them.
//!
//! Each terminal keeps its font in its own settings. Only reading files is used here, never
//! remote control of the terminal, so macOS asks for no automation permission. A terminal that is
//! not recognized, or whose settings cannot be read, yields `None`.

use std::io::Cursor;
use std::path::Path;

use plist::Value;

/// Environment variable through which macOS terminals name themselves.
const TERM_PROGRAM: &str = "TERM_PROGRAM";

/// `TERM_PROGRAM` value of iTerm2.
const ITERM2: &str = "iTerm.app";

/// `TERM_PROGRAM` value of Terminal.app.
const APPLE_TERMINAL: &str = "Apple_Terminal";

/// Environment variable in which iTerm2 names the profile of the current session.
const ITERM_PROFILE: &str = "ITERM_PROFILE";

/// iTerm2's preferences, relative to the home directory.
const ITERM2_PREFERENCES: &str = "Library/Preferences/com.googlecode.iterm2.plist";

/// Terminal.app's preferences, relative to the home directory.
const APPLE_TERMINAL_PREFERENCES: &str = "Library/Preferences/com.apple.Terminal.plist";

/// Keys in iTerm2's preferences.
mod iterm2_keys {
    /// Array of all profiles.
    pub const PROFILES: &str = "New Bookmarks";
    /// `Guid` of the profile new sessions use.
    pub const DEFAULT_GUID: &str = "Default Bookmark Guid";
    /// A profile's name, as `ITERM_PROFILE` gives it.
    pub const NAME: &str = "Name";
    /// A profile's unique id.
    pub const GUID: &str = "Guid";
    /// A profile's font, as `<PostScript name> <size>`.
    pub const FONT: &str = "Normal Font";
}

/// Keys in Terminal.app's preferences and in the `NSKeyedArchiver` archive of its font.
mod apple_terminal_keys {
    /// Name of the profile new windows use.
    pub const DEFAULT_PROFILE: &str = "Default Window Settings";
    /// Dictionary of all profiles by name.
    pub const PROFILES: &str = "Window Settings";
    /// A profile's font, an archived `NSFont`.
    pub const FONT: &str = "Font";
    /// The archive's object table.
    pub const OBJECTS: &str = "$objects";
    /// The archive's entry point.
    pub const TOP: &str = "$top";
    /// The archived object under `TOP`.
    pub const ROOT: &str = "root";
    /// `NSFont`'s PostScript name, a reference into `OBJECTS`.
    pub const FONT_NAME: &str = "NSName";
    /// `NSFont`'s size in points.
    pub const FONT_SIZE: &str = "NSSize";
}

/// The font a terminal profile uses.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalFont {
    /// PostScript name of the regular face, as macOS terminals store it, for example
    /// `FiraCodeNF-Reg`. The renderer maps it to the font's family.
    pub postscript_name: String,
    /// Size in points.
    pub size: f32,
}

/// Finds the font of the terminal this process runs in, from the environment and the home
/// directory of the current user.
pub fn detect() -> Option<TerminalFont> {
    let home = std::env::home_dir()?;
    detect_with(|name| std::env::var(name).ok(), &home)
}

/// Finds the terminal's font with `environment` standing in for the process environment and
/// `home` for the home directory.
fn detect_with(environment: impl Fn(&str) -> Option<String>, home: &Path) -> Option<TerminalFont> {
    match environment(TERM_PROGRAM)?.as_str() {
        ITERM2 => iterm2(
            &home.join(ITERM2_PREFERENCES),
            environment(ITERM_PROFILE).as_deref(),
        ),
        APPLE_TERMINAL => apple_terminal(&home.join(APPLE_TERMINAL_PREFERENCES)),
        _ => None,
    }
}

/// Reads the font of the iTerm2 profile named `profile` from the preferences at `path`, or of
/// the default profile when `profile` is `None` or names no profile.
fn iterm2(path: &Path, profile: Option<&str>) -> Option<TerminalFont> {
    let preferences = Value::from_file(path).ok()?;
    let preferences = preferences.as_dictionary()?;
    let profiles = preferences.get(iterm2_keys::PROFILES)?.as_array()?;
    let field = |profile: &'_ Value, key: &str| -> Option<String> {
        Some(profile.as_dictionary()?.get(key)?.as_string()?.to_owned())
    };
    let by_name = profile.and_then(|name| {
        profiles
            .iter()
            .find(|candidate| field(candidate, iterm2_keys::NAME).as_deref() == Some(name))
    });
    let chosen = match by_name {
        Some(chosen) => chosen,
        None => {
            let default_guid = preferences.get(iterm2_keys::DEFAULT_GUID)?.as_string()?;
            profiles.iter().find(|candidate| {
                field(candidate, iterm2_keys::GUID).as_deref() == Some(default_guid)
            })?
        }
    };
    parse_iterm2_font(&field(chosen, iterm2_keys::FONT)?)
}

/// Splits iTerm2's `<PostScript name> <size>`, as in `FiraCodeNF-Reg 15`.
fn parse_iterm2_font(text: &str) -> Option<TerminalFont> {
    let (name, size) = text.trim().rsplit_once(' ')?;
    Some(TerminalFont {
        postscript_name: name.to_owned(),
        size: size.parse().ok()?,
    })
}

/// Reads the font of Terminal.app's default profile from the preferences at `path`.
///
/// The profile of the window shotty runs in can differ from the default one, but only the
/// default is in the preferences; asking the running Terminal would need automation permission.
fn apple_terminal(path: &Path) -> Option<TerminalFont> {
    let preferences = Value::from_file(path).ok()?;
    let preferences = preferences.as_dictionary()?;
    let profile = preferences
        .get(apple_terminal_keys::DEFAULT_PROFILE)?
        .as_string()?;
    let archive = preferences
        .get(apple_terminal_keys::PROFILES)?
        .as_dictionary()?
        .get(profile)?
        .as_dictionary()?
        .get(apple_terminal_keys::FONT)?
        .as_data()?;
    unarchive_font(archive)
}

/// Reads PostScript name and size from an `NSKeyedArchiver` archive of an `NSFont`.
///
/// The archive is a plist whose `$top.root` points into `$objects` at a dictionary holding
/// `NSSize` directly and `NSName` as a pointer to a string in `$objects`.
fn unarchive_font(archive: &[u8]) -> Option<TerminalFont> {
    let archive = Value::from_reader(Cursor::new(archive)).ok()?;
    let archive = archive.as_dictionary()?;
    let objects = archive.get(apple_terminal_keys::OBJECTS)?.as_array()?;
    let resolve = |pointer: &Value| objects.get(usize::try_from(pointer.as_uid()?.get()).ok()?);
    let top = archive.get(apple_terminal_keys::TOP)?.as_dictionary()?;
    let font = resolve(top.get(apple_terminal_keys::ROOT)?)?.as_dictionary()?;
    Some(TerminalFont {
        postscript_name: resolve(font.get(apple_terminal_keys::FONT_NAME)?)?
            .as_string()?
            .to_owned(),
        size: font.get(apple_terminal_keys::FONT_SIZE)?.as_real()? as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Uid};
    use std::fs;
    use std::path::PathBuf;

    /// A home directory for one test, removed again when the test ends.
    struct Home(PathBuf);

    impl Home {
        /// Creates `Library/Preferences` below a directory named after the test and this process.
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("shotty-home-{name}-{}", std::process::id()));
            fs::create_dir_all(path.join("Library/Preferences")).unwrap();
            Self(path)
        }

        /// Writes `value` as a binary plist to `relative` below the home directory.
        fn write(&self, relative: &str, value: Value) {
            value.to_file_binary(self.0.join(relative)).unwrap();
        }
    }

    impl Drop for Home {
        /// Removes the directory with everything the test wrote into it.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Builds a plist dictionary from key and value pairs.
    fn dictionary(entries: Vec<(&str, Value)>) -> Value {
        Value::Dictionary(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<Dictionary>(),
        )
    }

    /// An iTerm2 profile with the given name, id and font.
    fn iterm2_profile(name: &str, guid: &str, font: &str) -> Value {
        dictionary(vec![
            (iterm2_keys::NAME, Value::from(name)),
            (iterm2_keys::GUID, Value::from(guid)),
            (iterm2_keys::FONT, Value::from(font)),
        ])
    }

    /// An environment with `TERM_PROGRAM` and, when given, `ITERM_PROFILE`.
    fn environment(
        program: &'static str,
        profile: Option<&'static str>,
    ) -> impl Fn(&str) -> Option<String> {
        move |name| match name {
            TERM_PROGRAM => Some(program.to_owned()),
            ITERM_PROFILE => profile.map(str::to_owned),
            _ => None,
        }
    }

    #[test]
    fn iterm2_uses_the_named_profile_and_falls_back_to_the_default() {
        let home = Home::new("iterm2");
        home.write(
            ITERM2_PREFERENCES,
            dictionary(vec![
                (iterm2_keys::DEFAULT_GUID, Value::from("B")),
                (
                    iterm2_keys::PROFILES,
                    Value::Array(vec![
                        iterm2_profile("Work", "A", "Menlo-Regular 12"),
                        iterm2_profile("Default", "B", "FiraCodeNF-Reg 15"),
                    ]),
                ),
            ]),
        );
        let font = |profile| detect_with(environment(ITERM2, profile), &home.0);
        assert_eq!(
            font(Some("Work")),
            Some(TerminalFont {
                postscript_name: "Menlo-Regular".to_owned(),
                size: 12.0
            })
        );
        assert_eq!(
            font(None),
            Some(TerminalFont {
                postscript_name: "FiraCodeNF-Reg".to_owned(),
                size: 15.0
            })
        );
        assert_eq!(font(Some("Gone")), font(None));
    }

    #[test]
    fn apple_terminal_unarchives_the_default_profile_font() {
        let home = Home::new("apple-terminal");
        let archive = dictionary(vec![
            ("$archiver", Value::from("NSKeyedArchiver")),
            (
                apple_terminal_keys::TOP,
                dictionary(vec![(apple_terminal_keys::ROOT, Value::Uid(Uid::new(1)))]),
            ),
            (
                apple_terminal_keys::OBJECTS,
                Value::Array(vec![
                    Value::from("$null"),
                    dictionary(vec![
                        (apple_terminal_keys::FONT_NAME, Value::Uid(Uid::new(2))),
                        (apple_terminal_keys::FONT_SIZE, Value::Real(14.0)),
                    ]),
                    Value::from("SFMono-Regular"),
                ]),
            ),
        ]);
        let mut bytes = Vec::new();
        archive.to_writer_binary(&mut bytes).unwrap();
        home.write(
            APPLE_TERMINAL_PREFERENCES,
            dictionary(vec![
                (apple_terminal_keys::DEFAULT_PROFILE, Value::from("Pro")),
                (
                    apple_terminal_keys::PROFILES,
                    dictionary(vec![(
                        "Pro",
                        dictionary(vec![(apple_terminal_keys::FONT, Value::Data(bytes))]),
                    )]),
                ),
            ]),
        );
        assert_eq!(
            detect_with(environment(APPLE_TERMINAL, None), &home.0),
            Some(TerminalFont {
                postscript_name: "SFMono-Regular".to_owned(),
                size: 14.0
            })
        );
    }

    #[test]
    fn unknown_terminals_and_missing_settings_give_nothing() {
        let home = Home::new("unknown");
        assert_eq!(detect_with(environment("vscode", None), &home.0), None);
        assert_eq!(detect_with(environment(ITERM2, None), &home.0), None);
        assert_eq!(detect_with(|_| None, &home.0), None);
    }

    #[test]
    fn malformed_iterm2_fonts_give_nothing() {
        assert_eq!(parse_iterm2_font("NoSize"), None);
        assert_eq!(parse_iterm2_font("Menlo-Regular big"), None);
    }
}
