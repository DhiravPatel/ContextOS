// Talks to the `contextos` CLI. Spawns it as a short-lived child process,
// pipes the request as JSON on stdin and reads the result from stdout.
//
// We intentionally avoid a long-running daemon for v1: one process per
// invocation is still <50ms in release mode and keeps the trust boundary
// simple.

import { spawn } from "child_process";
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { OptimizationRequest, OptimizationResult } from "./types";

const CLI_NAME = process.platform === "win32" ? "contextos.exe" : "contextos";

export class OptimizerClient {
  constructor(private readonly context: vscode.ExtensionContext) {}

  async optimize(request: OptimizationRequest): Promise<OptimizationResult> {
    const binary = await this.resolveBinary();
    const maxTokens = vscode.workspace
      .getConfiguration("contextos")
      .get<number>("maxTokens", 8000);

    const args = ["optimize", "--max-tokens", String(maxTokens)];
    const payload = JSON.stringify(request);

    return new Promise((resolve, reject) => {
      const child = spawn(binary, args, { stdio: ["pipe", "pipe", "pipe"] });
      const stdoutChunks: Buffer[] = [];
      const stderrChunks: Buffer[] = [];

      child.stdout.on("data", (b) => stdoutChunks.push(b));
      child.stderr.on("data", (b) => stderrChunks.push(b));
      child.on("error", reject);
      child.on("close", (code) => {
        if (code !== 0) {
          const err = Buffer.concat(stderrChunks).toString("utf8").trim();
          reject(new Error(`contextos exited with ${code}: ${err || "no stderr"}`));
          return;
        }
        const out = Buffer.concat(stdoutChunks).toString("utf8").trim();
        try {
          resolve(JSON.parse(out) as OptimizationResult);
        } catch (e) {
          reject(new Error(`failed to parse contextos output: ${(e as Error).message}`));
        }
      });

      child.stdin.end(payload);
    });
  }

  /**
   * Resolve the CLI binary, in order:
   *   1. `contextos.binaryPath` setting
   *   2. bundled binary inside the extension (`./bin/<platform>/contextos`)
   *   3. `contextos` on PATH (let the OS resolve)
   */
  private async resolveBinary(): Promise<string> {
    const configured = vscode.workspace
      .getConfiguration("contextos")
      .get<string>("binaryPath", "")
      .trim();

    if (configured) {
      if (!fs.existsSync(configured)) {
        throw new Error(`contextos.binaryPath points at missing file: ${configured}`);
      }
      return configured;
    }

    const bundled = path.join(
      this.context.extensionPath,
      "bin",
      process.platform,
      CLI_NAME,
    );
    if (fs.existsSync(bundled)) return bundled;

    // Fall through to PATH; spawn will surface ENOENT if missing.
    return CLI_NAME;
  }
}
