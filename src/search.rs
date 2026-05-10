use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, QueryParser};
use tantivy::schema::{Field, OwnedValue, Schema};
use tantivy::{Index, IndexReader, TantivyDocument};

use crate::embeddings::EmbeddingStore;
use crate::indexer::IndexFields;
use crate::model::{ChunkKind, CodeChunk, RankedChunk};
use crate::query::{AnalyzedQuery, QueryIntent, analyze_query, analyze_query_with_symbols};
use crate::repo_meta::{
    RepoMetadata, expand_with_repo_metadata, read_metadata, related_ids, repo_vocab_overlap,
};

#[derive(Debug, Clone)]
pub struct SearchSummary {
    pub results: Vec<RankedChunk>,
    pub elapsed: Duration,
    pub analyzed: AnalyzedQuery,
}

pub struct SearchSession {
    index: Index,
    reader: IndexReader,
    fields: IndexFields,
    meta: RepoMetadata,
    chunks_by_id: HashMap<String, CodeChunk>,
    embeddings: Option<EmbeddingStore>,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub use_embeddings: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            use_embeddings: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RankingFeatures {
    pub tantivy_score: f32,
    pub important_text_matches: usize,
    pub important_symbol_matches: usize,
    pub important_path_matches: usize,
    pub expansion_matches: usize,
    pub exact_symbol_match: bool,
    pub partial_symbol_match: bool,
    pub symbol_match_multiplier: f32,
    pub exact_phrase_match: bool,
    pub language_match: bool,
    pub doc_chunk_boost: f32,
    pub intent_chunk_boost: f32,
    pub repo_vocab_overlap: usize,
    pub symbol_graph_relation: bool,
    pub same_file_relation: bool,
    pub test_relation: bool,
    pub heading_match: bool,
    pub comment_or_blank_penalty: f32,
    pub markdown_intent_multiplier: f32,
    pub test_intent_multiplier: f32,
    pub precision_multiplier: f32,
}

pub fn search_repo(repo_root: &Path, query: &str, limit: usize) -> Result<SearchSummary> {
    let repo_root = repo_root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", repo_root.display()))?;
    let index_path = repo_root.join(".locus").join("index");
    if !index_path.exists() {
        return Err(anyhow!(
            "No locus index found at {}. Run: locus index {}",
            index_path.display(),
            repo_root.display()
        ));
    }

    let session = SearchSession::open(repo_root)?;
    session.search(query, limit)
}

pub fn load_indexed_chunks(repo_root: &Path) -> Result<Vec<CodeChunk>> {
    let repo_root = repo_root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", repo_root.display()))?;
    let index_path = repo_root.join(".locus").join("index");
    if !index_path.exists() {
        return Err(anyhow!(
            "No locus index found at {}. Run: locus index {}",
            index_path.display(),
            repo_root.display()
        ));
    }

    let index = Index::open_in_dir(&index_path)?;
    let schema = index.schema();
    let fields = IndexFields::from_schema(&schema);
    let reader = index.reader()?;
    load_chunks_from_index(&index, &reader, &fields)
}

impl SearchSession {
    pub fn open(repo_root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(repo_root, SearchOptions::default())
    }

    pub fn open_with_options(repo_root: impl AsRef<Path>, options: SearchOptions) -> Result<Self> {
        let repo_root = repo_root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", repo_root.as_ref().display()))?;
        let index_path = repo_root.join(".locus").join("index");
        if !index_path.exists() {
            return Err(anyhow!(
                "No locus index found at {}. Run: locus index {}",
                index_path.display(),
                repo_root.display()
            ));
        }

        let index = Index::open_in_dir(&index_path)?;
        let schema = index.schema();
        let fields = IndexFields::from_schema(&schema);
        let reader = index.reader()?;
        let meta = read_metadata(&repo_root)?.unwrap_or_default();
        let chunks = load_chunks_from_index(&index, &reader, &fields)?;
        let chunks_by_id = chunks
            .iter()
            .map(|chunk| (chunk.id.clone(), chunk.clone()))
            .collect();
        let embeddings = if options.use_embeddings {
            let embeddings_path = index_path.join("embeddings.bin");
            Some(
                EmbeddingStore::load(&embeddings_path)
                    .with_context(|| format!("failed to load {}", embeddings_path.display()))?,
            )
        } else {
            None
        };
        Ok(Self {
            index,
            reader,
            fields,
            meta,
            chunks_by_id,
            embeddings,
        })
    }

