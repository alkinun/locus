use std::path::Path;

use regex::Regex;
use tree_sitter::{Language, Node, Parser};

use crate::model::{ChunkKind, CodeChunk};

const CHUNK_LINES: usize = 80;
const OVERLAP_LINES: usize = 10;
const MAX_SYNTAX_LINES: usize = 160;
const MAX_SIGNATURE_BYTES: usize = 180;

pub fn detect_language(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => Some("rust"),
        Some("ts") | Some("tsx") => Some("typescript"),
        Some("js") | Some("jsx") => Some("javascript"),
        Some("py") => Some("python"),
        Some("md") => Some("markdown"),
        Some("toml") | Some("json") => Some("config"),
        _ if path.file_name().and_then(|name| name.to_str()) == Some(".gitignore") => {
            Some("config")
        }
        _ => None,
    }
}

pub fn chunk_file(repo_root: &Path, file_path: &Path, text: &str) -> Vec<CodeChunk> {
    let Some(language) = detect_language(file_path) else {
        return Vec::new();
    };
    if language == "markdown" {
        let chunks = markdown_chunks(repo_root, file_path, language, text);
        return if chunks.is_empty() {
            fallback_line_chunks(repo_root, file_path, language, text)
        } else {
            chunks
        };
    }
    if let Some(chunks) = syntax_chunks(repo_root, file_path, language, text) {
        if !chunks.is_empty() {
            return chunks;
        }
    }
    fallback_line_chunks(repo_root, file_path, language, text)
}

fn fallback_line_chunks(
    repo_root: &Path,
    file_path: &Path,
    language: &str,
    text: &str,
) -> Vec<CodeChunk> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let relative_path = file_path.strip_prefix(repo_root).unwrap_or(file_path);
    let boundaries = symbol_boundaries(language, &lines);
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < lines.len() {
        if let Some(boundary) = nearest_boundary(start, &boundaries) {
            start = boundary;
        }

        let end = usize::min(start + CHUNK_LINES, lines.len());
        let chunk_text = lines[start..end].join("\n");
        let start_line = start + 1;
        let end_line = end;
        let symbol = detect_symbol(language, &chunk_text);
        let id = stable_id(relative_path, start_line, end_line);

        chunks.push(CodeChunk {
            id,
            repo_root: repo_root.to_path_buf(),
            file_path: relative_path.to_path_buf(),
            language: language.to_string(),
            kind: fallback_kind(file_path, language, &chunk_text),
            symbol,
            signature: first_meaningful_line(&chunk_text),
            parent_symbol: None,
            start_line,
            end_line,
            text: chunk_text,
        });

        if end == lines.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP_LINES);
    }

    chunks
}

fn syntax_chunks(
    repo_root: &Path,
    file_path: &Path,
    language: &str,
    text: &str,
) -> Option<Vec<CodeChunk>> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_language(language)?).ok()?;
    let tree = parser.parse(text, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    let mut raw = Vec::new();
    collect_syntax_nodes(tree.root_node(), language, text.as_bytes(), None, &mut raw);
    let relative_path = file_path.strip_prefix(repo_root).unwrap_or(file_path);
    let mut chunks = Vec::new();
    for raw_chunk in raw {
        let start_line = raw_chunk.node.start_position().row + 1;
        let end_line = raw_chunk.node.end_position().row + 1;
        let chunk_text = text[raw_chunk.node.byte_range()].trim_end().to_string();
        if chunk_text.trim().is_empty() || should_drop_small(raw_chunk.kind, &chunk_text) {
            continue;
        }
        let signature = signature_for(&chunk_text);
        let base = CodeChunk {
            id: stable_id(relative_path, start_line, end_line),
            repo_root: repo_root.to_path_buf(),
            file_path: relative_path.to_path_buf(),
            language: language.to_string(),
            kind: raw_chunk.kind,
            symbol: raw_chunk.symbol,
            signature,
            parent_symbol: raw_chunk.parent_symbol,
            start_line,
            end_line,
            text: chunk_text,
        };
        push_safeguarded_chunk(base, &mut chunks);
    }
    Some(chunks)
}

