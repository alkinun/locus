use anyhow::Result;
use serde::Serialize;

use crate::model::{ChunkKind, RankedChunk, SearchResult};

const MAX_SNIPPET_LINES: usize = 60;
const MAX_SNIPPET_BYTES: usize = 8 * 1024;

pub fn print_index_summary(summary: &crate::indexer::IndexSummary) {
    println!(
        "Indexed {} files, {} chunks, {} ms",
        summary.files,
        summary.chunks,
        summary.elapsed.as_millis()
    );
    if !summary.kind_counts.is_empty() {
        println!("Kinds:");
        for (kind, count) in &summary.kind_counts {
            println!("  {}: {}", kind.as_str(), count);
        }
    }
    println!("Repo metadata:");
    println!(
        "  languages: {}",
        summary.repo_metadata.vocabulary.languages.len()
    );
    println!(
        "  extensions: {}",
        summary.repo_metadata.vocabulary.extensions.len()
    );
    println!(
        "  symbols: {}",
        summary.repo_metadata.vocabulary.symbols.len()
    );
    println!(
        "  headings: {}",
        summary.repo_metadata.vocabulary.headings.len()
    );
    println!(
        "  symbol references: {}",
        summary.repo_metadata.symbol_graph.references.len()
    );
    println!("Index: {}", summary.index_path.display());
}

pub fn print_human_results(results: &[RankedChunk], elapsed_ms: u128) {
    for (idx, result) in results.iter().enumerate() {
        let symbol = result.chunk.symbol.as_deref().unwrap_or("-");
        println!(
            "{}. {}:{}-{}  score={:.2}  kind={}  symbol={}",
            idx + 1,
            result.chunk.file_path.display(),
            result.chunk.start_line,
            result.chunk.end_line,
            result.score,
            result.chunk.kind.as_str(),
            symbol
        );
        println!("   language: {}", result.chunk.language);
        println!("   reason: {}", result.reason);
        println!();
        println!("{}", indent(&truncate_snippet(&result.chunk.text)));
        println!();
    }
    println!("Found {} results in {} ms", results.len(), elapsed_ms);
}

pub fn print_json_results(results: Vec<SearchResult>) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GroupedResults {
    pub primary: Vec<SearchResult>,
    pub supporting: Vec<SearchResult>,
    pub tests: Vec<SearchResult>,
    pub docs: Vec<SearchResult>,
    pub config: Vec<SearchResult>,
}

pub fn group_ranked_results(results: &[RankedChunk]) -> GroupedResults {
    let mut grouped = GroupedResults::default();
    for (idx, ranked) in results.iter().cloned().enumerate() {
        let result = ranked.into_result(idx + 1);
        match group_for(&result) {
            "tests" => grouped.tests.push(result),
            "docs" => grouped.docs.push(result),
            "config" => grouped.config.push(result),
            "supporting" => grouped.supporting.push(result),
            _ => grouped.primary.push(result),
        }
    }
    grouped
}

pub fn print_human_grouped_results(results: &[RankedChunk], elapsed_ms: u128) {
    let grouped = group_ranked_results(results);
    print_group("Primary matches", &grouped.primary);
    print_group("Supporting definitions", &grouped.supporting);
    print_group("Tests", &grouped.tests);
    print_group("Docs", &grouped.docs);
    print_group("Config", &grouped.config);
    println!("Found {} results in {} ms", results.len(), elapsed_ms);
}

pub fn print_json_grouped_results(grouped: GroupedResults) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&grouped)?);
    Ok(())
}

fn print_group(title: &str, results: &[SearchResult]) {
    if results.is_empty() {
        return;
    }
    println!("{title}");
    for result in results {
        println!(
            "{}. {}:{}-{}  score={:.2}  kind={}  symbol={}",
            result.rank,
            result.file_path,
            result.start_line,
            result.end_line,
            result.score,
            result.kind.as_str(),
            result.symbol.as_deref().unwrap_or("-")
        );
        println!("   language: {}", result.language);
        println!("   reason: {}", result.reason);
        println!();
    }
}

fn group_for(result: &SearchResult) -> &'static str {
    let path = result.file_path.to_lowercase();
    if result.kind == ChunkKind::Test || path.contains("test") || path.contains("tests/") {
        "tests"
    } else if result.kind == ChunkKind::MarkdownSection
        || path.contains("readme")
        || path.contains("docs/")
    {
        "docs"
    } else if result.kind == ChunkKind::Config
        || path.contains("config")
        || path.contains("setting")
        || path.ends_with("cargo.toml")
        || path.ends_with("package.json")
    {
        "config"
    } else if matches!(
        result.kind,
        ChunkKind::Struct | ChunkKind::Enum | ChunkKind::Trait | ChunkKind::Class
    ) {
        "supporting"
    } else {
        "primary"
    }
}

pub fn truncate_snippet(text: &str) -> String {
    let mut output = String::new();
    let mut bytes = 0usize;
    let mut truncated = false;

    for (idx, line) in text.lines().enumerate() {
        if idx >= MAX_SNIPPET_LINES || bytes + line.len() + 1 > MAX_SNIPPET_BYTES {
            truncated = true;
            break;
        }
        output.push_str(line);
        output.push('\n');
        bytes += line.len() + 1;
    }

    if truncated {
        output.push_str("...\n");
    }
    output.trim_end().to_string()
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("   {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ranked(path: &str, kind: ChunkKind, symbol: &str) -> RankedChunk {
        RankedChunk {
            chunk: crate::model::CodeChunk {
                id: symbol.into(),
                repo_root: PathBuf::from("/repo"),
                file_path: PathBuf::from(path),
                language: if path.ends_with(".md") {
                    "markdown"
                } else {
                    "rust"
                }
                .into(),
                kind,
                symbol: Some(symbol.into()),
                signature: None,
                parent_symbol: None,
                start_line: 1,
                end_line: 2,
                doc_comment: String::new(),
                callees: Vec::new(),
                sibling_symbols: Vec::new(),
                text: String::new(),
            },
            score: 1.0,
            reason: "test".into(),
        }
    }

    #[test]
    fn grouping_places_tests_in_tests_section() {
        let grouped = group_ranked_results(&[ranked(
            "tests/chunker_test.rs",
            ChunkKind::Test,
            "test_chunking",
        )]);
        assert_eq!(grouped.tests.len(), 1);
    }

    #[test]
    fn grouping_places_markdown_sections_in_docs_section() {
        let grouped =
            group_ranked_results(&[ranked("README.md", ChunkKind::MarkdownSection, "Supported")]);
        assert_eq!(grouped.docs.len(), 1);
    }

    #[test]
    fn grouping_keeps_implementation_chunks_primary() {
        let grouped =
            group_ranked_results(&[ranked("src/search.rs", ChunkKind::Function, "rerank")]);
        assert_eq!(grouped.primary.len(), 1);
    }
}