    pub fn chunk_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }

    pub fn analyze(&self, query: &str) -> AnalyzedQuery {
        expand_with_repo_metadata(
            &analyze_query_with_symbols(
                query,
                self.meta.vocabulary.symbols.iter().map(String::as_str),
            ),
            &self.meta,
        )
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<SearchSummary> {
        let started = Instant::now();
        let searcher = self.reader.searcher();
        let analyzed = self.analyze(query);
        let schema = self.index.schema();
        let fields = self.fields;
        let parser = QueryParser::for_index(
            &self.index,
            vec![
                fields.text,
                fields.symbol,
                fields.heading_or_symbol,
                fields.kind,
                fields.signature,
                fields.parent_symbol,
                fields.file_path,
                fields.language,
                fields.extension,
            ],
        );
        let search_query = tantivy_query_string(&analyzed);
        let parsed = parser
            .parse_query(&search_query)
            .or_else(|_| parser.parse_query(&sanitize_query(&search_query)))?;
        let candidate_limit = limit.saturating_mul(4).max(limit);
        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(candidate_limit))?;

        let mut ranked = Vec::new();
        let mut bm25_ids = Vec::new();
        let mut candidate_chunks = HashMap::new();
        for (_, address) in top_docs {
            let doc: TantivyDocument = searcher.doc(address)?;
            let chunk = document_to_chunk(&schema, &fields, &doc)?;
            bm25_ids.push(chunk.id.clone());
            candidate_chunks.insert(chunk.id.clone(), chunk);
        }

        let semantic_ids = if let Some(embeddings) = &self.embeddings {
            embeddings
                .search(query, candidate_limit)?
                .into_iter()
                .map(|(chunk_id, _)| chunk_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for chunk_id in &semantic_ids {
            if !candidate_chunks.contains_key(chunk_id) {
                if let Some(chunk) = self.chunks_by_id.get(chunk_id) {
                    candidate_chunks.insert(chunk_id.clone(), chunk.clone());
                }
            }
        }

        for (chunk_id, fused_score) in reciprocal_rank_fusion(&bm25_ids, &semantic_ids) {
            let Some(chunk) = candidate_chunks.get(&chunk_id) else {
                continue;
            };
            ranked.push(rerank_analyzed_with_context(
                &analyzed,
                chunk,
                fused_score,
                Some(&self.meta),
                false,
                false,
                false,
            ));
        }

        ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
        apply_relationship_boosts(&mut ranked, &analyzed, &self.meta);
        ranked.truncate(limit);

        Ok(SearchSummary {
            results: ranked,
            elapsed: started.elapsed(),
            analyzed,
        })
    }
}

pub fn rerank(query: &str, chunk: &CodeChunk, tantivy_score: f32) -> RankedChunk {
    let analyzed = analyze_query(query);
    rerank_analyzed(&analyzed, chunk, tantivy_score)
}

pub fn rerank_analyzed(
    analyzed: &AnalyzedQuery,
    chunk: &CodeChunk,
    tantivy_score: f32,
) -> RankedChunk {
    rerank_analyzed_with_context(analyzed, chunk, tantivy_score, None, false, false, false)
}

fn rerank_analyzed_with_context(
    analyzed: &AnalyzedQuery,
    chunk: &CodeChunk,
    tantivy_score: f32,
    meta: Option<&RepoMetadata>,
    symbol_graph_relation: bool,
    same_file_relation: bool,
    test_relation: bool,
) -> RankedChunk {
    let symbol = chunk.symbol.as_deref().unwrap_or("").to_lowercase();
    let signature = chunk.signature.as_deref().unwrap_or("").to_lowercase();
    let parent_symbol = chunk.parent_symbol.as_deref().unwrap_or("").to_lowercase();
    let file_path = chunk.file_path.display().to_string().to_lowercase();
    let language = chunk.language.to_lowercase();
    let text = format!(
        "{}\n{}\n{}",
        chunk.text.to_lowercase(),
        signature,
        parent_symbol
    );

    let important_text_matches = count_matches(&analyzed.important_terms, &text);
    let important_symbol_matches = count_matches(&analyzed.important_terms, &symbol);
    let important_path_matches = count_matches(&analyzed.important_terms, &file_path);
    let exact_symbol_match = exact_symbol_match(analyzed, &symbol);
    let partial_symbol_match = !exact_symbol_match && partial_symbol_match(analyzed, &symbol);
    let symbol_match_multiplier =
        symbol_match_multiplier(analyzed.intent, exact_symbol_match, partial_symbol_match);
    let expansion_matches = count_matches(&analyzed.expansions, &text)
        + count_matches(&analyzed.expansions, &symbol)
        + count_matches(&analyzed.expansions, &file_path);
    let exact_phrase_match =
        !analyzed.raw.trim().is_empty() && text.contains(&analyzed.raw.to_lowercase());
    let language_match = analyzed
        .important_terms
        .iter()
        .chain(analyzed.expansions.iter())
        .any(|term| language_aliases(&language).contains(&term.as_str()));
    let doc_chunk_boost = doc_chunk_boost(
        analyzed.intent,
        chunk.kind,
        &file_path,
        &language,
        &symbol,
        &text,
    );
    let intent_chunk_boost = intent_chunk_boost(
        analyzed.intent,
        chunk.kind,
        &file_path,
        &language,
        &symbol,
        &text,
    );
    let repo_vocab_overlap = meta
        .map(|meta| repo_vocab_overlap(analyzed, meta, chunk))
        .unwrap_or_default();
    let heading_match = chunk.kind == ChunkKind::MarkdownSection
        && chunk.symbol.as_ref().is_some_and(|symbol| {
            let heading = symbol.to_lowercase();
            analyzed
                .important_terms
                .iter()
                .chain(analyzed.expansions.iter())
                .any(|term| heading.contains(term))
        });
    let comment_or_blank_ratio = comment_or_blank_ratio(&chunk.text, &language);
    let comment_or_blank_penalty = if comment_or_blank_ratio > 0.65 {
        0.75
    } else {
        1.0
    };
    let markdown_intent_multiplier =
        markdown_intent_multiplier(analyzed.intent, chunk.kind, &language);
    let test_intent_multiplier = test_intent_multiplier(analyzed.intent, chunk.kind);
    let precision_multiplier = if chunk_line_count(chunk) < 30 {
        1.15
    } else {
        1.0
    };

    let features = RankingFeatures {
        tantivy_score,
        important_text_matches,
        important_symbol_matches,
        important_path_matches,
        expansion_matches,
        exact_symbol_match,
        partial_symbol_match,
        symbol_match_multiplier,
        exact_phrase_match,
        language_match,
        doc_chunk_boost,
        intent_chunk_boost,
        repo_vocab_overlap,
        symbol_graph_relation,
        same_file_relation,
        test_relation,
        heading_match,
        comment_or_blank_penalty,
        markdown_intent_multiplier,
        test_intent_multiplier,
        precision_multiplier,
    };
    let score = score_features(&features);
    let reason = reason(analyzed, &features, &language, chunk.kind);

    RankedChunk {
        chunk: chunk.clone(),
        score,
        reason,
    }
}

fn apply_relationship_boosts(
    ranked: &mut Vec<RankedChunk>,
    analyzed: &AnalyzedQuery,
    meta: &RepoMetadata,
) {
    if ranked.is_empty() {
        return;
    }
    let primary_ids = ranked
        .iter()
        .take(3)
        .map(|ranked| ranked.chunk.id.clone())
        .collect::<HashSet<_>>();
    let primary_files = ranked
        .iter()
        .take(2)
        .map(|ranked| ranked.chunk.file_path.clone())
        .collect::<HashSet<_>>();
    let related = related_ids(meta, &primary_ids);
    for ranked_chunk in ranked.iter_mut() {
        let relation = related.contains(&ranked_chunk.chunk.id);
        let same_file = !primary_ids.contains(&ranked_chunk.chunk.id)
            && primary_files.contains(&ranked_chunk.chunk.file_path);
        let test_relation = analyzed.intent != QueryIntent::FindTests
            && ranked_chunk.chunk.kind == ChunkKind::Test
            && (relation || same_file);
        if relation || same_file || test_relation {
            *ranked_chunk = rerank_analyzed_with_context(
                analyzed,
                &ranked_chunk.chunk,
                ranked_chunk.score,
                Some(meta),
                relation,
                same_file,
                test_relation,
            );
        }
    }
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
}

fn load_chunks_from_index(
    index: &Index,
    reader: &IndexReader,
    fields: &IndexFields,
) -> Result<Vec<CodeChunk>> {
    let searcher = reader.searcher();
    let limit = searcher.num_docs() as usize;
    if limit == 0 {
        return Ok(Vec::new());
    }

    let schema = index.schema();
    let docs = searcher.search(&AllQuery, &TopDocs::with_limit(limit))?;
    let mut chunks = Vec::with_capacity(docs.len());
    for (_, address) in docs {
        let doc: TantivyDocument = searcher.doc(address)?;
        chunks.push(document_to_chunk(&schema, fields, &doc)?);
    }
    Ok(chunks)
}

fn reciprocal_rank_fusion(first: &[String], second: &[String]) -> Vec<(String, f32)> {
    const RRF_K: f32 = 60.0;
    const RRF_SCORE_SCALE: f32 = 120.0;

    let mut scores = HashMap::<String, f32>::new();
    for ids in [first, second] {
        for (index, chunk_id) in ids.iter().enumerate() {
            let rank = index as f32 + 1.0;
            *scores.entry(chunk_id.clone()).or_default() += RRF_SCORE_SCALE / (RRF_K + rank);
        }
    }

    let mut fused = scores.into_iter().collect::<Vec<_>>();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1));
    fused
}

