use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::model::{ChunkKind, CodeChunk};
use crate::search::load_indexed_chunks;

const MAX_PROMPT_CHARS: usize = 10_000;
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct GenerateEvalOptions {
    pub path: PathBuf,
    pub out: PathBuf,
    pub count: usize,
    pub endpoint: String,
    pub model: String,
    pub seed: u64,
    pub concurrency: usize,
}

#[derive(Debug, Clone)]
pub struct GenerateEvalSummary {
    pub generated: usize,
    pub skipped: usize,
    pub style_counts: BTreeMap<QueryStyle, usize>,
    pub skip_reasons: BTreeMap<String, usize>,
    pub skip_examples: BTreeMap<String, String>,
    pub out: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum QueryStyle {
    LiteralSymbol,
    FuzzyImplementation,
    NewContributor,
    ChangeTarget,
    DebuggingSymptom,
    Architecture,
    Capability,
    TestFinding,
    ConfigFinding,
    DocsQuestion,
    UsageQuestion,
    DefinitionQuestion,
    CasualVague,
    AgentTask,
}

impl QueryStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LiteralSymbol => "LiteralSymbol",
            Self::FuzzyImplementation => "FuzzyImplementation",
            Self::NewContributor => "NewContributor",
            Self::ChangeTarget => "ChangeTarget",
            Self::DebuggingSymptom => "DebuggingSymptom",
            Self::Architecture => "Architecture",
            Self::Capability => "Capability",
            Self::TestFinding => "TestFinding",
            Self::ConfigFinding => "ConfigFinding",
            Self::DocsQuestion => "DocsQuestion",
            Self::UsageQuestion => "UsageQuestion",
            Self::DefinitionQuestion => "DefinitionQuestion",
            Self::CasualVague => "CasualVague",
            Self::AgentTask => "AgentTask",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EvalItem {
    pub id: String,
    pub query: String,
    pub intent: String,
    pub difficulty: String,
    pub style: String,
    pub expected: Vec<ExpectedResult>,
    pub notes: String,
}

#[derive(Debug, Serialize)]
pub struct ExpectedResult {
    pub path: String,
    pub symbol: Option<String>,
    pub kind: String,
    pub chunk_id: String,
    pub relevance: u8,
}

#[derive(Debug, Deserialize)]
struct ModelEvalItem {
    query: String,
    intent: String,
    difficulty: String,
    notes: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

struct WorkItem {
    chunk: CodeChunk,
    style: QueryStyle,
}

struct WorkResult {
    style: QueryStyle,
    result: Result<EvalItem>,
}

pub fn generate_eval_dataset(options: GenerateEvalOptions) -> Result<GenerateEvalSummary> {
    let repo_root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", options.path.display()))?;
    let chunks = load_indexed_chunks(&repo_root).map_err(|err| {
        if err.to_string().contains("No locus index found") {
            anyhow!(
                "No locus index found. Run: locus index {}",
                options.path.display()
            )
        } else {
            err
        }
    })?;

    let mut rng = StdRng::seed_from_u64(options.seed);
    let sampled = sample_chunks(
        chunks,
        options.count.saturating_mul(4).max(options.count),
        &mut rng,
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;
    if let Some(parent) = options
        .out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    initialize_output_file(&options.out, &repo_root, &options.model)?;

    let mut seen_queries = HashSet::new();
    let mut style_counts = BTreeMap::new();
    let mut skip_reasons = BTreeMap::new();
    let mut skip_examples = BTreeMap::new();
    let mut generated = 0usize;

    let mut work = sampled
        .into_iter()
        .map(|chunk| {
            let style = compatible_style(chunk.kind, &mut rng);
            WorkItem { chunk, style }
        })
        .collect::<Vec<_>>()
        .into_iter();
    let concurrency = options.concurrency.max(1);
    let (tx, rx) = mpsc::channel::<WorkResult>();
    let mut in_flight = 0usize;

    while in_flight > 0 || generated < options.count {
        while generated + in_flight < options.count && in_flight < concurrency {
            let Some(item) = work.next() else {
                break;
            };
            let tx = tx.clone();
            let client = client.clone();
            let options = options.clone();
            let repo_root = repo_root.clone();
            in_flight += 1;
            thread::spawn(move || {
                let result =
                    generate_item_for_chunk(&client, &options, &repo_root, &item.chunk, item.style);
                let _ = tx.send(WorkResult {
                    style: item.style,
                    result,
                });
            });
        }

        if in_flight == 0 {
            break;
        }

        let result = rx.recv().context("generator worker channel closed")?;
        in_flight -= 1;
        match result.result {
            Ok(mut item) => {
                let normalized_query = item.query.to_lowercase();
                if seen_queries.insert(normalized_query) {
                    generated += 1;
                    item.id = format!("synthetic_{generated:04}");
                    *style_counts.entry(result.style).or_default() += 1;
                    append_eval_item(&options.out, &item)?;
                } else {
                    record_skip(
                        anyhow!("duplicate query"),
                        &mut skip_reasons,
                        &mut skip_examples,
                    );
                }
            }
            Err(err) => record_skip(err, &mut skip_reasons, &mut skip_examples),
        }
    }

    let skipped = options.count.saturating_sub(generated);

    Ok(GenerateEvalSummary {
        generated,
        skipped,
        style_counts,
        skip_reasons,
        skip_examples,
        out: options.out,
    })
}

fn generate_item_for_chunk(
    client: &reqwest::blocking::Client,
    options: &GenerateEvalOptions,
    repo_root: &Path,
    chunk: &CodeChunk,
    style: QueryStyle,
) -> Result<EvalItem> {
    let mut last_error = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let prompt = prompt_for_chunk(chunk, style, attempt);
        let response = call_chat(client, &options.endpoint, &options.model, prompt)?;
        let candidate = match parse_model_json(&response) {
            Ok(candidate) => candidate,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        match validate_model_item_fields(&candidate, chunk, style) {
            Ok(()) => return Ok(to_eval_item(repo_root, chunk, style, candidate)),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("model did not produce a valid item")))
}

fn call_chat(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    model: &str,
    prompt: String,
) -> Result<String> {
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: "system",
                content: "You generate strict JSON for code search eval datasets.".to_string(),
            },
            ChatMessage {
                role: "user",
                content: prompt,
            },
        ],
        temperature: 0.7,
    };
    let response = client
        .post(endpoint)
        .json(&request)
        .send()
        .with_context(|| format!("failed to call {endpoint}"))?
        .error_for_status()
        .with_context(|| format!("endpoint returned an error: {endpoint}"))?
        .json::<ChatResponse>()?;
    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| anyhow!("chat response had no choices"))
}