fn tree_sitter_language(language: &str) -> Option<Language> {
    match language {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        _ => None,
    }
}

#[derive(Clone)]
struct RawNode<'a> {
    node: Node<'a>,
    kind: ChunkKind,
    symbol: Option<String>,
    parent_symbol: Option<String>,
}

fn collect_syntax_nodes<'a>(
    node: Node<'a>,
    language: &str,
    source: &[u8],
    parent_symbol: Option<String>,
    output: &mut Vec<RawNode<'a>>,
) {
    let kind = node.kind();
    let (chunk_kind, symbol) = match language {
        "rust" => rust_chunk(kind, node, source),
        "typescript" | "javascript" => js_chunk(kind, node, source),
        "python" => python_chunk(kind, node, source),
        _ => (None, None),
    };

    let mut next_parent = parent_symbol.clone();
    if let Some(chunk_kind) = chunk_kind {
        let symbol = symbol.or_else(|| detect_symbol(language, node_text(node, source)));
        output.push(RawNode {
            node,
            kind: chunk_kind,
            symbol: symbol.clone(),
            parent_symbol: parent_symbol.clone(),
        });
        if matches!(chunk_kind, ChunkKind::Class | ChunkKind::Impl) {
            next_parent = symbol;
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_nodes(child, language, source, next_parent.clone(), output);
    }
}

fn rust_chunk(kind: &str, node: Node<'_>, source: &[u8]) -> (Option<ChunkKind>, Option<String>) {
    let symbol = node
        .child_by_field_name("name")
        .map(|name| node_text(name, source).to_string());
    match kind {
        "function_item" => {
            let chunk_kind = if is_rust_test(node, source) {
                ChunkKind::Test
            } else {
                ChunkKind::Function
            };
            (Some(chunk_kind), symbol)
        }
        "struct_item" => (Some(ChunkKind::Struct), symbol),
        "enum_item" => (Some(ChunkKind::Enum), symbol),
        "trait_item" => (Some(ChunkKind::Trait), symbol),
        "impl_item" => (Some(ChunkKind::Impl), Some(rust_impl_symbol(node, source))),
        "mod_item" => (Some(ChunkKind::Module), symbol),
        _ => (None, None),
    }
}

fn js_chunk(kind: &str, node: Node<'_>, source: &[u8]) -> (Option<ChunkKind>, Option<String>) {
    match kind {
        "function_declaration" => (
            Some(ChunkKind::Function),
            node.child_by_field_name("name")
                .map(|n| node_text(n, source).to_string()),
        ),
        "class_declaration" => (
            Some(ChunkKind::Class),
            node.child_by_field_name("name")
                .map(|n| node_text(n, source).to_string()),
        ),
        "method_definition" => (
            Some(ChunkKind::Method),
            node.child_by_field_name("name")
                .map(|n| node_text(n, source).to_string()),
        ),
        "lexical_declaration" | "variable_declaration"
            if node_text(node, source).contains("=>") =>
        {
            (Some(ChunkKind::Function), js_decl_symbol(node, source))
        }
        "export_statement" => (None, None),
        _ => (None, None),
    }
}

fn python_chunk(kind: &str, node: Node<'_>, source: &[u8]) -> (Option<ChunkKind>, Option<String>) {
    match kind {
        "function_definition" => {
            let symbol = node
                .child_by_field_name("name")
                .map(|n| node_text(n, source).to_string());
            let chunk_kind = if symbol
                .as_deref()
                .is_some_and(|name| name.starts_with("test_"))
            {
                ChunkKind::Test
            } else {
                ChunkKind::Function
            };
            (Some(chunk_kind), symbol)
        }
        "class_definition" => {
            let symbol = node
                .child_by_field_name("name")
                .map(|n| node_text(n, source).to_string());
            let chunk_kind = if symbol
                .as_deref()
                .is_some_and(|name| name.starts_with("Test"))
            {
                ChunkKind::Test
            } else {
                ChunkKind::Class
            };
            (Some(chunk_kind), symbol)
        }
        _ => (None, None),
    }
}

fn is_rust_test(node: Node<'_>, source: &[u8]) -> bool {
    let start = node.start_byte().saturating_sub(160);
    let prefix = std::str::from_utf8(&source[start..node.start_byte()]).unwrap_or_default();
    prefix.contains("#[test]")
        || prefix.contains("#[tokio::test]")
        || prefix.contains("#[async_std::test]")
}

fn rust_impl_symbol(node: Node<'_>, source: &[u8]) -> String {
    node_text(node, source)
        .lines()
        .next()
        .unwrap_or("impl")
        .trim()
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn js_decl_symbol(node: Node<'_>, source: &[u8]) -> Option<String> {
    let text = node_text(node, source).trim();
    Regex::new(r"^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)")
        .expect("valid js decl regex")
        .captures(text)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or_default()
}

fn markdown_chunks(
    repo_root: &Path,
    file_path: &Path,
    language: &str,
    text: &str,
) -> Vec<CodeChunk> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let relative_path = file_path.strip_prefix(repo_root).unwrap_or(file_path);
    let headings = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| markdown_heading(line).map(|heading| (idx, heading)))
        .collect::<Vec<_>>();
    if headings.is_empty() {
        return fallback_line_chunks(repo_root, file_path, language, text);
    }
    let mut chunks = Vec::new();
    for (pos, (start, heading)) in headings.iter().enumerate() {
        let end = headings
            .get(pos + 1)
            .map(|(next, _)| *next)
            .unwrap_or(lines.len());
        let chunk_text = lines[*start..end].join("\n");
        let chunk = CodeChunk {
            id: stable_id(relative_path, start + 1, end),
            repo_root: repo_root.to_path_buf(),
            file_path: relative_path.to_path_buf(),
            language: language.to_string(),
            kind: ChunkKind::MarkdownSection,
            symbol: Some(heading.clone()),
            signature: Some(lines[*start].trim().to_string()),
            parent_symbol: None,
            start_line: start + 1,
            end_line: end,
            text: chunk_text,
        };
        push_safeguarded_chunk(chunk, &mut chunks);
    }
    chunks
}

