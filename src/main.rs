//! shotty turns the output of a command into a PNG or JPG image of a terminal window.

mod ansi;
mod cli;
mod color;
mod output;
mod render;
mod settings;
mod theme;

use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::ansi::Screen;
use crate::cli::{Cli, RenderArgs};
use crate::settings::{ConfigFile, Settings};

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
    render_input(cli.render)
}

/// Renders standard input to an image and prints the image's path.
///
/// # Errors
///
/// Fails when standard input is a terminal rather than a pipe, when it is empty, and on any error
/// while reading settings, rendering or writing the file.
fn render_input(args: RenderArgs) -> Result<()> {
    let config = match ConfigFile::path() {
        Some(path) => ConfigFile::load(&path)?,
        None => ConfigFile::default(),
    };
    let now = jiff::ZonedDateTime::now()
        .strftime(FILE_TIMESTAMP_FORMAT)
        .to_string();
    let settings = Settings::resolve(args, config, &now)?;

    let stdin = io::stdin();
    if stdin.is_terminal() {
        bail!(
            "nothing to render; pipe a command's output into shotty, for example: ls --color=always | shotty"
        );
    }
    let mut input = Vec::new();
    stdin
        .lock()
        .read_to_end(&mut input)
        .context("cannot read standard input")?;

    let screen = Screen::parse(&input, None);
    if screen.lines.is_empty() {
        bail!("the input is empty, so there is nothing to render");
    }
    let image = render::render(&screen, &settings)?;
    output::write(image, settings.format, &settings.output)?;
    println!("{}", settings.output.display());
    Ok(())
}
