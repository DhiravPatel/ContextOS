import type { Metadata } from "next";
import { DocsPage } from "@/components/docs/DocsPage";

export const metadata: Metadata = { title: "Algorithms" };

export default function Algorithms() {
  return (
    <DocsPage
      kicker="Deep dive"
      title="Algorithms"
      lede="Every transformation is justified by an information-theoretic argument, not by guesswork. This page has the formulas."
      pathname="/docs/algorithms"
    >
      <h2>SHA-256 incremental indexing</h2>
      <p>
        The graph store keeps a <code>files(path, sha256)</code> manifest. On
        update, we hash the file; if the SHA matches the stored value, the
        reparse is skipped.
      </p>
      <p>
        <strong>Complexity:</strong> <code>O(changed bytes)</code> per update
        instead of <code>O(repo bytes)</code>.
      </p>
      <p>
        <strong>Correctness:</strong> SHA-256 collision probability is ≈ 2⁻²⁵⁶
        — a false negative is not a risk in practice.
      </p>

      <h2>Tree-sitter AST stripping</h2>
      <p>
        Each file is parsed with the language's Tree-sitter grammar. Nodes
        classified as <code>comment</code>, <code>line_comment</code>,{" "}
        <code>block_comment</code>, or <code>doc_comment</code> are removed.
        Debug logs (<code>console.log</code>, <code>println!</code>,{" "}
        <code>print(...)</code> as a statement) are matched by node kind plus
        textual prefix.
      </p>
      <p>
        Because the classification is the parser's — not a regex — string
        literals that happen to contain <code>{"//"}</code> are never touched.
      </p>

      <h2>Exact dedup (AHash)</h2>
      <p>
        Chunk content is whitespace-normalised and hashed with AHash (64-bit).
        Hash collisions drop the chunk.
      </p>
      <p>
        <strong>Complexity:</strong> <code>O(n)</code>.
      </p>

      <h2>Near-duplicate dedup (MinHash + LSH)</h2>
      <p>
        Pairwise Jaccard is <code>O(n²)</code>. We replace it with banded
        MinHash when <code>n ≥ 64</code>:
      </p>
      <ul>
        <li>
          128 permutations generated via{" "}
          <code>h_i(x) = h_1(x) + i · h_2(x)</code> — the standard
          linear-combination trick that avoids keeping 128 separate hash states.
        </li>
        <li>Signatures banded into 16 bands × 8 rows.</li>
        <li>
          Two docs collide in at least one band iff they hash to the same band
          slice.
        </li>
      </ul>
      <p>
        <strong>Collision math:</strong> at Jaccard <em>t</em> with <em>b</em>{" "}
        bands of <em>r</em> rows each,
      </p>
      <pre>
        <code>{`P(collision | Jaccard = t) = 1 − (1 − t^r)^b`}</code>
      </pre>
      <p>
        With <code>b=16, r=8</code>, this is ≈ 0.99 at <code>t = 0.85</code> and
        ≈ 0.24 at <code>t = 0.60</code>. We tune for near-duplicates (≥ 0.85)
        and verify every candidate with an exact Jaccard comparison before
        dropping.
      </p>

      <h2>Okapi BM25 ranking</h2>
      <p>
        When the user supplies a query, we switch from TF-IDF density to Okapi
        BM25 (<code>k1 = 1.5</code>, <code>b = 0.75</code>):
      </p>
      <pre>
        <code>{`score(D, Q) = Σ IDF(q) · ((tf · (k1+1)) / (tf + k1·(1 − b + b·|D|/avgdl)))
IDF(q)     = ln(((N − df(q) + 0.5) / (df(q) + 0.5)) + 1)`}</code>
      </pre>
      <p>
        <code>tf</code> saturates so long docs don't dominate by sheer volume;
        the length normaliser <code>(1 − b + b·|D|/avgdl)</code> penalises
        bloat.
      </p>

      <h2>PageRank prior</h2>
      <p>
        Power-iteration PageRank runs over the full edge set (damping 0.85, up
        to 50 iterations, tolerance 1e-6). Dangling-node mass is redistributed
        uniformly — the standard formulation, which guarantees convergence on
        any graph.
      </p>
      <p>
        The scalar per node becomes an additive prior in the ranker, scaled up
        1000× so its influence is comparable to BM25 output (raw PageRank lives
        in the 10⁻⁴ range).
      </p>
      <p>
        <strong>Cost:</strong> runs once per index build — about 10 ms for 10 k
        nodes, 100 ms for 100 k.
      </p>

      <h2>Blast radius</h2>
      <p>
        Given changed files, collect every node in them as a seed, then BFS up
        to <code>max_depth</code> hops over <em>incoming</em> edges of kind{" "}
        <code>calls ∪ imports ∪ inherits</code>. Union the touched files.
      </p>
      <p>
        <strong>Why reverse?</strong> Callers are the ones affected by a change;
        callees were already in the graph anyway. We traverse{" "}
        <code>dst → src</code>.
      </p>

      <h2>Skeleton extraction</h2>
      <p>
        Tree-sitter walks the file; for each{" "}
        <code>function / method / class / interface / import</code> node we
        emit the byte range from the start of the declaration to the body
        opener, then drop the body and append <code>&#123; /* … */ &#125;</code>.
        The emitted text is a prefix of the original source — no information
        about the signature is lost.
      </p>

      <h2>Greedy budget packing</h2>
      <p>
        Walk ranked chunks top-down, add each whose token cost fits the
        remaining budget. The first chunk that overshoots is kept if within a
        5% slack (almost always worth it — rank order correlates with value per
        token).
      </p>
      <p>
        <strong>Why not exact knapsack?</strong> Exact knapsack is{" "}
        <code>O(n · max_tokens)</code> with integer weights — too slow at repo
        scale. Greedy is within a constant factor for almost all inputs because
        the ranker already captures value-per-token.
      </p>

      <h2>Heuristic token estimator</h2>
      <p>
        The default build ships a calibrated estimator rather than a vocab-heavy
        BPE tokeniser:
      </p>
      <pre>
        <code>{`by_chars ≈ ceil(chars / 3.6)
by_words ≈ ceil(words · 1.3)
estimate = max(by_chars, by_words)`}</code>
      </pre>
      <p>
        Within ~5% of real GPT / Claude BPE tokenisers on typical source code.
        Slight over-estimate, which is the safe direction for budget
        enforcement.
      </p>
    </DocsPage>
  );
}
