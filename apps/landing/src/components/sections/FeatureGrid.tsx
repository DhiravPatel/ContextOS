import { Cpu, GitBranch, Network } from "lucide-react";
import { GlowCard } from "@/components/ui/GlowCard";
import { FEATURES } from "@/lib/constants";

const ICONS = {
  graph: GitBranch,
  compress: Cpu,
  mcp: Network,
} as const;

type IconKey = keyof typeof ICONS;

export function FeatureGrid() {
  return (
    <section id="features" className="container-tight py-24 md:py-32">
      <div className="mx-auto max-w-2xl text-center">
        <h2 className="text-balance text-3xl font-semibold tracking-tight md:text-4xl">
          Three layers of optimization.{" "}
          <span className="text-fg-muted">One goal.</span>
        </h2>
        <p className="mt-4 text-base text-fg-muted">
          Each layer is lossless by construction. Combined, they cut tokens by
          an order of magnitude.
        </p>
      </div>

      <ul className="mt-14 grid gap-6 md:grid-cols-3">
        {FEATURES.map((feature) => {
          const Icon = ICONS[feature.icon as IconKey];
          return (
            <li key={feature.title} className="h-full">
              <GlowCard className="flex h-full flex-col">
              <span
                aria-hidden
                className="inline-flex h-10 w-10 items-center justify-center rounded-xl border border-line-strong bg-bg-muted text-accent"
              >
                <Icon size={18} />
              </span>
              <div className="mt-5 text-xs uppercase tracking-wider text-fg-subtle">
                {feature.tagline}
              </div>
              <h3 className="mt-1 text-xl font-semibold tracking-tight">
                {feature.title}
              </h3>
              <p className="mt-3 text-sm leading-relaxed text-fg-muted">
                {feature.body}
              </p>
              <ul className="mt-5 space-y-2 text-sm text-fg-muted">
                {feature.bullets.map((b) => (
                  <li key={b} className="flex items-start gap-2">
                    <span
                      aria-hidden
                      className="mt-1.5 inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-accent"
                    />
                    <span>{b}</span>
                  </li>
                ))}
              </ul>
              </GlowCard>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