fn parse_model_json(text: &str) -> Result<ModelEvalItem> {
    let trimmed = extract_json_object(text).unwrap_or(text).trim();
    serde_json::from_str(trimmed).with_context(|| "model output was not strict JSON")
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (start < end).then_some(&text[start..=end])
}

fn to_eval_item(
    repo_root: &Path,
    chunk: &CodeChunk,
    style: QueryStyle,
    model_item: ModelEvalItem,
) -> EvalItem {
    let path = relative_path(repo_root, &chunk.file_path);
    EvalItem {
        id: String::new(),
        query: model_item.query,
        intent: model_item.intent,
        difficulty: model_item.difficulty,
        style: style.as_str().to_string(),
        expected: vec![ExpectedResult {
            path,
            symbol: chunk.symbol.clone(),
            kind: chunk.kind.as_str().to_string(),
            chunk_id: chunk.id.clone(),
            relevance: 3,
        }],
        notes: model_item.notes,
    }
}

fn prompt_for_chunk(chunk: &CodeChunk, style: QueryStyle, attempt: usize) -> String {
    let stricter = if attempt > 1 {
        "\nPrevious output was invalid. Be stricter: return only one JSON object, keep the query specific, and do not leak disallowed symbols or paths."
    } else {
        ""
    };
    format!(
        r#"Create one realistic developer code-search query for this chunk.

Output strict JSON only:
{{"query":"where does locus decide which directories not to scan?","intent":"implementation","difficulty":"medium","notes":"Natural-language query for the matching implementation."}}

Rules:
- The query should sound like something a developer would type into a code search tool.
- The query should be answerable primarily by the provided chunk.
- The query should be natural language, not a docstring.
- Avoid copying the exact symbol name unless the style is LiteralSymbol or DefinitionQuestion.
- Avoid copying the exact file path unless the style requires it.
- Prefer fuzzy wording for non-literal styles.
- Keep query length between 4 and 18 words.
- Output JSON only.{stricter}

Query style: {style}
Chunk path: {path}
Chunk kind: {kind}
Chunk symbol: {symbol}
Chunk signature: {signature}
Chunk text:
```text
{text}
```"#,
        style = style.as_str(),
        path = chunk.file_path.display(),
        kind = chunk.kind.as_str(),
        symbol = chunk.symbol.as_deref().unwrap_or("-"),
        signature = chunk.signature.as_deref().unwrap_or("-"),
        text = truncate_chars(&chunk.text, MAX_PROMPT_CHARS),
    )
}

