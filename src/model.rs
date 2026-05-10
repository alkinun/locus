use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Impl,
    Test,
    Module,
    MarkdownSection,
    Config,
    Unknown,
}

impl ChunkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Test => "test",
            Self::Module => "module",
            Self::MarkdownSection => "markdown_section",
            Self::Config => "config",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_index(value: &str) -> Self {
        match value {
            "function" => Self::Function,
            "method" => Self::Method,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "impl" => Self::Impl,
            "test" => Self::Test,
            "module" => Self::Module,
            "markdown_section" => Self::MarkdownSection,
            "config" => Self::Config,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChunk {
    pub id: String,
    pub repo_root: PathBuf,
    pub file_path: PathBuf,
    pub language: String,
    pub kind: ChunkKind,
    pub symbol: Option<String>,
    pub signature: Option<String>,
    pub parent_symbol: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub rank: usize,
    pub score: f32,
    pub file_path: String,
    pub language: String,
    pub kind: ChunkKind,
    pub symbol: Option<String>,
    pub signature: Option<String>,
    pub parent_symbol: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub reason: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct RankedChunk {
    pub chunk: CodeChunk,
    pub score: f32,
    pub reason: String,
}

impl RankedChunk {
    pub fn into_result(self, rank: usize) -> SearchResult {
        SearchResult {
            rank,
            score: self.score,
            file_path: self.chunk.file_path.display().to_string(),
            language: self.chunk.language,
            kind: self.chunk.kind,
            symbol: self.chunk.symbol,
            signature: self.chunk.signature,
            parent_symbol: self.chunk.parent_symbol,
            start_line: self.chunk.start_line,
            end_line: self.chunk.end_line,
            reason: self.reason,
            text: self.chunk.text,
        }
    }
}
