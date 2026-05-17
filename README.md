locus.

mind readingly accurate, blazingly fast codebase search, written in pure rust.

its built for the kinds of fuzzy queries developers and coding agents ask.

---

### what it does

you ask locus something like:

```
where is the retry logic for failed http requests
```

```
the function that validates jwt tokens before the handler runs
```

```
error handling when the db connection drops
```

and locus finds the right parts of the code you are looking for.

it uses a three layer approach: lexical search (tantivy), semantic embeddings (fastembed, runs locally), and a reranker. you choose how much accuracy you need versus how much latency you can tolerate.

---

### numbers

192 queries. developer questions across 11 query styles.

| mode | recall@1 | recall@5 | mrr | p50 |
|---|---|---|---|---|
| lexical only | 9.9% | 31% | 0.19 | 19ms |
| + embeddings | 37.0% | 75% | 0.53 | 59ms |
| + embeddings + reranker | **71.4%** | **87%** | **0.79** | 2.5s |

the reranker mode is slow but powerful. it is perfect for an agent making one search call.

full eval breakdown in [evals.md](./evals.md).

---

### install

```bash
cargo install --path .
```

---

### usage

```bash
# build the local index first
locus index --path /path/to/repo

# first run, if the embedding model is not already cached
locus index --path /path/to/repo --download-embedding

# interactive terminal ui
locus --path /path/to/repo

# lexical search. instant, good for known symbol names
locus search "AuthMiddleware" --no-embedding

# semantic search. default, much better fuzzy recall
locus search "where does the session get invalidated"

# reranker. best recall, worth the wait for agents and scripts
locus search "retry logic for failed http requests" --rerank

# json output. pipe to anything
locus search "database connection pooling" --format json | jq '.[].file_path'

# grouped output. split primary code, supporting types, tests, docs, and config
locus search "tests for chunking" --grouped --format json

# search a specific directory
locus search "error handling in the parser" --path /path/to/repo

# run as a stdio mcp server for coding agents
locus mcp --path /path/to/repo
```

---

### mcp server

`locus mcp` starts a stdio mcp server that a coding agent can use to search the repo it is working in. configure the agent to launch:

```bash
locus mcp --path /path/to/repo
```

it exposes three tools:

- `search_codebase`: search indexed code chunks by natural language or symbol.
- `index_codebase`: build or rebuild `.locus/index` for the repo.
- `index_status`: check whether the repo has a usable locus index.

the mcp server writes protocol messages only to stdout. it indexes progress and diagnostics go to stderr.

---

### how it works

locus indexes your codebase using treesitter to parse code into semantically meaningful chunks. it understands function boundaries, class definitions, impls, structs, enums, traits, tests, and modules in rust, python, javascript, typescript, go, java, and c. it also indexes markdown headings and small config files.

at query time it combines:

- **bm25** (tantivy) for exact and near-exact matches
- **local embeddings** (fastembed, runs entirely on your machine) for semantic similarity
- **an optional cross-encoder reranker** to re-score the top candidates

no data leaves your machine during indexing or search. the index lives next to your code in `.locus/index`.

---

### supported languages

rust · python · javascript · typescript · go · java · c · markdown · config

more coming.

---

### philosophy

one thing done well. locus is a search tool. it does not generate code, summarize files, or try to be an agent itself. it answers "where is this" with high accuracy and low noise.

---

### contributing

issues and prs very welcome. if you are adding a code language, start with the tree-sitter grammar and a set of eval queries.
