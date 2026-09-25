use clap::Parser;
use console::style;
use std::process;

use crate::cli::util::{is_standalone, prompt_for_rerun};
use crate::cli::{check_if_first_run, run_cli_app, runner::*};

mod cli;
mod core;
mod http;

fn main() {
    #[cfg(target_os = "linux")]
    {
        use crate::cli::glossary::util::CLIPBOARD_DAEMON_ARG;
        use crate::cli::glossary::util::run_clipboard_daemon;

        if std::env::args().nth(1).as_deref() == Some(CLIPBOARD_DAEMON_ARG) {
            if let Err(e) = run_clipboard_daemon() {
                eprintln!("{}", style(format!("Clipboard daemon failed: {e}")).red());
                process::exit(1);
            }

            return;
        }
    }

    let cli = Cli::parse();

    if cli.server {
        if let Err(e) = http::launch() {
            eprintln!("{}", style(format!("HTTP server failed: {e}")).red());
            process::exit(1);
        }

        return;
    }

    if let Err(e) = check_if_first_run() {
        eprintln!(
            "{}",
            style(format!("Failed to initialize application: {}", e)).red()
        );
        process::exit(1);
    }

    let standalone = is_standalone();
    loop {
        if let Err(e) = run_cli_app(cli.clone()) {
            eprintln!("{}", style(format!("[ERROR] {}", e)).red());
            if !standalone {
                process::exit(1);
            }
        }

        if !standalone {
            break;
        }

        if !prompt_for_rerun() {
            break;
        }
    }
}
