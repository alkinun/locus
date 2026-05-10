use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{CodeChunk, RankedChunk};
use crate::search::{SearchOptions, SearchSession};

#[derive(Debug, Clone)]
pub struct EvalOptions {
    pub path: PathBuf,
    pub dataset: PathBuf,
    pub limit: usize,
    pub use_embeddings: bool,
    pub failures: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalDataset {
    #[serde(default)]
    pub version: Option<u8>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub items: Vec<EvalDatasetItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalDatasetItem {
    #[serde(default)]
    pub id: String,
    pub query: String,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub expected: Vec<ExpectedMatch>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpectedMatch {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub chunk_id: Option<String>,
    #[serde(default)]
    pub relevance: Option<u32>,
    #[serde(default)]
    pub path_contains: Option<String>,
    #[serde(default)]
    pub symbol_contains: Option<String>,
    #[serde(default)]
    pub text_contains: Option<String>,
    #[serde(default)]
    pub signature_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub dataset: String,
    pub repo: String,
    pub queries: usize,
    pub limit: usize,
    pub use_embeddings: bool,
    pub overall: MetricSummary,
    pub by_style: BTreeMap<String, MetricSummary>,
    pub by_intent: BTreeMap<String, MetricSummary>,
    pub failures: Vec<FailureReport>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MetricSummary {
    pub n: usize,
    pub recall_at_1: f64,
    pub recall_at_3: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub latency_p50_ms: u128,
    pub latency_p95_ms: u128,
    pub latency_max_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureReport {
    pub id: String,
    pub query: String,
    pub style: Option<String>,
    pub intent: Option<String>,
    pub first_relevant_rank: Option<usize>,
    pub reciprocal_rank: f64,
    pub ndcg_at_10: f64,
    pub expected: Vec<ExpectedMatch>,
    pub top_results: Vec<EvalTopResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalTopResult {
    pub rank: usize,
    pub path: String,
    pub symbol: Option<String>,
    pub kind: String,
    pub chunk_id: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
struct QueryEval {
    item: EvalDatasetItem,
    recall_at_1: f64,
    recall_at_3: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    reciprocal_rank: f64,
    ndcg_at_5: f64,
    ndcg_at_10: f64,
    latency: Duration,
    first_relevant_rank: Option<usize>,
    top_results: Vec<EvalTopResult>,
}

pub fn run_eval(options: EvalOptions) -> Result<EvalReport> {
    let repo_root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", options.path.display()))?;
    let dataset = read_dataset(&options.dataset)?;
    let session = SearchSession::open_with_options(
        &repo_root,
        SearchOptions {
            use_embeddings: options.use_embeddings,
        },
    )?;
    let mut query_evals = Vec::with_capacity(dataset.items.len());

    for item in dataset.items {
        let search = session.search(&item.query, options.limit)?;
        query_evals.push(evaluate_query(item, search.results, search.elapsed));
    }

    let failures = select_failures(&query_evals, options.failures);
    Ok(EvalReport {
        dataset: options.dataset.display().to_string(),
        repo: repo_root.display().to_string(),
        queries: query_evals.len(),
        limit: options.limit,
        use_embeddings: options.use_embeddings,
        overall: summarize_metrics(&query_evals),
        by_style: summarize_by(&query_evals, |item| item.style.as_deref()),
        by_intent: summarize_by(&query_evals, |item| item.intent.as_deref()),
        failures,
    })
}

pub fn read_dataset(path: &Path) -> Result<EvalDataset> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn evaluate_query(
    item: EvalDatasetItem,
    results: Vec<RankedChunk>,
    latency: Duration,
) -> QueryEval {
    let relevance_by_rank = results
        .iter()
        .map(|ranked| relevance_for_chunk(&ranked.chunk, &item.expected))
        .collect::<Vec<_>>();
    let first_relevant_rank = relevance_by_rank
        .iter()
        .position(|relevance| *relevance > 0)
        .map(|idx| idx + 1);
    let reciprocal_rank = first_relevant_rank
        .map(|rank| 1.0 / rank as f64)
        .unwrap_or(0.0);
    let top_results = results
        .iter()
        .enumerate()
        .take(10)
        .map(|(idx, ranked)| top_result(idx + 1, ranked))
        .collect::<Vec<_>>();

    QueryEval {
        recall_at_1: recall_at(first_relevant_rank, 1),
        recall_at_3: recall_at(first_relevant_rank, 3),
        recall_at_5: recall_at(first_relevant_rank, 5),
        recall_at_10: recall_at(first_relevant_rank, 10),
        reciprocal_rank,
        ndcg_at_5: ndcg_at(&relevance_by_rank, &item.expected, 5),
        ndcg_at_10: ndcg_at(&relevance_by_rank, &item.expected, 10),
        latency,
        first_relevant_rank,
        top_results,
        item,
    }
}

pub fn expected_matches_chunk(expected: &ExpectedMatch, chunk: &CodeChunk) -> bool {
    if let Some(chunk_id) = non_empty(expected.chunk_id.as_deref()) {
        if chunk.id == chunk_id {
            return true;
        }
    }

    let mut has_matcher = false;
    if let Some(path) = non_empty(expected.path.as_deref()) {
        has_matcher = true;
        if normalize_path(&chunk.file_path.display().to_string()) != normalize_path(path) {
            return false;
        }
    }
    if let Some(symbol) = non_empty(expected.symbol.as_deref()) {
        has_matcher = true;
        if chunk.symbol.as_deref() != Some(symbol) {
            return false;
        }
    }
    if let Some(kind) = non_empty(expected.kind.as_deref()) {
        has_matcher = true;
        if chunk.kind.as_str() != kind {
            return false;
        }
    }
    if let Some(path_contains) = non_empty(expected.path_contains.as_deref()) {
        has_matcher = true;
        if !normalize_path(&chunk.file_path.display().to_string())
            .contains(&normalize_path(path_contains))
        {
            return false;
        }
    }
    if let Some(symbol_contains) = non_empty(expected.symbol_contains.as_deref()) {
        has_matcher = true;
        if !chunk
            .symbol
            .as_deref()
            .unwrap_or_default()
            .contains(symbol_contains)
        {
            return false;
        }
    }
    if let Some(text_contains) = non_empty(expected.text_contains.as_deref()) {
        has_matcher = true;
        if !chunk.text.contains(text_contains) {
            return false;
        }
    }
    if let Some(signature_contains) = non_empty(expected.signature_contains.as_deref()) {
        has_matcher = true;
        if !chunk
            .signature
            .as_deref()
            .unwrap_or_default()
            .contains(signature_contains)
        {
            return false;
        }
    }
    has_matcher
}

fn relevance_for_chunk(chunk: &CodeChunk, expected: &[ExpectedMatch]) -> u32 {
    expected
        .iter()
        .filter(|expected| expected_matches_chunk(expected, chunk))
        .map(|expected| expected.relevance.unwrap_or(3))
        .max()
        .unwrap_or(0)
}

pub fn recall_at(first_relevant_rank: Option<usize>, k: usize) -> f64 {
    if first_relevant_rank.is_some_and(|rank| rank <= k) {
        1.0
    } else {
        0.0
    }
}

pub fn reciprocal_rank(first_relevant_rank: Option<usize>) -> f64 {
    first_relevant_rank
        .map(|rank| 1.0 / rank as f64)
        .unwrap_or(0.0)
}

pub fn ndcg_at(relevance_by_rank: &[u32], expected: &[ExpectedMatch], k: usize) -> f64 {
    let dcg = relevance_by_rank
        .iter()
        .take(k)
        .enumerate()
        .map(|(idx, relevance)| discounted_gain(*relevance, idx + 1))
        .sum::<f64>();
    let mut ideal = expected
        .iter()
        .map(|expected| expected.relevance.unwrap_or(3))
        .collect::<Vec<_>>();
    ideal.sort_by(|a, b| b.cmp(a));
    let idcg = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(idx, relevance)| discounted_gain(*relevance, idx + 1))
        .sum::<f64>();
    if idcg == 0.0 {
        0.0
    } else {
        (dcg / idcg).min(1.0)
    }
}

pub fn percentile_latency(mut latencies: Vec<Duration>, percentile: f64) -> u128 {
    if latencies.is_empty() {
        return 0;
    }
    latencies.sort();
    let rank = ((latencies.len() as f64 - 1.0) * percentile).ceil() as usize;
    latencies[rank.min(latencies.len() - 1)].as_millis()
}

fn discounted_gain(relevance: u32, rank: usize) -> f64 {
    if relevance == 0 {
        return 0.0;
    }
    ((1u32 << relevance.min(30)) - 1) as f64 / (rank as f64 + 1.0).log2()
}

fn summarize_metrics(items: &[QueryEval]) -> MetricSummary {
    if items.is_empty() {
        return MetricSummary::default();
    }
    let n = items.len();
    let latencies = items
        .iter()
        .map(|item| item.latency)
        .collect::<Vec<Duration>>();
    MetricSummary {
        n,
        recall_at_1: mean(items.iter().map(|item| item.recall_at_1)),
        recall_at_3: mean(items.iter().map(|item| item.recall_at_3)),
        recall_at_5: mean(items.iter().map(|item| item.recall_at_5)),
        recall_at_10: mean(items.iter().map(|item| item.recall_at_10)),
        mrr: mean(items.iter().map(|item| item.reciprocal_rank)),
        ndcg_at_5: mean(items.iter().map(|item| item.ndcg_at_5)),
        ndcg_at_10: mean(items.iter().map(|item| item.ndcg_at_10)),
        latency_p50_ms: percentile_latency(latencies.clone(), 0.50),
        latency_p95_ms: percentile_latency(latencies.clone(), 0.95),
        latency_max_ms: latencies
            .into_iter()
            .max()
            .map(|latency| latency.as_millis())
            .unwrap_or(0),
    }
}

fn summarize_by(
    items: &[QueryEval],
    key_for: impl Fn(&EvalDatasetItem) -> Option<&str>,
) -> BTreeMap<String, MetricSummary> {
    let mut grouped = BTreeMap::<String, Vec<QueryEval>>::new();
    for item in items {
        let key = key_for(&item.item).unwrap_or("unknown").to_string();
        grouped.entry(key).or_default().push(item.clone());
    }
    grouped
        .into_iter()
        .map(|(key, items)| (key, summarize_metrics(&items)))
        .collect()
}

fn select_failures(items: &[QueryEval], limit: usize) -> Vec<FailureReport> {
    let mut candidates = items
        .iter()
        .filter(|item| item.first_relevant_rank.is_none_or(|rank| rank > 5))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        let a_hard = a.first_relevant_rank.is_none();
        let b_hard = b.first_relevant_rank.is_none();
        b_hard
            .cmp(&a_hard)
            .then_with(|| a.reciprocal_rank.total_cmp(&b.reciprocal_rank))
            .then_with(|| a.ndcg_at_10.total_cmp(&b.ndcg_at_10))
    });
    candidates
        .into_iter()
        .take(limit)
        .map(|item| FailureReport {
            id: item.item.id,
            query: item.item.query,
            style: item.item.style,
            intent: item.item.intent,
            first_relevant_rank: item.first_relevant_rank,
            reciprocal_rank: item.reciprocal_rank,
            ndcg_at_10: item.ndcg_at_10,
            expected: item.item.expected,
            top_results: item.top_results,
        })
        .collect()
}

fn top_result(rank: usize, ranked: &RankedChunk) -> EvalTopResult {
    EvalTopResult {
        rank,
        path: normalize_path(&ranked.chunk.file_path.display().to_string()),
        symbol: ranked.chunk.symbol.clone(),
        kind: ranked.chunk.kind.as_str().to_string(),
        chunk_id: ranked.chunk.id.clone(),
        score: ranked.score,
    }
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for value in values {
        total += value;
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn print_human_report(report: &EvalReport) {
    println!("locus eval");
    println!();
    println!("Dataset: {}", report.dataset);
    println!("Repo: {}", report.repo);
    println!("Queries: {}", report.queries);
    println!("Limit: {}", report.limit);
    println!(
        "Embeddings: {}",
        if report.use_embeddings {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!();
    print_summary("Overall:", &report.overall);
    println!();
    print_grouped_summary("By style:", &report.by_style);
    println!();
    print_grouped_summary("By intent:", &report.by_intent);
    if !report.failures.is_empty() {
        println!();
        println!("Worst failures:");
        for failure in &report.failures {
            print_failure(failure);
        }
    }
}

fn print_summary(title: &str, metrics: &MetricSummary) {
    println!("{title}");
    println!("  Recall@1:   {}", percent(metrics.recall_at_1));
    println!("  Recall@3:   {}", percent(metrics.recall_at_3));
    println!("  Recall@5:   {}", percent(metrics.recall_at_5));
    println!("  Recall@10:  {}", percent(metrics.recall_at_10));
    println!("  MRR:        {:.2}", metrics.mrr);
    println!("  nDCG@5:     {:.2}", metrics.ndcg_at_5);
    println!("  nDCG@10:    {:.2}", metrics.ndcg_at_10);
    println!("  p50:        {} ms", metrics.latency_p50_ms);
    println!("  p95:        {} ms", metrics.latency_p95_ms);
    println!("  max:        {} ms", metrics.latency_max_ms);
}

fn print_grouped_summary(title: &str, grouped: &BTreeMap<String, MetricSummary>) {
    println!("{title}");
    for (key, metrics) in grouped {
        println!(
            "  {:22} n={:<4} R@5 {:>6}   MRR {:.2}   nDCG@5 {:.2}",
            key,
            metrics.n,
            percent(metrics.recall_at_5),
            metrics.mrr,
            metrics.ndcg_at_5
        );
    }
}

fn print_failure(failure: &FailureReport) {
    let style = failure.style.as_deref().unwrap_or("unknown");
    println!("  {} [{}]", failure.id, style);
    println!("    query: {}", failure.query);
    println!("    expected:");
    for expected in &failure.expected {
        println!("      - {}", format_expected(expected));
    }
    println!("    top results:");
    for result in &failure.top_results {
        println!(
            "      {}. {} {} {} score={:.2}",
            result.rank,
            result.path,
            result.kind,
            result.symbol.as_deref().unwrap_or("-"),
            result.score
        );
    }
}

fn format_expected(expected: &ExpectedMatch) -> String {
    let mut parts = Vec::new();
    if let Some(path) = &expected.path {
        parts.push(path.clone());
    }
    if let Some(symbol) = &expected.symbol {
        parts.push(format!("symbol={symbol}"));
    }
    if let Some(kind) = &expected.kind {
        parts.push(format!("kind={kind}"));
    }
    if let Some(chunk_id) = &expected.chunk_id {
        parts.push(format!("chunk_id={chunk_id}"));
    }
    parts.push(format!("relevance={}", expected.relevance.unwrap_or(3)));
    parts.join(" ")
}

fn percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChunkKind;

    fn chunk(id: &str, path: &str, symbol: Option<&str>, kind: ChunkKind) -> CodeChunk {
        CodeChunk {
            id: id.to_string(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from(path),
            language: "rust".to_string(),
            kind,
            symbol: symbol.map(str::to_string),
            signature: Some("fn target()".to_string()),
            parent_symbol: None,
            start_line: 1,
            end_line: 5,
            doc_comment: String::new(),
            callees: Vec::new(),
            sibling_symbols: Vec::new(),
            text: "fn target() { search(); }".to_string(),
        }
    }

    #[test]
    fn matcher_uses_chunk_id() {
        let expected = ExpectedMatch {
            chunk_id: Some("abc".to_string()),
            path: Some("wrong.rs".to_string()),
            ..empty_expected()
        };
        assert!(expected_matches_chunk(
            &expected,
            &chunk("abc", "src/lib.rs", Some("target"), ChunkKind::Function)
        ));
    }

    #[test]
    fn matcher_falls_back_when_chunk_id_is_stale() {
        let expected = ExpectedMatch {
            chunk_id: Some("old-id".to_string()),
            path: Some("src/lib.rs".to_string()),
            symbol: Some("target".to_string()),
            kind: Some("function".to_string()),
            ..empty_expected()
        };
        assert!(expected_matches_chunk(
            &expected,
            &chunk("new-id", "src/lib.rs", Some("target"), ChunkKind::Function)
        ));
    }

    #[test]
    fn matcher_uses_path_and_symbol() {
        let expected = ExpectedMatch {
            path: Some("src/lib.rs".to_string()),
            symbol: Some("target".to_string()),
            ..empty_expected()
        };
        assert!(expected_matches_chunk(
            &expected,
            &chunk("abc", "src/lib.rs", Some("target"), ChunkKind::Function)
        ));
        assert!(!expected_matches_chunk(
            &expected,
            &chunk("abc", "src/lib.rs", Some("other"), ChunkKind::Function)
        ));
    }

    #[test]
    fn matcher_uses_path_only() {
        let expected = ExpectedMatch {
            path: Some("src/lib.rs".to_string()),
            ..empty_expected()
        };
        assert!(expected_matches_chunk(
            &expected,
            &chunk("abc", "src/lib.rs", None, ChunkKind::Unknown)
        ));
    }

    #[test]
    fn matcher_supports_contains_fields() {
        let expected = ExpectedMatch {
            path_contains: Some("src".to_string()),
            symbol_contains: Some("tar".to_string()),
            text_contains: Some("search".to_string()),
            signature_contains: Some("target".to_string()),
            ..empty_expected()
        };
        assert!(expected_matches_chunk(
            &expected,
            &chunk("abc", "src/lib.rs", Some("target"), ChunkKind::Function)
        ));
    }

    #[test]
    fn recall_at_calculates_hits() {
        assert_eq!(recall_at(Some(3), 1), 0.0);
        assert_eq!(recall_at(Some(3), 3), 1.0);
        assert_eq!(recall_at(None, 10), 0.0);
    }

    #[test]
    fn reciprocal_rank_calculates_first_hit_score() {
        assert_eq!(reciprocal_rank(Some(4)), 0.25);
        assert_eq!(reciprocal_rank(None), 0.0);
    }

    #[test]
    fn ndcg_calculates_discounted_gain() {
        let expected = vec![ExpectedMatch {
            relevance: Some(3),
            ..empty_expected()
        }];
        assert!((ndcg_at(&[3], &expected, 5) - 1.0).abs() < f64::EPSILON);
        assert!(ndcg_at(&[0, 3], &expected, 5) < 1.0);
    }

    #[test]
    fn percentile_latency_calculates_percentiles() {
        let latencies = vec![
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(10),
        ];
        assert_eq!(percentile_latency(latencies.clone(), 0.50), 2);
        assert_eq!(percentile_latency(latencies, 0.95), 10);
    }

    #[test]
    fn grouping_metrics_by_style() {
        let items = vec![
            QueryEval {
                item: item("1", "StyleA", "implementation"),
                recall_at_1: 1.0,
                recall_at_3: 1.0,
                recall_at_5: 1.0,
                recall_at_10: 1.0,
                reciprocal_rank: 1.0,
                ndcg_at_5: 1.0,
                ndcg_at_10: 1.0,
                latency: Duration::from_millis(1),
                first_relevant_rank: Some(1),
                top_results: Vec::new(),
            },
            QueryEval {
                item: item("2", "StyleA", "implementation"),
                recall_at_1: 0.0,
                recall_at_3: 0.0,
                recall_at_5: 0.0,
                recall_at_10: 0.0,
                reciprocal_rank: 0.0,
                ndcg_at_5: 0.0,
                ndcg_at_10: 0.0,
                latency: Duration::from_millis(3),
                first_relevant_rank: None,
                top_results: Vec::new(),
            },
        ];
        let grouped = summarize_by(&items, |item| item.style.as_deref());
        assert_eq!(grouped["StyleA"].n, 2);
        assert_eq!(grouped["StyleA"].recall_at_5, 0.5);
    }

    #[test]
    fn yaml_parses_minimal_dataset() {
        let dataset: EvalDataset = serde_yaml::from_str(
            r#"
version: 1
items:
  - id: one
    query: where is search implemented
    expected:
      - path: src/search.rs
"#,
        )
        .expect("dataset");
        assert_eq!(dataset.items.len(), 1);
        assert_eq!(
            dataset.items[0].expected[0].path.as_deref(),
            Some("src/search.rs")
        );
    }

    fn empty_expected() -> ExpectedMatch {
        ExpectedMatch {
            path: None,
            symbol: None,
            kind: None,
            chunk_id: None,
            relevance: None,
            path_contains: None,
            symbol_contains: None,
            text_contains: None,
            signature_contains: None,
        }
    }

    fn item(id: &str, style: &str, intent: &str) -> EvalDatasetItem {
        EvalDatasetItem {
            id: id.to_string(),
            query: "query".to_string(),
            intent: Some(intent.to_string()),
            difficulty: None,
            style: Some(style.to_string()),
            expected: Vec::new(),
            notes: None,
        }
    }
}