fn score_features(features: &RankingFeatures) -> f32 {
    let mut score = features.tantivy_score;
    score += features.important_symbol_matches as f32 * 3.0;
    score += features.important_path_matches as f32 * 1.5;
    score += features.important_text_matches as f32 * 1.0;
    score += features.expansion_matches as f32 * 0.35;
    if features.exact_phrase_match {
        score += 2.0;
    }
    if features.language_match {
        score += 1.5;
    }
    score += features.doc_chunk_boost;
    score += features.intent_chunk_boost;
    score += features.repo_vocab_overlap as f32 * 0.4;
    if features.symbol_graph_relation {
        score += 1.1;
    }
    if features.same_file_relation {
        score += 0.4;
    }
    if features.test_relation {
        score += 0.5;
    }
    if features.heading_match {
        score += 1.0;
    }
    score
        * features.symbol_match_multiplier
        * features.comment_or_blank_penalty
        * features.markdown_intent_multiplier
        * features.test_intent_multiplier
        * features.precision_multiplier
}

fn reason(
    analyzed: &AnalyzedQuery,
    features: &RankingFeatures,
    language: &str,
    kind: ChunkKind,
) -> String {
    let mut parts = Vec::new();
    let matched_terms = analyzed
        .important_terms
        .iter()
        .filter(|term| {
            features.important_text_matches > 0
                || features.important_symbol_matches > 0
                || features.important_path_matches > 0
                || features.language_match
                    && analyzed
                        .expansions
                        .iter()
                        .any(|expansion| expansion == *term)
        })
        .take(6)
        .cloned()
        .collect::<Vec<_>>();

    if !matched_terms.is_empty() {
        parts.push(format!(
            "matched important terms {}",
            quote_list(&matched_terms)
        ));
    } else if features.expansion_matches > 0 {
        let expansions = analyzed
            .expansions
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>();
        parts.push(format!(
            "matched important terms via expansions {}",
            quote_list(&expansions)
        ));
    } else {
        parts.push("matched analyzed query terms".to_string());
    }

    if features.important_symbol_matches > 0 {
        parts.push("boosted symbol match".to_string());
    }
    if features.important_path_matches > 0 {
        parts.push("boosted path match".to_string());
    }
    if features.language_match {
        parts.push(format!("language {language}"));
    }
    if features.repo_vocab_overlap > 0 {
        parts.push("matched repo vocabulary".to_string());
    }
    if features.heading_match {
        parts.push("boosted heading match".to_string());
    }
    if features.symbol_graph_relation {
        parts.push("related to top result through symbol reference".to_string());
    }
    if features.same_file_relation {
        parts.push("related by same file as a top result".to_string());
    }
    if features.test_relation {
        parts.push("related test coverage".to_string());
    }
    if features.doc_chunk_boost > 0.0 && analyzed.intent == QueryIntent::ExplainCapability {
        if kind == ChunkKind::MarkdownSection {
            parts.push("boosted markdown section for capability-style query".to_string());
        } else {
            parts.push("boosted docs for capability-style query".to_string());
        }
    }
    if features.intent_chunk_boost > 0.0 {
        parts.push(structural_reason(analyzed.intent, kind).to_string());
    }
    if features.comment_or_blank_penalty < 1.0 {
        parts.push("penalized mostly comment/blank chunk".to_string());
    }

    parts.join("; ")
}

