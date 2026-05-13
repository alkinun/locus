use std::path::PathBuf;

use crate::search::DEFAULT_RERANK_INPUT_LIMIT;
use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "locus")]
#[command(about = "Fast local codebase search")]
pub struct Cli {
    /// Repository path to use for interactive search.
    #[arg(long, default_value = ".")]
    pub path: PathBuf,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build or rebuild the local code search index.
    Index {
        path: PathBuf,
        #[arg(long = "download_embedding")]
        download_embedding: bool,
        #[arg(long = "download-reranker")]
        download_reranker: bool,
    },
    /// Generate a synthetic retrieval eval dataset from indexed chunks.
    GenerateEval {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "evals/generated.yaml")]
        out: PathBuf,
        #[arg(long, default_value_t = 100)]
        count: usize,
        #[arg(long, default_value = "http://localhost:8000/v1/chat/completions")]
        endpoint: String,
        #[arg(long, default_value = "gemma4")]
        model: String,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 24)]
        concurrency: usize,
    },
    /// Benchmark retrieval quality against an eval dataset.
    Eval {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "evals/locus.synthetic.yaml")]
        dataset: PathBuf,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long = "embedding", default_value_t = true, action = ArgAction::SetTrue, conflicts_with = "no_embedding")]
        embedding: bool,
        #[arg(long = "no-embedding", action = ArgAction::SetTrue)]
        no_embedding: bool,
        #[arg(long)]
        rerank: bool,
        #[arg(long = "rerank-limit", default_value_t = DEFAULT_RERANK_INPUT_LIMIT)]
        rerank_limit: usize,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 10)]
        failures: usize,
    },
    /// Search the indexed codebase.
    Search {
        query: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        grouped: bool,
        #[arg(long)]
        rerank: bool,
        #[arg(long = "rerank-limit", default_value_t = DEFAULT_RERANK_INPUT_LIMIT)]
        rerank_limit: usize,
    },
}
