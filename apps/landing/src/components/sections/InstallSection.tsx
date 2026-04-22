import { ArrowRight, Boxes, Package } from "lucide-react";
import { LinkButton } from "@/components/ui/Button";
import { SITE } from "@/lib/constants";

const STEPS = [
  {
    label: "1",
    title: "Install the extension",
    body: "Search for ContextOS in VS Code extensions, or click the button below to open the Marketplace page.",
  },
  {
    label: "2",
    title: "Open any project",
    body: "Click “Enable” on the one-time consent dialog. ContextOS writes its MCP config and indexes your codebase in seconds.",
  },
  {
    label: "3",
    title: "Open Claude Code",
    body: "Every AI request in that project now routes through the ContextOS pipeline. You don't have to do anything else.",
  },
];

export function InstallSection() {
  return (
    <section id="install" className="relative overflow-hidden py-24 md:py-32">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-mesh"
      />
      <div className="container-tight">
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-semibold tracking-tight md:text-4xl">
            Three steps. Then never think about it again.
          </h2>
          <p className="mt-4 text-base text-fg-muted">
            No config files. No dashboard. No account.
          </p>
        </div>

        <ol className="mx-auto mt-14 grid max-w-4xl gap-4 md:grid-cols-3">
          {STEPS.map((step) => (
            <li
              key={step.label}
              className="soft-border relative flex flex-col p-6"
            >
              <span className="font-mono text-xs font-semibold uppercase tracking-widest text-accent">
                Step {step.label}
              </span>
              <h3 className="mt-3 text-lg font-semibold tracking-tight">
                {step.title}
              </h3>
              <p className="mt-2 text-sm leading-relaxed text-fg-muted">
                {step.body}
              </p>
            </li>
          ))}
        </ol>

        <div className="mx-auto mt-14 flex flex-col items-center gap-3 sm:flex-row sm:justify-center">
          <LinkButton
            variant="primary"
            size="lg"
            href={SITE.marketplace}
            external
          >
            <Package size={16} />
            VS Code Marketplace
            <ArrowRight size={16} />
          </LinkButton>
          <LinkButton
            variant="secondary"
            size="lg"
            href={SITE.openVsx}
            external
          >
            <Boxes size={16} />
            Open VSX (Cursor / VSCodium)
          </LinkButton>
        </div>
      </div>
    </section>
  );
}
