use std::{collections::HashSet, sync::LazyLock};

use aho_corasick::AhoCorasick;
use anyhow::Result;
use bk_tree::{BKTree, metrics::Levenshtein};
use fancy_regex::Regex;
use jieba_rs::Jieba;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// Removes everything except:
///
/// - Han characters
/// - punctuation
/// - numbers
static RE_PREPROCESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[^\p{Han}\p{P}\p{N}]").expect("Invalid glossary preprocessing regex")
});

/// Shared Jieba tokenizer.
///
/// Initialised once on first use and reused for every glossary request.
static JIEBA: LazyLock<Jieba> = LazyLock::new(Jieba::new);

#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, utoipa::ToSchema)]
pub struct GlossaryEntry {
    /// English translation.
    #[schema(example = "Li Bai")]
    pub en: String,
    /// Chinese term to match.
    #[schema(example = "李白")]
    pub cn: String,
    #[schema(example = "Li Bai")]
    pub pinyin: String,

    #[serde(rename = "type")]
    #[schema(example = "character")]
    pub _type: String,

    pub gender: Option<String>,
    pub file: Option<i32>,
    pub summary: Option<String>,
}

/// Controls how glossary matching is performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct GlossaryOptions {
    /// Whether fuzzy matching should be performed after exact matching.
    #[schema(default = true)]
    pub fuzzy: bool,

    /// Maximum Levenshtein distance accepted by fuzzy matching.
    #[schema(default = 1, minimum = 0, maximum = 4)]
    pub fuzzy_threshold: u32,
}

impl Default for GlossaryOptions {
    fn default() -> Self {
        Self {
            fuzzy: true,
            fuzzy_threshold: 1,
        }
    }
}

/// Creates a micro-glossary from chapter content and glossary entries using
/// the default matching options.
///
/// Results are returned in the same order as the supplied glossary.
#[allow(dead_code)]
pub fn create_micro_glossary(
    content: &str,
    glossary: &[GlossaryEntry],
) -> Result<Vec<GlossaryEntry>> {
    create_micro_glossary_with_options(content, glossary, GlossaryOptions::default())
}

/// Creates a micro-glossary using explicit matching options.
///
/// Matching is performed in two phases:
///
/// 1. Exact matching with Aho-Corasick.
/// 2. Fuzzy matching with Jieba segmentation + BK-tree/Levenshtein,
///    if enabled.
///
/// Exact matches are removed from the fuzzy candidate set so they are not
/// processed twice.
///
/// Results preserve the order of the supplied glossary.
pub fn create_micro_glossary_with_options(
    content: &str,
    glossary: &[GlossaryEntry],
    options: GlossaryOptions,
) -> Result<Vec<GlossaryEntry>> {
    if content.is_empty() || glossary.is_empty() {
        return Ok(Vec::new());
    }

    let processed_content = preprocess_chinese_text(content);

    if processed_content.is_empty() {
        return Ok(Vec::new());
    }

    let cleaned_terms: Vec<String> = glossary
        .iter()
        .map(|entry| preprocess_chinese_text(&entry.cn))
        .collect();

    let mut seen = HashSet::with_capacity(cleaned_terms.len());
    let mut valid_terms = Vec::with_capacity(cleaned_terms.len());

    for term in &cleaned_terms {
        let term = term.as_str();

        if !term.is_empty() && seen.insert(term) {
            valid_terms.push(term);
        }
    }

    if valid_terms.is_empty() {
        return Ok(Vec::new());
    }

    /*
     * Phase 1: exact search.
     */
    let exact_matches = aho_corasick_find_all(&valid_terms, &processed_content)?;

    let mut found_terms = exact_matches;

    /*
     * Phase 2: fuzzy search.
     *
     * Only terms not already found exactly are considered.
     */
    if options.fuzzy {
        let fuzzy_candidates: Vec<&str> = valid_terms
            .iter()
            .copied()
            .filter(|term| !found_terms.contains(term))
            .collect();

        if !fuzzy_candidates.is_empty() {
            let fuzzy_matches = chinese_fuzzy_search(
                &fuzzy_candidates,
                &processed_content,
                options.fuzzy_threshold,
            );

            found_terms.extend(fuzzy_matches);
        }
    }

    /*
     * Recover the original glossary entries.
     */
    let result = glossary
        .iter()
        .zip(cleaned_terms.iter())
        .filter(|(_, clean_term)| found_terms.contains(clean_term.as_str()))
        .map(|(entry, _)| entry.clone())
        .collect();

    Ok(result)
}

/// Normalizes Chinese text for matching.
fn preprocess_chinese_text(text: &str) -> String {
    let normalized = text.nfkc().collect::<String>();

    RE_PREPROCESS.replace_all(&normalized, "").to_string()
}

/// Performs exact multi-pattern matching using Aho-Corasick.
fn aho_corasick_find_all<'a>(terms: &[&'a str], text: &str) -> Result<HashSet<&'a str>> {
    if terms.is_empty() {
        return Ok(HashSet::new());
    }

    let matcher = AhoCorasick::new(terms.iter().copied())?;

    let mut found = HashSet::new();

    for mat in matcher.find_overlapping_iter(text) {
        let pattern_index = mat.pattern().as_usize();

        found.insert(terms[pattern_index]);
    }

    Ok(found)
}

/// Performs fuzzy matching using Jieba segmentation and a BK-tree.
///
/// Each word identified by Jieba is compared against the candidate glossary
/// terms using Levenshtein distance.
fn chinese_fuzzy_search<'a>(terms: &[&'a str], text: &'a str, threshold: u32) -> HashSet<&'a str> {
    if terms.is_empty() || text.is_empty() || threshold == 0 {
        return HashSet::new();
    }

    let words = JIEBA.cut(text, true);

    if words.is_empty() {
        return HashSet::new();
    }

    let mut bk_tree = BKTree::new(Levenshtein);
    bk_tree.extend(terms);

    words
        .par_iter()
        .flat_map(|word| {
            bk_tree
                .find(&word.word, threshold)
                .map(|(_distance, term)| **term)
                .collect::<Vec<_>>()
        })
        .collect()
}
