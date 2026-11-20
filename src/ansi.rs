//! Turns terminal output with ANSI escape sequences into a grid of styled cells.
//!
//! The grid follows what a terminal shows rather than the raw text: a carriage return lets later
//! text overwrite the line, cursor movements and line erasing are applied, and wide characters
//! take two columns. Escape sequences that do not change what is visible are dropped.

use unicode_width::UnicodeWidthChar;
use vte::{Params, Parser, Perform};

use crate::color::Rgb;

/// Distance between tab stops, as in every common terminal.
const TAB_WIDTH: usize = 8;

/// Number of columns a wide character such as a CJK ideograph takes.
const WIDE_CHARACTER_WIDTH: usize = 2;

/// SGR parameters that `ansi::Screen::select_graphic_rendition` understands.
mod sgr {
    pub const RESET: u16 = 0;
    pub const BOLD: u16 = 1;
    pub const DIM: u16 = 2;
    pub const ITALIC: u16 = 3;
    pub const UNDERLINE: u16 = 4;
    pub const INVERSE: u16 = 7;
    pub const STRIKETHROUGH: u16 = 9;
    pub const DOUBLE_UNDERLINE: u16 = 21;
    pub const NORMAL_INTENSITY: u16 = 22;
    pub const NO_ITALIC: u16 = 23;
    pub const NO_UNDERLINE: u16 = 24;
    pub const NO_INVERSE: u16 = 27;
    pub const NO_STRIKETHROUGH: u16 = 29;
    pub const FOREGROUND_FIRST: u16 = 30;
    pub const FOREGROUND_LAST: u16 = 37;
    pub const FOREGROUND_EXTENDED: u16 = 38;
    pub const FOREGROUND_DEFAULT: u16 = 39;
    pub const BACKGROUND_FIRST: u16 = 40;
    pub const BACKGROUND_LAST: u16 = 47;
    pub const BACKGROUND_EXTENDED: u16 = 48;
    pub const BACKGROUND_DEFAULT: u16 = 49;
    pub const UNDERLINE_COLOR_EXTENDED: u16 = 58;
    pub const BRIGHT_FOREGROUND_FIRST: u16 = 90;
    pub const BRIGHT_FOREGROUND_LAST: u16 = 97;
    pub const BRIGHT_BACKGROUND_FIRST: u16 = 100;
    pub const BRIGHT_BACKGROUND_LAST: u16 = 107;
    /// Offset from a normal ANSI color index to its bright counterpart.
    pub const BRIGHT_OFFSET: u8 = 8;
    /// Selector after `38` or `48` for a 256 color palette index.
    pub const EXTENDED_INDEXED: u16 = 5;
    /// Selector after `38` or `48` for a true color.
    pub const EXTENDED_RGB: u16 = 2;
}

/// A color as the terminal output names it, before a theme turns it into pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TermColor {
    /// The theme's foreground or background, depending on where it is used.
    #[default]
    Default,
    /// An index into the 256 color palette.
    Indexed(u8),
    /// A true color.
    Rgb(Rgb),
}

/// How a cell is drawn. The default is plain text in the theme's default colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// Text color.
    pub foreground: TermColor,
    /// Color behind the cell.
    pub background: TermColor,
    /// Bold weight.
    pub bold: bool,
    /// Faint text, drawn halfway between the foreground and the background.
    pub dim: bool,
    /// Italic style.
    pub italic: bool,
    /// A line under the text.
    pub underline: bool,
    /// Foreground and background swapped.
    pub inverse: bool,
    /// A line through the text.
    pub strikethrough: bool,
}

/// One column of one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The character with any combining marks. Empty in the second column of a wide character.
    pub text: String,
    /// How the cell is drawn.
    pub style: Style,
}

impl Cell {
    /// A space in the default style, used where nothing was written.
    fn blank() -> Self {
        Self {
            text: " ".to_owned(),
            style: Style::default(),
        }
    }

    /// Tells whether the cell is the second column of a wide character.
    pub fn is_continuation(&self) -> bool {
        self.text.is_empty()
    }

    /// Tells whether the cell shows nothing: a space on the default background, not inverted.
    pub fn is_empty(&self) -> bool {
        self.text == " " && self.style.background == TermColor::Default && !self.style.inverse
    }
}

/// The parsed output: lines of cells, without trailing empty lines and without trailing empty
/// cells on each line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    /// The lines from top to bottom.
    pub lines: Vec<Vec<Cell>>,
}

impl Screen {
    /// Parses terminal output.
    ///
    /// `columns` is the width at which long lines wrap, as they would in a terminal that wide;
    /// `None` keeps every line in one piece. Invalid UTF-8 shows as U+FFFD.
    pub fn parse(input: &[u8], columns: Option<usize>) -> Self {
        let mut builder = ScreenBuilder {
            columns,
            ..ScreenBuilder::default()
        };
        let mut parser = Parser::new();
        parser.advance(&mut builder, input);
        builder.finish()
    }

