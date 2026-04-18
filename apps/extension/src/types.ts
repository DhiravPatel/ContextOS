// Wire types shared between the extension and the Rust CLI.
// Keep in sync with crates/core-engine/src/types.rs.

export type Language =
  | "rust"
  | "typescript"
  | "javascript"
  | "python"
  | "json"
  | "markdown"
  | "unknown";

export type ChunkKind = "code" | "comment" | "doc" | "diagnostic" | "selection";

export interface InputChunk {
  id: string;
  path?: string;
  language: Language;
  content: string;
  kind: ChunkKind;
  priority: number;
}

export interface OptimizationRequest {
  chunks: InputChunk[];
  query?: string;
}

export interface PipelineStats {
  dedup: { exact_removed: number; near_removed: number; kept: number };
  compress: {
    tokens_before: number;
    tokens_after: number;
    bytes_before: number;
    bytes_after: number;
    chunks_touched: number;
  };
  budget: { kept: number; dropped: number; final_tokens: number };
}

export interface OptimizationResult {
  chunks: InputChunk[];
  original_tokens: number;
  final_tokens: number;
  tokens_saved: number;
  reduction_pct: number;
  elapsed_ms: number;
  stats: PipelineStats;
}

export function languageFromVsCodeId(id: string): Language {
  switch (id) {
    case "rust":
      return "rust";
    case "typescript":
    case "typescriptreact":
      return "typescript";
    case "javascript":
    case "javascriptreact":
      return "javascript";
    case "python":
      return "python";
    case "json":
    case "jsonc":
      return "json";
    case "markdown":
      return "markdown";
    default:
      return "unknown";
  }
}