fn markdown_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
        Some(trimmed[hashes..].trim().to_string())
    } else {
        None
    }
}

fn push_safeguarded_chunk(chunk: CodeChunk, chunks: &mut Vec<CodeChunk>) {
    if chunk.end_line.saturating_sub(chunk.start_line) + 1 <= MAX_SYNTAX_LINES {
        chunks.push(chunk);
        return;
    }
    let lines = chunk.text.lines().collect::<Vec<_>>();
    let mut start = 0usize;
    while start < lines.len() {
        let end = usize::min(start + CHUNK_LINES, lines.len());
        let text = lines[start..end].join("\n");
        let start_line = chunk.start_line + start;
        let end_line = chunk.start_line + end - 1;
        let mut split = chunk.clone();
        split.id = stable_id(&chunk.file_path, start_line, end_line);
        split.start_line = start_line;
        split.end_line = end_line;
        split.text = text;
        split.signature = chunk.signature.clone();
        chunks.push(split);
        if end == lines.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP_LINES);
    }
}

fn should_drop_small(kind: ChunkKind, text: &str) -> bool {
    let non_blank = text.lines().filter(|line| !line.trim().is_empty()).count();
    non_blank < 4
        && !matches!(
            kind,
            ChunkKind::Struct
                | ChunkKind::Enum
                | ChunkKind::Trait
                | ChunkKind::Class
                | ChunkKind::Impl
                | ChunkKind::Config
                | ChunkKind::MarkdownSection
                | ChunkKind::Function
                | ChunkKind::Method
                | ChunkKind::Test
        )
}

fn fallback_kind(file_path: &Path, language: &str, text: &str) -> ChunkKind {
    let path = file_path.to_string_lossy().to_lowercase();
    if language == "markdown" {
        ChunkKind::MarkdownSection
    } else if path.contains("config")
        || path.contains("setting")
        || path.ends_with("cargo.toml")
        || path.ends_with("package.json")
        || text.contains("config")
    {
        ChunkKind::Config
    } else {
        ChunkKind::Unknown
    }
}

