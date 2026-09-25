use std::{fs, path::Path};

use anyhow::{Result, bail};

pub fn get_projects(base_path: &Path) -> Result<Vec<String>> {
    if !base_path.is_dir() {
        bail!(
            "Failed to read projects: {} is not a valid directory",
            base_path.display()
        );
    }

    let mut projects: Vec<String> = fs::read_dir(base_path)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();

            if path.is_dir() && path.file_name()?.to_str()? != "tscripts" {
                return Some(path.file_name()?.to_str()?.to_string());
            }
            None
        })
        .collect();

    projects.sort();

    if projects.is_empty() {
        bail!("No projects found in: {}", base_path.display());
    }

    Ok(projects)
}
