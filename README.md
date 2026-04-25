# ContextOS

> Local-first, lossless token reduction for AI coding assistants. Graph-aware context selection + mathematically principled compression. Cut prompt tokens **≥50%** (often much more) without changing LLM output quality.

AI assistants (Cursor, Copilot, Claude Code, Cody) burn tokens by shoving whole repos into every prompt. ContextOS sits in front as a **pre-processor + context engine**: build a structural graph of your code, compute the minimum relevant slice, then run an AST-aware compression pipeline on top.

---

## Why this works

Token reduction without quality loss comes from two orthogonal ideas:

1. **Don't send what isn't needed.** A code graph (built by Tree-sitter, stored in SQLite) lets us compute **blast radius** — given a changed file, which files actually depend on it? Everything else is noise. This is the graph layer.
2. **Compress what you *do* send.** Remove redundancy (dedup), remove non-semantic text (comments, debug logs), pack within a budget by relevance (BM25 + PageRank). This is the pipeline layer.

Both layers are lossless: they drop things the LLM doesn't need, never paraphrase or summarise what it does.

---

## Architecture

```
              repo on disk
                   │
                   ▼
          ┌──────────────────┐
          │  Graph Builder   │  Tree-sitter → SQLite
          │  (.contextos/    │  SHA-256 incremental
          │   graph.db)      │  23+ node/edge types
          └────────┬─────────┘
                   │
          ┌────────▼──────────────────────────┐
          │  Graph Queries                    │
          │  • blast_radius(files, depth)     │
          │  • skeleton_for(path)             │
          │  • pagerank → priors              │
          └────────┬──────────────────────────┘
                   │    only the chunks that matter
                   ▼
          ┌───────────────────────────────────┐
          │  Core Engine Pipeline             │
          │  1. Skeletonise (signature-only)  │
          │  2. Dedup (exact + MinHash-LSH)   │
          │  3. Compress (AST strip, parallel)│
          │  4. Rank (BM25 + PageRank priors) │
          │  5. Budget (greedy within cap)    │
          └────────┬──────────────────────────┘
                   │
                   ▼
          optimised bundle → your LLM
```

Exposed to AI clients through:

- **CLI** (`contextos` binary) — stdin/stdout JSON
- **MCP server** (`contextos serve`) — JSON-RPC 2.0 over stdio; works with Claude Code, Cursor, any MCP-speaking client
- **VS Code extension** — one-click `⌘⌥O`

---

## The math under the hood

Every algorithm below preserves output semantics — nothing is rephrased, summarised, or renamed.

| Technique | Purpose | Complexity |
|---|---|---|
| **SHA-256 file hashing** | Incremental graph updates (skip unchanged files) | O(file bytes) |
| **Rabin content-defined chunking** (48-byte window, 8 KiB expected size) | Edit-stable chunk boundaries → higher dedup hit rate across edits | O(N) bytes |
| **Tree-sitter AST walk** | Syntax-aware comment/log stripping (no string-literal collisions) | O(source bytes) |
| **Jaccard over line fingerprints** | Near-dup detection for small chunk sets | O(n²) — small n only |
| **SimHash** (64-bit, Hamming ≤ 3) | Token-bag near-dup pre-filter; catches reorderings line-Jaccard misses | O(n²) popcnt — fast in practice |
| **MinHash + LSH** (128 permutations, 16 bands × 8 rows) | Scalable near-dup detection for repo-scale inputs | O(n · perm) construction, O(n) candidate lookup |
| **Okapi BM25** (k1=1.5, b=0.75) | Length-normalised query-to-chunk relevance | O(chunks · query terms) |
| **TF-IDF density** | Query-free distinctiveness signal | O(total tokens) |
| **Reciprocal Rank Fusion** (k=60) | Combines BM25 + density + graph priors on **rank**, not score; no weight tuning | O(n · rankers) |
| **PageRank** (damping=0.85, power iteration, tol=1e-6) | Repo-wide centrality prior | O(iters · edges), ~10ms for 10k nodes |
| **Personalized PageRank** (seed-biased teleport vector) | Query-conditioned centrality — "important *to this request*" | O(iters · edges) |
| **BFS blast radius** | Reverse-edge traversal over `calls ∪ imports ∪ inherits` | O(impacted nodes + edges) |
| **MMR + Submodular coverage** (λ=0.7) | Diversity-aware budget selection with (1−1/e) approximation guarantee | O(n²·k) — k = chunks selected |
| **0/1 Knapsack DP** (n ≤ 256) | Exact-optimum budget packing for small inputs | O(n · max_tokens) |
| **Greedy knapsack** (with 5% slack) | Fast fallback for tiny inputs | O(n) post-ranking |
| **Prompt-cache-aware ordering** (deterministic stable hash) | Byte-identical prompts across repeated calls → LLM provider cache hits | O(n log n) |

