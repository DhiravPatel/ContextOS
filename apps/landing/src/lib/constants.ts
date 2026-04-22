/**
 * Single source of truth for content that the marketing page references.
 * Edit here; components pick it up automatically.
 */

export const SITE = {
  name: "ContextOS",
  tagline: "Stop burning tokens. Start vibe-coding smarter.",
  description:
    "Local-first token reduction for AI coding assistants. Cut context tokens ≥50% without changing LLM output quality.",
  url: "https://contextos.dev",
  ogImage: "/og-image.png",
  twitter: "@contextos_dev",
  github: "https://github.com/DhiravPatel/ContextOS",
  marketplace:
    "https://marketplace.visualstudio.com/items?itemName=DhiravPatel.contextos-vscode",
  openVsx: "https://open-vsx.org/extension/DhiravPatel/contextos-vscode",
} as const;

export const NAV_LINKS = [
  { label: "How it works", href: "#how" },
  { label: "Features", href: "#features" },
  { label: "Install", href: "#install" },
  { label: "Docs", href: "https://github.com/DhiravPatel/ContextOS/blob/main/README.md" },
] as const;

export const PIPELINE_STAGES = [
  {
    index: 1,
    title: "Skeletonise",
    subtitle: "Signature-only views",
    body:
      "Tree-sitter walks each peripheral file and replaces function/class bodies with their signatures. Claude still knows the symbol exists and its shape — it just doesn't read the implementation.",
    savings: "−20% to −40%",
  },
  {
    index: 2,
    title: "Dedup",
    subtitle: "Exact hash + MinHash-LSH",
    body:
      "AHash kills byte-identical duplicates in O(n). Above 64 chunks, MinHash with 128 permutations banded into 16×8 LSH buckets catches near-duplicates at Jaccard ≥ 0.85 — all in O(n).",
    savings: "−5% to −25%",
  },
  {
    index: 3,
    title: "Compress",
    subtitle: "AST-aware stripping",
    body:
      "Remove comments, debug logs, redundant whitespace. Syntax-aware via Tree-sitter — won't touch `// not a comment` inside a string literal. Runs in parallel via rayon.",
    savings: "−20% to −40%",
  },
  {
    index: 4,
    title: "Rank",
    subtitle: "BM25 + PageRank",
    body:
      "Okapi BM25 (k1=1.5, b=0.75) scores chunks against the user query. Power-iteration PageRank over the call graph adds a centrality prior so structurally important symbols float up.",
    savings: "reorders",
  },
  {
    index: 5,
    title: "Budget",
    subtitle: "Greedy knapsack",
    body:
      "Walk top-down in rank order, keep each chunk whose token cost fits the remaining budget. 5% overshoot slack for the last-fitting chunk. Guarantees you never blow the context window.",
    savings: "hard cap",
  },
] as const;

export const FEATURES = [
  {
    title: "Code graph",
    tagline: "Know exactly what matters",
    body:
      "Tree-sitter parses your repo into a symbol graph (functions, classes, calls, imports). Reverse BFS computes the blast radius of every change — the LLM reads 5 files instead of 500.",
    bullets: ["SQLite, on disk, in-repo", "Incremental SHA-256 updates", "<200ms blast-radius queries"],
    icon: "graph",
  },
  {
    title: "AST compression",
    tagline: "Lossless by construction",
    body:
      "Every transformation is verified at the syntax tree, never at the byte level. No string-literal collisions, no identifier renames, no summarisation — just the stuff the LLM truly doesn't need.",
    bullets: ["Rust-speed pipeline", "Parallel via rayon", "<50ms on typical inputs"],
    icon: "compress",
  },
  {
    title: "MCP server",
    tagline: "Your AI calls us automatically",
    body:
      "Claude Code picks up ContextOS through the Model Context Protocol. Every prompt transparently routes through the optimization pipeline before it reaches the LLM. Zero buttons, zero friction.",
    bullets: ["JSON-RPC 2.0 over stdio", "6 tools registered", "Zero-touch install"],
    icon: "mcp",
  },
] as const;

export const HERO_METRICS = {
  originalTokens: 38_400,
  optimisedTokens: 3_920,
  elapsedMs: 47,
  filesOriginally: 312,
  filesAfterGraph: 5,
} as const;

export const LEGAL = {
  copyright: `© ${new Date().getFullYear()} ContextOS. MIT-licensed.`,
} as const;
