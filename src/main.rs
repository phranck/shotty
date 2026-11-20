//! shotty turns the output of a command into a PNG or JPG image of a terminal window.

mod ansi;
mod capture;
mod cli;
mod clipboard;
mod color;
mod maintenance;
mod output;
mod render;
mod settings;
mod theme;

use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::ansi::Screen;
use crate::cli::{Cli, Command, RenderArgs};
use crate::output::Image;
use crate::settings::{ConfigFile, DEFAULT_EXECUTE_COLUMNS, Settings};

/// Timestamp format in default output file names, for example `20261120-101500`.
const FILE_TIMESTAMP_FORMAT: &str = "%Y%m%d-%H%M%S";

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shotty: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Carries out what the command line asks for.
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => render_input(cli.render),
        Some(Command::Version) => {
            maintenance::print_version();
            Ok(())
        }
        Some(Command::Update) => maintenance::update(),
        Some(Command::Uninstall { yes }) => maintenance::uninstall(yes),
        Some(Command::Completions { shell }) => {
            maintenance::print_completions(shell);
            Ok(())
        }
        Some(Command::Manpages { directory }) => maintenance::write_manpages(&directory),
    }
}

/// Renders standard input or the output of `--execute` to an image and prints the image's path.
///
/// # Errors
///
/// Fails when there is nothing to read, when the output is empty, and on any error while reading
/// settings, running the command, rendering, writing the file or copying to the clipboard.
fn render_input(args: RenderArgs) -> Result<()> {
    let config = match ConfigFile::path() {
        Some(path) => ConfigFile::load(&path)?,
        None => ConfigFile::default(),
    };
    let now = jiff::ZonedDateTime::now()
        .strftime(FILE_TIMESTAMP_FORMAT)
        .to_string();
    let command = args.execute.clone();
    let copy_to_clipboard = args.clipboard;
    let settings = Settings::resolve(args, config, &now)?;

    let input = match &command {
        Some(command) => run_command(command, settings.columns.unwrap_or(DEFAULT_EXECUTE_COLUMNS))?,
        None => read_stdin()?,
    };
    let screen = Screen::parse(&input, settings.columns.map(usize::from));
    if screen.lines.is_empty() {
        bail!("the input is empty, so there is nothing to render");
    }

    let image = Image::from(render::render(&screen, &settings)?);
    output::write(&image, settings.format, &settings.output)?;
    if copy_to_clipboard {
        clipboard::copy(&image)?;
    }
    println!("{}", settings.output.display());
    Ok(())
}

/// Reads all of standard input.
///
/// # Errors
///
/// Fails when standard input is a terminal, since then nobody is piping anything in, or when
/// reading fails.
fn read_stdin() -> Result<Vec<u8>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        bail!(
            "nothing to render; pipe a command's output into shotty (ls --color=always | shotty) or use --execute"
        );
    }
    let mut input = Vec::new();
    stdin
        .lock()
        .read_to_end(&mut input)
        .context("cannot read standard input")?;
    Ok(input)
}

/// Runs `command` in a terminal `columns` wide and returns its output. A command that fails is
/// still rendered, because its error output is often what the picture is for; shotty says so on
/// standard error.
///
/// # Errors
///
/// Fails when the command cannot be started or its output cannot be read.
fn run_command(command: &str, columns: u16) -> Result<Vec<u8>> {
    let capture = capture::run(command, columns)?;
    if let Some(failure) = capture.failure {
        eprintln!("shotty: `{command}` {failure}");
    }
    Ok(capture.output)
}
