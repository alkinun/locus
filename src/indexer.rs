use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tantivy::schema::{STORED, STRING, Schema, TEXT};
use tantivy::{Index, doc};
use walkdir::{DirEntry, WalkDir};

use crate::chunker::{chunk_file, detect_language};
use crate::embeddings::{
    DEFAULT_EMBEDDING_BATCH_SIZE, EmbeddingProgress, download_embedding_model,
    embed_chunks_with_progress,
};
use crate::model::{ChunkKind, CodeChunk};
use crate::repo_meta::{RepoMetadata, build_metadata, write_metadata};

const MAX_FILE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone)]
pub struct IndexSummary {
    pub files: usize,
    pub chunks: usize,
    pub kind_counts: Vec<(ChunkKind, usize)>,
    pub repo_metadata: RepoMetadata,
    pub elapsed: Duration,
    pub index_path: PathBuf,
}

pub fn index_repo(path: &Path, download_embedding: bool) -> Result<IndexSummary> {
    let repo_root = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    if download_embedding {
        eprintln!("Downloading JinaEmbeddingsV2BaseCode if needed...");
        download_embedding_model()?;
    }

    let started = Instant::now();
    let index_path = repo_root.join(".locus").join("index");
    if index_path.exists() {
        fs::remove_dir_all(&index_path)
            .with_context(|| format!("failed to clear {}", index_path.display()))?;
    }
    fs::create_dir_all(&index_path)
        .with_context(|| format!("failed to create {}", index_path.display()))?;

    let schema = build_schema();
    let index = Index::create_in_dir(&index_path, schema.clone())?;
    let fields = IndexFields::from_schema(&schema);
    let mut writer = index.writer(50_000_000)?;
    let mut files = 0usize;
    let mut kind_counts = std::collections::HashMap::<ChunkKind, usize>::new();
    let mut all_chunks = Vec::<CodeChunk>::new();

    for entry in WalkDir::new(&repo_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_enter(entry, &repo_root))
    {
        let entry = entry?;
        if !entry.file_type().is_file() || !is_supported_file(entry.path()) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }

        let text = fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read {}", entry.path().display()))?;
        let file_chunks = chunk_file(&repo_root, entry.path(), &text);
        if file_chunks.is_empty() {
            continue;
        }
        files += 1;
        all_chunks.extend(file_chunks);
    }

    let chunks = all_chunks.len();
    let repo_metadata = build_metadata(&repo_root, &all_chunks);
    write_metadata(&repo_root, &repo_metadata)?;

    for chunk in &all_chunks {
        *kind_counts.entry(chunk.kind).or_default() += 1;
        let extension = chunk
            .file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_string();
        let heading_or_symbol = chunk.symbol.clone().unwrap_or_default();
        writer.add_document(doc!(
            fields.id => chunk.id.clone(),
            fields.repo_root => chunk.repo_root.display().to_string(),
            fields.file_path => chunk.file_path.display().to_string(),
            fields.language => chunk.language.clone(),
            fields.extension => extension,
            fields.kind => chunk.kind.as_str(),
            fields.symbol => chunk.symbol.clone().unwrap_or_default(),
            fields.signature => chunk.signature.clone().unwrap_or_default(),
            fields.parent_symbol => chunk.parent_symbol.clone().unwrap_or_default(),
            fields.heading_or_symbol => heading_or_symbol,
            fields.start_line => chunk.start_line as u64,
            fields.end_line => chunk.end_line as u64,
            fields.doc_comment => chunk.doc_comment.clone(),
            fields.callees => chunk.callees.join("\n"),
            fields.sibling_symbols => chunk.sibling_symbols.join("\n"),
            fields.text => chunk.text.clone(),
        ))?;
    }

    writer.commit()?;
    eprintln!(
        "Embedding {chunks} chunks with JinaEmbeddingsV2BaseCode in batches of {DEFAULT_EMBEDDING_BATCH_SIZE}..."
    );
    let mut progress_bar = IndexProgressBar::new(chunks);
    embed_chunks_with_progress(&all_chunks, |progress| progress_bar.draw(progress))?
        .save(&index_path.join("embeddings.bin"))?;
    progress_bar.finish();
    let mut kind_counts = kind_counts.into_iter().collect::<Vec<_>>();
    kind_counts.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    Ok(IndexSummary {
        files,
        chunks,
        kind_counts,
        repo_metadata,
        elapsed: started.elapsed(),
        index_path,
    })
}

struct IndexProgressBar {
    total: usize,
    width: usize,
    last_draw: Option<Instant>,
    finished: bool,
}

impl IndexProgressBar {
    fn new(total: usize) -> Self {
        Self {
            total,
            width: 28,
            last_draw: None,
            finished: false,
        }
    }

