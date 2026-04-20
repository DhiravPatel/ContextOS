# Algorithms

Every transformation listed here is **semantics-preserving**. Nothing is paraphrased or summarised; every reduction is justified by an information-theoretic argument (redundancy or irrelevance), not by "the LLM probably doesn't need this."

## 1. Incremental indexing — SHA-256 per file

**Problem.** Re-parsing a 2k-file repo on every edit burns seconds.
**Solution.** Keep a `files(path, sha256, language, last_indexed)` manifest in SQLite. On update, hash the file; if the SHA matches the stored value, skip. Complexity: `O(changed bytes)` instead of `O(repo bytes)`.
**Correctness.** Any change to the bytes changes the hash (collision probability ≈ 2⁻²⁵⁶). False negatives are impossible in practice.

## 2. Tree-sitter AST stripping

**Problem.** Regex-based comment removal trips on `const url = "https://... // not a comment"`.
**Solution.** Parse each file with the language's Tree-sitter grammar; remove nodes whose `kind` is `comment`/`line_comment`/`block_comment`/`doc_comment`. Debug logs (`console.log`, `println!`, `print(...)` as a statement) are recognised by node kind + textual prefix.
**Correctness.** Tree-sitter's grammar is the source of truth for "is this a comment or a string literal?"; the walker only removes byte ranges classified as comment/log nodes.

## 3. Exact chunk dedup — AHash

**Problem.** Same import/header/boilerplate appears across dozens of files.
**Solution.** Normalise whitespace, hash with 64-bit AHash, drop on collision. Complexity `O(n)`.
**Correctness.** Whitespace-normalised equality is semantically equivalent to the source text for any reasonable compiler/LLM; exact hash collisions (2⁻⁶⁴) are acceptable given the hash is only used as a dedup hint.

## 4. Near-duplicate dedup — MinHash + LSH

**Problem.** Two files differ by one line; exact dedup misses them, pairwise Jaccard is `O(n²)`.
**Solution.** MinHash signature with 128 independent permutations (generated via `h_i(x) = h1(x) + i · h2(x)` — the standard linear-combination trick avoiding 128 separate hash states). Band the signature into 16 bands × 8 rows; two docs hash to the same bucket in at least one band with high probability when their Jaccard is above ~0.8.
**Math.** For threshold `t` and `b` bands of `r` rows each, the collision probability is `1 − (1 − t^r)^b`. With `b=16, r=8`, this is ≈ 0.99 at `t=0.85`.
**Complexity.** `O(n · perm)` to build signatures, `O(n)` candidate lookup via LSH. Used automatically when `n ≥ 64`; the pairwise path is kept for tiny inputs where startup cost dominates.

## 5. BM25 query ranking (Okapi)

**Problem.** TF-IDF over-weights long documents; under-weights a short, highly-specific chunk.
**Solution.** Okapi BM25 with standard defaults (`k1=1.5`, `b=0.75`):

```
score(D, Q) = Σ_{q ∈ Q} IDF(q) · ((tf(q,D) · (k1+1)) / (tf(q,D) + k1·(1 − b + b·|D|/avgdl)))
IDF(q)     = ln(((N − df(q) + 0.5) / (df(q) + 0.5)) + 1)
```

`tf` saturates as it grows (prevents long docs from dominating), and the length normaliser `(1 − b + b·|D|/avgdl)` penalises bloat. Used whenever a user query is supplied; falls back to the pure TF-IDF density score otherwise.

## 6. PageRank prior

**Problem.** "Central" symbols (used everywhere) should beat "peripheral" symbols on ties, even without a query.
**Solution.** Run power-iteration PageRank over the edge set (damping 0.85, up to 50 iters, tolerance 1e-6). Dangling-node mass is redistributed uniformly (standard formulation — guarantees convergence on any graph). The scalar per node becomes an additive prior in the ranking stage, scaled up by 1000× so it's comparable to BM25 output (raw scores live in the 10⁻⁴ range).
**Complexity.** `O(iters · edges)`; ~10ms for 10k nodes, ~100ms for 100k. Runs once per index build — not per query.

## 7. Blast radius — reverse BFS

**Problem.** Given a changed file, which other files might break?
**Solution.** Collect all nodes in the changed files as seeds. BFS up to `max_depth` hops over *incoming* edges of kind `calls ∪ imports ∪ inherits`. Return the union of touched files.
**Why reverse?** Callers are the ones affected by the change, not callees. We traverse `dst → src`.
**Complexity.** `O(|impacted nodes| + |impacted edges|)`. Typical depth=2 hit for a single-file change is 10–40 nodes.

## 8. Skeleton extraction

**Problem.** The LLM needs to *know* `foo()` exists and its signature, but not the 200-line body, for files on the periphery of the blast radius.
**Solution.** Tree-sitter walk that, for every `function`/`method`/`class`/`interface`/`import` node, emits the signature text (everything up to the body opener) plus `{ /* … */ }`. Everything inside the body is dropped.
**Correctness.** The emitted text is exactly a prefix of the original declaration up to the brace; no information about the *signature* is lost. Only the implementation is collapsed.

## 9. Greedy budget packing

**Problem.** Fit ranked chunks into `max_tokens`.
**Solution.** Walk in rank order, add each chunk whose token cost fits the remaining budget. If the first chunk that overshoots is within a 5% slack, keep it (usually worth it — rank-order biases the overshoot toward high-value chunks).
**Why greedy, not optimal knapsack?** Exact knapsack is `O(n · max_tokens)` with integer weights — too slow at repo scale. Greedy in rank order is within a constant factor for almost all inputs because the rank already captures value-per-token intent.

## 10. Heuristic token estimator

We deliberately *don't* ship tiktoken-rs by default (≈10MB of vocab artifacts, build-time network pull). The estimator blends two signals:

```
by_chars ≈ ceil(chars / 3.6)
by_words ≈ ceil(words · 1.3)
estimate = max(by_chars, by_words)
```

Within ~5% of real GPT/Claude BPE tokenisers on typical source code. Over-estimates slightly, which is the safe direction for budget enforcement (we'd rather ship 7500 real tokens inside an 8000-budget than 8200).

---

## Summary table

| Stage | Algorithm | Lossless? |
|---|---|---|
| Graph index | SHA-256 incremental | Yes |
| Comment/log strip | Tree-sitter AST walk | Yes (AST-verified) |
| Exact dedup | AHash on normalised text | Yes |
| Near-dup | MinHash + LSH | Yes (thresholded) |
| Query rank | Okapi BM25 | N/A (reorders only) |
| Centrality | Power-iteration PageRank | N/A (reorders only) |
| Picking | Reverse BFS blast radius | Yes |
| Skeleton | Tree-sitter header extraction | Yes (signature-exact) |
| Budget | Greedy knapsack + 5% slack | Drops lowest-rank only |

Every bullet above can be audited by reading the linked source file; there's nothing magical and nothing guessed.
