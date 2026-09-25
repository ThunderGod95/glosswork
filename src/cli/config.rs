use anyhow::{Context, Result};
use console::style;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

use crate::cli::util::get_config_file_path;

pub static PROJECT_PATH_QUALIFIERS: [&str; 3] = ["com", "tg", "glosswork"];

pub static CONFIG: LazyLock<RwLock<AppConfig>> =
    LazyLock::new(|| RwLock::new(load_config().expect("Failed to load configuration")));

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    // -------------------------------------------------------------------------
    // General Configuration
    // -------------------------------------------------------------------------
    /// The command name of the preferred text editor.
    pub preferred_editor: String,

    /// The root directory containing all project workspaces.
    pub base_projects_dir: PathBuf,

    /// The directory where final, translated chapters are stored.
    pub translations_folder: PathBuf,

    /// The root directory for project assets.
    ///
    /// This folder serves as the base path for relative configuration files
    /// such as the glossary, prompts, and separation info.
    pub assets_folder: PathBuf,

    /// The output directory where compiled books (EPUB/PDF) are generated.
    pub dist_folder: PathBuf,

    /// The directory containing the source raw (untranslated) chapters.
    pub raws_folder: PathBuf,

    /// The path to the main application cache file.
    pub cache_file: PathBuf,

    /// The sensitivity threshold for fuzzy search operations in the glossary.
    ///
    /// A higher value typically implies a laxer match requirement.
    pub fuzzy_search_threshold: u32,

    // -------------------------------------------------------------------------
    // Asset Paths (Relative to `assets_folder`)
    // -------------------------------------------------------------------------
    /// The path to the project's glossary file.
    ///
    /// This path is relative to the `assets_folder`.
    pub glossary_file: PathBuf,

    /// The path to the file tracking the next raw chapter to be translated.
    ///
    /// This path is relative to the `assets_folder`.
    pub chapter_file: PathBuf,

    /// The path to the file containing the LLM/AI translation prompt.
    ///
    /// This is configurable per project to allow for novel-specific context.
    /// This path is relative to the `assets_folder`.
    pub translation_prompt_file: PathBuf,

    /// The path to the configuration file defining book compilation logic.
    ///
    /// This file controls how individual chapters are merged into the final volume.
    /// This path is relative to the `assets_folder`.
    pub sep_info_file: PathBuf,
}

#[derive(Debug)]
pub struct ProjectPaths {
    /// The directory where final, translated chapters are stored.
    pub translations_folder: PathBuf,

    /// The root directory for project assets.
    ///
    /// This folder serves as the base path for relative configuration files
    /// such as the glossary, prompts, and separation info.
    pub assets_folder: PathBuf,

    /// The output directory where compiled books (EPUB/PDF) are generated.
    pub dist_folder: PathBuf,

    /// The directory containing the source raw (untranslated) chapters.
    pub raws_folder: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        let base_dirs = BaseDirs::new().unwrap();
        let user_dir = base_dirs.home_dir();
        let projects_dir = user_dir.join("Translations");

        Self {
            preferred_editor: "zed".into(),
            base_projects_dir: projects_dir,
            glossary_file: "glossary.json".into(),
            chapter_file: "cr_ch.txt".into(),
            translation_prompt_file: "translation_prompt.md".into(),
            sep_info_file: "sep.json".into(),
            cache_file: ".runner-cache.json".into(),
            fuzzy_search_threshold: 2,
            translations_folder: "translations".into(),
            assets_folder: "assets".into(),
            dist_folder: "dist".into(),
            raws_folder: "raws".into(),
        }
    }
}

impl ProjectPaths {
    pub fn new(project: &str) -> Self {
        CONFIG.read().unwrap().get_paths_for_project(project)
    }
}

impl AppConfig {
    pub fn get_translations_dir(&self, project: &str) -> PathBuf {
        self.base_projects_dir
            .join(project)
            .join(&self.translations_folder)
    }

    pub fn get_assets_dir(&self, project: &str) -> PathBuf {
        self.base_projects_dir
            .join(project)
            .join(&self.assets_folder)
    }

    pub fn get_dist_dir(&self, project: &str) -> PathBuf {
        self.base_projects_dir.join(project).join(&self.dist_folder)
    }

    pub fn get_raws_dir(&self, project: &str) -> PathBuf {
        self.base_projects_dir.join(project).join(&self.raws_folder)
    }

    pub fn get_paths_for_project(&self, project: &str) -> ProjectPaths {
        ProjectPaths {
            translations_folder: self.get_translations_dir(&project),
            assets_folder: self.get_assets_dir(&project),
            dist_folder: self.get_dist_dir(&project),
            raws_folder: self.get_raws_dir(&project),
        }
    }
}

/// Updates the global configuration in memory AND writes it to disk.
pub fn update_config(new_config: AppConfig) -> Result<()> {
    {
        let mut w = CONFIG
            .write()
            .map_err(|e| anyhow::anyhow!("Config lock poisoned: {}", e))?;
        *w = new_config.clone();
    }

    write_config_to_disk(&new_config)?;

    Ok(())
}

/// Helper to write a specific config instance to disk.
fn write_config_to_disk(config: &AppConfig) -> Result<()> {
    let config_path = get_config_file_path()?;
    let config_str = toml::to_string_pretty(config)?;

    if let Some(parent_dir) = config_path.parent() {
        if !parent_dir.exists() {
            fs::create_dir_all(parent_dir).context(format!(
                "Failed to create config directory at {:?}",
                parent_dir
            ))?;
        }
    }

    fs::write(&config_path, config_str)
        .context(format!("Failed to write config file to {:?}", config_path))?;

    Ok(())
}

fn load_config() -> Result<AppConfig> {
    let config_path = get_config_file_path()?;

    match fs::read_to_string(&config_path) {
        Ok(config_str) => {
            let config: AppConfig = toml::from_str(&config_str)
                .context(format!("Failed to parse config file at {:?}", config_path))?;

            // Check for schema updates/migrations
            let current_schema_str = toml::to_string_pretty(&config)?;
            if config_str.trim() != current_schema_str.trim() {
                println!(
                    "{}",
                    style(format!(
                        "[WARN] Config file at {:?} is outdated. Updating...",
                        config_path
                    ))
                    .yellow()
                );
                write_config_to_disk(&config)?;
            }

            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{}",
                style(format!(
                    "[WARN] Config file not found, creating default at: {:?}",
                    config_path
                ))
                .yellow()
            );

            let config = AppConfig::default();
            write_config_to_disk(&config)?;

            Ok(config)
        }
        Err(e) => Err(e.into()),
    }
}
