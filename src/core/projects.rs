use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use sanitize_filename::{OptionsForCheck, is_sanitized_with_options};
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

            if metadata.is_dir() && visible_entry(path.file_name()?.to_str()?, &metadata) {
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
    let root = project_root(base, project)?;

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

fn validate_name(name: &str) -> io::Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();

    let valid = !name.is_empty()
        && !name.starts_with('.')
        && is_sanitized_with_options(
            name,
            OptionsForCheck {
                windows: true,
                truncate: false,
            },
        )
        && name.chars().all(|c| !c.is_control())
        && !matches!(stem.as_str(), "CONIN$" | "CONOUT$")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|suffix| matches!(suffix, "¹" | "²" | "³"))
        });

    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid path component",
        ))
    }
}

fn project_root(base: &Path, project: &str) -> io::Result<PathBuf> {
    validate_name(project)?;

    let root = base.join(project);
    let metadata = fs::symlink_metadata(&root)?;

    if !metadata.is_dir() || !visible_entry(project, &metadata) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Project not found"));
    }

    Ok(root)
}

fn project_path(base: &Path, project: &str, relative: &str, new: bool) -> io::Result<PathBuf> {
    let mut path = project_root(base, project)?;
    let parts: Vec<_> = relative.split('/').collect();

    for (index, name) in parts.iter().enumerate() {
        validate_name(name)?;

        path.push(name);

        let last = index + 1 == parts.len();

        match fs::symlink_metadata(&path) {
            Err(error) if new && last && error.kind() == io::ErrorKind::NotFound => {}

            Err(error) => return Err(error),

            Ok(metadata) => {
                if !visible_entry(name, &metadata) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Path is not accessible",
                    ));
                }

                if new && last {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "Destination exists",
                    ));
                }

                if (!last && !metadata.is_dir())
                    || (last && !metadata.is_dir() && !metadata.is_file())
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid entry type",
                    ));
                }
            }
        }
    }

    Ok(path)
}

pub fn read_project_file(base: &Path, project: &str, path: &str) -> io::Result<String> {
    let path = project_path(base, project, path, false)?;
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected a file",
        ));
    }
    fs::read_to_string(path)
}

pub enum FileOperation {
    Write(String),
    CreateFile(String),
    CreateDirectory,
    Move(String),
}

pub fn modify_project_file(
    base: &Path,
    project: &str,
    path: &str,
    operation: FileOperation,
) -> io::Result<()> {
    static MUTATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let _guard = MUTATIONS
        .lock()
        .map_err(|_| io::Error::other("Mutation lock poisoned"))?;

    let new = matches!(
        operation,
        FileOperation::CreateFile(_) | FileOperation::CreateDirectory
    );

    let source = project_path(base, project, path, new)?;

    match operation {
        FileOperation::CreateDirectory => fs::create_dir(source),

        FileOperation::CreateFile(content) => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(source)?
            .write_all(content.as_bytes()),

        FileOperation::Write(content) => {
            if !source.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Expected a file",
                ));
            }

            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(source)?
                .write_all(content.as_bytes())
        }

        FileOperation::Move(destination) => {
            let destination = project_path(base, project, &destination, true)?;

            if destination.starts_with(&source) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Cannot move into itself",
                ));
            }

            fs::rename(source, destination)
        }
    }
}