pub fn compatible_style(kind: ChunkKind, rng: &mut impl Rng) -> QueryStyle {
    let styles: &[QueryStyle] = match kind {
        ChunkKind::Test => &[
            QueryStyle::TestFinding,
            QueryStyle::DebuggingSymptom,
            QueryStyle::LiteralSymbol,
        ],
        ChunkKind::MarkdownSection => &[
            QueryStyle::DocsQuestion,
            QueryStyle::Capability,
            QueryStyle::NewContributor,
        ],
        ChunkKind::Config => &[QueryStyle::ConfigFinding, QueryStyle::ChangeTarget],
        ChunkKind::Function | ChunkKind::Method | ChunkKind::Impl | ChunkKind::Module => &[
            QueryStyle::FuzzyImplementation,
            QueryStyle::ChangeTarget,
            QueryStyle::DebuggingSymptom,
            QueryStyle::AgentTask,
            QueryStyle::LiteralSymbol,
            QueryStyle::CasualVague,
        ],
        ChunkKind::Struct | ChunkKind::Enum | ChunkKind::Trait | ChunkKind::Class => &[
            QueryStyle::DefinitionQuestion,
            QueryStyle::Architecture,
            QueryStyle::UsageQuestion,
            QueryStyle::LiteralSymbol,
        ],
        ChunkKind::Unknown => &[
            QueryStyle::FuzzyImplementation,
            QueryStyle::ChangeTarget,
            QueryStyle::CasualVague,
        ],
    };
    *styles.choose(rng).expect("non-empty style list")
}

#[cfg(test)]
fn validate_model_item(
    item: &ModelEvalItem,
    chunk: &CodeChunk,
    style: QueryStyle,
    seen_queries: &mut HashSet<String>,
) -> Result<()> {
    validate_model_item_fields(item, chunk, style)?;
    let normalized = item.query.trim().to_lowercase();
    if !seen_queries.insert(normalized) {
        return Err(anyhow!("duplicate query"));
    }
    Ok(())
}

fn validate_model_item_fields(
    item: &ModelEvalItem,
    chunk: &CodeChunk,
    style: QueryStyle,
) -> Result<()> {
    let query = item.query.trim();
    if query.is_empty() || item.intent.trim().is_empty() || item.difficulty.trim().is_empty() {
        return Err(anyhow!("empty required field"));
    }
    let words = query.split_whitespace().count();
    if !(4..=18).contains(&words) {
        return Err(anyhow!("query must be 4 to 18 words"));
    }
    if is_generic_query(query) {
        return Err(anyhow!("query is too generic"));
    }
    if !allows_exact_symbol(style) {
        if let Some(symbol) = &chunk.symbol {
            if contains_exact(query, symbol) {
                return Err(anyhow!("query leaked exact symbol"));
            }
        }
    }
    if !allows_exact_path(style) && contains_exact(query, &chunk.file_path.display().to_string()) {
        return Err(anyhow!("query leaked exact file path"));
    }
    Ok(())
}

fn sample_chunks(mut chunks: Vec<CodeChunk>, count: usize, rng: &mut StdRng) -> Vec<CodeChunk> {
    chunks.retain(is_sample_candidate);
    chunks.sort_by(|a, b| chunk_quality(b).cmp(&chunk_quality(a)));
    let better_count = chunks
        .iter()
        .filter(|chunk| chunk.kind != ChunkKind::Unknown)
        .count();
    if better_count >= count {
        chunks.retain(|chunk| chunk.kind != ChunkKind::Unknown);
    }

    let mut seen_symbols = HashSet::new();
    chunks.retain(|chunk| {
        let Some(symbol) = &chunk.symbol else {
            return true;
        };
        seen_symbols.insert(symbol.to_lowercase())
    });
    chunks.shuffle(rng);
    chunks.truncate(count);
    chunks
}

fn is_sample_candidate(chunk: &CodeChunk) -> bool {
    let path = chunk.file_path.to_string_lossy().to_lowercase();
    if is_generated_or_vendor_path(&path) {
        return false;
    }
    let meaningful_lines = chunk
        .text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    (5..=200).contains(&meaningful_lines)
}

