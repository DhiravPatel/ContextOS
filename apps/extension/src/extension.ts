// ContextOS VS Code extension entry point.
//
// Activates on startup (cheap — just wires commands). All heavy lifting
// happens inside the Rust CLI, spawned on demand.

import * as vscode from "vscode";
import { registerOptimizeCommands } from "./commands/optimize";
import { registerStatsCommand } from "./commands/stats";
import { OptimizerClient } from "./optimizerClient";
import { SessionStats } from "./sessionStats";

let output: vscode.OutputChannel | undefined;

export function activate(context: vscode.ExtensionContext): void {
  output = vscode.window.createOutputChannel("ContextOS");
  context.subscriptions.push(output);
  output.appendLine("ContextOS activated.");

  const client = new OptimizerClient(context);
  const stats = new SessionStats();

  registerOptimizeCommands(context, client, stats, output);
  registerStatsCommand(context, stats);

  // Status bar item — one-click access to the optimizer.
  const statusItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  );
  statusItem.text = "$(rocket) ContextOS";
  statusItem.tooltip = "Optimize context for AI (ContextOS)";
  statusItem.command = "contextos.optimize";
  statusItem.show();
  context.subscriptions.push(statusItem);
}

export function deactivate(): void {
  output?.appendLine("ContextOS deactivated.");
}
