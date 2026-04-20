// `contextos.showStats` — surfaces cumulative savings across the session.

import * as vscode from "vscode";
import { SessionStats } from "../sessionStats";

export function registerStatsCommand(
  context: vscode.ExtensionContext,
  stats: SessionStats,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("contextos.showStats", () => {
      const s = stats.summary();
      if (s.runs === 0) {
        vscode.window.showInformationMessage(
          "ContextOS: no optimizations yet. Run `ContextOS: Optimize Current Context` first.",
        );
        return;
      }
      vscode.window.showInformationMessage(
        `ContextOS session: ${s.runs} runs • ` +
          `${s.totalOriginalTokens} → ${s.totalFinalTokens} tokens • ` +
          `saved ${s.totalSavedTokens} (−${s.avgReductionPct.toFixed(1)}% avg, ` +
          `${s.avgElapsedMs.toFixed(0)}ms avg)`,
      );
    }),
  );
}
