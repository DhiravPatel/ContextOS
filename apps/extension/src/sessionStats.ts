// Tracks per-session optimization runs so the user can see aggregate impact.

import { OptimizationResult } from "./types";

export interface SessionSummary {
  runs: number;
  totalOriginalTokens: number;
  totalFinalTokens: number;
  totalSavedTokens: number;
  avgReductionPct: number;
  avgElapsedMs: number;
}

export class SessionStats {
  private runs = 0;
  private originalTokens = 0;
  private finalTokens = 0;
  private totalElapsedMs = 0;

  record(result: OptimizationResult): void {
    this.runs += 1;
    this.originalTokens += result.original_tokens;
    this.finalTokens += result.final_tokens;
    this.totalElapsedMs += result.elapsed_ms;
  }

  summary(): SessionSummary {
    const saved = Math.max(0, this.originalTokens - this.finalTokens);
    const avgReduction =
      this.originalTokens === 0 ? 0 : (saved / this.originalTokens) * 100;
    return {
      runs: this.runs,
      totalOriginalTokens: this.originalTokens,
      totalFinalTokens: this.finalTokens,
      totalSavedTokens: saved,
      avgReductionPct: avgReduction,
      avgElapsedMs: this.runs === 0 ? 0 : this.totalElapsedMs / this.runs,
    };
  }
}
