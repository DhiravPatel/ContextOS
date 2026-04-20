// Zero-touch installer for the ContextOS extension.
//
// On first activation (per workspace):
//   1. Ask for one-time consent to write .mcp.json + .claude/settings.local.json
//      so Claude Code picks up the ContextOS MCP server.
//   2. Run `contextos install --root <workspace>` — writes Claude Code config.
//   3. Run `contextos build --root <workspace>` — builds the graph.
//   4. Spawn `contextos watch --root <workspace>` as a long-running child
//      so the graph stays fresh as the user edits.
//
// Subsequent activations skip the consent step and just refresh/start watch.
//
// A workspace-scoped flag in `workspaceState` marks "already installed here."
// A global-state flag in `globalState` records the user's one-time consent
// across workspaces; once they've said yes, we don't nag again.

import { spawn, ChildProcess, execFile } from "child_process";
import * as fs from "fs";
import * as path from "path";
import { promisify } from "util";
import * as vscode from "vscode";

const execFileP = promisify(execFile);

const CLI_NAME = process.platform === "win32" ? "contextos.exe" : "contextos";
const WS_INSTALLED_KEY = "contextos.workspaceInstalled";
const GLOBAL_CONSENT_KEY = "contextos.userConsentedToAutoInstall";

export interface AutoInstallerOptions {
  /** If true, skip the consent dialog (e.g. user invoked the command directly). */
  force?: boolean;
}

