use std::collections::HashMap;
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
use crate::query::{AnalyzedQuery, analyze_query, analyze_query_with_symbols};
use crate::repo_meta::{
    RepoMetadata, expand_with_repo_metadata, read_metadata, repo_vocab_overlap,
};
use crate::reranker::CrossEncoderReranker;

const CANDIDATE_MULTIPLIER: usize = 12;
const MIN_CANDIDATES: usize = 100;
const MAX_CANDIDATES: usize = 500;
pub const DEFAULT_RERANK_INPUT_LIMIT: usize = 25;

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
    reranker: Option<CrossEncoderReranker>,
    rerank_limit: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub use_embeddings: bool,
    pub use_reranker: bool,
    pub rerank_limit: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            use_embeddings: true,
            use_reranker: false,
            rerank_limit: DEFAULT_RERANK_INPUT_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RankingFeatures {
    pub important_text_matches: usize,
    pub important_symbol_matches: usize,
    pub important_path_matches: usize,
    pub expansion_matches: usize,
    pub exact_symbol_match: bool,
    pub partial_symbol_match: bool,
    pub symbol_match_multiplier: f32,
    pub language_match: bool,
    pub repo_vocab_overlap: usize,
    pub heading_match: bool,
}

pub fn search_repo(repo_root: &Path, query: &str, limit: usize) -> Result<SearchSummary> {
    search_repo_with_options(repo_root, query, limit, SearchOptions::default())
}

pub fn search_repo_with_options(
    repo_root: &Path,
    query: &str,
    limit: usize,
    options: SearchOptions,
) -> Result<SearchSummary> {
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

    let session = SearchSession::open_with_options(repo_root, options)?;
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
        let reranker = if options.use_reranker {
            Some(CrossEncoderReranker::new(false)?)
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
            reranker,
            rerank_limit: options.rerank_limit,
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
        let candidate_limit = candidate_limit(limit);
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
            ));
        }

        ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
        if let Some(reranker) = &self.reranker {
            ranked = rerank_with_cross_encoder(reranker, query, ranked, limit, self.rerank_limit)?;
        }
        ranked.truncate(limit);

        Ok(SearchSummary {
            results: ranked,
            elapsed: started.elapsed(),
            analyzed,
        })
    }
}

fn rerank_with_cross_encoder(
    reranker: &CrossEncoderReranker,
    query: &str,
    mut ranked: Vec<RankedChunk>,
    result_limit: usize,
    rerank_limit: usize,
) -> Result<Vec<RankedChunk>> {
    if ranked.is_empty() || result_limit == 0 {
        return Ok(ranked);
    }

    let rerank_limit = ranked.len().min(result_limit.max(rerank_limit));
    let chunks = ranked
        .iter()
        .take(rerank_limit)
        .map(|ranked| ranked.chunk.clone())
        .collect::<Vec<_>>();
    let reranked = reranker.rerank(query, &chunks)?;

    let mut reordered = Vec::with_capacity(ranked.len());
    let mut used = vec![false; rerank_limit];
    for (idx, score) in reranked {
        let Some(mut result) = ranked.get(idx).cloned() else {
            continue;
        };
        if idx >= used.len() || used[idx] {
            continue;
        }
        used[idx] = true;
        result.score = score;
        if !result.reason.contains("cross-encoder reranked") {
            result.reason = format!("{}; cross-encoder reranked", result.reason);
        }
        reordered.push(result);
    }

    for (idx, result) in ranked.drain(..rerank_limit).enumerate() {
        if !used[idx] {
            reordered.push(result);
        }
    }
    reordered.extend(ranked);
    Ok(reordered)
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
    rerank_analyzed_with_context(analyzed, chunk, tantivy_score, None)
}

