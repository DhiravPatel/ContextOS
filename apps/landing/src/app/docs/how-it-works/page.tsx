import type { Metadata } from "next";
import { DocsPage } from "@/components/docs/DocsPage";
import { PIPELINE_STAGES } from "@/lib/constants";

export const metadata: Metadata = { title: "How it works" };

export default function HowItWorks() {
  return (
    <DocsPage
      kicker="Deep dive"
      title="How ContextOS reduces tokens"
      lede="Two orthogonal strategies run together: pick only the relevant files from a code graph, then compress what you do send. Both are lossless."
      pathname="/docs/how-it-works"
    >
      <h2>The pipeline in one picture</h2>
      <pre>
        <code>{`repo on disk
      ▼
Tree-sitter parser   →   SQLite graph (.contextos/graph.db)
      ▼
Claude Code asks "help with auth.ts"
      ▼
impact_radius(files, depth) → 5 files (not 312)
      ▼
┌──────────────────────────────────────┐
│  1. Skeletonise   (signatures only)  │
│  2. Dedup         (hash + MinHash)   │
│  3. Compress      (AST strip)        │
│  4. Rank          (BM25 + PageRank)  │
│  5. Budget        (greedy knapsack)  │
└──────────────────────────────────────┘
      ▼
3,920 tokens (was 38,400) → LLM`}</code>
      </pre>

      <h2>Layer 1: graph-based picking</h2>
      <p>
        ContextOS builds a directed property graph of your code. Nodes are
        symbols (files, functions, classes, imports). Edges are relationships
        (contains, calls, imports, inherits, tests).
      </p>
      <p>
        When Claude Code asks about a change to <code>auth.ts</code>, we run a
        reverse BFS over <em>incoming</em> edges up to a configurable depth. The
        result is the "blast radius" — every symbol that could be affected by
        the change. Everything else is noise.
      </p>
      <p>
        A 300-file repo typically collapses to 3–10 truly relevant files. That
        alone is a 30–100× reduction in what the LLM would have read.
      </p>

      <h2>Layer 2: the compression pipeline</h2>
      <p>
        Once a slice is picked, chunks flow through five stages in order. Each
        one is verified at the syntax tree, not the byte level — no string
        literals get clobbered, no identifiers get renamed, no code is
        paraphrased. Only redundancy and irrelevance are removed.
      </p>

      <ol>
        {PIPELINE_STAGES.map((stage) => (
          <li key={stage.index} className="!my-6">
            <div className="flex items-baseline gap-3">
              <span className="font-mono text-xs text-fg-subtle">
                0{stage.index}
              </span>
              <strong>{stage.title}</strong>
              <span className="rounded-full border border-line px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-fg-muted">
                {stage.savings}
              </span>
            </div>
            <p className="!mt-2 text-sm text-fg-muted">{stage.body}</p>
          </li>
        ))}
      </ol>

      <h2>Why the output stays identical</h2>
      <p>
        Three invariants, enforced by construction:
      </p>
      <ul>
        <li>
          <strong>Nothing is paraphrased.</strong> We never summarise, rewrite,
          or rename. Every byte that survives was in the original source.
        </li>
        <li>
          <strong>Signatures are preserved verbatim.</strong> Skeletonisation
          emits the exact prefix of a declaration up to its body. Function
          names, argument types, and return types are byte-identical to source.
        </li>
        <li>
          <strong>Dedup runs on whitespace-normalised hashes.</strong> If two
          chunks collapse, it's because they were already semantically
          equivalent.
        </li>
      </ul>

      <p>
        For the algorithmic details with formulas, complexity proofs, and
        collision probabilities, see{" "}
        <a href="/docs/algorithms">Algorithms</a>.
      </p>
    </DocsPage>
  );
}
