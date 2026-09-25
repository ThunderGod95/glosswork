use std::{fs, io, path::Path};

use anyhow::{Result, bail};
use serde::Serialize;

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

            let metadata = fs::symlink_metadata(&path).ok()?;

            if metadata.is_dir()
                && visible_entry(path.file_name()?.to_str()?, &metadata)
            {
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

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ProjectEntry {
    pub path: String,
    pub kind: &'static str,
}

fn visible_entry(name: &str, metadata: &fs::Metadata) -> bool {
    if name.starts_with('.') || metadata.file_type().is_symlink() {
        return false;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_REPARSE_POINT
        if metadata.file_attributes() & (0x2 | 0x4 | 0x400) != 0 {
            return false;
        }
    }

    true
}

/// List only visible files and directories, using '/'-separated project-relative paths.
pub fn project_tree(base: &Path, project: &str) -> io::Result<Vec<ProjectEntry>> {
    if project.is_empty()
        || project.starts_with('.')
        || project.ends_with(['.', ' '])
        || project.contains(['/', '\\', ':'])
        || project.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid project name",
        ));
    }

    let root = base.join(project);
    let metadata = fs::symlink_metadata(&root)?;
    if !metadata.is_dir() || !visible_entry(project, &metadata) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Project not found"));
    }

    let mut entries = Vec::new();
    let mut pending = vec![(root, String::new())];

    while let Some((directory, prefix)) = pending.pop() {
        // Recheck directories before descending, since they can change during a scan.
        let metadata = fs::symlink_metadata(&directory)?;

        if !metadata.is_dir() || !visible_entry("", &metadata) {
            return Err(io::Error::other("Project directory changed during scan"));
        }

        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let metadata = fs::symlink_metadata(entry.path())?;
            if !visible_entry(name, &metadata) {
                continue;
            }
            let path = format!("{prefix}{name}");
            let kind = if metadata.is_dir() {
                pending.push((entry.path(), format!("{path}/")));
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                continue;
            };
            entries.push(ProjectEntry { path, kind });
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(entries)
}
