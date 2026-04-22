"use client";

import { ArrowDown, Sparkles } from "lucide-react";
import { forwardRef } from "react";
import { useCountUp } from "@/hooks/useCountUp";
import { HERO_METRICS } from "@/lib/constants";
import { cn, formatNumber, formatPercent } from "@/lib/utils";

/**
 * Animated before/after token comparison. Two columns: the raw token count
 * that the LLM *would* have read vs. the optimised count that ContextOS
 * actually sends. Numbers count up when the widget scrolls into view.
 */
export function TokenCounter() {
  const { ref: rawRef, value: raw } = useCountUp<HTMLDivElement>({
    to: HERO_METRICS.originalTokens,
    durationMs: 1200,
  });
  const { ref: optRef, value: opt } = useCountUp<HTMLDivElement>({
    to: HERO_METRICS.optimisedTokens,
    durationMs: 1600,
  });

  const reduction =
    1 - HERO_METRICS.optimisedTokens / HERO_METRICS.originalTokens;

  return (
    <div className="relative mx-auto mt-14 w-full max-w-3xl">
      <div className="soft-border p-6 md:p-8">
        <div className="grid items-stretch gap-4 md:grid-cols-2">
          <MetricBox
            ref={rawRef}
            label="Without ContextOS"
            subLabel={`${HERO_METRICS.filesOriginally} files`}
            value={raw}
            tone="muted"
          />
          <MetricBox
            ref={optRef}
            label="With ContextOS"
            subLabel={`${HERO_METRICS.filesAfterGraph} files`}
            value={opt}
            tone="accent"
          />
        </div>

        <div className="mt-6 flex flex-col items-center justify-between gap-3 rounded-xl border border-line bg-bg/50 px-5 py-4 text-sm text-fg-muted md:flex-row">
          <div className="flex items-center gap-2">
            <Sparkles size={14} className="text-accent" />
            <span>
              <span className="text-fg">{formatPercent(reduction, 0)}</span> fewer
              tokens per request
            </span>
          </div>
          <div className="flex items-center gap-2">
            <ArrowDown size={14} className="text-accent" />
            <span>
              optimised in{" "}
              <span className="text-fg">{HERO_METRICS.elapsedMs}ms</span> on a
              50-chunk workload
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

interface MetricBoxProps {
  label: string;
  subLabel: string;
  value: number;
  tone: "muted" | "accent";
}

const MetricBox = forwardRef<HTMLDivElement, MetricBoxProps>(function MetricBox(
  { label, subLabel, value, tone },
  ref,
) {
  return (
    <div
      ref={ref}
      className={cn(
        "rounded-xl border p-5",
        tone === "accent"
          ? "border-accent/40 bg-accent/5"
          : "border-line bg-bg-muted/50",
      )}
    >
      <div className="flex items-center justify-between text-xs uppercase tracking-wider text-fg-subtle">
        <span>{label}</span>
        <span>{subLabel}</span>
      </div>
      <div
        className={cn(
          "mt-3 font-mono text-4xl font-semibold tabular-nums md:text-5xl",
          tone === "accent" ? "gradient-text" : "text-fg",
        )}
      >
        {formatNumber(value)}
      </div>
      <div className="mt-1 text-xs text-fg-subtle">tokens</div>
    </div>
  );
});
