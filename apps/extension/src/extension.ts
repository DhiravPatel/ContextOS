// ContextOS VS Code extension entry point.
//
// Activation is the *only* touchpoint for most users:
//   • AutoInstaller asks once for consent, then writes Claude Code MCP config,
//     builds the graph, and starts `contextos watch` in the background.
//   • After that, Claude Code sees the ContextOS MCP server and calls it on
//     every AI request — token reduction happens with zero extra clicks.
//
// The old commands (optimize, showStats) remain for the paste-into-browser
// workflow and for debugging / visibility.

import * as vscode from "vscode";
import { AutoInstaller } from "./autoInstaller";
import { registerOptimizeCommands } from "./commands/optimize";
import { registerStatsCommand } from "./commands/stats";
import { OptimizerClient } from "./optimizerClient";
import { SessionStats } from "./sessionStats";

let output: vscode.OutputChannel | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel("ContextOS");
  context.subscriptions.push(output);
  output.appendLine("ContextOS activated.");

  const installer = new AutoInstaller(context, output);
  context.subscriptions.push({ dispose: () => installer.dispose() });

  const client = new OptimizerClient(context);
  const stats = new SessionStats();

  // Legacy / debugging commands (manual paste flow, stats view).
  registerOptimizeCommands(context, client, stats, output);
  registerStatsCommand(context, stats);

  // Zero-touch install commands.
  context.subscriptions.push(
    vscode.commands.registerCommand("contextos.reconfigure", () =>
      installer.runManually(),
    ),
    vscode.commands.registerCommand("contextos.disableForProject", () =>
      installer.runUninstall(),
    ),
  );

  // Status bar.
  const statusItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  );
  statusItem.text = "$(rocket) ContextOS";
  statusItem.tooltip = "ContextOS is active. Click to optimize the current context.";
  statusItem.command = "contextos.optimize";
  statusItem.show();
  context.subscriptions.push(statusItem);

  // Fire and forget — we don't want activation to hang on install.
  installer.runOnActivate().catch((err) => {
    output?.appendLine(`auto-install error: ${(err as Error).message}`);
  });
}

export function deactivate(): void {
  output?.appendLine("ContextOS deactivated.");
}