fn document_to_chunk(
    schema: &Schema,
    fields: &IndexFields,
    doc: &TantivyDocument,
) -> Result<CodeChunk> {
    Ok(CodeChunk {
        id: string_value(schema, doc, fields.id)?,
        repo_root: string_value(schema, doc, fields.repo_root)?.into(),
        file_path: string_value(schema, doc, fields.file_path)?.into(),
        language: string_value(schema, doc, fields.language)?,
        kind: ChunkKind::from_index(&string_value(schema, doc, fields.kind)?),
        symbol: {
            let value = string_value(schema, doc, fields.symbol)?;
            if value.is_empty() { None } else { Some(value) }
        },
        signature: {
            let value = string_value(schema, doc, fields.signature)?;
            if value.is_empty() { None } else { Some(value) }
        },
        parent_symbol: {
            let value = string_value(schema, doc, fields.parent_symbol)?;
            if value.is_empty() { None } else { Some(value) }
        },
        start_line: u64_value(schema, doc, fields.start_line)? as usize,
        end_line: u64_value(schema, doc, fields.end_line)? as usize,
        text: string_value(schema, doc, fields.text)?,
    })
}

fn string_value(schema: &Schema, doc: &TantivyDocument, field: Field) -> Result<String> {
    let named = schema.get_field_name(field).to_string();
    let Some(value) = doc.get_first(field) else {
        return Ok(String::new());
    };
    match value {
        OwnedValue::Str(text) => Ok(text.clone()),
        other => Err(anyhow!("field {named} is not text: {other:?}")),
    }
}

