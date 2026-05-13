use std::collections::BTreeSet;
use std::fs;
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

use crate::embeddings::model_cache_dir;
use crate::model::CodeChunk;

const RERANKER_MODEL: RerankerModel = RerankerModel::JINARerankerV1TurboEn;
const RERANKER_BATCH_SIZE: usize = 8;
const MAX_RERANK_DOCUMENT_CHARS: usize = 2_800;
const MAX_SELECTED_CODE_LINES: usize = 36;
const MAX_FALLBACK_CODE_LINES: usize = 28;
const TOKENIZER_FILES: &[&str] = &[
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

pub struct CrossEncoderReranker {
    model: Mutex<TextRerank>,
}

impl CrossEncoderReranker {
    pub fn new(allow_download: bool) -> Result<Self> {
        if !allow_download && !reranker_model_downloaded()? {
            bail!(
                "reranker model is not downloaded; run `locus index --download-reranker <path>` first"
            );
        }

        let model = TextRerank::try_new(
            RerankInitOptions::new(RERANKER_MODEL).with_cache_dir(model_cache_dir()),
        )
        .context("failed to load jina-reranker-v1-turbo-en reranker model")?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }

    pub fn rerank(&self, query: &str, chunks: &[CodeChunk]) -> Result<Vec<(usize, f32)>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let documents = chunks
            .iter()
            .map(|chunk| reranker_document(query, chunk))
            .collect::<Vec<_>>();
        let results = self
            .model
            .lock()
            .map_err(|_| anyhow!("reranker model mutex poisoned"))?
            .rerank(
                query.to_string(),
                documents,
                false,
                Some(RERANKER_BATCH_SIZE),
            )
            .context("failed to rerank search results")?;

        Ok(results
            .into_iter()
            .map(|result| (result.index, result.score))
            .collect())
    }
}

pub fn download_reranker_model() -> Result<()> {
    if !reranker_model_downloaded()? {
        let _ = CrossEncoderReranker::new(true)?;
    }
    Ok(())
}

fn reranker_model_downloaded() -> Result<bool> {
    let model_info = TextRerank::get_model_info(&RERANKER_MODEL);
    let cache_dir = model_cache_dir();
    let repo_dir = cache_dir.join(format!(
        "models--{}",
        model_info.model_code.replace('/', "--")
    ));
    let refs_main = repo_dir.join("refs").join("main");
    let Ok(commit) = fs::read_to_string(refs_main) else {
        return Ok(false);
    };
    let snapshot = repo_dir.join("snapshots").join(commit.trim());

    let model_present = snapshot.join(&model_info.model_file).exists();
    let tokenizer_present = TOKENIZER_FILES
        .iter()
        .all(|file| snapshot.join(file).exists());
    let additional_present = model_info
        .additional_files
        .iter()
        .all(|file| snapshot.join(file).exists());

    Ok(model_present && tokenizer_present && additional_present)
}

fn reranker_document(query: &str, chunk: &CodeChunk) -> String {
    let mut parts = Vec::new();
    parts.push(format!("path: {}", chunk.file_path.display()));
    parts.push(format!("language: {}", chunk.language));
    parts.push(format!("kind: {}", chunk.kind.as_str()));
    if let Some(symbol) = &chunk.symbol {
        parts.push(format!("symbol: {symbol}"));
    }
    if let Some(signature) = &chunk.signature {
        parts.push(format!("signature: {signature}"));
    }
    if let Some(parent) = &chunk.parent_symbol {
        parts.push(format!("parent: {parent}"));
    }
    if !chunk.doc_comment.trim().is_empty() {
        parts.push(format!("docs: {}", chunk.doc_comment.trim()));
    }
    if !chunk.callees.is_empty() {
        parts.push(format!("callees: {}", chunk.callees.join(", ")));
    }
    parts.push(format!(
        "code:\n{}",
        compact_code_for_query(query, &chunk.text)
    ));
    truncate_chars(&parts.join("\n"), MAX_RERANK_DOCUMENT_CHARS)
}

fn compact_code_for_query(query: &str, code: &str) -> String {
    let lines = code.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let terms = query_terms(query);
    let mut selected = BTreeSet::new();
    if !terms.is_empty() {
        for (idx, line) in lines.iter().enumerate() {
            let lower = line.to_lowercase();
            if terms.iter().any(|term| lower.contains(term)) {
                selected.insert(idx);
                if idx > 0 {
                    selected.insert(idx - 1);
                }
                if idx + 1 < lines.len() {
                    selected.insert(idx + 1);
                }
            }
            if selected.len() >= MAX_SELECTED_CODE_LINES {
                break;
            }
        }
    }

    if selected.is_empty() {
        return lines
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.trim().is_empty())
            .take(MAX_FALLBACK_CODE_LINES)
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut output = Vec::new();
    let mut last = None;
    for idx in selected.into_iter().take(MAX_SELECTED_CODE_LINES) {
        if last.is_some_and(|last_idx| idx > last_idx + 1) {
            output.push("...".to_string());
        }
        output.push(lines[idx].trim_end().to_string());
        last = Some(idx);
    }
    output.join("\n")
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            push_query_term(&mut terms, &mut current);
        }
    }
    if !current.is_empty() {
        push_query_term(&mut terms, &mut current);
    }
    terms
}

fn push_query_term(terms: &mut Vec<String>, current: &mut String) {
    if current.len() > 2 && !is_common_query_term(current) && !terms.contains(current) {
        terms.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn is_common_query_term(term: &str) -> bool {
    matches!(
        term,
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "where"
            | "what"
            | "when"
            | "why"
            | "how"
            | "are"
            | "not"
            | "after"
            | "during"
            | "being"
            | "users"
            | "user"
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text
        .char_indices()
        .take_while(|(idx, _)| *idx < max_chars)
        .map(|(_, ch)| ch)
        .collect::<String>();
    truncated.push_str("\n...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChunkKind;
    use std::path::PathBuf;

    #[test]
    fn reranker_document_prioritizes_code_metadata() {
        let chunk = CodeChunk {
            id: "1".into(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from("src/search.rs"),
            language: "rust".into(),
            kind: ChunkKind::Function,
            symbol: Some("search_repo".into()),
            signature: Some("pub fn search_repo(...)".into()),
            parent_symbol: None,
            start_line: 1,
            end_line: 3,
            doc_comment: "Runs code search.".into(),
            callees: vec!["rerank".into()],
            sibling_symbols: Vec::new(),
            text: "pub fn search_repo() {}".into(),
        };

        let document = reranker_document("where is code search implemented", &chunk);

        assert!(document.starts_with("path: src/search.rs\nlanguage: rust"));
        assert!(document.contains("symbol: search_repo"));
        assert!(document.contains("docs: Runs code search."));
        assert!(document.contains("code:\npub fn search_repo() {}"));
    }

    #[test]
    fn reranker_document_keeps_query_matching_code_lines() {
        let chunk = CodeChunk {
            id: "1".into(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from("src/ui.rs"),
            language: "rust".into(),
            kind: ChunkKind::Function,
            symbol: Some("render".into()),
            signature: Some("fn render()".into()),
            parent_symbol: None,
            start_line: 1,
            end_line: 6,
            doc_comment: String::new(),
            callees: Vec::new(),
            sibling_symbols: Vec::new(),
            text: "let unrelated = true;\nlet animated_wave = draw_wave();\ncanvas.paint(animated_wave);\nlet other = true;".into(),
        };

        let document = reranker_document("animated wave effect", &chunk);

        assert!(document.contains("animated_wave"));
        assert!(document.contains("canvas.paint"));
    }
}
