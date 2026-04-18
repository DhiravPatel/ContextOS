# ContextOS

> Local-first token reduction for AI coding assistants. Cut context tokens **≥50%** without changing output quality.

AI assistants (Cursor, Copilot, Claude Code, Cody) burn tokens by shoving whole repos into the prompt. ContextOS sits in front as a **pre-processor**: dedup → compress → rank → budget. Fewer tokens in, same quality out.

---

## Architecture

```
VS Code Extension (TypeScript)
        │
        ▼
Context Collector   ── active file, selection, imports, visible editors
        │
        ▼
contextos CLI (Rust)   ── stdin/stdout JSON
        │
        ├─► Dedup      exact hash + Jaccard near-duplicate
        ├─► Compress   tree-sitter AST strip (comments, logs, whitespace)
        ├─► Rank       TF-IDF against user's query + priority bias
        └─► Budget     greedy fit to max_tokens
        │
        ▼
Optimized bundle  →  your LLM
```

---

## Repository layout

```
ContextOS/
├── Cargo.toml                       # Rust workspace
├── package.json                     # npm workspace (extension only)
├── apps/
│   ├── cli/                         # `contextos` CLI binary (Rust)
│   └── extension/                   # VS Code extension (TypeScript)
├── crates/
│   ├── utils/                       # language detection, hashing, tokenising
│   ├── tokenizer/                   # dependency-free token estimator
│   ├── parser/                      # tree-sitter stripper + regex fallback
│   └── core-engine/                 # pipeline orchestrator
│       └── src/{dedup,compress,ranking,budget}/
├── infra/
│   ├── docker/Dockerfile            # CLI-only container image
│   └── scripts/build.sh             # one-shot full build
└── docs/
    ├── ARCHITECTURE.md
    └── USAGE.md
```

---

## What each crate does

| Crate | Responsibility |
|---|---|
| `contextos-utils` | language detection, AHash fingerprints, whitespace-normalised hashing, cheap tokeniser for ranking |
| `contextos-tokenizer` | BPE-approximating token estimator — calibrated to stay within ~5% of real tokenisers without shipping vocab files |
| `contextos-parser` | AST-aware stripping of comments / debug logs / whitespace via tree-sitter (default) or regex (fallback for no-C-compiler boxes) |
| `contextos-core-engine` | orchestrates the four-stage pipeline; returns before/after token counts so the UI can prove the reduction |
| `contextos-cli` (`apps/cli`) | thin stdin/stdout JSON bridge — spawnable from any editor, CI job, or shell script |

---

## Pipeline stages

1. **Dedup** — O(n) exact hash pass, then Jaccard-over-line-fingerprints for near-duplicates (threshold configurable, default 0.92).
2. **Compress** — tree-sitter AST walk removes comments, debug logs (`console.log`, `println!`, `print`), and redundant whitespace; runs in parallel via rayon. Regex fallback for environments without a C compiler.
3. **Rank** — TF-IDF of every chunk against the user's query, boosted by `kind` (user selections bubble up) and caller-supplied `priority`.
4. **Budget** — greedy packer that keeps highest-ranked chunks until `max_tokens` is reached, with a 5% overshoot slack for the last-fitting chunk.

Every stage reports token counts so you can see exactly where savings came from.

---

## VS Code extension

**Commands**

| Command | Key | Purpose |
|---|---|---|
| `ContextOS: Optimize Current Context` | `⌘⌥O` / `Ctrl+Alt+O` | Run the full pipeline on the active editor context |
| `ContextOS: Optimize Selection` | — | Same pipeline, selection-only |
| `ContextOS: Show Session Stats` | — | Cumulative tokens saved this session |

**Settings**

- `contextos.maxTokens` (default `8000`)
- `contextos.binaryPath` — override CLI location
- `contextos.includeImports` — pull local import targets into the bundle
- `contextos.includeOpenEditors` — also pull in other visible editors
- `contextos.showReductionToast` — notify after each run

---

## Quickstart

```bash
# Prerequisites: Rust (stable), Node.js ≥18
./infra/scripts/build.sh
```

That single script:
1. Builds the Rust workspace in release mode
2. Installs extension dependencies (first run only)
3. Compiles the TypeScript extension
4. Stages the CLI binary inside `apps/extension/bin/<platform>/` so the extension ships with it

### Running the CLI directly

```bash
echo '{
  "chunks": [{
    "id": "a",
    "language": "typescript",
    "content": "// large comment block...\nexport function add(a,b){ return a+b; }",
    "kind": "code",
    "priority": 0
  }],
  "query": "addition helper"
}' | ./target/release/contextos optimize --pretty
```

### Running the extension

1. Open `apps/extension` in VS Code.
2. Press `F5` to launch the Extension Development Host.
3. Open any file → `⌘⌥O` → type your intent → optimized bundle opens in a new editor.

---

## Testing

```bash
cargo test --workspace        # unit + integration tests for all Rust crates
npm --workspace apps/extension run compile   # extension type-check
```

Key integration tests assert:
- ≥40% reduction on realistic redundant TypeScript input
- Pipeline completes in <200ms on 50-chunk workloads
- Budget enforces its cap within 5% slack
- Near-dup detection catches small-edit variants

---

## Why a separate process instead of in-extension?

- **Trust surface is smaller.** Only the Rust binary reads bulk repo bytes. The extension is a thin collector + renderer.
- **Speed.** Rust + rayon compresses in parallel; typical runs <50ms end-to-end.
- **Portability.** The CLI is usable outside VS Code — pipe from `git diff`, run in CI, chain into any editor that can spawn a subprocess.

---

## Roadmap

- Persistent embedding cache keyed by content hash
- Symbol-graph ranking (follow imports two hops, not one)
- Daemon mode via Unix socket for zero-spawn overhead
- Optional exact-BPE tokenisation via `tiktoken-rs` feature

---

## License

MIT
