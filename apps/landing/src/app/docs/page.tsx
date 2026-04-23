import Link from "next/link";
import { DocsPage } from "@/components/docs/DocsPage";
import { DOCS_NAV } from "@/lib/constants";

export default function DocsIndex() {
  return (
    <DocsPage
      kicker="Documentation"
      title="Ship with fewer tokens, not fewer answers."
      lede="Everything you need to install ContextOS, understand how it reduces tokens without changing LLM output, and operate it in production."
      pathname="/docs"
    >
      <p>
        ContextOS sits between your IDE and your LLM. It builds a structural
        graph of your repo, picks only the code the current task actually
        depends on, and runs a lossless compression pipeline on top. Claude Code
        picks it up automatically via the Model Context Protocol, so reduction
        is invisible and zero-touch.
      </p>

      <h2>Where to start</h2>
      <ul className="!list-none !p-0">
        {DOCS_NAV.filter((g) => g.group !== "Start here" || true).flatMap(
          (group) => [
            <li
              key={`head-${group.group}`}
              className="!mt-8 text-xs font-semibold uppercase tracking-widest text-fg-subtle"
            >
              {group.group}
            </li>,
            ...group.items
              .filter((i) => i.href !== "/docs")
              .map((item) => (
                <li key={item.href} className="!my-2 !pl-0">
                  <Link
                    href={item.href}
                    className="group flex items-center justify-between gap-4 rounded-xl border border-line bg-bg-elevated/60 px-5 py-4 !no-underline transition-colors hover:border-accent/40"
                  >
                    <span>
                      <span className="block text-sm font-semibold text-fg">
                        {item.title}
                      </span>
                      {item.description && (
                        <span className="mt-0.5 block text-sm text-fg-muted">
                          {item.description}
                        </span>
                      )}
                    </span>
                    <span
                      aria-hidden
                      className="text-fg-subtle transition-transform group-hover:translate-x-0.5"
                    >
                      →
                    </span>
                  </Link>
                </li>
              )),
          ],
        )}
      </ul>
    </DocsPage>
  );
}