fn signature_for(text: &str) -> Option<String> {
    first_meaningful_line(text).map(|line| truncate_signature(line.trim_end_matches('{').trim()))
}

fn first_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(truncate_signature)
}

fn truncate_signature(line: &str) -> String {
    if line.len() <= MAX_SIGNATURE_BYTES {
        return line.to_string();
    }
    let mut truncated = line
        .char_indices()
        .take_while(|(idx, _)| *idx < MAX_SIGNATURE_BYTES)
        .map(|(_, ch)| ch)
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

pub fn detect_symbol(language: &str, text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(symbol) = detect_symbol_line(language, line) {
            return Some(symbol);
        }
    }
    None
}

fn symbol_boundaries(language: &str, lines: &[&str]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| detect_symbol_line(language, line).map(|_| idx))
        .collect()
}

fn nearest_boundary(start: usize, boundaries: &[usize]) -> Option<usize> {
    boundaries
        .iter()
        .copied()
        .find(|boundary| *boundary >= start && *boundary <= start + 10)
}

fn detect_symbol_line(language: &str, line: &str) -> Option<String> {
    let trimmed = line.trim();
    let patterns: &[&str] = match language {
        "rust" => &[
            r"^(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^impl(?:\s+([A-Za-z_][A-Za-z0-9_]*))?(?:\s+for\s+([A-Za-z_][A-Za-z0-9_]*))?",
        ],
        "typescript" | "javascript" => &[
            r"^(?:export\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"^(?:export\s+)?const\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=",
            r"^let\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=",
            r"^(?:export\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)",
        ],
        "python" => &[
            r"^def\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^class\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        "markdown" => &[r"^#{1,2}\s+(.+)$"],
        _ => &[],
    };

    for pattern in patterns {
        let regex = Regex::new(pattern).expect("valid symbol regex");
        if let Some(captures) = regex.captures(trimmed) {
            if language == "rust" && trimmed.starts_with("impl") {
                return Some(trimmed.trim_end_matches('{').trim().to_string());
            }
            if let Some(value) = captures.get(1) {
                return Some(value.as_str().trim().to_string());
            }
        }
    }
    None
}

fn stable_id(path: &Path, start_line: usize, end_line: usize) -> String {
    let input = format!("{}:{start_line}:{end_line}", path.display());
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_languages() {
        assert_eq!(detect_language(Path::new("lib.rs")), Some("rust"));
        assert_eq!(detect_language(Path::new("app.tsx")), Some("typescript"));
        assert_eq!(detect_language(Path::new("script.js")), Some("javascript"));
        assert_eq!(detect_language(Path::new("main.py")), Some("python"));
        assert_eq!(detect_language(Path::new("README.md")), Some("markdown"));
        assert_eq!(detect_language(Path::new("image.png")), None);
    }

    #[test]
    fn chunks_with_line_ranges_and_overlap() {
        let source = (1..=100)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = fallback_line_chunks(
            Path::new("/repo"),
            Path::new("/repo/src/lib.rs"),
            "rust",
            &source,
        );
        assert_eq!(chunks.len(), 2);
        assert_eq!((chunks[0].start_line, chunks[0].end_line), (1, 80));
        assert_eq!((chunks[1].start_line, chunks[1].end_line), (71, 100));
    }

    #[test]
    fn detects_symbols() {
        assert_eq!(
            detect_symbol("rust", "pub fn refresh_access_token() {}"),
            Some("refresh_access_token".into())
        );
        assert_eq!(
            detect_symbol("typescript", "export const refreshToken = () => {}"),
            Some("refreshToken".into())
        );
        assert_eq!(
            detect_symbol("python", "class TokenRefresher:"),
            Some("TokenRefresher".into())
        );
        assert_eq!(
            detect_symbol("markdown", "## Auth Refresh"),
            Some("Auth Refresh".into())
        );
    }

    #[test]
    fn rust_syntax_chunking_extracts_complete_function() {
        let source = r#"
fn helper() {
    let value = 1;
    println!("{value}");
}
"#;
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/src/lib.rs"), source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Function);
        assert_eq!(chunks[0].symbol.as_deref(), Some("helper"));
        assert!(chunks[0].text.contains("println!"));
    }

    #[test]
    fn rust_syntax_chunking_extracts_types_and_impls() {
        let source = r#"
pub struct Searcher;
enum Mode { Fast }
trait Rank { fn rank(&self); }
impl Rank for Searcher {
    fn rank(&self) {}
}
"#;
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/src/lib.rs"), source);
        let kinds = chunks.iter().map(|chunk| chunk.kind).collect::<Vec<_>>();
        assert!(kinds.contains(&ChunkKind::Struct));
        assert!(kinds.contains(&ChunkKind::Enum));
        assert!(kinds.contains(&ChunkKind::Trait));
        assert!(kinds.contains(&ChunkKind::Impl));
    }

    #[test]
    fn rust_test_function_is_marked_test() {
        let source = r#"
#[test]
fn chunks_work() {
    assert!(true);
}
"#;
        let chunks = chunk_file(
            Path::new("/repo"),
            Path::new("/repo/tests/chunker.rs"),
            source,
        );
        assert_eq!(chunks[0].kind, ChunkKind::Test);
        assert_eq!(chunks[0].symbol.as_deref(), Some("chunks_work"));
    }

    #[test]
    fn typescript_extracts_functions_classes_and_methods() {
        let source = r#"
export function rankResults() {
  return 1;
}
export const chunkFile = () => {
  return 2;
};
class Searcher {
  search() {
    return [];
  }
}
"#;
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/src/search.ts"), source);
        assert!(chunks.iter().any(|chunk| chunk.kind == ChunkKind::Function
            && chunk.symbol.as_deref() == Some("rankResults")));
        assert!(chunks.iter().any(|chunk| chunk.kind == ChunkKind::Function
            && chunk.symbol.as_deref() == Some("chunkFile")));
        assert!(
            chunks.iter().any(|chunk| chunk.kind == ChunkKind::Class
                && chunk.symbol.as_deref() == Some("Searcher"))
        );
        assert!(
            chunks.iter().any(|chunk| chunk.kind == ChunkKind::Method
                && chunk.symbol.as_deref() == Some("search"))
        );
    }

    #[test]
    fn python_extracts_functions_and_classes() {
        let source = r#"
def rank_results():
    return 1

class Searcher:
    def search(self):
        return []
"#;
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/search.py"), source);
        assert!(chunks.iter().any(|chunk| chunk.kind == ChunkKind::Function
            && chunk.symbol.as_deref() == Some("rank_results")));
        assert!(
            chunks.iter().any(|chunk| chunk.kind == ChunkKind::Class
                && chunk.symbol.as_deref() == Some("Searcher"))
        );
    }

    #[test]
    fn markdown_headings_become_sections() {
        let source = "# Title\nintro\n## Supported\nRust\n## Tests\ncargo test\n";
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/README.md"), source);
        assert_eq!(chunks.len(), 3);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.kind == ChunkKind::MarkdownSection)
        );
        assert_eq!(chunks[1].symbol.as_deref(), Some("Supported"));
        assert_eq!((chunks[1].start_line, chunks[1].end_line), (3, 4));
    }

    #[test]
    fn parser_failure_falls_back_to_line_chunking() {
        let source = "fn broken( {\nlet x = ;\n";
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/src/lib.rs"), source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Unknown);
        assert_eq!((chunks[0].start_line, chunks[0].end_line), (1, 2));
    }

    #[test]
    fn large_syntax_chunks_are_split_safely() {
        let body = (1..=190)
            .map(|n| format!("    let value_{n} = {n};"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("fn large() {{\n{body}\n}}\n");
        let chunks = chunk_file(Path::new("/repo"), Path::new("/repo/src/lib.rs"), &source);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.kind == ChunkKind::Function));
        assert_eq!(chunks[0].start_line, 1);
        assert!(chunks[0].end_line <= 80);
    }
}
