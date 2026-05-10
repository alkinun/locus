pub mod chunker;
pub mod cli;
pub mod embeddings;
pub mod eval;
pub mod evalgen;
pub mod indexer;
pub mod model;
pub mod output;
pub mod query;
pub mod repo_meta;
pub mod search;
pub mod tui;

#[cfg(test)]
mod integration_tests {
    use tempfile::tempdir;

    use crate::indexer::index_repo;
    use crate::model::ChunkKind;
    use crate::output::group_ranked_results;
    use crate::repo_meta::metadata_path;
    use crate::search::{SearchOptions, SearchSession, search_repo};

    #[test]
    fn indexes_and_searches_fake_repo() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(
            src.join("session.rs"),
            r#"
pub fn refresh_access_token() {
    let token = "new";
    println!("{}", token);
}
"#,
        )
        .expect("write rust file");

        let summary = index_repo(dir.path(), false).expect("index repo");
        assert_eq!(summary.files, 1);
        assert!(summary.chunks >= 1);

        let search = search_repo(dir.path(), "refresh token", 5).expect("search repo");
        assert!(!search.results.is_empty());
        let top = &search.results[0].chunk;
        assert_eq!(top.file_path.to_string_lossy(), "src/session.rs");
        assert_eq!(top.symbol.as_deref(), Some("refresh_access_token"));
    }

    #[test]
    fn search_can_skip_embeddings_when_disabled() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(
            src.join("session.rs"),
            r#"
pub fn refresh_access_token() {
    let token = "new";
    println!("{}", token);
}
"#,
        )
        .expect("write rust file");

        index_repo(dir.path(), false).expect("index repo");
        std::fs::remove_file(dir.path().join(".locus/index/embeddings.bin"))
            .expect("remove embeddings");

        let session = SearchSession::open_with_options(
            dir.path(),
            SearchOptions {
                use_embeddings: false,
            },
        )
        .expect("open without embeddings");
        let search = session.search("refresh token", 5).expect("search repo");
        assert!(!search.results.is_empty());
        assert_eq!(
            search.results[0].chunk.file_path.to_string_lossy(),
            "src/session.rs"
        );
    }

    #[test]
    fn exact_indexed_heading_query_finds_readme() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(
            dir.path().join("README.md"),
            r#"
## Current v0

Supported files:

- Rust: `.rs`
- TypeScript / JavaScript: `.ts`, `.tsx`, `.js`, `.jsx`
- Python: `.py`
- Markdown: `.md`
"#,
        )
        .expect("write readme");
        std::fs::write(
            src.join("chunker.rs"),
            r#"
pub fn chunk_file() {
    let this = "code that mentions are and the filler words";
}
"#,
        )
        .expect("write chunker");
        std::fs::write(
            src.join("search.rs"),
            r#"
pub fn rank_results() {
    let where_is_this = "irrelevant search code";
}
"#,
        )
        .expect("write search");

        index_repo(dir.path(), false).expect("index repo");

        let search = search_repo(dir.path(), "Current v0", 5).expect("search repo");
        assert!(!search.results.is_empty());
        let top_two = search
            .results
            .iter()
            .take(2)
            .map(|result| result.chunk.file_path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(top_two.iter().any(|path| path == "README.md"));
        assert!(search.results[0].reason.contains("important terms"));
    }

    #[test]
    fn syntax_chunks_improve_search_precision() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src");
        let tests = dir.path().join("tests");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::create_dir_all(&tests).expect("tests dir");
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "fixture"
version = "0.1.0"

[dependencies]
regex = "1"
"#,
        )
        .expect("write cargo");
        std::fs::write(
            dir.path().join("README.md"),
            r#"
# Locus

## Supported languages

Rust, TypeScript, JavaScript, Python, and Markdown are supported.
"#,
        )
        .expect("write readme");
        std::fs::write(
            src.join("query.rs"),
            r#"
pub enum QueryIntent {
    FindImplementation,
    FindTests,
}
"#,
        )
        .expect("write query");
        std::fs::write(
            src.join("search.rs"),
            r#"
use crate::query::QueryIntent;

pub struct RankingFeatures {
    score: f32,
}

pub fn rerank(intent: QueryIntent) {
    let ranking_algorithm = "bm25 plus reranking";
    let features = RankingFeatures { score: 1.0 };
    println!("{} {:?}", ranking_algorithm, features.score);
}
"#,
        )
        .expect("write search");
        std::fs::write(
            src.join("chunker.rs"),
            r#"
fn detect_symbol_line(line: &str) -> Option<String> {
    line.strip_prefix("fn ").map(str::to_string)
}

pub fn chunk_file() {
    println!("chunking implementation");
}
"#,
        )
        .expect("write chunker");
        std::fs::write(
            tests.join("chunker_test.rs"),
            r#"
#[test]
fn tests_for_chunking() {
    assert!(true);
}
"#,
        )
        .expect("write test");

        index_repo(dir.path(), false).expect("index repo");
        assert!(metadata_path(dir.path()).exists());

        let ranking =
            search_repo(dir.path(), "where does ranking happen", 5).expect("search ranking");
        assert!(ranking.results.iter().take(2).any(|result| {
            result.chunk.file_path.to_string_lossy() == "src/search.rs"
                && result.chunk.symbol.as_deref() == Some("rerank")
        }));

        let languages =
            search_repo(dir.path(), "what languages are supported", 5).expect("search docs");
        assert!(languages.results.iter().take(2).any(|result| {
            result.chunk.file_path.to_string_lossy() == "README.md"
                && result.chunk.kind == ChunkKind::MarkdownSection
        }));

        let test_results = search_repo(dir.path(), "tests for chunking", 5).expect("search tests");
        assert!(test_results.results.iter().take(2).any(|result| {
            result.chunk.file_path.to_string_lossy() == "tests/chunker_test.rs"
                && result.chunk.kind == ChunkKind::Test
        }));

        let symbol_detection = search_repo(dir.path(), "where is symbol detection implemented", 5)
            .expect("search symbol detection");
        assert!(
            symbol_detection
                .results
                .iter()
                .take(2)
                .any(|result| result.chunk.symbol.as_deref() == Some("detect_symbol_line"))
        );

        let query_intent =
            search_repo(dir.path(), "where is query intent used", 5).expect("search query intent");
        let grouped = group_ranked_results(&query_intent.results);
        assert!(
            grouped
                .primary
                .iter()
                .chain(grouped.supporting.iter())
                .any(|result| result.symbol.as_deref() == Some("QueryIntent"))
        );
    }
}
