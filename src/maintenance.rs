//! Updating and removing shotty, and writing the files packages ship besides the binary.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use clap_complete::aot::{Shell, generate};

use crate::cli::Cli;
use crate::settings;

/// Name of the program, the formula and the binary.
const NAME: &str = "shotty";

/// Fully qualified formula name, so `brew` finds it even when another tap has a `shotty`.
const TAP_FORMULA: &str = "phranck/tap/shotty";

/// Directory below a Homebrew prefix that holds installed formulae.
const CELLAR: &str = "Cellar";

/// Answer that confirms the uninstall prompt; anything else cancels.
const CONFIRMATION: &str = "y";

/// How shotty got onto this computer.
#[derive(Debug, PartialEq, Eq)]
enum Installation {
    /// Installed by Homebrew; `brew` is the Homebrew executable of that installation.
    Homebrew { brew: PathBuf },
    /// Copied somewhere by hand or built from source; `executable` is the running binary.
    Standalone { executable: PathBuf },
}

impl Installation {
    /// Finds out how the running binary was installed.
    ///
    /// # Errors
    ///
    /// Fails when the path of the running binary cannot be determined.
    fn detect() -> Result<Self> {
        let executable = std::env::current_exe()
            .and_then(fs::canonicalize)
            .context("cannot find the shotty executable")?;
        Ok(match homebrew_prefix(&executable) {
            Some(prefix) => Self::Homebrew {
                brew: prefix.join("bin").join("brew"),
            },
            None => Self::Standalone { executable },
        })
    }
}

/// Returns the Homebrew prefix when `executable` lies in a Homebrew Cellar, as in
/// `/opt/homebrew/Cellar/shotty/0.1.0/bin/shotty`, and `None` otherwise.
fn homebrew_prefix(executable: &Path) -> Option<PathBuf> {
    let components: Vec<_> = executable.components().collect();
    let cellar = components
        .windows(2)
        .position(|pair| pair[0].as_os_str() == CELLAR && pair[1].as_os_str() == NAME)?;
    Some(components[..cellar].iter().collect())
}

/// Prints the version.
pub fn print_version() {
    println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
}

/// Updates shotty through Homebrew.
///
/// Runs `brew update` first, because Homebrew otherwise may skip refreshing the tap for up to a
/// day and miss a new release.
///
/// # Errors
///
/// Fails when shotty was not installed with Homebrew or when `brew` fails.
pub fn update() -> Result<()> {
    let brew = match Installation::detect()? {
        Installation::Homebrew { brew } => brew,
        Installation::Standalone { executable } => {
            bail!(
                "{} was not installed with Homebrew; update it the way it was installed, or install it with: brew install {TAP_FORMULA}",
                executable.display()
            )
        }
    };
    run_brew(&brew, &["update", "--quiet"])?;
    run_brew(&brew, &["upgrade", TAP_FORMULA])
}

/// Removes shotty and everything it put on this computer.
///
/// With Homebrew that is the formula, which takes the binary, the man pages and the shell
/// completions with it, and Homebrew's download cache for it. Without Homebrew it is the running
/// binary. In both cases the config directory goes as well. The tap stays, since other formulae
/// may come from it.
///
/// # Errors
///
/// Fails when confirmation is needed but cannot be asked for, and when removing fails.
pub fn uninstall(assume_yes: bool) -> Result<()> {
    let installation = Installation::detect()?;
    let config = settings::config_directory().filter(|directory| directory.exists());

    eprintln!("This removes:");
    match &installation {
        Installation::Homebrew { .. } => eprintln!(
            "  the Homebrew formula {TAP_FORMULA} with its binary, man pages and shell completions"
        ),
        Installation::Standalone { executable } => eprintln!("  {}", executable.display()),
    }
    if let Some(config) = &config {
        eprintln!("  {} (configuration)", config.display());
    }
    if !assume_yes && !confirm()? {
        eprintln!("Nothing was removed.");
        return Ok(());
    }

    match &installation {
        Installation::Homebrew { brew } => {
            run_brew(brew, &["uninstall", TAP_FORMULA])?;
            // The cache only holds downloads; a failure to clear it leaves shotty removed.
            if let Err(error) = run_brew(brew, &["cleanup", "--prune=all", NAME]) {
                eprintln!("shotty: {error:#}");
            }
        }
        Installation::Standalone { executable } => fs::remove_file(executable)
            .with_context(|| format!("cannot remove {}", executable.display()))?,
    }
    if let Some(config) = &config {
        fs::remove_dir_all(config)
            .with_context(|| format!("cannot remove {}", config.display()))?;
    }
    eprintln!("shotty is uninstalled.");
    Ok(())
}

/// Asks on the terminal whether to go ahead.
///
/// # Errors
///
/// Fails when standard input is not a terminal, because then nobody can answer.
fn confirm() -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "cannot ask for confirmation without a terminal; run `shotty uninstall --yes` to remove without asking"
        );
    }
    eprint!("Remove shotty? [{CONFIRMATION}/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .context("cannot read the answer")?;
    Ok(answer.trim().eq_ignore_ascii_case(CONFIRMATION))
}

/// Runs `brew` with `args`, its output going straight to the terminal.
///
/// # Errors
///
/// Fails when `brew` cannot start or exits with an error.
fn run_brew(brew: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(brew)
        .args(args)
        .status()
        .with_context(|| format!("cannot run {}", brew.display()))?;
    if !status.success() {
        bail!("`brew {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

/// Prints the completion script for `shell` to standard output.
pub fn print_completions(shell: Shell) {
    generate(shell, &mut Cli::command(), NAME, &mut io::stdout());
}

/// Writes `shotty.1` and one page per visible subcommand into `directory`.
///
/// # Errors
///
/// Fails when the directory does not exist or a page cannot be written.
pub fn write_manpages(directory: &Path) -> Result<()> {
    clap_mangen::generate_to(Cli::command(), directory)
        .with_context(|| format!("cannot write the man pages into {}", directory.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_prefix_of_a_cellar_binary() {
        assert_eq!(
            homebrew_prefix(Path::new("/opt/homebrew/Cellar/shotty/0.1.0/bin/shotty")),
            Some(PathBuf::from("/opt/homebrew"))
        );
        assert_eq!(
            homebrew_prefix(Path::new(
                "/home/linuxbrew/.linuxbrew/Cellar/shotty/1.2.3/bin/shotty"
            )),
            Some(PathBuf::from("/home/linuxbrew/.linuxbrew"))
        );
    }

    #[test]
    fn other_locations_are_standalone() {
        assert_eq!(homebrew_prefix(Path::new("/usr/local/bin/shotty")), None);
        assert_eq!(
            homebrew_prefix(Path::new("/opt/homebrew/Cellar/other/1.0/bin/shotty")),
            None
        );
    }

    #[test]
    fn writes_a_page_per_visible_command() {
        let directory =
            std::env::temp_dir().join(format!("shotty-manpages-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        write_manpages(&directory).unwrap();
        let pages: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        fs::remove_dir_all(&directory).unwrap();
        assert!(pages.iter().any(|page| page == "shotty.1"), "{pages:?}");
        assert!(
            pages.iter().any(|page| page == "shotty-uninstall.1"),
            "{pages:?}"
        );
        assert!(
            !pages.iter().any(|page| page.contains("manpages")),
            "the hidden command has no page: {pages:?}"
        );
    }
}