export class AutoInstaller {
  private watchProc: ChildProcess | null = null;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly output: vscode.OutputChannel,
  ) {}

  get isWatching(): boolean {
    return this.watchProc !== null && this.watchProc.exitCode === null;
  }

  /** Fire during activation. Short-circuits if we've already set this workspace up. */
  async runOnActivate(): Promise<void> {
    const workspace = pickWorkspaceRoot();
    if (!workspace) {
      this.output.appendLine("auto-install: no workspace folder — nothing to do.");
      return;
    }

    const alreadyInstalled = this.context.workspaceState.get<boolean>(
      WS_INSTALLED_KEY,
      false,
    );

    if (!alreadyInstalled) {
      const consented = await this.ensureConsent();
      if (!consented) {
        this.output.appendLine("auto-install: user declined — will retry on next activation.");
        return;
      }
      await this.doFirstRun(workspace);
      await this.context.workspaceState.update(WS_INSTALLED_KEY, true);
    } else {
      this.output.appendLine("auto-install: already configured for this workspace.");
    }

    // Always (re)start watch; it's cheap and keeps the graph fresh.
    await this.startWatch(workspace);
  }

  /** Force a full re-run — exposed via the "ContextOS: Reconfigure" command. */
  async runManually(): Promise<void> {
    const workspace = pickWorkspaceRoot();
    if (!workspace) {
      vscode.window.showWarningMessage("ContextOS: open a folder first.");
      return;
    }
    await this.doFirstRun(workspace);
    await this.context.workspaceState.update(WS_INSTALLED_KEY, true);
    await this.restartWatch(workspace);
  }

  /** Clean teardown — stops watch and removes MCP config. Used on extension uninstall-by-user. */
  async runUninstall(): Promise<void> {
    const workspace = pickWorkspaceRoot();
    this.stopWatch();
    if (!workspace) return;
    const binary = await this.resolveBinary();
    try {
      await execFileP(binary, ["uninstall", "--root", workspace], { timeout: 10_000 });
      await this.context.workspaceState.update(WS_INSTALLED_KEY, false);
      vscode.window.showInformationMessage(
        "ContextOS: removed Claude Code configuration for this workspace.",
      );
    } catch (err) {
      this.output.appendLine(`uninstall failed: ${(err as Error).message}`);
    }
  }

  dispose(): void {
    this.stopWatch();
  }

  // ---- internals ------------------------------------------------------

  private async ensureConsent(): Promise<boolean> {
    const priorConsent = this.context.globalState.get<boolean>(GLOBAL_CONSENT_KEY, false);
    if (priorConsent) return true;

    const choice = await vscode.window.showInformationMessage(
      "ContextOS will auto-configure Claude Code for this project (writes .mcp.json) and keep a " +
        "local code graph in .contextos/ so your AI uses fewer tokens. Continue?",
      { modal: false },
      "Enable",
      "Not now",
      "Never",
    );
    if (choice === "Enable") {
      await this.context.globalState.update(GLOBAL_CONSENT_KEY, true);
      return true;
    }
    if (choice === "Never") {
      await this.context.globalState.update(GLOBAL_CONSENT_KEY, false);
      // Persistent no — don't ask again this workspace. Mark installed so we
      // skip the prompt; user can run "ContextOS: Reconfigure" later.
      await this.context.workspaceState.update(WS_INSTALLED_KEY, true);
    }
    return false;
  }

  private async doFirstRun(workspace: string): Promise<void> {
    const binary = await this.resolveBinary();

    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "ContextOS: setting up…",
        cancellable: false,
      },
      async (progress) => {
        progress.report({ message: "wiring Claude Code MCP…" });
        try {
          await execFileP(binary, ["install", "--root", workspace], { timeout: 15_000 });
        } catch (err) {
          this.output.appendLine(`install failed: ${(err as Error).message}`);
          vscode.window.showErrorMessage(
            `ContextOS install failed: ${(err as Error).message}`,
          );
          return;
        }

        progress.report({ message: "indexing your project…" });
        try {
          const { stdout } = await execFileP(
            binary,
            ["build", "--root", workspace],
            { timeout: 5 * 60_000, maxBuffer: 10 * 1024 * 1024 },
          );
          this.output.appendLine(`build: ${stdout.trim()}`);
        } catch (err) {
          this.output.appendLine(`build failed: ${(err as Error).message}`);
        }
      },
    );

    vscode.window.showInformationMessage(
      "ContextOS: wired into Claude Code for this project. Reload Claude Code to activate.",
    );
  }

  private async startWatch(workspace: string): Promise<void> {
    if (this.isWatching) return;
    const binary = await this.resolveBinary();

    this.watchProc = spawn(binary, ["watch", "--root", workspace], {
      stdio: ["ignore", "pipe", "pipe"],
      detached: false,
    });
    this.output.appendLine(
      `watch: pid=${this.watchProc.pid} cmd="${binary} watch --root ${workspace}"`,
    );
    this.watchProc.stdout?.on("data", (b) =>
      this.output.append(`watch> ${b.toString("utf8")}`),
    );
    this.watchProc.stderr?.on("data", (b) =>
      this.output.append(`watch> ${b.toString("utf8")}`),
    );
    this.watchProc.on("exit", (code, signal) => {
      this.output.appendLine(`watch: exited (code=${code} signal=${signal})`);
      this.watchProc = null;
    });
  }

  private async restartWatch(workspace: string): Promise<void> {
    this.stopWatch();
    await this.startWatch(workspace);
  }

  private stopWatch(): void {
    if (this.watchProc && this.watchProc.exitCode === null) {
      try {
        this.watchProc.kill();
      } catch {
        /* process might already be dead */
      }
    }
    this.watchProc = null;
  }

  private async resolveBinary(): Promise<string> {
    const configured = vscode.workspace
      .getConfiguration("contextos")
      .get<string>("binaryPath", "")
      .trim();
    if (configured && fs.existsSync(configured)) return configured;

    const bundled = path.join(
      this.context.extensionPath,
      "bin",
      process.platform,
      CLI_NAME,
    );
    if (fs.existsSync(bundled)) return bundled;

    return CLI_NAME; // fall through to PATH
  }
}

function pickWorkspaceRoot(): string | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) return undefined;
  // Multi-root: use the first folder — most projects are single-root, and
  // running multiple graphs simultaneously would compete for .contextos/.
  return folders[0].uri.fsPath;
}
