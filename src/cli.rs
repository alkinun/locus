use std::path::PathBuf;

use crate::search::DEFAULT_RERANK_INPUT_LIMIT;
use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "locus")]
#[command(version)]
#[command(about = "Fast local codebase search")]
#[command(
    long_about = "Fast local codebase search with a focused terminal UI and scriptable commands."
)]
#[command(after_help = "Examples:
  locus
  locus --path ~/work/project
  locus index --path ~/work/project
  locus search \"where are access tokens refreshed\" --path ~/work/project
  locus search \"tests for chunking\" --grouped --format json")]
pub struct Cli {
    /// Repository path for the interactive TUI.
    #[arg(short, long, value_name = "PATH", default_value = ".")]
    pub path: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build or rebuild the local code search index.
    Index {
        /// Repository path to index.
        #[arg(short, long, value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Legacy positional path. Prefer `--path`.
        #[arg(value_name = "PATH", hide = true, conflicts_with = "path")]
        legacy_path: Option<PathBuf>,

        /// Download the embedding model before indexing.
        #[arg(long = "download-embedding", alias = "download_embedding")]
        download_embedding: bool,

        /// Download the reranker model before indexing.
        #[arg(long = "download-reranker")]
        download_reranker: bool,
    },
    /// Generate a synthetic retrieval eval dataset from indexed chunks.
    GenerateEval {
        /// Repository path containing a locus index.
        #[arg(short, long, value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Output YAML dataset path.
        #[arg(
            short,
            long,
            value_name = "FILE",
            default_value = "evals/generated.yaml"
        )]
        out: PathBuf,

        /// Number of eval items to generate.
        #[arg(long, default_value_t = 100)]
        count: usize,

        /// OpenAI-compatible chat completions endpoint.
        #[arg(long, default_value = "http://localhost:8000/v1/chat/completions")]
        endpoint: String,

        /// Model name sent to the endpoint.
        #[arg(long, default_value = "gemma4")]
        model: String,

        /// Sampling seed.
        #[arg(long, default_value_t = 42)]
        seed: u64,

        /// Maximum concurrent generation requests.
        #[arg(long, default_value_t = 24)]
        concurrency: usize,
    },
    /// Benchmark retrieval quality against an eval dataset.
    Eval {
        /// Repository path containing a locus index.
        #[arg(short, long, value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Eval dataset YAML path.
        #[arg(
            short,
            long,
            value_name = "FILE",
            default_value = "evals/locus.synthetic.yaml"
        )]
        dataset: PathBuf,

        /// Number of results to retrieve per query.
        #[arg(long, default_value_t = 10)]
        limit: usize,

        /// Legacy no-op. Embeddings are enabled unless `--no-embedding` is set.
        #[arg(long = "embedding", hide = true, action = ArgAction::SetTrue, conflicts_with = "no_embedding")]
        embedding: bool,

        /// Search without vector embeddings.
        #[arg(long = "no-embedding", action = ArgAction::SetTrue)]
        no_embedding: bool,

        /// Rerank top candidates with the cross-encoder reranker.
        #[arg(long)]
        rerank: bool,

        /// Number of candidates to send to the reranker.
        #[arg(long = "rerank-limit", default_value_t = DEFAULT_RERANK_INPUT_LIMIT)]
        rerank_limit: usize,

        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,

        /// Legacy alias for `--format json`.
        #[arg(long, hide = true)]
        json: bool,

        /// Number of worst failures to print in human output.
        #[arg(long, default_value_t = 10)]
        failures: usize,
    },
    /// Search the indexed codebase.
    Search {
        /// Natural-language or symbol search query.
        query: String,

        /// Repository path containing a locus index.
        #[arg(short, long, value_name = "PATH", default_value = ".")]
        path: PathBuf,

        /// Number of results to return.
        #[arg(long, default_value_t = 5)]
        limit: usize,

        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,

        /// Legacy alias for `--format json`.
        #[arg(long, hide = true)]
        json: bool,

        /// Group results by role: primary, supporting, tests, docs, config.
        #[arg(long)]
        grouped: bool,

        /// Include code snippets in human output.
        #[arg(long)]
        snippets: bool,

        /// Search without vector embeddings.
        #[arg(long = "no-embedding", action = ArgAction::SetTrue)]
        no_embedding: bool,

        /// Rerank top candidates with the cross-encoder reranker.
        #[arg(long)]
        rerank: bool,

        /// Number of candidates to send to the reranker.
        #[arg(long = "rerank-limit", default_value_t = DEFAULT_RERANK_INPUT_LIMIT)]
        rerank_limit: usize,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn index_accepts_clear_path_flag() {
        let cli = Cli::try_parse_from(["locus", "index", "--path", "/repo"]).unwrap();
        let Some(Command::Index {
            path, legacy_path, ..
        }) = cli.command
        else {
            panic!("expected index command");
        };
        assert_eq!(path, PathBuf::from("/repo"));
        assert!(legacy_path.is_none());
    }

    #[test]
    fn index_keeps_legacy_positional_path() {
        let cli = Cli::try_parse_from(["locus", "index", "/repo"]).unwrap();
        let Some(Command::Index {
            path, legacy_path, ..
        }) = cli.command
        else {
            panic!("expected index command");
        };
        assert_eq!(path, PathBuf::from("."));
        assert_eq!(legacy_path, Some(PathBuf::from("/repo")));
    }

    #[test]
    fn search_uses_format_enum() {
        let cli = Cli::try_parse_from([
            "locus",
            "search",
            "token refresh",
            "--format",
            "json",
            "--path",
            "/repo",
        ])
        .unwrap();
        let Some(Command::Search { format, path, .. }) = cli.command else {
            panic!("expected search command");
        };
        assert_eq!(format, OutputFormat::Json);
        assert_eq!(path, PathBuf::from("/repo"));
    }
}
