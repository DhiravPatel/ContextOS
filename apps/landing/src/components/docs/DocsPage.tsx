import type { ReactNode } from "react";
import { DocsPager } from "./DocsPager";

interface DocsPageProps {
  title: string;
  lede?: string;
  kicker?: string;
  pathname: string;
  children: ReactNode;
}

/**
 * Layout wrapper for an individual doc page — renders the title block + the
 * prose body + next/prev pagination. Each `/docs/<slug>/page.tsx` composes
 * this with its own JSX body.
 */
export function DocsPage({
  title,
  lede,
  kicker,
  pathname,
  children,
}: DocsPageProps) {
  return (
    <article>
      <header className="mb-10">
        {kicker && (
          <p className="text-xs font-semibold uppercase tracking-widest text-accent">
            {kicker}
          </p>
        )}
        <h1 className="mt-2 text-balance text-3xl font-semibold tracking-tight md:text-4xl">
          {title}
        </h1>
        {lede && (
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-fg-muted">
            {lede}
          </p>
        )}
      </header>

      <div className="prose prose-invert prose-neutral max-w-none prose-headings:scroll-mt-24 prose-headings:tracking-tight prose-a:text-accent prose-a:no-underline hover:prose-a:text-accent-cyan prose-code:rounded prose-code:bg-bg-muted prose-code:px-1.5 prose-code:py-0.5 prose-code:text-[0.85em] prose-code:font-medium prose-code:text-fg prose-code:before:content-none prose-code:after:content-none prose-pre:rounded-xl prose-pre:border prose-pre:border-line prose-pre:bg-bg-elevated prose-pre:p-5 prose-li:marker:text-accent">
        {children}
      </div>

      <DocsPager href={pathname} />
    </article>
  );
}
