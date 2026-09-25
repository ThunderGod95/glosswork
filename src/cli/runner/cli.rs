use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use strum::{Display, EnumString, VariantArray};

use crate::cli::distribute::DistributionFormat;

#[derive(Parser, Debug, Clone)]
#[command(
    version,
    about = "Glosswork translation workbench.",
    long_about = "A translation workbench for managing source text, glossaries, translation workflows, and publication."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Name of the translation project to operate on
    #[arg(short, long, value_name = "PROJECT_NAME")]
    pub project: Option<String>,

    /// Sets the base directory where all translation projects are located
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Start the HTTP server instead of CLI.
    #[arg(long)]
    pub server: bool,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
pub enum Command {
    /// Clean all chapters for known errors.
    #[command(
        name = "clean",
        long_about = "Scans all translated chapters to detect and correct common formatting errors.
    This process standardizes punctuation, fixes whitespace inconsistencies,
    and removes known artifacts to ensure a clean text."
    )]
    Clean(CleanArgs),

    /// Compile translated chapters into distributable formats (EPUB, PDF)
    #[command(
        name = "distribute",
        long_about = "Compiles all translated chapters into final EPUB and PDF files.
This command uses Pandoc to create book-like volumes, with chapter
separation defined in the 'sep.json' configuration file."
    )]
    Distribute(DistArgs),

    /// Prepare an untranslated chapter for translation
    #[command(
        name = "glossary",
        long_about = "Prepares the next untranslated chapter for translation by:
1. Generating a micro-glossary from its content.
2. Copying the glossary, translation prompt, and chapter text to the clipboard.
3. Creating empty placeholder files for all other untranslated chapters."
    )]
    Glossary,

    /// Initialize a new translation project directory
    #[command(
        name = "init",
        long_about = "Sets up a new translation project in the current directory.
This creates the necessary directory structure (e.g., 'source',
'translated') and default configuration files to get started."
    )]
    Init(InitArgs),

    /// Edit internal application configuration files
    #[command(
        name = "internal",
        long_about = "Opens the internal application configuration files in the preferred editor for
manual editing."
    )]
    Internal,

    /// Runs the glossary command for the next untranslated chapter
    #[command(
        name = "next",
        long_about = "Automatically identifies the next missing chapter in the translation sequence.
        If the corresponding raw source file is found, it immediately triggers the
        preparation workflow (glossary generation and context setup) for that chapter."
    )]
    Next,

    /// Open project files or chapters in the preferred editor.
    #[command(
        name = "open",
        long_about = "Quickly opens specified chapter files (translated) or
the entire project root directory in the preferred editor."
    )]
    Open(OpenArgs),
}

impl Command {
    pub fn from_task(task: Task) -> Self {
        match task {
            Task::Glossary => Command::Glossary,
            Task::Distribute => Command::Distribute(DistArgs::default()),
            Task::Open => Command::Open(OpenArgs::default()),
            Task::Init => Command::Init(InitArgs::default()),
            Task::Internal => Command::Internal,
            Task::Next => Command::Next,
            Task::Clean => Command::Clean(CleanArgs::default()),
        }
    }
}

#[derive(Debug, Clone, Args, PartialEq, Eq, Default)]
pub struct DistArgs {
    /// Specify output formats. If left empty, all formats will be generated.
    /// Example: --formats epub pdf
    #[arg(short, long, value_enum)]
    pub formats: Vec<DistributionFormat>,
    /// Specify the volumes to process. If left empty, all volumes will be processed.
    /// Example: --volumes 1 2
    #[arg(short, long)]
    pub volumes: Vec<usize>,
}

#[derive(Debug, Clone, Args, PartialEq, Eq, Default)]
pub struct OpenArgs {
    pub files: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Args, PartialEq, Eq, Default)]
pub struct InitArgs {
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Args, PartialEq, Eq, Default)]
pub struct CleanArgs {
    pub file: Option<usize>,
    /// Add YAML Front Matter instead of navigation links.
    #[arg(required = false, short, long, action = clap::ArgAction::SetTrue)]
    pub yaml: bool,
    /// If specified, filenames will not be padded with 0s
    #[arg(required = false, short, long, action = clap::ArgAction::SetFalse)]
    pub pad: bool,
}

#[derive(Debug, Clone, Args, PartialEq, Eq, Default)]
pub struct NavArgs {
    pub chapter: u32,
}

#[derive(Debug, Clone, Copy, VariantArray, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum Task {
    /// The clean task
    Clean,
    /// The glossary generation task
    Glossary,
    /// The distribution task
    Distribute,
    /// The chapter opening task
    Open,
    /// The next chapter glossary task
    Next,
    /// Initializing new project task
    Init,
    /// Editing the config task
    Internal,
}