    /// Number of columns of the widest line.
    pub fn width(&self) -> usize {
        self.lines.iter().map(Vec::len).max().unwrap_or(0)
    }
}

/// Collects cells while the parser walks the input.
#[derive(Default)]
struct ScreenBuilder {
    /// Lines written so far; a line that was never written to may be missing at the end.
    lines: Vec<Vec<Cell>>,
    /// Cursor line, counted from the first line of the output.
    row: usize,
    /// Cursor column.
    column: usize,
    /// Style applied to the next printed character.
    style: Style,
    /// Wrap width; `None` means no wrapping.
    columns: Option<usize>,
}

impl ScreenBuilder {
    /// Returns the cursor line, creating it and every line above it when they do not exist yet.
    fn current_line(&mut self) -> &mut Vec<Cell> {
        if self.lines.len() <= self.row {
            self.lines.resize_with(self.row + 1, Vec::new);
        }
        &mut self.lines[self.row]
    }

    /// Writes a cell at `column` of the cursor line, filling any gap before it with blanks.
    fn put(&mut self, column: usize, cell: Cell) {
        let line = self.current_line();
        if line.len() <= column {
            line.resize_with(column + 1, Cell::blank);
        }
        line[column] = cell;
    }

    /// Moves the cursor to the start of the next line.
    fn new_line(&mut self) {
        self.row += 1;
        self.column = 0;
    }

    /// Erases part of the cursor line, as `CSI K` does.
    ///
    /// `mode` `0` erases from the cursor to the end, `1` from the start to the cursor, `2` the whole
    /// line. Other values change nothing.
    fn erase_in_line(&mut self, mode: u16) {
        let column = self.column;
        let line = self.current_line();
        match mode {
            0 => line.truncate(column),
            1 => line
                .iter_mut()
                .take(column + 1)
                .for_each(|cell| *cell = Cell::blank()),
            2 => line.clear(),
            _ => {}
        }
    }

    /// Erases part of the screen, as `CSI J` does.
    ///
    /// `mode` `0` erases from the cursor to the end, `2` and `3` everything. Erasing from the start,
    /// mode `1`, is ignored because the output above stays relevant for a picture.
    fn erase_in_display(&mut self, mode: u16) {
        match mode {
            0 => {
                self.erase_in_line(0);
                self.lines.truncate(self.row + 1);
            }
            2 | 3 => self.lines.clear(),
            _ => {}
        }
    }

