#[derive(Debug, Clone)]
pub struct AnalyzedQuery {
    pub raw: String,
    pub normalized_terms: Vec<String>,
    pub important_terms: Vec<String>,
    pub downweighted_terms: Vec<String>,
    pub expansions: Vec<String>,
    pub intent: QueryIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    FindImplementation,
    FindDefinition,
    FindUsage,
    FindTests,
    ExplainCapability,
    FindConfig,
    Unknown,
}

pub fn analyze_query(query: &str) -> AnalyzedQuery {
    analyze_query_with_symbols(query, std::iter::empty::<&str>())
}

pub fn analyze_query_with_symbols<'a>(
    query: &str,
    symbols: impl IntoIterator<Item = &'a str>,
) -> AnalyzedQuery {
    let raw_terms = split_terms(query);
    let mut normalized_terms = Vec::new();
    let mut important_terms = Vec::new();
    let mut downweighted_terms = Vec::new();
    let expansions = Vec::new();

    let has_specific_term = raw_terms.iter().any(|term| {
        let normalized = term.to_lowercase();
        let code_like = is_code_like(term);
        !is_downweighted_term(&normalized)
            && !is_domain_generic_term(&normalized)
            && (normalized.len() > 1 || code_like)
    });

    for raw_term in raw_terms {
        let normalized = raw_term.to_lowercase();
        let code_like = is_code_like(&raw_term);
        normalized_terms.push(normalized.clone());

        if (is_downweighted_term(&normalized)
            || has_specific_term && is_domain_generic_term(&normalized))
            && !code_like
        {
            downweighted_terms.push(normalized.clone());
        } else if normalized.len() > 1 || code_like {
            push_unique(&mut important_terms, normalized.clone());
        }
    }

    let intent = detect_intent(query, &normalized_terms, symbols);

    AnalyzedQuery {
        raw: query.to_string(),
        normalized_terms,
        important_terms,
        downweighted_terms,
        expansions,
        intent,
    }
}

fn split_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_alphanumeric() || matches!(ch, '_' | ':' | '.' | '/') {
            current.push(ch);
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

fn is_code_like(term: &str) -> bool {
    term.contains('_')
        || term.contains("::")
        || term.contains('.')
        || term.contains('/')
        || has_mixed_case(term)
}

pub(crate) fn is_code_like_for_search(term: &str) -> bool {
    is_code_like(term)
}

fn has_mixed_case(term: &str) -> bool {
    term.chars().any(char::is_lowercase) && term.chars().any(char::is_uppercase)
}

fn is_downweighted_term(term: &str) -> bool {
    let len = term.chars().count();
    len <= 3 && term.chars().all(char::is_alphabetic)
}

fn is_domain_generic_term(term: &str) -> bool {
    matches!(
        term,
        "search"
            | "searches"
            | "query"
            | "queries"
            | "chunk"
            | "chunks"
            | "result"
            | "results"
            | "term"
            | "terms"
            | "code"
    )
}

fn detect_intent<'a>(
    query: &str,
    terms: &[String],
    symbols: impl IntoIterator<Item = &'a str>,
) -> QueryIntent {
    let normalized_query = query.to_lowercase();

    if symbols
        .into_iter()
        .filter(|symbol| !symbol.is_empty())
        .any(|symbol| normalized_query.contains(&symbol.to_lowercase()))
        || normalized_query.contains("definition of")
        || normalized_query.contains(" defined ")
        || normalized_query.contains(" variants")
        || normalized_query.contains("what data is stored")
        || normalized_query.contains("what fields")
    {
        QueryIntent::FindDefinition
    } else if terms.iter().any(|term| term.contains("test")) {
        QueryIntent::FindTests
    } else if terms
        .iter()
        .any(|term| term.contains("config") || term.contains("toml") || term.contains("json"))
    {
        QueryIntent::FindConfig
    } else {
        QueryIntent::Unknown
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_plain_terms_are_downweighted() {
        let analyzed = analyze_query("what are the languages this supports");
        assert!(analyzed.downweighted_terms.contains(&"are".into()));
        assert!(!analyzed.important_terms.contains(&"the".into()));
        assert!(analyzed.important_terms.contains(&"what".into()));
        assert!(analyzed.important_terms.contains(&"languages".into()));
        assert!(analyzed.important_terms.contains(&"supports".into()));
    }

    #[test]
    fn domain_generic_terms_are_downweighted_when_specific_terms_exist() {
        let analyzed = analyze_query("how are search terms formatted into a quoted comma string");
        assert!(analyzed.downweighted_terms.contains(&"search".into()));
        assert!(analyzed.downweighted_terms.contains(&"terms".into()));
        assert!(analyzed.important_terms.contains(&"quoted".into()));
        assert!(analyzed.important_terms.contains(&"comma".into()));
    }

    #[test]
    fn code_like_terms_are_preserved() {
        let analyzed = analyze_query("where is FooBar::from_path in src/search.rs");
        assert!(
            analyzed
                .important_terms
                .contains(&"foobar::from_path".into())
        );
        assert!(analyzed.important_terms.contains(&"src/search.rs".into()));
    }

    #[test]
    fn hardcoded_morphology_expansions_are_not_generated() {
        let analyzed = analyze_query("supported directories implemented configuration");
        assert!(analyzed.expansions.is_empty());
    }

    #[test]
    fn detects_intent() {
        assert_eq!(
            analyze_query_with_symbols("where is QueryIntent", ["QueryIntent"]).intent,
            QueryIntent::FindDefinition
        );
        assert_eq!(
            analyze_query("tests for chunking").intent,
            QueryIntent::FindTests
        );
        assert_eq!(
            analyze_query("ignored directories config").intent,
            QueryIntent::FindConfig
        );
        assert_eq!(
            analyze_query("where is ranking implemented").intent,
            QueryIntent::Unknown
        );
        assert_eq!(
            analyze_query("What are the possible Action variants defined in the TUI module?")
                .intent,
            QueryIntent::FindDefinition
        );
    }
}
