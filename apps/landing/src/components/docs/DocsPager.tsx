import Link from "next/link";
import { ArrowLeft, ArrowRight } from "lucide-react";
import { DOCS_PAGES } from "@/lib/constants";

/**
 * Previous / next-page footer, driven by the flat page list in constants.
 * Pass the current pathname via `href`.
 */
export function DocsPager({ href }: { href: string }) {
  const idx = DOCS_PAGES.findIndex((p) => p.href === href);
  if (idx === -1) return null;
  const prev = idx > 0 ? DOCS_PAGES[idx - 1] : null;
  const next = idx < DOCS_PAGES.length - 1 ? DOCS_PAGES[idx + 1] : null;

  return (
    <nav
      className="mt-16 grid gap-4 border-t border-line pt-8 md:grid-cols-2"
      aria-label="Doc pagination"
    >
      {prev ? (
        <Link
          href={prev.href}
          className="group flex items-center gap-3 rounded-xl border border-line bg-bg-elevated/60 px-5 py-4 transition-colors hover:border-line-strong"
        >
          <ArrowLeft
            size={16}
            className="text-fg-subtle transition-transform group-hover:-translate-x-0.5"
          />
          <div className="text-left">
            <div className="text-xs text-fg-subtle">Previous</div>
            <div className="text-sm font-medium text-fg">{prev.title}</div>
          </div>
        </Link>
      ) : (
        <span />
      )}
      {next ? (
        <Link
          href={next.href}
          className="group flex items-center justify-end gap-3 rounded-xl border border-line bg-bg-elevated/60 px-5 py-4 text-right transition-colors hover:border-line-strong"
        >
          <div>
            <div className="text-xs text-fg-subtle">Next</div>
            <div className="text-sm font-medium text-fg">{next.title}</div>
          </div>
          <ArrowRight
            size={16}
            className="text-fg-subtle transition-transform group-hover:translate-x-0.5"
          />
        </Link>
      ) : (
        <span />
      )}
    </nav>
  );
}