    /// Applies an SGR sequence (`CSI … m`) to the current style.
    fn select_graphic_rendition(&mut self, params: &Params) {
        let groups: Vec<&[u16]> = params.iter().collect();
        if groups.is_empty() {
            self.style = Style::default();
            return;
        }
        let mut index = 0;
        while index < groups.len() {
            let group = groups[index];
            index += 1;
            let code = group[0];
            match code {
                sgr::RESET => self.style = Style::default(),
                sgr::BOLD => self.style.bold = true,
                sgr::DIM => self.style.dim = true,
                sgr::ITALIC => self.style.italic = true,
                // `4:0` is the colon form of "no underline"; every other `4:n` is an underline style.
                sgr::UNDERLINE => self.style.underline = group.get(1) != Some(&0),
                sgr::INVERSE => self.style.inverse = true,
                sgr::STRIKETHROUGH => self.style.strikethrough = true,
                sgr::DOUBLE_UNDERLINE => self.style.underline = true,
                sgr::NORMAL_INTENSITY => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                sgr::NO_ITALIC => self.style.italic = false,
                sgr::NO_UNDERLINE => self.style.underline = false,
                sgr::NO_INVERSE => self.style.inverse = false,
                sgr::NO_STRIKETHROUGH => self.style.strikethrough = false,
                sgr::FOREGROUND_FIRST..=sgr::FOREGROUND_LAST => {
                    self.style.foreground =
                        TermColor::Indexed((code - sgr::FOREGROUND_FIRST) as u8);
                }
                sgr::FOREGROUND_DEFAULT => self.style.foreground = TermColor::Default,
                sgr::BACKGROUND_FIRST..=sgr::BACKGROUND_LAST => {
                    self.style.background =
                        TermColor::Indexed((code - sgr::BACKGROUND_FIRST) as u8);
                }
                sgr::BACKGROUND_DEFAULT => self.style.background = TermColor::Default,
                sgr::BRIGHT_FOREGROUND_FIRST..=sgr::BRIGHT_FOREGROUND_LAST => {
                    self.style.foreground = TermColor::Indexed(
                        (code - sgr::BRIGHT_FOREGROUND_FIRST) as u8 + sgr::BRIGHT_OFFSET,
                    );
                }
                sgr::BRIGHT_BACKGROUND_FIRST..=sgr::BRIGHT_BACKGROUND_LAST => {
                    self.style.background = TermColor::Indexed(
                        (code - sgr::BRIGHT_BACKGROUND_FIRST) as u8 + sgr::BRIGHT_OFFSET,
                    );
                }
                sgr::FOREGROUND_EXTENDED
                | sgr::BACKGROUND_EXTENDED
                | sgr::UNDERLINE_COLOR_EXTENDED => {
                    let color = extended_color(group, &groups, &mut index);
                    match (code, color) {
                        (sgr::FOREGROUND_EXTENDED, Some(color)) => self.style.foreground = color,
                        (sgr::BACKGROUND_EXTENDED, Some(color)) => self.style.background = color,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Ends parsing: drops trailing empty cells and lines and returns the screen.
    fn finish(mut self) -> Screen {
        for line in &mut self.lines {
            while line.last().is_some_and(Cell::is_empty) {
                line.pop();
            }
        }
        while self.lines.last().is_some_and(Vec::is_empty) {
            self.lines.pop();
        }
        Screen { lines: self.lines }
    }
}

/// Reads the color of a `38`, `48` or `58` SGR parameter.
///
/// Handles the colon form, where the color sits in the subparameters of `group` (`38:5:n`,
/// `38:2:r:g:b`, and `38:2:colorspace:r:g:b`), and the semicolon form, where it follows in the next
/// groups (`38;5;n`, `38;2;r;g;b`). In the semicolon form `index` is advanced past the consumed
/// groups. Returns `None` when the sequence is incomplete or uses an unknown selector.
fn extended_color(group: &[u16], groups: &[&[u16]], index: &mut usize) -> Option<TermColor> {
    let channel = |value: u16| u8::try_from(value).ok();
    if group.len() > 1 {
        return match group[1] {
            sgr::EXTENDED_INDEXED => group
                .get(2)
                .copied()
                .and_then(channel)
                .map(TermColor::Indexed),
            sgr::EXTENDED_RGB => {
                let rgb = if group.len() >= 6 {
                    &group[3..6]
                } else {
                    group.get(2..5)?
                };
                Some(TermColor::Rgb(Rgb::new(
                    channel(rgb[0])?,
                    channel(rgb[1])?,
                    channel(rgb[2])?,
                )))
            }
            _ => None,
        };
    }
    let mut next = || {
        let value = groups.get(*index).map(|group| group[0]);
        *index += 1;
        value
    };
    match next()? {
        sgr::EXTENDED_INDEXED => next().and_then(channel).map(TermColor::Indexed),
        sgr::EXTENDED_RGB => {
            let (red, green, blue) = (next()?, next()?, next()?);
            Some(TermColor::Rgb(Rgb::new(
                channel(red)?,
                channel(green)?,
                channel(blue)?,
            )))
        }
        _ => None,
    }
}

/// Reads the first parameter of a CSI sequence, with `default` when it is missing or zero.
fn count_parameter(params: &Params, default: u16) -> usize {
    match params.iter().next().map(|group| group[0]) {
        None | Some(0) => usize::from(default),
        Some(value) => usize::from(value),
    }
}

impl Perform for ScreenBuilder {
    fn print(&mut self, character: char) {
        let Some(width) = character.width() else {
            return;
        };
        if width == 0 {
            // A combining mark joins the character before it, skipping the second half of a wide one.
            let column = self.column;
            let line = self.current_line();
            let target = (0..column.min(line.len()))
                .rev()
                .find(|&candidate| !line[candidate].is_continuation());
            if let Some(target) = target {
                line[target].text.push(character);
            }
            return;
        }
        if self
            .columns
            .is_some_and(|columns| self.column + width > columns)
        {
            self.new_line();
        }
        let style = self.style;
        self.put(
            self.column,
            Cell {
                text: character.to_string(),
                style,
            },
        );
        if width >= WIDE_CHARACTER_WIDTH {
            self.put(
                self.column + 1,
                Cell {
                    text: String::new(),
                    style,
                },
            );
        }
        self.column += width;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.new_line(),
            b'\r' => self.column = 0,
            b'\t' => {
                let next_stop = (self.column / TAB_WIDTH + 1) * TAB_WIDTH;
                self.column = self.columns.map_or(next_stop, |columns| {
                    next_stop.min(columns.saturating_sub(1))
                });
            }
            0x08 => self.column = self.column.saturating_sub(1),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        // Private sequences such as `CSI ? 25 l` carry their marker as an intermediate and change
        // nothing a picture shows.
        if ignore || !intermediates.is_empty() {
            return;
        }
        let first = || params.iter().next().map_or(0, |group| group[0]);
        match action {
            'm' => self.select_graphic_rendition(params),
            'K' => self.erase_in_line(first()),
            'J' => self.erase_in_display(first()),
            'A' => self.row = self.row.saturating_sub(count_parameter(params, 1)),
            'B' => self.row += count_parameter(params, 1),
            'C' => self.column += count_parameter(params, 1),
            'D' => self.column = self.column.saturating_sub(count_parameter(params, 1)),
            'E' => {
                self.row += count_parameter(params, 1);
                self.column = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(count_parameter(params, 1));
                self.column = 0;
            }
            'G' => self.column = count_parameter(params, 1) - 1,
            'H' | 'f' => {
                let mut groups = params.iter();
                let mut position = || match groups.next().map(|group| group[0]) {
                    None | Some(0) => 0,
                    Some(value) => usize::from(value) - 1,
                };
                self.row = position();
                self.column = position();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(screen: &Screen) -> Vec<String> {
        screen
            .lines
            .iter()
            .map(|line| line.iter().map(|cell| cell.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn splits_lines_and_drops_trailing_blank_ones() {
        let screen = Screen::parse(b"one\ntwo\r\n\n\n", None);
        assert_eq!(text(&screen), ["one", "two"]);
    }

    #[test]
    fn carriage_return_overwrites() {
        let screen = Screen::parse(b"loading 10%\rdone       \n", None);
        assert_eq!(text(&screen), ["done"]);
    }

    #[test]
    fn erase_in_line_clears_the_rest() {
        let screen = Screen::parse(b"progress 50%\r\x1b[Kfinished", None);
        assert_eq!(text(&screen), ["finished"]);
    }

    #[test]
    fn tabs_jump_to_the_next_stop() {
        let screen = Screen::parse(b"a\tb", None);
        assert_eq!(text(&screen), ["a       b"]);
    }

    #[test]
    fn basic_and_bright_colors() {
        let screen = Screen::parse(b"\x1b[31;1mr\x1b[0;94mb\x1b[42mg", None);
        let line = &screen.lines[0];
        assert_eq!(line[0].style.foreground, TermColor::Indexed(1));
        assert!(line[0].style.bold);
        assert_eq!(line[1].style.foreground, TermColor::Indexed(12));
        assert!(!line[1].style.bold);
        assert_eq!(line[2].style.background, TermColor::Indexed(2));
    }

    #[test]
    fn extended_colors_in_both_notations() {
        let screen = Screen::parse(
            b"\x1b[38;5;196ma\x1b[38;2;1;2;3mb\x1b[48:2::4:5:6mc\x1b[38:5:42md",
            None,
        );
        let line = &screen.lines[0];
        assert_eq!(line[0].style.foreground, TermColor::Indexed(196));
        assert_eq!(line[1].style.foreground, TermColor::Rgb(Rgb::new(1, 2, 3)));
        assert_eq!(line[2].style.background, TermColor::Rgb(Rgb::new(4, 5, 6)));
        assert_eq!(line[3].style.foreground, TermColor::Indexed(42));
    }

    #[test]
    fn attributes_switch_off_again() {
        let screen = Screen::parse(b"\x1b[3;4;7;9mx\x1b[23;24;27;29my", None);
        let (on, off) = (screen.lines[0][0].style, screen.lines[0][1].style);
        assert!(on.italic && on.underline && on.inverse && on.strikethrough);
        assert_eq!(off, Style::default());
    }

    #[test]
    fn wide_characters_take_two_columns() {
        let screen = Screen::parse("日本x".as_bytes(), None);
        assert_eq!(screen.width(), 5);
        assert!(screen.lines[0][1].is_continuation());
        assert_eq!(screen.lines[0][4].text, "x");
    }

    #[test]
    fn combining_marks_join_the_previous_character() {
        let screen = Screen::parse("e\u{301}x".as_bytes(), None);
        assert_eq!(text(&screen), ["e\u{301}x"]);
    }

    #[test]
    fn wraps_at_the_given_width() {
        let screen = Screen::parse(b"abcdef", Some(4));
        assert_eq!(text(&screen), ["abcd", "ef"]);
    }

    #[test]
    fn colored_trailing_spaces_stay() {
        let screen = Screen::parse(b"a\x1b[41m  \x1b[0m  ", None);
        assert_eq!(screen.width(), 3);
    }

    #[test]
    fn private_sequences_and_osc_are_dropped() {
        let screen = Screen::parse(b"\x1b[?25l\x1b]0;title\x07ok\x1b[?25h", None);
        assert_eq!(text(&screen), ["ok"]);
    }

    #[test]
    fn clear_screen_starts_over() {
        let screen = Screen::parse(b"old\n\x1b[H\x1b[2Jnew", None);
        assert_eq!(text(&screen), ["new"]);
    }
}