fn u64_value(schema: &Schema, doc: &TantivyDocument, field: Field) -> Result<u64> {
    let named = schema.get_field_name(field).to_string();
    let Some(value) = doc.get_first(field) else {
        return Ok(0);
    };
    match value {
        OwnedValue::U64(number) => Ok(*number),
        other => Err(anyhow!("field {named} is not u64: {other:?}")),
    }
}

fn sanitize_query(query: &str) -> String {
    query_terms(query).join(" ")
}

fn tantivy_query_string(analyzed: &AnalyzedQuery) -> String {
    let mut terms = Vec::new();
    for term in analyzed
        .important_terms
        .iter()
        .chain(analyzed.expansions.iter())
    {
        if !terms.contains(term) {
            terms.push(term.clone());
        }
    }
    if terms.is_empty() {
        analyzed.normalized_terms.join(" ")
    } else {
        terms.join(" ")
    }
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|term| term.len() > 1)
        .map(|term| term.to_lowercase())
        .collect()
}

fn language_aliases(language: &str) -> &'static [&'static str] {
    match language {
        "rust" => &["rust", "rs"],
        "python" => &["python", "py"],
        "typescript" => &["typescript", "ts", "tsx"],
        "javascript" => &["javascript", "js", "jsx"],
        "markdown" => &["markdown", "md"],
        _ => &[],
    }
}

fn exact_symbol_match(analyzed: &AnalyzedQuery, symbol: &str) -> bool {
    !symbol.is_empty()
        && analyzed
            .important_terms
            .iter()
            .chain(analyzed.normalized_terms.iter())
            .any(|term| term == symbol)
        && (analyzed.intent == QueryIntent::FindDefinition
            || query_has_code_like_term(analyzed, symbol))
}

fn partial_symbol_match(analyzed: &AnalyzedQuery, symbol: &str) -> bool {
    !symbol.is_empty()
        && analyzed
            .important_terms
            .iter()
            .any(|term| term.len() > 2 && symbol.contains(term))
}

fn query_has_code_like_term(analyzed: &AnalyzedQuery, symbol: &str) -> bool {
    analyzed
        .raw
        .split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | ':' | '.' | '/')))
        .any(|term| term.to_lowercase() == symbol && crate::query::is_code_like_for_search(term))
}

fn symbol_match_multiplier(
    intent: QueryIntent,
    exact_symbol_match: bool,
    partial_symbol_match: bool,
) -> f32 {
    if exact_symbol_match && intent == QueryIntent::FindDefinition {
        3.0
    } else if exact_symbol_match {
        2.0
    } else if partial_symbol_match {
        1.5
    } else {
        1.0
    }
}

fn markdown_intent_multiplier(intent: QueryIntent, kind: ChunkKind, language: &str) -> f32 {
    if language != "markdown" || kind != ChunkKind::MarkdownSection {
        return 1.0;
    }
    match intent {
        QueryIntent::FindDefinition | QueryIntent::FindImplementation | QueryIntent::FindUsage => {
            0.4
        }
        _ => 0.7,
    }
}