fn rerank_analyzed_with_context(
    analyzed: &AnalyzedQuery,
    chunk: &CodeChunk,
    tantivy_score: f32,
    meta: Option<&RepoMetadata>,
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
    let exact_symbol_match = exact_symbol_match(&analyzed.normalized_terms, &symbol);
    let partial_symbol_match =
        !exact_symbol_match && partial_symbol_match(&analyzed.normalized_terms, &symbol);
    let symbol_match_multiplier = symbol_match_multiplier(&analyzed.normalized_terms, &symbol);
    let expansion_matches = count_matches(&analyzed.expansions, &text)
        + count_matches(&analyzed.expansions, &symbol)
        + count_matches(&analyzed.expansions, &file_path);
    let language_match = analyzed
        .important_terms
        .iter()
        .chain(analyzed.expansions.iter())
        .any(|term| language_aliases(&language).contains(&term.as_str()));
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

    let features = RankingFeatures {
        important_text_matches,
        important_symbol_matches,
        important_path_matches,
        expansion_matches,
        exact_symbol_match,
        partial_symbol_match,
        symbol_match_multiplier,
        language_match,
        repo_vocab_overlap,
        heading_match,
    };
    let score = tantivy_score * features.symbol_match_multiplier;
    let reason = reason(analyzed, &features, &language);

    RankedChunk {
        chunk: chunk.clone(),
        score,
        reason,
    }
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

fn candidate_limit(result_limit: usize) -> usize {
    if result_limit == 0 {
        return 0;
    }

    result_limit
        .saturating_mul(CANDIDATE_MULTIPLIER)
        .max(MIN_CANDIDATES)
        .min(MAX_CANDIDATES)
        .max(result_limit)
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

fn reason(analyzed: &AnalyzedQuery, features: &RankingFeatures, language: &str) -> String {
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
    if features.exact_symbol_match {
        parts.push("exact symbol match".to_string());
    } else if features.partial_symbol_match {
        parts.push("partial symbol match".to_string());
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
        doc_comment: string_value(schema, doc, fields.doc_comment)?,
        callees: stored_list_value(schema, doc, fields.callees)?,
        sibling_symbols: stored_list_value(schema, doc, fields.sibling_symbols)?,
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

fn stored_list_value(schema: &Schema, doc: &TantivyDocument, field: Field) -> Result<Vec<String>> {
    Ok(string_value(schema, doc, field)?
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect())
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
        "go" => &["go", "golang"],
        "java" => &["java"],
        "c" => &["c"],
        "markdown" => &["markdown", "md"],
        _ => &[],
    }
}

fn exact_symbol_match(terms: &[String], symbol: &str) -> bool {
    !symbol.is_empty() && terms.iter().any(|term| term == symbol)
}

fn partial_symbol_match(terms: &[String], symbol: &str) -> bool {
    !symbol.is_empty()
        && terms
            .iter()
            .any(|term| !term.is_empty() && (symbol.contains(term) || term.contains(symbol)))
}

fn symbol_match_multiplier(terms: &[String], symbol: &str) -> f32 {
    if exact_symbol_match(terms, symbol) {
        2.5
    } else if partial_symbol_match(terms, symbol) {
        1.5
    } else {
        1.0
    }
}

fn count_matches(terms: &[String], haystack: &str) -> usize {
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

fn quote_list(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(", ")
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
            doc_comment: String::new(),
            callees: Vec::new(),
            sibling_symbols: Vec::new(),
            text: text.into(),
        }
    }

    #[test]
    fn candidate_pool_is_wider_than_requested_limit() {
        assert_eq!(candidate_limit(0), 0);
        assert_eq!(candidate_limit(5), MIN_CANDIDATES);
        assert_eq!(candidate_limit(10), 120);
        assert_eq!(candidate_limit(100), 500);
        assert_eq!(candidate_limit(600), 600);
    }

    #[test]
    fn exact_symbol_match_gets_2_5x_boost_and_ignores_intent() {
        let exact = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("RepoVocabulary"),
            "pub struct RepoVocabulary {}",
        );

        let definition_query = rerank("RepoVocabulary", &exact, 1.0);
        let test_query = rerank("tests for RepoVocabulary", &exact, 1.0);

        assert!((definition_query.score - 2.5).abs() < f32::EPSILON);
        assert_eq!(definition_query.score, test_query.score);
        assert!(definition_query.reason.contains("exact symbol match"));
    }

    #[test]
    fn partial_symbol_match_gets_1_5x_boost() {
        let partial = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("RepoVocabularyBuilder"),
            "pub struct RepoVocabularyBuilder {}",
        );

        let ranked = rerank("RepoVocabulary", &partial, 1.0);

        assert!((ranked.score - 1.5).abs() < f32::EPSILON);
        assert!(ranked.reason.contains("partial symbol match"));
    }

    #[test]
    fn symbol_boost_is_case_insensitive_and_substring_based() {
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
        let none = test_chunk(
            "src/repo_meta.rs",
            "rust",
            ChunkKind::Struct,
            Some("OtherSymbol"),
            "pub struct OtherSymbol {}",
        );

        let exact_ranked = rerank("repovocabulary", &exact, 1.0);
        let partial_ranked = rerank("repo", &partial, 1.0);
        let none_ranked = rerank("repo", &none, 1.0);

        assert!((exact_ranked.score - 2.5).abs() < f32::EPSILON);
        assert!((partial_ranked.score - 1.5).abs() < f32::EPSILON);
        assert!((none_ranked.score - 1.0).abs() < f32::EPSILON);
        assert!(exact_ranked.score > partial_ranked.score);
        assert!(partial_ranked.score > none_ranked.score);
    }
}