fn is_generated_or_vendor_path(path: &str) -> bool {
    path.contains("vendor/")
        || path.contains("node_modules/")
        || path.contains("target/")
        || path.contains("dist/")
        || path.contains("build/")
        || path.contains("generated")
        || path.ends_with(".min.js")
}

fn chunk_quality(chunk: &CodeChunk) -> u8 {
    match chunk.kind {
        ChunkKind::Function | ChunkKind::Method | ChunkKind::Test => 5,
        ChunkKind::Struct | ChunkKind::Class | ChunkKind::Enum | ChunkKind::Trait => 4,
        ChunkKind::MarkdownSection | ChunkKind::Config | ChunkKind::Impl => 3,
        ChunkKind::Module => 2,
        ChunkKind::Unknown => 1,
    }
}

fn allows_exact_symbol(style: QueryStyle) -> bool {
    matches!(
        style,
        QueryStyle::LiteralSymbol | QueryStyle::DefinitionQuestion
    )
}

fn allows_exact_path(style: QueryStyle) -> bool {
    matches!(style, QueryStyle::ConfigFinding | QueryStyle::DocsQuestion)
}

fn contains_exact(haystack: &str, needle: &str) -> bool {
    !needle.trim().is_empty() && haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn is_generic_query(query: &str) -> bool {
    let normalized = query.trim().to_lowercase();
    matches!(
        normalized.as_str(),
        "where is this implemented"
            | "how does this work"
            | "what does this do"
            | "find this code"
            | "show me this code"
    )
}

fn relative_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut output = text.chars().take(max_chars).collect::<String>();
    output.push_str("\n...");
    output
}

fn initialize_output_file(path: &Path, repo_root: &Path, model: &str) -> Result<()> {
    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    writeln!(file, "version: 1")?;
    writeln!(
        file,
        "repo: {}",
        serde_yaml::to_string(&repo_root.display().to_string())?.trim()
    )?;
    writeln!(file, "source: synthetic")?;
    writeln!(file, "model: {}", serde_yaml::to_string(model)?.trim())?;
    writeln!(file, "items:")?;
    Ok(())
}

fn append_eval_item(path: &Path, item: &EvalItem) -> Result<()> {
    let yaml = serde_yaml::to_string(item)?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .with_context(|| format!("failed to append {}", path.display()))?;

    let mut lines = yaml.lines();
    if let Some(first) = lines.next() {
        writeln!(file, "  - {first}")?;
    }
    for line in lines {
        writeln!(file, "    {line}")?;
    }
    Ok(())
}

fn record_skip(
    err: anyhow::Error,
    skip_reasons: &mut BTreeMap<String, usize>,
    skip_examples: &mut BTreeMap<String, String>,
) {
    let reason = normalize_skip_reason(&err);
    *skip_reasons.entry(reason.clone()).or_default() += 1;
    skip_examples
        .entry(reason)
        .or_insert_with(|| format!("{err:#}"));
}

