use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::model::{ChunkKind, CodeChunk};
use crate::query::{AnalyzedQuery, QueryIntent};

pub const META_FILE: &str = "repo_meta.json";
const MAX_EXPANSIONS: usize = 32;
const MAX_REFERENCES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoMetadata {
    pub vocabulary: RepoVocabulary,
    pub symbol_graph: SymbolGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoVocabulary {
    pub languages: Vec<String>,
    pub extensions: Vec<String>,
    pub symbols: Vec<String>,
    pub symbol_terms: Vec<String>,
    pub path_terms: Vec<String>,
    pub headings: Vec<String>,
    pub dependencies: Vec<String>,
    pub config_keys: Vec<String>,
    pub test_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SymbolGraph {
    pub definitions: Vec<SymbolDefinition>,
    pub references: Vec<SymbolReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub symbol: String,
    pub normalized_terms: Vec<String>,
    pub chunk_id: String,
    pub file_path: String,
    pub kind: ChunkKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolReference {
    pub symbol: String,
    pub from_chunk_id: String,
    pub to_chunk_id: String,
}

pub fn normalize_term(input: &str) -> String {
    input
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .to_lowercase()
}

pub fn identifier_terms(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for raw in input.split(|ch: char| !ch.is_alphanumeric() && ch != '_') {
        if raw.is_empty() {
            continue;
        }
        let cleaned = raw.trim_matches('_');
        if cleaned.is_empty() {
            continue;
        }
        for part in cleaned.split('_').filter(|part| !part.is_empty()) {
            for camel in split_case(part) {
                push_unique(&mut terms, normalize_term(&camel));
            }
        }
        if cleaned.contains('_') {
            push_unique(&mut terms, cleaned.to_string());
        } else if has_case_boundary(cleaned) {
            push_unique(&mut terms, cleaned.to_string());
        }
    }
    terms.retain(|term| !term.is_empty());
    terms
}

pub fn metadata_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".locus").join("index").join(META_FILE)
}

pub fn write_metadata(repo_root: &Path, meta: &RepoMetadata) -> Result<()> {
    let path = metadata_path(repo_root);
    let text = serde_json::to_string_pretty(meta)?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

pub fn read_metadata(repo_root: &Path) -> Result<Option<RepoMetadata>> {
    let path = metadata_path(repo_root);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(Some(serde_json::from_str(&text)?))
}

pub fn build_metadata(repo_root: &Path, chunks: &[CodeChunk]) -> RepoMetadata {
    let mut meta = RepoMetadata::default();
    for chunk in chunks {
        push_unique(&mut meta.vocabulary.languages, chunk.language.clone());
        if let Some(ext) = chunk.file_path.extension().and_then(|ext| ext.to_str()) {
            push_unique(&mut meta.vocabulary.extensions, ext.to_lowercase());
        }
        for term in identifier_terms(&chunk.file_path.to_string_lossy()) {
            push_unique(&mut meta.vocabulary.path_terms, term);
        }
        if let Some(symbol) = &chunk.symbol {
            push_unique(&mut meta.vocabulary.symbols, symbol.clone());
            for term in identifier_terms(symbol) {
                push_unique(&mut meta.vocabulary.symbol_terms, term.clone());
                if chunk.kind == ChunkKind::Test
                    || chunk
                        .file_path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains("test")
                {
                    push_unique(&mut meta.vocabulary.test_terms, term);
                }
            }
            if chunk.kind == ChunkKind::MarkdownSection {
                push_unique(&mut meta.vocabulary.headings, symbol.clone());
            }
            meta.symbol_graph.definitions.push(SymbolDefinition {
                symbol: symbol.clone(),
                normalized_terms: identifier_terms(symbol),
                chunk_id: chunk.id.clone(),
                file_path: chunk.file_path.display().to_string(),
                kind: chunk.kind,
            });
        }
    }
    collect_repo_files(repo_root, &mut meta.vocabulary);
    meta.symbol_graph.references = build_references(chunks, &meta.symbol_graph.definitions);
    sort_vocab(&mut meta.vocabulary);
    meta
}

pub fn expand_with_repo_metadata(analyzed: &AnalyzedQuery, meta: &RepoMetadata) -> AnalyzedQuery {
    let mut expanded = analyzed.clone();
    let query_terms = analyzed
        .important_terms
        .iter()
        .flat_map(|term| identifier_terms(term))
        .chain(analyzed.expansions.iter().cloned())
        .collect::<Vec<_>>();

    let mut candidates = Vec::new();
    candidates.extend(meta.vocabulary.symbol_terms.iter().cloned());
    candidates.extend(meta.vocabulary.path_terms.iter().cloned());
    candidates.extend(
        meta.vocabulary
            .headings
            .iter()
            .flat_map(|heading| identifier_terms(heading)),
    );
    match analyzed.intent {
        QueryIntent::ExplainCapability => {
            candidates.extend(meta.vocabulary.languages.iter().cloned());
            candidates.extend(meta.vocabulary.extensions.iter().cloned());
        }
        QueryIntent::FindImplementation | QueryIntent::FindDefinition | QueryIntent::FindUsage => {
            candidates.extend(
                meta.vocabulary
                    .symbols
                    .iter()
                    .flat_map(|symbol| identifier_terms(symbol)),
            );
        }
        QueryIntent::FindTests => candidates.extend(meta.vocabulary.test_terms.iter().cloned()),
        QueryIntent::FindConfig => {
            candidates.extend(meta.vocabulary.config_keys.iter().cloned());
            candidates.extend(
                meta.vocabulary
                    .path_terms
                    .iter()
                    .filter(|term| {
                        term.contains("config")
                            || term.contains("setting")
                            || term.contains("ignore")
                    })
                    .cloned(),
            );
        }
        QueryIntent::Unknown => {}
    }

    for candidate in candidates {
        if expanded.expansions.len() >= MAX_EXPANSIONS {
            break;
        }
        let normalized = normalize_term(&candidate);
        if normalized.len() < 2 {
            continue;
        }
        let matched = query_terms.iter().any(|term| {
            term == &normalized
                || normalized.contains(term)
                || term.contains(&normalized)
                || token_overlap(term, &normalized)
        });
        if matched
            && !expanded.important_terms.contains(&normalized)
            && !expanded.expansions.contains(&normalized)
        {
            expanded.expansions.push(normalized);
        }
    }
    expanded
}

pub fn repo_vocab_overlap(
    analyzed: &AnalyzedQuery,
    meta: &RepoMetadata,
    chunk: &CodeChunk,
) -> usize {
    let vocab = repo_vocab_set(meta);
    let terms = chunk_terms(chunk);
    analyzed
        .important_terms
        .iter()
        .chain(analyzed.expansions.iter())
        .filter(|term| vocab.contains(term.as_str()) && terms.contains(term.as_str()))
        .count()
}

pub fn related_ids(meta: &RepoMetadata, primary_ids: &HashSet<String>) -> HashSet<String> {
    let mut ids = HashSet::new();
    for edge in &meta.symbol_graph.references {
        if primary_ids.contains(&edge.from_chunk_id) {
            ids.insert(edge.to_chunk_id.clone());
        }
        if primary_ids.contains(&edge.to_chunk_id) {
            ids.insert(edge.from_chunk_id.clone());
        }
    }
    ids
}

fn build_references(
    chunks: &[CodeChunk],
    definitions: &[SymbolDefinition],
) -> Vec<SymbolReference> {
    let mut references = Vec::new();
    for chunk in chunks {
        let text = chunk.text.to_lowercase();
        for def in definitions {
            if chunk.id == def.chunk_id {
                continue;
            }
            let symbol_hit = text.contains(&def.symbol.to_lowercase());
            let term_hits = def
                .normalized_terms
                .iter()
                .filter(|term| term.len() > 2 && text.contains(term.as_str()))
                .count();
            if symbol_hit || term_hits >= 2 {
                references.push(SymbolReference {
                    symbol: def.symbol.clone(),
                    from_chunk_id: chunk.id.clone(),
                    to_chunk_id: def.chunk_id.clone(),
                });
            }
            if references.len() >= MAX_REFERENCES {
                return references;
            }
        }
    }
    references
}

fn collect_repo_files(repo_root: &Path, vocab: &mut RepoVocabulary) {
    collect_cargo_toml(repo_root, vocab);
    collect_package_json(repo_root, vocab);
    collect_requirements(repo_root, vocab);
}

fn collect_cargo_toml(repo_root: &Path, vocab: &mut RepoVocabulary) {
    let path = repo_root.join("Cargo.toml");
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let mut in_deps = false;
    let key_re = Regex::new(r#"^([A-Za-z0-9_.-]+)\s*="#).expect("valid toml key regex");
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = matches!(
                trimmed,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            push_unique(
                &mut vocab.config_keys,
                trimmed.trim_matches(&['[', ']'][..]).to_string(),
            );
            continue;
        }
        if let Some(captures) = key_re.captures(trimmed) {
            let key = captures[1].to_string();
            push_unique(&mut vocab.config_keys, key.clone());
            if in_deps {
                push_unique(&mut vocab.dependencies, key);
            }
        }
    }
}

fn collect_package_json(repo_root: &Path, vocab: &mut RepoVocabulary) {
    let path = repo_root.join("package.json");
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let key_re = Regex::new(r#""([^"]+)"\s*:"#).expect("valid json key regex");
    for captures in key_re.captures_iter(&text) {
        push_unique(&mut vocab.config_keys, captures[1].to_string());
    }
}

fn collect_requirements(repo_root: &Path, vocab: &mut RepoVocabulary) {
    let path = repo_root.join("requirements.txt");
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let name = line
            .split(['=', '<', '>', '~', '!'])
            .next()
            .unwrap_or("")
            .trim();
        if !name.is_empty() && !name.starts_with('#') {
            push_unique(&mut vocab.dependencies, name.to_string());
        }
    }
}

fn chunk_terms(chunk: &CodeChunk) -> HashSet<String> {
    let mut terms = HashSet::new();
    for term in identifier_terms(&chunk.file_path.to_string_lossy()) {
        terms.insert(term);
    }
    if let Some(symbol) = &chunk.symbol {
        for term in identifier_terms(symbol) {
            terms.insert(term);
        }
    }
    for term in identifier_terms(&chunk.text) {
        terms.insert(term);
    }
    terms
}

fn repo_vocab_set(meta: &RepoMetadata) -> HashSet<&str> {
    meta.vocabulary
        .languages
        .iter()
        .chain(meta.vocabulary.extensions.iter())
        .chain(meta.vocabulary.symbol_terms.iter())
        .chain(meta.vocabulary.path_terms.iter())
        .chain(meta.vocabulary.config_keys.iter())
        .chain(meta.vocabulary.test_terms.iter())
        .map(String::as_str)
        .collect()
}

fn split_case(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars = input.chars().collect::<Vec<_>>();
    for (idx, ch) in chars.iter().enumerate() {
        if idx > 0
            && ch.is_uppercase()
            && (chars[idx - 1].is_lowercase()
                || chars.get(idx + 1).is_some_and(|next| next.is_lowercase()))
        {
            parts.push(std::mem::take(&mut current));
        }
        current.push(*ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn has_case_boundary(input: &str) -> bool {
    input.chars().any(char::is_lowercase) && input.chars().any(char::is_uppercase)
}

fn token_overlap(left: &str, right: &str) -> bool {
    let left_terms = identifier_terms(left);
    let right_terms = identifier_terms(right);
    left_terms.iter().any(|term| right_terms.contains(term))
}

fn sort_vocab(vocab: &mut RepoVocabulary) {
    for values in [
        &mut vocab.languages,
        &mut vocab.extensions,
        &mut vocab.symbols,
        &mut vocab.symbol_terms,
        &mut vocab.path_terms,
        &mut vocab.headings,
        &mut vocab.dependencies,
        &mut vocab.config_keys,
        &mut vocab.test_terms,
    ] {
        values.sort();
        values.dedup();
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn chunk(id: &str, path: &str, kind: ChunkKind, symbol: Option<&str>, text: &str) -> CodeChunk {
        CodeChunk {
            id: id.into(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from(path),
            language: if path.ends_with(".md") {
                "markdown"
            } else {
                "rust"
            }
            .into(),
            kind,
            symbol: symbol.map(str::to_string),
            signature: None,
            parent_symbol: None,
            start_line: 1,
            end_line: text.lines().count().max(1),
            text: text.into(),
        }
    }

    #[test]
    fn identifier_terms_handles_snake_case() {
        assert_eq!(
            identifier_terms("detect_symbol_line"),
            ["detect", "symbol", "line", "detect_symbol_line"]
        );
    }

    #[test]
    fn identifier_terms_handles_camel_case() {
        assert_eq!(
            identifier_terms("refreshAccessToken"),
            ["refresh", "access", "token", "refreshAccessToken"]
        );
    }

    #[test]
    fn identifier_terms_handles_pascal_case() {
        assert_eq!(
            identifier_terms("MarkdownSection"),
            ["markdown", "section", "MarkdownSection"]
        );
    }

    #[test]
    fn identifier_terms_handles_paths() {
        assert!(identifier_terms("src/query_analyzer.rs").contains(&"query_analyzer".into()));
        assert!(identifier_terms("src/query_analyzer.rs").contains(&"rs".into()));
    }

    #[test]
    fn repo_vocabulary_collects_core_terms() {
        let chunks = vec![
            chunk(
                "1",
                "src/search.rs",
                ChunkKind::Function,
                Some("rerank"),
                "fn rerank() {}",
            ),
            chunk(
                "2",
                "README.md",
                ChunkKind::MarkdownSection,
                Some("Supported languages"),
                "## Supported languages",
            ),
        ];
        let meta = build_metadata(Path::new("/no/such/repo"), &chunks);
        assert!(meta.vocabulary.languages.contains(&"rust".into()));
        assert!(meta.vocabulary.extensions.contains(&"rs".into()));
        assert!(meta.vocabulary.symbols.contains(&"rerank".into()));
        assert!(
            meta.vocabulary
                .headings
                .contains(&"Supported languages".into())
        );
    }

    #[test]
    fn query_expansion_uses_repo_symbols() {
        let chunks = vec![chunk(
            "1",
            "src/chunker.rs",
            ChunkKind::Function,
            Some("detect_symbol_line"),
            "fn detect_symbol_line() {}",
        )];
        let meta = build_metadata(Path::new("/no/such/repo"), &chunks);
        let analyzed = crate::query::analyze_query("where is symbol detection implemented");
        let expanded = expand_with_repo_metadata(&analyzed, &meta);
        assert!(
            expanded.expansions.contains(&"symbol".into())
                || expanded.expansions.contains(&"detect".into())
        );
    }

    #[test]
    fn query_expansion_caps_terms() {
        let mut meta = RepoMetadata::default();
        meta.vocabulary.symbol_terms = (0..100).map(|n| format!("symbol{n}")).collect();
        let analyzed = crate::query::analyze_query("symbol");
        let expanded = expand_with_repo_metadata(&analyzed, &meta);
        assert!(expanded.expansions.len() <= MAX_EXPANSIONS);
    }

    #[test]
    fn symbol_graph_creates_reference_edges() {
        let chunks = vec![
            chunk(
                "1",
                "src/search.rs",
                ChunkKind::Struct,
                Some("RankingFeatures"),
                "struct RankingFeatures;",
            ),
            chunk(
                "2",
                "src/search.rs",
                ChunkKind::Function,
                Some("rerank"),
                "fn rerank() { let _: RankingFeatures; }",
            ),
        ];
        let meta = build_metadata(Path::new("/no/such/repo"), &chunks);
        assert!(
            meta.symbol_graph
                .references
                .iter()
                .any(|edge| edge.from_chunk_id == "2" && edge.to_chunk_id == "1")
        );
    }
}
