# ContextOS — Architecture

## Problem

AI coding assistants (Cursor, Claude Code, Copilot, Cody) embed the editor and
send large swaths of the repo as context every time the user asks for help.
Most of that payload is redundant: repeated imports, boilerplate, stale
comments, logs, whole functions that are irrelevant to the current query.

The LLM doesn't need the raw repo. It needs the **relevant, compressed** slice.

## Pipeline

```
VS Code
  │
  ▼
Context Collector (TypeScript)        collects: active file, selection,
  │                                    imports, visible editors
  ▼
contextos CLI (Rust, stdin/stdout JSON)
  │
  ├─► 1. Dedup          (exact + Jaccard near-dup)
  ├─► 2. Compress       (tree-sitter AST → strip comments/logs/whitespace)
  ├─► 3. Rank           (TF-IDF vs query + priority + kind bias)
  └─► 4. Budget         (greedy fit to max_tokens)
  │
  ▼
Optimized bundle → user pastes into their LLM
```

## Crates

| Crate | Role |
|---|---|
| `contextos-utils` | language detection, hashing, tokenization helpers |
| `contextos-tokenizer` | heuristic token estimator (no tiktoken dep) |
| `contextos-parser` | tree-sitter stripping with regex fallback |
| `contextos-core-engine` | pipeline orchestrator + dedup/compress/rank/budget |
| `contextos-cli` | stdin/stdout JSON bridge (`apps/cli`) |

## Token budget math

Each pass reports `tokens_before → tokens_after` so the extension can prove
reduction to the user:

- **Dedup** typically 5–25% reduction on real repos (high on microservice
  templates, low on libraries).
- **Compress** 20–40% on comment-heavy code (JS/TS, internal tooling).
- **Budget** whatever is left after ranking — hard cap at `max_tokens`.

Combined, the median reduction sits in the 50–65% band on our test corpus.

## Why a separate process?

- **Trust** — no TS/Node code reads the repo's bytes in bulk. Only the Rust
  binary does. Audit surface stays small.
- **Perf** — Rust + rayon compresses in parallel; typical runs ≤50ms.
- **Portability** — the CLI works outside VS Code: pipe from `git diff`,
  embed in CI, script with shell.

## Roadmap

- Symbol-graph ranking (follow imports two hops instead of one)
- Persistent embedding cache keyed by content hash
- Daemon mode with Unix socket (avoids spawn overhead for heavy users)
- Exact BPE tokenization via optional `tiktoken-rs` feature
