use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::indexer::index_repo;
use crate::output::group_ranked_results;
use crate::repo_meta::read_metadata;
use crate::reranker::download_reranker_model;
use crate::search::{DEFAULT_RERANK_INPUT_LIMIT, SearchOptions, SearchSession};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub repo_root: PathBuf,
    pub default_use_embeddings: bool,
    pub default_use_reranker: bool,
    pub default_rerank_limit: usize,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Map<String, Value>,
}

pub fn run_stdio_server(config: McpServerConfig) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = handle_json_message(&config, &line);
        if let Some(response) = response {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_json_message(config: &McpServerConfig, line: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Array(messages)) => {
            let responses = messages
                .into_iter()
                .filter_map(|message| handle_message(config, message))
                .collect::<Vec<_>>();
            if responses.is_empty() {
                None
            } else {
                Some(Value::Array(responses))
            }
        }
        Ok(message) => handle_message(config, message),
        Err(error) => Some(error_response(
            Value::Null,
            -32700,
            "Parse error",
            Some(json!(error.to_string())),
        )),
    }
}

fn handle_message(config: &McpServerConfig, message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    let Some(method) = method else {
        return id.map(|id| error_response(id, -32600, "Invalid request", None));
    };

    if id.is_none() {
        return handle_notification(method);
    }

    let id = id.expect("checked above");
    if !matches!(method, "initialize" | "ping" | "tools/list" | "tools/call") {
        return Some(error_response(
            id,
            -32601,
            "Method not found",
            Some(json!(method)),
        ));
    }

    match dispatch_request(config, method, params) {
        Ok(result) => Some(success_response(id, result)),
        Err(error) => Some(error_response(
            id,
            -32602,
            &error.to_string(),
            error.source().map(|source| json!(source.to_string())),
        )),
    }
}

fn handle_notification(method: &str) -> Option<Value> {
    match method {
        "notifications/initialized" | "notifications/cancelled" => None,
        method if method.starts_with("notifications/") => None,
        _ => None,
    }
}

fn dispatch_request(config: &McpServerConfig, method: &str, params: Value) -> Result<Value> {
    match method {
        "initialize" => Ok(initialize_result(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(config, params),
        _ => unreachable!("method checked before dispatch"),
    }
}

fn initialize_result(params: Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let protocol_version = SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .find(|version| *version == requested)
        .unwrap_or(MCP_PROTOCOL_VERSION);

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "locus",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Use search_codebase to locate relevant code chunks. Run index_status first if searches report a missing index; run index_codebase to build or rebuild .locus/index."
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "search_codebase",
            "description": "Search the configured codebase with locus and return ranked code chunks with paths, line ranges, symbols, reasons, and snippet text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language or symbol query describing the code to find."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional repository path override. Defaults to the path passed to `locus mcp --path`."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_SEARCH_LIMIT,
                        "default": 5,
                        "description": "Maximum number of ranked chunks to return."
                    },
                    "use_embeddings": {
                        "type": "boolean",
                        "description": "Use the local embedding index for semantic search. Defaults to the server setting."
                    },
                    "rerank": {
                        "type": "boolean",
                        "description": "Use the local cross-encoder reranker. Defaults to the server setting."
                    },
                    "rerank_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "default": DEFAULT_RERANK_INPUT_LIMIT,
                        "description": "Number of candidates to send to the reranker."
                    },
                    "grouped": {
                        "type": "boolean",
                        "default": false,
                        "description": "Group results into primary, supporting, tests, docs, and config buckets."
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true
            }
        },
        {
            "name": "index_codebase",
            "description": "Build or rebuild the locus index for a repository. This writes .locus/index inside the target repository.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional repository path override. Defaults to the path passed to `locus mcp --path`."
                    },
                    "download_embedding": {
                        "type": "boolean",
                        "default": false,
                        "description": "Download the embedding model before indexing if it is not already cached."
                    },
                    "download_reranker": {
                        "type": "boolean",
                        "default": false,
                        "description": "Download the reranker model before indexing if it is not already cached."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false
            }
        },
        {
            "name": "index_status",
            "description": "Report whether a repository has a locus index and summarize indexed metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional repository path override. Defaults to the path passed to `locus mcp --path`."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true
            }
        }
    ])
}

fn call_tool(config: &McpServerConfig, params: Value) -> Result<Value> {
    let params: ToolCallParams =
        serde_json::from_value(params).context("invalid tools/call params")?;
    let result = match params.name.as_str() {
        "search_codebase" => search_codebase(config, &params.arguments),
        "index_codebase" => index_codebase(config, &params.arguments),
        "index_status" => index_status(config, &params.arguments),
        other => Err(anyhow!("unknown tool `{other}`")),
    };

    Ok(match result {
        Ok(value) => tool_success(value),
        Err(error) => tool_error(error.to_string()),
    })
}