fn test_intent_multiplier(intent: QueryIntent, kind: ChunkKind) -> f32 {
    if kind != ChunkKind::Test {
        return 1.0;
    }
    if intent == QueryIntent::FindTests {
        1.3
    } else {
        0.5
    }
}

fn chunk_line_count(chunk: &CodeChunk) -> usize {
    if chunk.end_line >= chunk.start_line {
        chunk.end_line - chunk.start_line + 1
    } else {
        chunk.text.lines().count().max(1)
    }
}

fn count_matches(terms: &[String], haystack: &str) -> usize {
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

fn doc_chunk_boost(
    intent: QueryIntent,
    kind: ChunkKind,
    file_path: &str,
    language: &str,
    symbol: &str,
    text: &str,
) -> f32 {
    if intent != QueryIntent::ExplainCapability {
        return 0.0;
    }
    let docs = kind == ChunkKind::MarkdownSection
        || language == "markdown"
        || file_path.ends_with(".md")
        || file_path.contains("readme")
        || file_path.contains("docs/");
    let heading = !symbol.is_empty() || text.lines().any(|line| line.trim_start().starts_with('#'));
    match (docs, heading) {
        (true, true) => 4.0,
        (true, false) => 2.5,
        (false, true) => 0.5,
        (false, false) => 0.0,
    }
}

fn intent_chunk_boost(
    intent: QueryIntent,
    kind: ChunkKind,
    file_path: &str,
    language: &str,
    symbol: &str,
    text: &str,
) -> f32 {
    match intent {
        QueryIntent::FindConfig => {
            if matches!(kind, ChunkKind::Config | ChunkKind::MarkdownSection)
                || file_path.contains("config")
                || file_path.contains("setting")
                || file_path.contains("settings")
                || file_path.ends_with("cargo.toml")
                || file_path.ends_with("package.json")
                || file_path.contains("ignore")
                || text.contains("config")
                || text.contains("ignore")
            {
                1.75
            } else {
                0.0
            }
        }
        QueryIntent::FindTests => {
            if kind == ChunkKind::Test
                || file_path.contains("test")
                || file_path.contains("spec")
                || symbol.contains("test")
                || text.contains("#[test]")
            {
                2.0
            } else {
                0.0
            }
        }
        QueryIntent::FindImplementation => {
            if matches!(
                kind,
                ChunkKind::Function | ChunkKind::Method | ChunkKind::Impl | ChunkKind::Module
            ) || matches!(language, "rust" | "python" | "typescript" | "javascript")
                && (!symbol.is_empty()
                    || text.contains("fn ")
                    || text.contains("def ")
                    || text.contains("function ")
                    || text.contains("impl "))
            {
                3.0
            } else {
                0.0
            }
        }
        QueryIntent::FindDefinition => {
            if matches!(
                kind,
                ChunkKind::Struct
                    | ChunkKind::Enum
                    | ChunkKind::Trait
                    | ChunkKind::Class
                    | ChunkKind::Function
            ) || !symbol.is_empty()
                || text.contains("struct ")
                || text.contains("enum ")
                || text.contains("trait ")
                || text.contains("class ")
                || text.contains("type ")
            {
                1.5
            } else {
                0.0
            }
        }
        QueryIntent::FindUsage => {
            if matches!(language, "rust" | "python" | "typescript" | "javascript") {
                0.75
            } else {
                0.0
            }
        }
        QueryIntent::ExplainCapability => {
            if matches!(kind, ChunkKind::MarkdownSection | ChunkKind::Config)
                || file_path.contains("readme")
                || file_path.contains("docs/")
            {
                1.25
            } else {
                0.0
            }
        }
        QueryIntent::Unknown => 0.0,
    }
}

fn structural_reason(intent: QueryIntent, kind: ChunkKind) -> &'static str {
    match (intent, kind) {
        (QueryIntent::FindImplementation, ChunkKind::Function | ChunkKind::Method) => {
            "boosted function chunk for implementation-style query"
        }
        (QueryIntent::FindImplementation, ChunkKind::Impl) => {
            "boosted impl chunk for implementation-style query"
        }
        (
            QueryIntent::FindDefinition,
            ChunkKind::Struct | ChunkKind::Enum | ChunkKind::Trait | ChunkKind::Class,
        ) => "boosted type chunk for definition-style query",
        (QueryIntent::FindTests, ChunkKind::Test) => "boosted test chunk for test-style query",
        (QueryIntent::ExplainCapability, ChunkKind::MarkdownSection) => {
            "boosted markdown section for capability-style query"
        }
        (QueryIntent::FindConfig, ChunkKind::Config | ChunkKind::MarkdownSection) => {
            "boosted config/documentation chunk for config-style query"
        }
        _ => "boosted intent fit",
    }
}

