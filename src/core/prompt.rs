use anyhow::Result;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use super::glossary::{GlossaryEntry, GlossaryOptions, create_micro_glossary_with_options};

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PreparePromptRequest {
    /// Chinese chapter text; must contain non-whitespace characters.
    #[schema(example = "李白走进了房间。", min_length = 1)]
    pub chapter: String,
    pub glossary: Vec<GlossaryEntry>,
    /// Instructions prepended to the glossary and chapter; must not be blank.
    #[schema(
        example = "Translate the following chapter into English.",
        min_length = 1
    )]
    pub translation_prompt: String,

    #[serde(default, rename = "options")]
    pub glossary_options: GlossaryOptions,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PreparePromptResult {
    /// Complete prompt containing instructions, formatted glossary, and chapter.
    pub prompt: String,
    pub micro_glossary: Vec<GlossaryEntry>,
}

pub fn prepare_translation_prompt(request: PreparePromptRequest) -> Result<PreparePromptResult> {
    let micro_glossary = create_micro_glossary_with_options(
        &request.chapter,
        &request.glossary,
        request.glossary_options,
    )?;

    let formatted_glossary = format_micro_glossary(&micro_glossary);

    let prompt = build_translation_prompt(
        &request.translation_prompt,
        &formatted_glossary,
        &request.chapter,
    );

    Ok(PreparePromptResult {
        prompt,
        micro_glossary,
    })
}

/// Formats structured glossary entries for the translation prompt.
pub fn format_micro_glossary(found_entries: &[GlossaryEntry]) -> String {
    found_entries
        .iter()
        .map(|entry| {
            let mut details = vec![entry._type.as_str()];

            if let Some(gender) = entry.gender.as_deref() {
                details.push(gender);
            }

            let details_string = format!("[{}]", details.into_iter().join(", "));

            let mut line = format!(
                "* {} ({}) -> {} {}",
                entry.cn, entry.pinyin, entry.en, details_string
            );

            if let Some(summary) = &entry.summary {
                line.push_str(&format!(" - {}", summary));
            }

            line
        })
        .join("\n")
}

pub fn build_translation_prompt(
    prompt_template: &str,
    micro_glossary_string: &str,
    combined_chapter_text: &str,
) -> String {
    format!(
        "{}\n\n\
             **Glossary**\n\n\
             {}\n\n\
             ---\n\n\
             **Chinese Chapter(s) to Translate:**\n\
             {}",
        prompt_template, micro_glossary_string, combined_chapter_text
    )
}
