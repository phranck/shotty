//! Runs a command in a pseudo-terminal and collects what it prints.
//!
//! Many programs check whether they write to a terminal and switch colors, columns or icons off
//! when they do not. Running them in a pseudo-terminal makes them print what a user sees.

use std::io::Read;
use std::thread;

use anyhow::{Context, Result, anyhow};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// Shell that interprets the command, so pipes, quotes and variables work as typed.
const SHELL: &str = "sh";

/// Flag that makes `SHELL` run the following argument as a script.
const SHELL_COMMAND_FLAG: &str = "-c";

/// Script the shell runs. It points standard input at `/dev/null`, so a program that reads input
/// ends instead of waiting, and then runs the user's command, passed as `$1`, exactly as typed.
/// Closing the terminal's input instead would make the terminal echo `^D` into the output.
const SHELL_SCRIPT: &str = "exec </dev/null\neval \"$1\"";

/// Value for `$0` in `SHELL_SCRIPT`, which shells show in their error messages.
const SCRIPT_NAME: &str = "shotty";

/// Number of lines the pseudo-terminal reports. Output longer than this scrolls in a real
/// terminal; shotty keeps every line, so the value only matters to programs that fill the screen.
const TERMINAL_ROWS: u16 = 50;

/// Environment for the command. `TERM` announces a terminal with 256 colors, `COLORTERM` adds true
/// color, and the pager variables stop programs such as `git log` from waiting in a pager.
const ENVIRONMENT: [(&str, &str); 5] = [
    ("TERM", "xterm-256color"),
    ("COLORTERM", "truecolor"),
    ("PAGER", "cat"),
    ("GIT_PAGER", "cat"),
    ("MANPAGER", "cat"),
];

/// What a command printed and how it ended.
pub struct Capture {
    /// Everything written to the terminal, standard output and standard error interleaved.
    pub output: Vec<u8>,
    /// `None` when the command succeeded, otherwise a description of how it failed.
    pub failure: Option<String>,
}

/// Runs `command` through `sh` in a pseudo-terminal `columns` wide, in the current directory.
///
/// The command's standard input is `/dev/null`, so a program waiting for input ends instead of
/// hanging.
///
/// # Errors
///
/// Fails when the pseudo-terminal cannot be created, the shell cannot start, or reading fails.
/// A command that runs and exits with an error is not an error here; see `Capture::failure`.
pub fn run(command: &str, columns: u16) -> Result<Capture> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: TERMINAL_ROWS,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| anyhow!("cannot open a pseudo-terminal: {error}"))?;

    let mut builder = CommandBuilder::new(SHELL);
    for arg in [SHELL_COMMAND_FLAG, SHELL_SCRIPT, SCRIPT_NAME, command] {
        builder.arg(arg);
    }
    builder.cwd(std::env::current_dir().context("cannot read the current directory")?);
    for (key, value) in ENVIRONMENT {
        builder.env(key, value);
    }

    let mut child = pair
        .slave
        .spawn_command(builder)
        .map_err(|error| anyhow!("cannot start `{command}`: {error}"))?;
    // Reading reaches the end only once every handle to the terminal's other side is closed.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| anyhow!("cannot read from the pseudo-terminal: {error}"))?;
    // The reader runs on its own thread so a command with much output never blocks on a full
    // terminal buffer while this thread waits for it to exit.
    let collector = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).map(|_| output)
    });

    let status = child.wait().context("cannot wait for the command")?;
    let output = collector
        .join()
        .map_err(|_| anyhow!("the output reader stopped unexpectedly"))?
        .context("cannot read the command's output")?;
    drop(pair.master);

    let failure = (!status.success()).then(|| match status.signal() {
        Some(signal) => format!("was ended by {signal}"),
        None => format!("exited with status {}", status.exit_code()),
    });
    Ok(Capture { output, failure })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_arrives_with_terminal_line_endings() {
        let capture = run("printf 'a\\nb\\n'", 80).unwrap();
        assert_eq!(capture.output, b"a\r\nb\r\n");
        assert!(capture.failure.is_none());
    }

    #[test]
    fn the_command_sees_a_terminal_of_the_given_width() {
        let capture = run("test -t 1 && tput cols", 57).unwrap();
        assert_eq!(String::from_utf8_lossy(&capture.output).trim(), "57");
    }

    #[test]
    fn failing_commands_report_their_status() {
        let capture = run("echo oops; exit 3", 80).unwrap();
        assert_eq!(capture.failure.as_deref(), Some("exited with status 3"));
        assert_eq!(capture.output, b"oops\r\n");
    }

    #[test]
    fn input_is_empty_so_reading_programs_end() {
        let capture = run("cat; echo done", 80).unwrap();
        assert!(String::from_utf8_lossy(&capture.output).contains("done"));
    }
}