fn normalize_skip_reason(err: &anyhow::Error) -> String {
    let message = err.to_string();
    if message.contains("strict JSON") {
        "model output was not strict JSON".to_string()
    } else if message.contains("endpoint returned an error") {
        "endpoint returned an error".to_string()
    } else if message.contains("failed to call") {
        "request to endpoint failed".to_string()
    } else if message.contains("duplicate query") {
        "duplicate query".to_string()
    } else if message.contains("exact symbol") {
        "query leaked exact symbol".to_string()
    } else if message.contains("exact file path") {
        "query leaked exact file path".to_string()
    } else if message.contains("4 to 18 words") {
        "query length invalid".to_string()
    } else if message.contains("generic") {
        "query too generic".to_string()
    } else if message.contains("empty required field") {
        "empty required field".to_string()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: ChunkKind, symbol: Option<&str>, file_path: &str) -> CodeChunk {
        CodeChunk {
            id: "abc123".to_string(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from(file_path),
            language: "rust".to_string(),
            kind,
            symbol: symbol.map(str::to_string),
            signature: symbol.map(|symbol| format!("fn {symbol}()")),
            parent_symbol: None,
            start_line: 1,
            end_line: 8,
            text:
                "fn sample() {\nlet a = 1;\nlet b = 2;\nlet c = a + b;\nprintln!(\"{}\", c);\n}\n"
                    .to_string(),
        }
    }

    #[test]
    fn style_selection_uses_compatible_styles() {
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..50 {
            assert!(matches!(
                compatible_style(ChunkKind::Test, &mut rng),
                QueryStyle::TestFinding | QueryStyle::DebuggingSymptom | QueryStyle::LiteralSymbol
            ));
            assert!(matches!(
                compatible_style(ChunkKind::MarkdownSection, &mut rng),
                QueryStyle::DocsQuestion | QueryStyle::Capability | QueryStyle::NewContributor
            ));
        }
    }

    #[test]
    fn generated_yaml_serializes_expected_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eval.yaml");
        initialize_output_file(&path, Path::new("/repo"), "gemma4").expect("header");
        append_eval_item(
            &path,
            &EvalItem {
                id: "synthetic_0001".to_string(),
                query: "where is chunk filtering handled".to_string(),
                intent: "implementation".to_string(),
                difficulty: "medium".to_string(),
                style: "FuzzyImplementation".to_string(),
                expected: vec![ExpectedResult {
                    path: "src/indexer.rs".to_string(),
                    symbol: Some("should_ignore_path".to_string()),
                    kind: "function".to_string(),
                    chunk_id: "abc123".to_string(),
                    relevance: 3,
                }],
                notes: "test item".to_string(),
            },
        )
        .expect("append");
        let yaml = fs::read_to_string(&path).expect("read yaml");
        assert!(yaml.contains("version: 1"));
        assert!(yaml.contains("id: synthetic_0001"));
        assert!(yaml.contains("chunk_id: abc123"));
    }

    #[test]
    fn validation_rejects_exact_symbol_leakage_for_fuzzy_styles() {
        let chunk = chunk(
            ChunkKind::Function,
            Some("should_ignore_path"),
            "src/indexer.rs",
        );
        let item = ModelEvalItem {
            query: "where is should_ignore_path implemented in scanning".to_string(),
            intent: "implementation".to_string(),
            difficulty: "medium".to_string(),
            notes: "bad".to_string(),
        };
        let mut seen = HashSet::new();
        assert!(
            validate_model_item(&item, &chunk, QueryStyle::FuzzyImplementation, &mut seen).is_err()
        );
    }

    #[test]
    fn validation_accepts_exact_symbol_for_literal_symbol() {
        let chunk = chunk(
            ChunkKind::Function,
            Some("should_ignore_path"),
            "src/indexer.rs",
        );
        let item = ModelEvalItem {
            query: "where is should_ignore_path used for filtering".to_string(),
            intent: "implementation".to_string(),
            difficulty: "easy".to_string(),
            notes: "ok".to_string(),
        };
        let mut seen = HashSet::new();
        assert!(validate_model_item(&item, &chunk, QueryStyle::LiteralSymbol, &mut seen).is_ok());
    }

    #[test]
    fn validation_deduplicates_queries_case_insensitively() {
        let chunk = chunk(ChunkKind::Function, Some("sample"), "src/lib.rs");
        let first = ModelEvalItem {
            query: "where is path filtering implemented".to_string(),
            intent: "implementation".to_string(),
            difficulty: "medium".to_string(),
            notes: "ok".to_string(),
        };
        let second = ModelEvalItem {
            query: "WHERE is PATH filtering implemented".to_string(),
            intent: "implementation".to_string(),
            difficulty: "medium".to_string(),
            notes: "duplicate".to_string(),
        };
        let mut seen = HashSet::new();
        assert!(
            validate_model_item(&first, &chunk, QueryStyle::FuzzyImplementation, &mut seen).is_ok()
        );
        assert!(
            validate_model_item(&second, &chunk, QueryStyle::FuzzyImplementation, &mut seen)
                .is_err()
        );
    }

    #[test]
    fn relative_path_conversion_prefers_repo_relative_paths() {
        assert_eq!(
            relative_path(Path::new("/repo"), Path::new("/repo/src/indexer.rs")),
            "src/indexer.rs"
        );
        assert_eq!(
            relative_path(Path::new("/repo"), Path::new("src/indexer.rs")),
            "src/indexer.rs"
        );
    }

    #[test]
    fn parse_model_json_accepts_fenced_json() {
        let item = parse_model_json(
            "```json\n{\"query\":\"where is chunk filtering handled\",\"intent\":\"implementation\",\"difficulty\":\"medium\",\"notes\":\"ok\"}\n```",
        )
        .expect("json");
        assert_eq!(item.intent, "implementation");
    }
}
