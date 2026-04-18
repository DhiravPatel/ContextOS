# ContextOS — VS Code Extension

Local-first token reduction for AI coding assistants. Runs the Rust `contextos`
CLI against the current editor state and hands the optimized bundle back for
pasting into your LLM of choice.

## Commands

| Command | Key | Purpose |
|---|---|---|
| `ContextOS: Optimize Current Context` | `⌘/Ctrl+Alt+O` | Collect active file + selection + imports + visible editors; run the pipeline |
| `ContextOS: Optimize Selection` | — | Pipeline only over the current selection |
| `ContextOS: Show Session Stats` | — | Aggregate token savings so far |

## Settings

See `Preferences → Settings → Extensions → ContextOS`:

- `contextos.maxTokens` — token budget (default `8000`)
- `contextos.binaryPath` — override CLI location
- `contextos.includeImports` — pull local import targets into the context bundle
- `contextos.includeOpenEditors` — also include other visible editors
- `contextos.showReductionToast` — show a toast after each run

## Build

```bash
npm install
npm run compile
```

Produces `out/extension.js`. Package with `npm run package`.
