//! Copies the rendered image to the system clipboard.
//!
//! macOS keeps clipboard contents after the program that set them exits, so shotty writes there
//! directly. On Linux the program that sets the clipboard has to keep serving it, so shotty hands
//! the image to `wl-copy` under Wayland or `xclip` under X11, which stay in the background for
//! that.

use anyhow::Result;

use crate::output::Image;

/// Puts `image` on the clipboard as a picture.
///
/// # Errors
///
/// Fails when the clipboard cannot be reached or, on Linux, when the helper program is missing.
#[cfg(target_os = "macos")]
pub fn copy(image: &Image) -> Result<()> {
    use std::borrow::Cow;

    use anyhow::Context;

    let data = arboard::ImageData {
        width: image.width as usize,
        height: image.height as usize,
        bytes: Cow::Borrowed(&image.rgba),
    };
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_image(data))
        .context("cannot copy the image to the clipboard")
}

/// Puts `image` on the clipboard as a picture.
///
/// # Errors
///
/// Fails when the clipboard cannot be reached or, on Linux, when the helper program is missing.
#[cfg(not(target_os = "macos"))]
pub fn copy(image: &Image) -> Result<()> {
    use std::io::{ErrorKind, Write};
    use std::process::{Command, Stdio};

    use anyhow::{Context, bail};

    use crate::cli::ImageFormat;
    use crate::output;

    /// MIME type the clipboard entry is offered as.
    const PNG_MIME_TYPE: &str = "image/png";

    let (program, args): (&str, Vec<&str>) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-copy", vec!["--type", PNG_MIME_TYPE])
    } else if std::env::var_os("DISPLAY").is_some() {
        (
            "xclip",
            vec!["-selection", "clipboard", "-target", PNG_MIME_TYPE, "-in"],
        )
    } else {
        bail!(
            "no graphical session found (neither WAYLAND_DISPLAY nor DISPLAY is set), so there is no clipboard"
        );
    };

    let mut png = Vec::new();
    output::encode(image, ImageFormat::Png, &mut png)?;

    // The helper's output streams are closed so its background process does not hold shotty's
    // terminal open after shotty exits.
    let mut child = match Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("--clipboard needs `{program}`; install it with your package manager")
        }
        Err(error) => return Err(error).with_context(|| format!("cannot start `{program}`")),
    };
    child
        .stdin
        .take()
        .context("no input to the clipboard helper")?
        .write_all(&png)
        .with_context(|| format!("cannot pass the image to `{program}`"))?;
    let status = child
        .wait()
        .with_context(|| format!("cannot wait for `{program}`"))?;
    if !status.success() {
        bail!("`{program}` could not set the clipboard ({status})");
    }
    Ok(())
}