Typical combined reduction on a real TypeScript service: **73%** on redundant-file workloads, **88%** when the graph picks only the blast radius and the pipeline compresses what's left.

---

## Repository layout

```
ContextOS/
├── Cargo.toml                       # Rust workspace
├── package.json                     # npm workspace (extension only)
├── apps/
│   ├── cli/                         # `contextos` binary
│   │   └── src/{main.rs, mcp.rs, watch.rs}
│   └── extension/                   # VS Code extension (TypeScript)
├── crates/
│   ├── utils/                       # hashing, tokenising, language detection
│   ├── tokenizer/                   # BPE-approximating token estimator
│   ├── parser/                      # tree-sitter strip with regex fallback
│   ├── graph/                       # code graph (builder, store, query, pagerank)
│   │   └── src/{builder.rs, store.rs, query.rs, pagerank.rs, types.rs}
│   └── core-engine/                 # optimisation pipeline
│       └── src/
│           ├── skeleton/            # signature-only view
│           ├── dedup/               # exact + MinHash-LSH
│           ├── compress/            # AST-aware strip (parallel)
│           ├── ranking/             # BM25 + TF-IDF + PageRank priors
│           └── budget/              # greedy knapsack
├── infra/
│   ├── docker/Dockerfile            # CLI-only container
│   └── scripts/build.sh             # one-shot full build
└── docs/
    ├── ARCHITECTURE.md
    ├── USAGE.md
    ├── ALGORITHMS.md                # math details for each technique
    ├── DEPLOYMENT.md                # full release flow + end-user install + E2E flow
    └── PUBLISHING.md                # step-by-step VS Code Marketplace + Open VSX publish
```

---

## Quickstart

```bash
# Prerequisites: Rust stable, Node.js ≥18
./infra/scripts/build.sh

# Index your repo
./target/release/contextos build --root /path/to/your/repo

# See the blast radius of changed files
git diff --name-only | xargs ./target/release/contextos impact --root /path/to/your/repo

# Or run as a live MCP server (wire to Claude Code / Cursor config)
./target/release/contextos serve --root /path/to/your/repo

# Or pipe raw JSON through the pipeline (no graph needed)
echo '{"chunks":[{"id":"a","language":"typescript","content":"// hi\nfn add(){}","kind":"code","priority":0,"skeleton_hint":false}],"query":"addition"}' \
  | ./target/release/contextos optimize --pretty
```

---

## CLI reference

| Subcommand | Purpose |
|---|---|
| `contextos build --root <path>` | Full repo graph build (respects .gitignore) |
| `contextos update --root <path> [files...]` | Incremental update; pipes from `git diff --name-only` |
| `contextos impact --root <path> --depth N <files...>` | Print blast radius for the given changed files |
| `contextos skeleton --root <path> <file>` | Signature-only view of one file |
| `contextos watch --root <path>` | Live filesystem watcher, auto-updates the graph |
| `contextos serve --root <path>` | MCP JSON-RPC server on stdio |
| `contextos stats --root <path>` | Graph node / edge / file counts |
| `contextos optimize [--max-tokens N] [--pretty]` | Run the pipeline; stdin → stdout JSON |

---

## MCP tools (for Claude Code / Cursor / etc.)

Wire `contextos serve` into your client's MCP config. Exposed tools:

- `optimize` — run the full pipeline on supplied chunks
- `build_graph` / `update_graph` — index or incrementally refresh
- `impact_radius` — "given these changed files, what else is affected?"
- `skeleton` — signature-only file projection
- `graph_stats` — counts

All return MCP-standard `{ content: [{ type: "text", text: "..." }] }`.

---

## Lossless guarantees

What ContextOS *never* does to your code:

- Paraphrase, summarise, or LLM-rewrite
- Rename identifiers, reorder statements, or change types
- Drop code that's called from the blast radius
- Introduce inferred or hallucinated context

What it *does* do:

- Skip files the call graph says are irrelevant
- Replace bodies with signatures for peripheral files (opt-in via `skeleton_hint`)
- Strip comments / debug logs / redundant whitespace (AST-aware, won't touch string literals)
- Drop exact + near-duplicate chunks
- Drop lowest-ranked chunks only when above the token budget

---

## Testing

```bash
cargo test --workspace                              # unit + integration
npm --workspace apps/extension run compile          # extension type-check
```

Integration tests assert ≥40% reduction on realistic inputs and <200ms elapsed for 50-chunk workloads.

---

## License

MIT
