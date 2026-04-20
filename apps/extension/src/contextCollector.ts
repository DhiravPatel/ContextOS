// Builds an OptimizationRequest from the current VS Code state.
//
// The collector is intentionally conservative: grab the active file, user's
// selection (if any), imports referenced in the active file, and — when the
// setting is on — the other visible editors. Everything is labelled so the
// Rust side can rank appropriately.

import * as path from "path";
import * as vscode from "vscode";
import {
  ChunkKind,
  InputChunk,
  Language,
  OptimizationRequest,
  languageFromVsCodeId,
} from "./types";

const IMPORT_PATTERNS: Record<Language, RegExp | null> = {
  rust: /^\s*use\s+([\w:{}*,\s]+);/gm,
  typescript: /^\s*import\s+[^;]+from\s+['"]([^'"]+)['"]/gm,
  javascript: /^\s*import\s+[^;]+from\s+['"]([^'"]+)['"]/gm,
  python: /^\s*(?:from\s+([\w.]+)\s+import|import\s+([\w.]+))/gm,
  json: null,
  markdown: null,
  unknown: null,
};

export interface CollectorOptions {
  includeImports: boolean;
  includeOpenEditors: boolean;
}

export async function collectContext(
  query: string | undefined,
  opts: CollectorOptions,
): Promise<OptimizationRequest> {
  const chunks: InputChunk[] = [];
  const editor = vscode.window.activeTextEditor;

  if (editor) {
    const doc = editor.document;
    const lang = languageFromVsCodeId(doc.languageId);
    const relPath = vscode.workspace.asRelativePath(doc.uri, false);

    chunks.push({
      id: `active:${relPath}`,
      path: relPath,
      language: lang,
      content: doc.getText(),
      kind: "code",
      priority: 5,
    });

    if (!editor.selection.isEmpty) {
      chunks.push({
        id: `selection:${relPath}`,
        path: relPath,
        language: lang,
        content: doc.getText(editor.selection),
        kind: "selection",
        priority: 10,
      });
    }

    if (opts.includeImports) {
      const importChunks = await collectImports(doc, lang);
      chunks.push(...importChunks);
    }
  }

  if (opts.includeOpenEditors) {
    const activePath = editor?.document.uri.toString();
    for (const other of vscode.window.visibleTextEditors) {
      if (other.document.uri.toString() === activePath) continue;
      const lang = languageFromVsCodeId(other.document.languageId);
      const relPath = vscode.workspace.asRelativePath(other.document.uri, false);
      chunks.push({
        id: `visible:${relPath}`,
        path: relPath,
        language: lang,
        content: other.document.getText(),
        kind: "code",
        priority: 1,
      });
    }
  }

  return { chunks, query };
}

async function collectImports(
  doc: vscode.TextDocument,
  lang: Language,
): Promise<InputChunk[]> {
  const pattern = IMPORT_PATTERNS[lang];
  if (!pattern) return [];

  const text = doc.getText();
  const modules = new Set<string>();
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(text)) !== null) {
    const mod = (match[1] ?? match[2] ?? "").trim();
    if (mod) modules.add(mod);
  }
  if (modules.size === 0) return [];

  const chunks: InputChunk[] = [];
  const baseDir = path.dirname(doc.uri.fsPath);

  for (const mod of modules) {
    // Only try to resolve local, relative imports. External packages are
    // skipped — loading node_modules would defeat the purpose.
    if (!mod.startsWith(".") && !mod.startsWith("/")) continue;
    const resolved = await resolveImport(baseDir, mod, lang);
    if (!resolved) continue;
    try {
      const imported = await vscode.workspace.openTextDocument(resolved);
      const relPath = vscode.workspace.asRelativePath(imported.uri, false);
      chunks.push({
        id: `import:${relPath}`,
        path: relPath,
        language: lang,
        content: imported.getText(),
        kind: "code",
        priority: 2,
      });
    } catch {
      // Unreadable or missing — skip silently; the engine can still run.
    }
  }
  return chunks;
}

async function resolveImport(
  baseDir: string,
  spec: string,
  lang: Language,
): Promise<vscode.Uri | null> {
  const exts = extensionsFor(lang);
  const candidates: string[] = [];
  const joined = path.resolve(baseDir, spec);
  for (const ext of exts) {
    candidates.push(joined + ext);
    candidates.push(path.join(joined, `index${ext}`));
  }
  for (const candidate of candidates) {
    try {
      const stat = await vscode.workspace.fs.stat(vscode.Uri.file(candidate));
      if (stat.type === vscode.FileType.File) return vscode.Uri.file(candidate);
    } catch {
      /* ignore missing */
    }
  }
  return null;
}

function extensionsFor(lang: Language): string[] {
  switch (lang) {
    case "typescript":
      return [".ts", ".tsx", ".js", ".jsx"];
    case "javascript":
      return [".js", ".jsx", ".ts", ".tsx"];
    case "python":
      return [".py"];
    case "rust":
      return [".rs"];
    default:
      return [];
  }
}

// Exposed for the "optimize selection" command — bypasses the broader collect.
export function buildSelectionRequest(
  editor: vscode.TextEditor,
  query: string | undefined,
): OptimizationRequest {
  const doc = editor.document;
  const lang = languageFromVsCodeId(doc.languageId);
  const relPath = vscode.workspace.asRelativePath(doc.uri, false);
  const kind: ChunkKind = editor.selection.isEmpty ? "code" : "selection";
  const content = editor.selection.isEmpty
    ? doc.getText()
    : doc.getText(editor.selection);
  return {
    chunks: [
      {
        id: `selection:${relPath}`,
        path: relPath,
        language: lang,
        content,
        kind,
        priority: 10,
      },
    ],
    query,
  };
}