fn search_codebase(config: &McpServerConfig, args: &Map<String, Value>) -> Result<Value> {
    let query = required_string_arg(args, "query")?;
    let repo_root = repo_root_arg(config, args)?;
    let limit = optional_usize_arg(args, "limit")?.unwrap_or(5);
    if limit == 0 || limit > MAX_SEARCH_LIMIT {
        bail!("limit must be between 1 and {MAX_SEARCH_LIMIT}");
    }

    let use_embeddings =
        optional_bool_arg(args, "use_embeddings")?.unwrap_or(config.default_use_embeddings);
    let use_reranker = optional_bool_arg(args, "rerank")?.unwrap_or(config.default_use_reranker);
    let rerank_limit =
        optional_usize_arg(args, "rerank_limit")?.unwrap_or(config.default_rerank_limit);
    let grouped = optional_bool_arg(args, "grouped")?.unwrap_or(false);

    let session = SearchSession::open_with_options(
        &repo_root,
        SearchOptions {
            use_embeddings,
            use_reranker,
            rerank_limit,
        },
    )?;
    let summary = session.search(query, limit)?;
    let results = summary
        .results
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, ranked)| ranked.into_result(idx + 1))
        .collect::<Vec<_>>();

    let mut value = json!({
        "query": query,
        "repo_root": repo_root.display().to_string(),
        "limit": limit,
        "elapsed_ms": summary.elapsed.as_millis(),
        "analyzed": summary.analyzed,
        "results": results,
    });
    if grouped {
        value["grouped"] = serde_json::to_value(group_ranked_results(&summary.results))?;
    }
    Ok(value)
}

fn index_codebase(config: &McpServerConfig, args: &Map<String, Value>) -> Result<Value> {
    let repo_root = repo_root_arg(config, args)?;
    if optional_bool_arg(args, "download_reranker")?.unwrap_or(false) {
        download_reranker_model()?;
    }
    let summary = index_repo(
        &repo_root,
        optional_bool_arg(args, "download_embedding")?.unwrap_or(false),
    )?;
    let kind_counts = summary
        .kind_counts
        .into_iter()
        .map(|(kind, count)| json!({ "kind": kind, "count": count }))
        .collect::<Vec<_>>();

    Ok(json!({
        "repo_root": repo_root.display().to_string(),
        "index_path": summary.index_path.display().to_string(),
        "files": summary.files,
        "chunks": summary.chunks,
        "kind_counts": kind_counts,
        "elapsed_ms": summary.elapsed.as_millis(),
        "repo_metadata": metadata_summary(&summary.repo_metadata),
    }))
}

fn index_status(config: &McpServerConfig, args: &Map<String, Value>) -> Result<Value> {
    let repo_root = repo_root_arg(config, args)?;
    let index_path = repo_root.join(".locus").join("index");
    if !index_path.exists() {
        return Ok(json!({
            "repo_root": repo_root.display().to_string(),
            "indexed": false,
            "index_path": index_path.display().to_string(),
            "hint": format!("Run `locus index --path {}` or call index_codebase.", repo_root.display()),
        }));
    }

    let session = SearchSession::open_with_options(
        &repo_root,
        SearchOptions {
            use_embeddings: false,
            use_reranker: false,
            rerank_limit: 0,
        },
    )?;
    let meta = read_metadata(&repo_root)?.unwrap_or_default();
    Ok(json!({
        "repo_root": repo_root.display().to_string(),
        "indexed": true,
        "index_path": index_path.display().to_string(),
        "chunks": session.chunk_count(),
        "repo_metadata": metadata_summary(&meta),
    }))
}

fn metadata_summary(meta: &crate::repo_meta::RepoMetadata) -> Value {
    json!({
        "languages": meta.vocabulary.languages,
        "extensions": meta.vocabulary.extensions,
        "symbols": meta.vocabulary.symbols.len(),
        "headings": meta.vocabulary.headings.len(),
        "dependencies": meta.vocabulary.dependencies,
        "config_keys": meta.vocabulary.config_keys.len(),
        "symbol_references": meta.symbol_graph.references.len(),
    })
}

fn repo_root_arg(config: &McpServerConfig, args: &Map<String, Value>) -> Result<PathBuf> {
    let path = match args.get("path") {
        Some(Value::String(path)) => Path::new(path),
        Some(_) => bail!("path must be a string"),
        None => config.repo_root.as_path(),
    };
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))
}

fn required_string_arg<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    match args.get(name) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value),
        Some(Value::String(_)) => bail!("{name} must not be empty"),
        Some(_) => bail!("{name} must be a string"),
        None => bail!("missing required argument `{name}`"),
    }
}

fn optional_bool_arg(args: &Map<String, Value>, name: &str) -> Result<Option<bool>> {
    match args.get(name) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => bail!("{name} must be a boolean"),
        None => Ok(None),
    }
}

fn optional_usize_arg(args: &Map<String, Value>, name: &str) -> Result<Option<usize>> {
    match args.get(name) {
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| anyhow!("{name} must be a non-negative integer")),
        Some(_) => bail!("{name} must be an integer"),
        None => Ok(None),
    }
}

fn tool_success(structured_content: Value) -> Value {
    let text = serde_json::to_string_pretty(&structured_content)
        .unwrap_or_else(|_| structured_content.to_string());
    json!({
        "content": [
            {
                "type": "text",
                "text": text,
            }
        ],
        "structuredContent": structured_content,
        "isError": false,
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": message,
            }
        ],
        "isError": true,
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> McpServerConfig {
        McpServerConfig {
            repo_root: PathBuf::from("."),
            default_use_embeddings: false,
            default_use_reranker: false,
            default_rerank_limit: DEFAULT_RERANK_INPUT_LIMIT,
        }
    }

    #[test]
    fn initialize_returns_capabilities() {
        let response = handle_json_message(
            &config(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .expect("response");

        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_exposes_search_index_and_status() {
        let response = handle_json_message(
            &config(),
            r#"{"jsonrpc":"2.0","id":"tools","method":"tools/list"}"#,
        )
        .expect("response");
        let tools = response["result"]["tools"].as_array().expect("tools array");
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec!["search_codebase", "index_codebase", "index_status"]
        );
    }

    #[test]
    fn notification_does_not_create_response() {
        assert!(
            handle_json_message(
                &config(),
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
    }
}
