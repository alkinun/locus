use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};

use crate::chunker::build_chunk_context;
use crate::model::CodeChunk;

const EMBEDDING_MODEL: EmbeddingModel = EmbeddingModel::JinaEmbeddingsV2BaseCode;
const TOKENIZER_FILES: &[&str] = &[
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

pub struct EmbeddingStore {
    model: Mutex<TextEmbedding>,
    vectors: Vec<(String, Vec<f32>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedEmbeddingStore {
    vectors: Vec<(String, Vec<f32>)>,
}

impl EmbeddingStore {
    pub fn new(allow_download: bool) -> Result<Self> {
        if !allow_download && !embedding_model_downloaded()? {
            bail!(
                "embedding model is not downloaded; run `locus index --download_embedding <path>` first"
            );
        }

        let model = TextEmbedding::try_new(
            InitOptions::new(EMBEDDING_MODEL).with_cache_dir(embedding_cache_dir()),
        )
        .context("failed to load JinaEmbeddingsV2BaseCode embedding model")?;

        Ok(Self {
            model: Mutex::new(model),
            vectors: Vec::new(),
        })
    }

    pub fn embed_chunks(chunks: &[CodeChunk]) -> Result<Self> {
        let mut store = Self::new(false)?;
        let texts = chunks.iter().map(contextualize_chunk).collect::<Vec<_>>();
        let embeddings = {
            let mut model = store
                .model
                .lock()
                .map_err(|_| anyhow!("embedding model mutex poisoned"))?;
            model
                .embed(texts, None)
                .context("failed to embed code chunks")?
        };

        store.vectors = chunks
            .iter()
            .zip(embeddings)
            .map(|(chunk, vector)| (chunk.id.clone(), normalize(vector)))
            .collect();
        Ok(store)
    }

    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>> {
        if top_k == 0 || self.vectors.is_empty() {
            return Ok(Vec::new());
        }

        let prefixed_query = format!("Represent this query for searching relevant code: {query}");
        let query_vector = normalize(self.embed_query(&prefixed_query)?);
        let mut scored = self
            .vectors
            .iter()
            .map(|(chunk_id, vector)| (chunk_id.clone(), dot(&query_vector, vector)))
            .collect::<Vec<_>>();

        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(top_k);
        Ok(scored)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let persisted = PersistedEmbeddingStore {
            vectors: self.vectors.clone(),
        };
        let file = fs::File::create(path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        serde_json::to_writer(file, &persisted)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut persisted: PersistedEmbeddingStore = serde_json::from_reader(file)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for (_, vector) in &mut persisted.vectors {
            normalize_in_place(vector);
        }

        let mut store = Self::new(false)?;
        store.vectors = persisted.vectors;
        Ok(store)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow!("embedding model mutex poisoned"))?;
        let mut embeddings = model
            .embed(vec![query], None)
            .context("failed to embed search query")?;
        embeddings
            .pop()
            .ok_or_else(|| anyhow!("embedding model returned no query embedding"))
    }
}

pub fn download_embedding_model() -> Result<()> {
    if !embedding_model_downloaded()? {
        let _ = EmbeddingStore::new(true)?;
    }
    Ok(())
}

pub fn embed_chunks(chunks: &[CodeChunk]) -> Result<EmbeddingStore> {
    EmbeddingStore::embed_chunks(chunks)
}

fn contextualize_chunk(chunk: &CodeChunk) -> String {
    let context = build_chunk_context(chunk);
    if context.is_empty() {
        return chunk.text.clone();
    }
    format!("{}\n{}", context, chunk.text)
}

fn embedding_model_downloaded() -> Result<bool> {
    let model_info = TextEmbedding::get_model_info(&EMBEDDING_MODEL)?;
    let cache_dir = embedding_cache_dir();
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

fn embedding_cache_dir() -> PathBuf {
    if let Ok(path) = env::var("HF_HOME") {
        return path.into();
    }
    if let Ok(path) = env::var("FASTEMBED_CACHE_DIR") {
        return path.into();
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("fastembed");
    }
    PathBuf::from(".fastembed_cache")
}

fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    normalize_in_place(&mut vector);
    vector
}

fn normalize_in_place(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum()
}
