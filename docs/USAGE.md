# ContextOS — Usage

## As a VS Code extension

1. Build everything: `./infra/scripts/build.sh`
2. In VS Code, press `F5` from the `apps/extension` folder to launch an
   Extension Development Host.
3. Open any file, press `⌘⌥O` (macOS) or `Ctrl+Alt+O` (Linux/Windows).
4. Type your intended prompt — ContextOS uses it to rank relevance.
5. The optimized bundle opens in a new editor. Paste into your LLM.

## As a CLI

```bash
# Build
cargo build --release --bin contextos

# Single-shot: pipe a JSON request in, receive result on stdout
echo '{
  "chunks": [
    { "id": "a", "language": "rust",
      "content": "fn add(a: i32, b: i32) -> i32 { a + b } // adds",
      "kind": "code", "priority": 0 }
  ],
  "query": "addition helper"
}' | ./target/release/contextos optimize --pretty
```

### Pipeline options

```bash
contextos optimize --max-tokens 4000 --pretty < request.json > result.json
```

## Request schema

```ts
type OptimizationRequest = {
  chunks: Array<{
    id: string;
    path?: string;
    language: "rust" | "typescript" | "javascript" | "python"
            | "json" | "markdown" | "unknown";
    content: string;
    kind: "code" | "comment" | "doc" | "diagnostic" | "selection";
    priority: number;  // arbitrary bump, larger = likelier to survive budget
  }>;
  query?: string;      // user intent; drives ranking
};
```

## Response schema

```ts
type OptimizationResult = {
  chunks: InputChunk[];      // reordered, compressed, trimmed
  original_tokens: number;
  final_tokens: number;
  tokens_saved: number;
  reduction_pct: number;
  elapsed_ms: number;
  stats: { dedup: ..., compress: ..., budget: ... };
};
```