fn quote_list(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn comment_or_blank_ratio(text: &str, language: &str) -> f32 {
    let mut total = 0usize;
    let mut comment_or_blank = 0usize;
    for line in text.lines() {
        total += 1;
        let trimmed = line.trim();
        if trimmed.is_empty()
            || matches!(language, "rust" | "typescript" | "javascript") && trimmed.starts_with("//")
            || language == "python" && trimmed.starts_with('#')
            || language == "markdown" && trimmed.starts_with("<!--")
        {
            comment_or_blank += 1;
        }
    }
    if total == 0 {
        1.0
    } else {
        comment_or_blank as f32 / total as f32
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn test_chunk(
        file_path: &str,
        language: &str,
        kind: ChunkKind,
        symbol: Option<&str>,
        text: &str,
    ) -> CodeChunk {
        CodeChunk {
            id: "1".into(),
            repo_root: PathBuf::from("/repo"),
            file_path: PathBuf::from(file_path),
            language: language.into(),
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
    fn rerank_boosts_symbol_path_and_language() {
        let chunk = test_chunk(
            "src/auth/session.rs",
            "rust",
            ChunkKind::Function,
            Some("refresh_access_token"),
            "fn refresh_access_token() {}",
        );
        let ranked = rerank("rust auth token refresh", &chunk, 1.0);
        assert!(ranked.score > 5.0);
        assert!(ranked.reason.contains("symbol"));
        assert!(ranked.reason.contains("path"));
        assert!(ranked.reason.contains("language rust"));
    }

    #[test]
    fn rerank_does_not_reward_filler_terms() {
        let filler_chunk = test_chunk(
            "src/filler.rs",
            "rust",
            ChunkKind::Unknown,
            None,
            "the are this that where what",
        );
        let useful_chunk = test_chunk(
            "src/language.rs",
            "rust",
            ChunkKind::Function,
            Some("supported_languages"),
            "pub fn supported_languages() { support(); }",
        );

        let filler = rerank("what are the languages this supports", &filler_chunk, 1.0);
        let useful = rerank("what are the languages this supports", &useful_chunk, 1.0);

        assert!(useful.score > filler.score);
        assert!(!filler.reason.contains("\"are\""));
        assert!(!filler.reason.contains("\"the\""));
    }

    #[test]
    fn capability_phrasing_does_not_trigger_doc_boost() {
        let readme = test_chunk(
            "README.md",
            "markdown",
            ChunkKind::MarkdownSection,
            Some("Current v0"),
            "## Current v0\nSupported files:\n- Rust: `.rs`",
        );
        let code = test_chunk(
            "src/support.rs",
            "rust",
            ChunkKind::Function,
            Some("support_files"),
            "pub fn support_files() {}",
        );

        let readme_ranked = rerank("what files are supported", &readme, 1.0);
        let code_ranked = rerank("what files are supported", &code, 1.0);

        assert!(!readme_ranked.reason.contains("capability-style query"));
        assert!(!code_ranked.reason.contains("capability-style query"));
    }

    #[test]
    fn test_chunks_are_boosted_for_test_queries() {
        let test_case_chunk = test_chunk(
            "src/chunker_tests.rs",
            "rust",
            ChunkKind::Test,
            Some("chunks_with_overlap"),
            "#[test]\nfn chunks_with_overlap() {}",
        );
        let code_chunk = test_chunk(
            "src/chunker.rs",
            "rust",
            ChunkKind::Function,
            Some("chunk_file"),
            "pub fn chunk_file() {}",
        );

        let test_ranked = rerank("tests for chunking", &test_case_chunk, 1.0);
        let code_ranked = rerank("tests for chunking", &code_chunk, 1.0);

        assert!(test_ranked.score > code_ranked.score);
        assert!(test_ranked.reason.contains("test chunk"));
    }

    #[test]
    fn implementation_phrasing_does_not_trigger_function_boost() {
        let function = test_chunk(
            "src/search.rs",
            "rust",
            ChunkKind::Function,
            Some("rank_results"),
            "pub fn rank_results() { ranking(); }",
        );
        let unknown = test_chunk(
            "src/search.rs",
            "rust",
            ChunkKind::Unknown,
            None,
            "ranking notes",
        );

        let function_ranked = rerank("where is ranking implemented", &function, 1.0);
        let unknown_ranked = rerank("where is ranking implemented", &unknown, 1.0);

        assert!(!function_ranked.reason.contains("function chunk"));
        assert!(!unknown_ranked.reason.contains("function chunk"));
    }

    #[test]
    fn capability_phrasing_does_not_trigger_markdown_boost() {
        let docs = test_chunk(
            "README.md",
            "markdown",
            ChunkKind::MarkdownSection,
            Some("Supported languages"),
            "## Supported languages\nRust and Python are supported.",
        );
        let code = test_chunk(
            "src/languages.rs",
            "rust",
            ChunkKind::Function,
            Some("supported_languages"),
            "pub fn supported_languages() -> Vec<&'static str> { vec![\"rust\"] }",
        );

        let docs_ranked = rerank("what languages are supported", &docs, 1.0);
        let code_ranked = rerank("what languages are supported", &code, 1.0);

        assert!(!docs_ranked.reason.contains("markdown section"));
        assert!(!code_ranked.reason.contains("markdown section"));
    }

    #[test]
    fn markdown_sections_are_penalized_for_definition_queries() {
        let analyzed = analyze_query_with_symbols("RepoVocabulary", ["RepoVocabulary"]);
        let docs = test_chunk(
            "codebase.md",
            "markdown",
            ChunkKind::MarkdownSection,
            Some("RepoVocabulary"),
            "## RepoVocabulary\nDocuments the RepoVocabulary struct.",
        );
        let code = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("RepoVocabulary"),
            "pub struct RepoVocabulary {}",
        );

        let docs_ranked = rerank_analyzed(&analyzed, &docs, 1.0);
        let code_ranked = rerank_analyzed(&analyzed, &code, 1.0);

        assert!(code_ranked.score > docs_ranked.score);
    }

    #[test]
    fn test_chunks_are_penalized_for_non_test_queries() {
        let analyzed = AnalyzedQuery {
            raw: "rank results implementation".into(),
            normalized_terms: vec!["rank".into(), "results".into(), "implementation".into()],
            important_terms: vec!["rank".into(), "results".into(), "implementation".into()],
            downweighted_terms: Vec::new(),
            expansions: Vec::new(),
            intent: QueryIntent::FindImplementation,
        };
        let test_case_chunk = test_chunk(
            "src/search_tests.rs",
            "rust",
            ChunkKind::Test,
            Some("rank_results"),
            "#[test]\nfn rank_results() {}",
        );
        let code_chunk = test_chunk(
            "src/search.rs",
            "rust",
            ChunkKind::Function,
            Some("rank_results"),
            "pub fn rank_results() {}",
        );

        let test_ranked = rerank_analyzed(&analyzed, &test_case_chunk, 1.0);
        let code_ranked = rerank_analyzed(&analyzed, &code_chunk, 1.0);

        assert!(code_ranked.score > test_ranked.score);
    }

    #[test]
    fn exact_symbol_matches_get_strong_definition_boost() {
        let analyzed = analyze_query_with_symbols("RepoVocabulary", ["RepoVocabulary"]);
        let exact = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("RepoVocabulary"),
            "pub struct RepoVocabulary {}",
        );
        let partial = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("RepoVocabularyBuilder"),
            "pub struct RepoVocabularyBuilder {}",
        );

        let exact_ranked = rerank_analyzed(&analyzed, &exact, 1.0);
        let partial_ranked = rerank_analyzed(&analyzed, &partial, 1.0);

        assert!(exact_ranked.score > partial_ranked.score);
    }

    #[test]
    fn small_chunks_receive_precision_bonus() {
        let small = test_chunk(
            "src/search.rs",
            "rust",
            ChunkKind::Unknown,
            None,
            "needle();",
        );
        let large = test_chunk(
            "src/search.rs",
            "rust",
            ChunkKind::Unknown,
            None,
            &vec!["needle();"; 35].join("\n"),
        );

        let small_ranked = rerank("needle", &small, 1.0);
        let large_ranked = rerank("needle", &large, 1.0);

        assert!(small_ranked.score > large_ranked.score);
    }
}
