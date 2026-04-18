// `contextos.optimize` and `contextos.optimizeSelection` command handlers.

import * as vscode from "vscode";
import { buildSelectionRequest, collectContext } from "../contextCollector";
import { OptimizerClient } from "../optimizerClient";
import { SessionStats } from "../sessionStats";
import { OptimizationResult } from "../types";

export function registerOptimizeCommands(
  context: vscode.ExtensionContext,
  client: OptimizerClient,
  stats: SessionStats,
  output: vscode.OutputChannel,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("contextos.optimize", async () => {
      await runOptimize({ selectionOnly: false }, client, stats, output);
    }),
    vscode.commands.registerCommand("contextos.optimizeSelection", async () => {
      await runOptimize({ selectionOnly: true }, client, stats, output);
    }),
  );
}

async function runOptimize(
  opts: { selectionOnly: boolean },
  client: OptimizerClient,
  stats: SessionStats,
  output: vscode.OutputChannel,
): Promise<void> {
  const config = vscode.workspace.getConfiguration("contextos");
  const showToast = config.get<boolean>("showReductionToast", true);
  const includeImports = config.get<boolean>("includeImports", true);
  const includeOpen = config.get<boolean>("includeOpenEditors", true);

  const query = await vscode.window.showInputBox({
    prompt: "What do you want to ask the AI? (used for relevance ranking)",
    placeHolder: "e.g. 'add pagination to the user list'",
    ignoreFocusOut: false,
  });
  if (query === undefined) return; // user cancelled

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "ContextOS: optimizing context…",
      cancellable: false,
    },
    async () => {
      try {
        const editor = vscode.window.activeTextEditor;
        const request =
          opts.selectionOnly && editor
            ? buildSelectionRequest(editor, query || undefined)
            : await collectContext(query || undefined, {
                includeImports,
                includeOpenEditors: includeOpen,
              });

        if (request.chunks.length === 0) {
          vscode.window.showWarningMessage(
            "ContextOS: no context to optimize (open a file first).",
          );
          return;
        }

        const result = await client.optimize(request);
        stats.record(result);
        await deliver(result, output);
        if (showToast) announceReduction(result);
      } catch (err) {
        const msg = (err as Error).message ?? String(err);
        vscode.window.showErrorMessage(`ContextOS failed: ${msg}`);
        output.appendLine(`[error] ${msg}`);
      }
    },
  );
}

async function deliver(
  result: OptimizationResult,
  output: vscode.OutputChannel,
): Promise<void> {
  const assembled = result.chunks
    .map((c) => {
      const header = c.path ? `// --- ${c.path} (${c.kind}) ---` : `// --- ${c.id} ---`;
      return `${header}\n${c.content}`;
    })
    .join("\n\n");

  output.appendLine(
    `[run] original=${result.original_tokens} final=${result.final_tokens} ` +
      `saved=${result.tokens_saved} (${result.reduction_pct.toFixed(1)}%) ` +
      `elapsed=${result.elapsed_ms.toFixed(1)}ms`,
  );

  const doc = await vscode.workspace.openTextDocument({
    content: assembled,
    language: "markdown",
  });
  await vscode.window.showTextDocument(doc, { preview: true });
}

function announceReduction(result: OptimizationResult): void {
  const pct = result.reduction_pct.toFixed(1);
  vscode.window.showInformationMessage(
    `ContextOS: ${result.original_tokens} → ${result.final_tokens} tokens ` +
      `(saved ${result.tokens_saved}, −${pct}% in ${result.elapsed_ms.toFixed(0)}ms)`,
  );
}
