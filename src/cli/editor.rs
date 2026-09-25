use std::ffi::{OsStr, OsString};
use std::process::Command;

use anyhow::{Result, bail};
use rayon::prelude::*;
use strum::{Display, EnumString, VariantNames};
use which::which;

use crate::cli::config::CONFIG;

use self::Editor::*;

#[derive(Debug, Clone, Copy, Display, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
enum Editor {
    #[strum(serialize = "code")]
    VSCode,
    #[strum(serialize = "codium")]
    VSCodium,
    #[strum(serialize = "subl")]
    SublimeText,
    Zed,
    Pulsar,
    Geany,
    #[strum(serialize = "lite-xl")]
    LiteXL,
    Bluefish,
    CudaText,
    SciTE,
    Atom,
    #[strum(serialize = "komodo-edit")]
    KomodoEdit,

    // Jetbrains IDE
    // These are just best effort check as users can have custom names
    // via Jetbrains Toolbox
    #[strum(serialize = "idea")]
    IntelliJ,
    #[strum(serialize = "pycharm")]
    PyCharm,
    #[strum(serialize = "goland")]
    GoLand,
    #[strum(serialize = "webstorm")]
    WebStorm,
    #[strum(serialize = "rider")]
    Rider,

    // Windows
    #[strum(serialize = "notepad++")]
    NotepadPlus,
    Notepad2,

    // macOS
    #[strum(serialize = "mate")]
    TextMate,
    BBEdit,
    CotEditor,
    Nova,

    // Linux
    Gedit,
    Kate,
    #[strum(serialize = "gnome-text-editor")]
    GNOMETextEditor,
    Mousepad,
    Pluma,
}

/// Attempts to open the given paths in the specified editor.
/// It's a non-blocking "fire and forget" operation.
pub fn open_in_editor(paths: &[impl AsRef<OsStr>]) -> Result<()> {
    if paths.is_empty() {
        bail!("No files/folders provided to open.");
    }

    let paths_owned: Vec<OsString> = paths.iter().map(|p| p.as_ref().to_owned()).collect();
    let editor_cmd = &CONFIG.read().unwrap().preferred_editor;

    if Editor::try_from(editor_cmd.as_str()).is_err() {
        bail!(
            "Invalid editor -> '{}'. `glosswork internal` and `open` are no longer going to work. Please edit the config files manually.",
            editor_cmd
        )
    }

    println!(
        "Opening {} file/folder(s) in {}...",
        paths_owned.len(),
        editor_cmd
    );

    let mut cmd;

    if cfg!(target_os = "windows") {
        cmd = Command::new("cmd");
        cmd.arg("/C");
        cmd.arg(&editor_cmd);
    } else {
        cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(format!("{} \"$@\"", &editor_cmd));
        cmd.arg("_");
    }

    cmd.args(&paths_owned);

    // We want it to be fire and forgot. We don't care
    // what happens after the command successfully fires.
    let cmd_result = cmd.spawn();

    if cmd_result.is_err() {
        bail!(
            "Could not execute '{}' task. Is '{}' in your system's PATH?",
            editor_cmd,
            editor_cmd
        );
    }

    Ok(())
}

/// Returns a list of known, supported GUI editor commands
/// based on the current operating system.
fn get_editor_candidates() -> Vec<Editor> {
    let mut candidates = vec![
        VSCode,
        VSCodium,
        SublimeText,
        Zed,
        Pulsar,
        Atom,
        Geany,
        LiteXL,
        CudaText,
        Bluefish,
        SciTE,
        // Common IDEs
        IntelliJ,
        PyCharm,
        GoLand,
        WebStorm,
        Rider,
        KomodoEdit,
    ];

    if cfg!(target_os = "windows") {
        candidates.extend_from_slice(&vec![NotepadPlus, Notepad2]);
    } else if cfg!(target_os = "macos") {
        candidates.extend_from_slice(&vec![TextMate, BBEdit, CotEditor, Nova]);
    } else if cfg!(target_os = "linux") {
        candidates.extend_from_slice(&vec![Gedit, Kate, GNOMETextEditor, Mousepad, Pluma]);
    }

    candidates
}

/// Scans the system for known GUI editors
/// and returns a list of found executable names.
pub fn find_editors() -> Vec<String> {
    get_editor_candidates()
        .par_iter()
        .map(|e| e.to_string())
        .filter(|e| which(e).is_ok())
        .collect()
}
