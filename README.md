# Glosswork

A command-line tool for managing translation projects. It assists with translation workflows, including glossary generation, content searching, formatting cleanup, and final distribution compilation.

## First-Run Setup

On the first execution, `glosswork` will run an interactive setup wizard to configure the following:
1.  **Preferred Editor**: Scans for installed editors (e.g., VS Code, Zed).
2.  **Projects Root Directory**: The base directory where all project folders will reside. 

These settings are saved locally and apply to all future runs.

## Environment Setup & Aliasing

To use the binary globally from your terminal without typing the full path or full name (`glosswork`), add it to your system's `PATH` and create a shorter alias (e.g., `gw`).

### Windows (PowerShell)

1. Move `glosswork.exe` to a permanent folder (e.g., `C:\Program Files\Glosswork`).
2. Add this folder to your Environment Variables:
    - Open the Start Menu, type "Environment Variables", and select "Edit the system environment variables".
    - Click "Environment Variables" > Select "Path" > "Edit" > "New" and paste the folder path.
3. Open your PowerShell profile (run `notepad $PROFILE` in PowerShell). If it doesn't exist, create it.
4. Add the following function to act as an alias:

```powershell
function tm {
    & "YOUR_PATH\glosswork.exe" $args
}
```
5. Restart PowerShell.

## Usage

The binary can be used in two modes:

1. **Interactive Mode:** Running the command without subcommands opens a dialoguer menu to select tasks interactively.
2. **CLI Mode:** Pass subcommands and arguments directly for scripting or fast execution.

### Global Arguments

These arguments can be applied to the base command before specifying a task.

- `-p, --project <PROJECT_NAME>`: Specifies the name of the translation project to operate on.
- `--path <PATH>`: Temporarily overrides the base directory where translation projects are located.

### Commands

#### `init`

Initializes a new translation project directory structure.

```powershell
gw init [PROJECT_NAME]
```

#### `glossary`

This is the core command of the application, automating several steps in the LLM-assisted translation workflow:

- Reads the raw chapter content from assets/cr_ch.txt and automatically determines the next chapter number based on your translations/ directory. If provided with a large block of text, it can split multiple chapters at once by detecting 第...章 headings.
- Reads your glossary located in assets/glossary.json and scans the raw chapter text using a two-pass system: an exact match search followed by a fuzzy search to catch typos and variations.
- Creates a micro-glossary optimized for the LLM workflow containing only the terms found in the current text. It then copies the translation prompt from assets/translation_prompt.md, the micro-glossary, and the raw chapter text directly to your clipboard.
- Creates a newly numbered file in translations/ and opens it in your preferred editor. It also creates a corresponding file in raws/ with the raw chapter content.

You can now easily paste this prompt in your chat.

Since you'll use this command the most I recommend creating an alias for this command:

```powershell
function gwg {
    & "YOUR_PATH\translations_manager.exe" glossary
}
```

#### `next`

A wrapper over the glossary command intended for sequential processing.

Rather than copying and pasting chapters one by one into assets/cr_ch.txt, running next forces the application to automatically read the next chronological chapter file from the raws/ folder.

#### `clean`

Scans all translated chapters to detect and correct common formatting errors. This standardizes punctuation, fixes whitespace inconsistencies, and removes known artifacts.

##### Arguments

- `-y, --yaml`: Adds YAML Front Matter instead of standard navigation links.
- `-p, --pad`: Disables padding filenames with 0s (default behavior pads filenames).

#### `distribute`

Compiles translated chapters into distributable formats like EPUB, PDF, or DOCX utilizing Pandoc.

#### Arguments

- `-f, --formats <FORMATS>`: Specify output formats. If left empty, all formats will be generated. Example: --formats epub pdf [possible values: epub, pdf, txt, docx]
- `-v, --volumes <VOLUMES>`: Specify the volumes to process. If left empty, all volumes will be processed. Example: --volumes 1 2

#### Example

```powershell
gw distribute -f pdf -f epub -v 8
```

_This command creates PDFs and EPUBs for Volume 8._

This command retrieves volume information from the sep.json file located in the `assets/` folder. Example reference: [TheMirrorLegacy](https://github.com/ThunderGod95/TheMirrorLegacy/blob/main/assets/sep.json)

#### `open` 

A convenience command to quickly open a project or a specific chapter from the terminal.

##### Examples

```powershell
gw open 717
```

Opens Chapter 717 from the currently selected project.

```powershell
gw open
```

Running the command without specifying a chapter will open the entire project folder.

#### `internal`

Opens the internal settings file of the application. The configuration options within this file are self-explanatory.
