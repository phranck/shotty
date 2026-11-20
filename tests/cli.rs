//! Runs the built binary the way a user does: output piped in, an image file out.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// File signatures from the PNG and JPEG specifications.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_SIGNATURE: &[u8] = &[0xff, 0xd8, 0xff];

/// A directory for one test that is removed again when the test ends, passed or failed.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("shotty-test-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs shotty in `directory` with `input` on standard input. The config home points into
/// `directory`, so a config file on the machine running the tests has no effect.
fn run(directory: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shotty"))
        .args(args)
        .current_dir(directory)
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn writes_png_and_jpg_and_prints_the_path() {
    let scratch = Scratch::new("formats");
    for (name, signature) in [("out.png", PNG_SIGNATURE), ("out.jpg", JPEG_SIGNATURE)] {
        let output = run(scratch.path(), &["-o", name], b"\x1b[32mhello\x1b[0m\n");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), name);
        assert!(
            fs::read(scratch.path().join(name))
                .unwrap()
                .starts_with(signature)
        );
    }
}

#[test]
fn default_name_carries_a_timestamp() {
    let scratch = Scratch::new("default-name");
    let output = run(scratch.path(), &["--format", "jpg"], b"x");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        printed.starts_with("shotty-") && printed.ends_with(".jpg"),
        "{printed}"
    );
    assert!(scratch.path().join(&printed).is_file());
}

#[test]
fn empty_input_fails_with_a_message() {
    let scratch = Scratch::new("empty");
    let output = run(scratch.path(), &["-o", "out.png"], b"\n\n");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("empty"));
    assert!(!scratch.path().join("out.png").exists());
}

#[test]
fn config_file_supplies_defaults() {
    let scratch = Scratch::new("config");
    let config = scratch.path().join("config/shotty");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), "format = \"jpg\"\n").unwrap();
    let output = run(scratch.path(), &[], b"x");
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .ends_with(".jpg")
    );

    fs::write(config.join("config.toml"), "colour = \"red\"\n").unwrap();
    let output = run(scratch.path(), &[], b"x");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.toml"));
}
