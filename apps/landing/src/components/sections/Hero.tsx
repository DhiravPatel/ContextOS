import { ArrowRight, Github, Zap } from "lucide-react";
import { LinkButton } from "@/components/ui/Button";
import { Badge, PulseDot } from "@/components/ui/Badge";
import { SITE } from "@/lib/constants";
import { TokenCounter } from "./TokenCounter";

export function Hero() {
  return (
    <section className="relative isolate overflow-hidden pb-24 pt-16 md:pb-32 md:pt-24">
      {/* Background grid + gradient mesh */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 grid-bg"
      />
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-[520px] bg-grid-fade"
      />

      <div className="container-tight flex flex-col items-center text-center">
        <Badge className="animate-fade-up">
          <PulseDot />
          v0.2 — zero-touch Claude Code integration
        </Badge>

        <h1 className="mt-6 max-w-4xl text-balance text-4xl font-semibold leading-[1.1] tracking-tight md:text-6xl lg:text-7xl">
          Stop burning tokens.{" "}
          <span className="gradient-text">Start vibe-coding smarter.</span>
        </h1>

        <p className="mt-6 max-w-2xl text-balance text-base leading-relaxed text-fg-muted md:text-lg">
          AI coding tools read your entire repo on every prompt.{" "}
          <span className="text-fg">{SITE.name}</span> sits in front as a
          local-first pre-processor — graph-picks the relevant files, runs a
          lossless compression pipeline, sends 5× fewer tokens to the LLM with
          identical output quality.
        </p>

        <div className="mt-8 flex flex-col items-center gap-3 sm:flex-row">
          <LinkButton
            variant="primary"
            size="lg"
            href={SITE.marketplace}
            external
          >
            <Zap size={16} />
            Install for VS Code
            <ArrowRight size={16} />
          </LinkButton>
          <LinkButton variant="secondary" size="lg" href={SITE.github} external>
            <Github size={16} />
            Star on GitHub
          </LinkButton>
        </div>

        <p className="mt-3 text-xs text-fg-subtle">
          Free · MIT · runs 100% locally · no telemetry
        </p>

        <TokenCounter />
      </div>
    </section>
  );
}
