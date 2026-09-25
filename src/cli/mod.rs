use std::{path::Path, time::Instant};

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use strum::VariantArray;

use crate::cli::{
    config::{CONFIG, update_config},
    projects::select_project,
    runner::{
        Cli, Command,
        cli::Task,
        populate_arguments, run_dist_task, run_glossary_task, run_init_task, run_internal_task,
        run_open_task,
        tasks::{run_clean_task, run_next_task},
    },
};

use crate::core::projects::get_projects;

pub mod cache;
pub mod clean;
pub mod config;
pub mod distribute;
pub mod editor;
pub mod glossary;
pub mod init;
pub mod projects;
pub mod runner;
pub mod util;

pub fn run_cli_app(cli: Cli) -> Result<()> {
    let base_path = if let Some(path) = cli.path {
        &path
            .canonicalize()
            .with_context(|| format!("Failed to find projects directory at: {}", path.display()))?
    } else {
        &CONFIG.read().unwrap().base_projects_dir
    };

    let project = if let Some(project_name) = cli.project {
        let projects = get_projects(&base_path)?;
        if !projects.contains(&project_name) {
            bail!(
                "Project '{}' not found in {}",
                project_name,
                base_path.display()
            );
        }
        println!("Using project from argument: {}", project_name);
        Some(project_name)
    } else {
        None
    };

    let mut selected_task: Command = if let Some(command) = cli.command {
        command
    } else {
        let task_index = Select::with_theme(&ColorfulTheme::default())
            .items(Task::VARIANTS)
            .default(0)
            .with_prompt("Select a task to run:")
            .interact_opt()?;

        if let Some(task_index) = task_index {
            let task = Task::VARIANTS[task_index];

            Command::from_task(task)
        } else {
            return Err(anyhow!("No task selected. Exiting."));
        }
    };

    handle_task(&mut selected_task, base_path, project)?;

    Ok(())
}

pub fn handle_task(
    task: &mut Command,
    base_path: impl AsRef<Path>,
    project: Option<String>,
) -> Result<()> {
    populate_arguments(task)?;

    let time = Instant::now();

    // First run tasks that don't need project path.
    match task {
        Command::Internal => {
            println!("\nTask finished in: {:.2}s\n", time.elapsed().as_secs_f32());
            return run_internal_task();
        }
        Command::Init(args) => {
            println!("\nTask finished in: {:.2}s\n", time.elapsed().as_secs_f32());
            return run_init_task(&args, base_path.as_ref());
        }
        _ => {}
    }

    let project_name = if let Some(project) = project {
        project
    } else {
        select_project(&base_path)?
    };

    let project_path = base_path.as_ref().join(&project_name);

    match task {
        Command::Glossary => run_glossary_task(&project_name)?,
        Command::Distribute(args) => run_dist_task(&args, &project_name)?,
        Command::Open(args) => {
            run_open_task(&args, &project_path)?;
        }
        Command::Next => {
            run_next_task(&project_name, &project_path)?;
        }
        Command::Clean(args) => {
            run_clean_task(&args, &project_name)?;
        }
        Command::Internal | Command::Init(_) => {
            unreachable!("Pathless commands should have been handled by the guard match")
        }
    }

    println!("\nTask finished in: {:.2}s\n", time.elapsed().as_secs_f32());

    Ok(())
}

pub fn check_if_first_run() -> Result<()> {
    let mut cache = cache::read_cache()?.unwrap_or_default();

    if !cache.has_run_before {
        first_run_task()?
    }

    cache.has_run_before = true;

    cache::write_cache(cache)?;

    Ok(())
}

pub fn first_run_task() -> Result<()> {
    println!("It looks like this is your first time running the application.");
    println!("To get started, we just need to configure a couple of quick settings.");
    println!();

    let editors = editor::find_editors();

    if editors.len() == 0 {
        bail!(
            "No supported editors found on your system. Please install something like 'VS Code' or 'Zed'."
        );
    }

    let mut config = CONFIG.read().unwrap().clone();

    let selected_editor_index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Please select your preferred editor")
        .items(&editors)
        .default(0)
        .interact()?;

    config.preferred_editor = editors[selected_editor_index].clone();

    let projects_base_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Projects root directory")
        .default(config.base_projects_dir.display().to_string())
        .show_default(true)
        .validate_with(|input: &String| -> Result<(), &str> {
            let path = Path::new(input);

            if !path.exists() {
                return Err("This path does not exist on your system.");
            }

            if !path.is_dir() {
                return Err("This path points to a file, not a directory.");
            }

            if path
                .metadata()
                .map(|m| m.permissions().readonly())
                .unwrap_or(true)
            {
                return Err("This directory is read-only.");
            }

            Ok(())
        })
        .interact_text()?;

    config.base_projects_dir = projects_base_dir.into();

    update_config(config)?;

    println!("\nConfiguration successfully saved.\n");

    Ok(())
}