    fn draw(&mut self, progress: EmbeddingProgress) {
        let now = Instant::now();
        if self
            .last_draw
            .is_some_and(|last_draw| now.duration_since(last_draw) < Duration::from_millis(80))
            && progress.embedded_chunks < progress.total_chunks
        {
            return;
        }

        self.last_draw = Some(now);
        let filled = if self.total == 0 {
            self.width
        } else {
            self.width * progress.embedded_chunks / self.total
        };
        let percent = if self.total == 0 {
            100
        } else {
            progress.embedded_chunks * 100 / self.total
        };
        let bar = format!(
            "{}{}",
            "#".repeat(filled),
            "-".repeat(self.width.saturating_sub(filled))
        );
        eprint!(
            "\rEmbedding [{bar}] {percent:>3}% ({}/{}) batch {}/{}",
            progress.embedded_chunks, progress.total_chunks, progress.batch, progress.total_batches
        );
        let _ = io::stderr().flush();
    }

    fn finish(&mut self) {
        if !self.finished {
            eprintln!();
            self.finished = true;
        }
    }
}

pub fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("id", STRING | STORED);
    builder.add_text_field("repo_root", STRING | STORED);
    builder.add_text_field("file_path", TEXT | STORED);
    builder.add_text_field("language", TEXT | STORED);
    builder.add_text_field("extension", TEXT | STORED);
    builder.add_text_field("kind", TEXT | STORED);
    builder.add_text_field("symbol", TEXT | STORED);
    builder.add_text_field("signature", TEXT | STORED);
    builder.add_text_field("parent_symbol", TEXT | STORED);
    builder.add_text_field("heading_or_symbol", TEXT | STORED);
    builder.add_u64_field("start_line", STORED);
    builder.add_u64_field("end_line", STORED);
    builder.add_text_field("doc_comment", STORED);
    builder.add_text_field("callees", STORED);
    builder.add_text_field("sibling_symbols", STORED);
    builder.add_text_field("text", TEXT | STORED);
    builder.build()
}

pub fn is_ignored_path(path: &Path, repo_root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(repo_root) else {
        return false;
    };
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | ".locus" | "target" | "node_modules" | "dist" | "build" | "evals"
        ) {
            return true;
        }
        if name.starts_with('.') {
            return true;
        }
    }
    false
}

fn should_enter(entry: &DirEntry, repo_root: &Path) -> bool {
    entry.path() == repo_root || !is_ignored_path(entry.path(), repo_root)
}

fn is_supported_file(path: &Path) -> bool {
    detect_language(path).is_some()
}

#[derive(Debug, Clone, Copy)]
pub struct IndexFields {
    pub id: tantivy::schema::Field,
    pub repo_root: tantivy::schema::Field,
    pub file_path: tantivy::schema::Field,
    pub language: tantivy::schema::Field,
    pub extension: tantivy::schema::Field,
    pub kind: tantivy::schema::Field,
    pub symbol: tantivy::schema::Field,
    pub signature: tantivy::schema::Field,
    pub parent_symbol: tantivy::schema::Field,
    pub heading_or_symbol: tantivy::schema::Field,
    pub start_line: tantivy::schema::Field,
    pub end_line: tantivy::schema::Field,
    pub doc_comment: tantivy::schema::Field,
    pub callees: tantivy::schema::Field,
    pub sibling_symbols: tantivy::schema::Field,
    pub text: tantivy::schema::Field,
}

impl IndexFields {
    pub fn from_schema(schema: &Schema) -> Self {
        Self {
            id: schema.get_field("id").expect("id field"),
            repo_root: schema.get_field("repo_root").expect("repo_root field"),
            file_path: schema.get_field("file_path").expect("file_path field"),
            language: schema.get_field("language").expect("language field"),
            extension: schema.get_field("extension").expect("extension field"),
            kind: schema.get_field("kind").expect("kind field"),
            symbol: schema.get_field("symbol").expect("symbol field"),
            signature: schema.get_field("signature").expect("signature field"),
            parent_symbol: schema
                .get_field("parent_symbol")
                .expect("parent_symbol field"),
            heading_or_symbol: schema
                .get_field("heading_or_symbol")
                .expect("heading_or_symbol field"),
            start_line: schema.get_field("start_line").expect("start_line field"),
            end_line: schema.get_field("end_line").expect("end_line field"),
            doc_comment: schema.get_field("doc_comment").expect("doc_comment field"),
            callees: schema.get_field("callees").expect("callees field"),
            sibling_symbols: schema
                .get_field("sibling_symbols")
                .expect("sibling_symbols field"),
            text: schema.get_field("text").expect("text field"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_expected_directories() {
        let root = Path::new("/repo");
        assert!(is_ignored_path(Path::new("/repo/.git/config"), root));
        assert!(is_ignored_path(Path::new("/repo/target/debug/app"), root));
        assert!(is_ignored_path(
            Path::new("/repo/node_modules/pkg/index.js"),
            root
        ));
        assert!(is_ignored_path(Path::new("/repo/.cache/file.rs"), root));
        assert!(!is_ignored_path(Path::new("/repo/src/lib.rs"), root));
    }
}
