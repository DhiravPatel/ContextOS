import Link from "next/link";
import { Github } from "lucide-react";
import { LEGAL, SITE } from "@/lib/constants";

export function Footer() {
  return (
    <footer className="border-t border-line bg-bg/80">
      <div className="container-tight py-12">
        <div className="grid grid-cols-2 gap-10 md:grid-cols-4">
          <div className="col-span-2">
            <Link
              href="/"
              className="inline-flex items-center gap-2 text-sm font-semibold"
            >
              <span
                aria-hidden
                className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-line-strong bg-gradient-to-br from-accent to-accent-cyan text-bg"
              >
                <svg
                  viewBox="0 0 20 20"
                  width="14"
                  height="14"
                  fill="currentColor"
                >
                  <path d="M10 1 1 6l9 5 9-5-9-5Zm0 13L1 9v5l9 5 9-5V9l-9 5Z" />
                </svg>
              </span>
              {SITE.name}
            </Link>
            <p className="mt-4 max-w-sm text-sm text-fg-muted">
              {SITE.description}
            </p>
          </div>

          <FooterColumn
            title="Product"
            links={[
              { label: "How it works", href: "/#how" },
              { label: "Features", href: "/#features" },
              { label: "Install", href: "/#install" },
            ]}
          />

          <FooterColumn
            title="Docs"
            links={[
              { label: "Getting started", href: "/docs/getting-started" },
              { label: "How it works", href: "/docs/how-it-works" },
              { label: "Algorithms", href: "/docs/algorithms" },
              { label: "MCP server", href: "/docs/mcp" },
              { label: "GitHub", href: SITE.github, external: true },
            ]}
          />
        </div>

        <div className="mt-12 flex flex-col items-start justify-between gap-4 border-t border-line pt-6 text-sm text-fg-subtle md:flex-row md:items-center">
          <p>{LEGAL.copyright}</p>
          <div className="flex items-center gap-4">
            <a
              href={SITE.github}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="GitHub"
              className="transition-colors hover:text-fg"
            >
              <Github size={18} />
            </a>
            <a
              href={SITE.marketplace}
              target="_blank"
              rel="noopener noreferrer"
              className="transition-colors hover:text-fg"
            >
              VS Code Marketplace
            </a>
            <a
              href={SITE.openVsx}
              target="_blank"
              rel="noopener noreferrer"
              className="transition-colors hover:text-fg"
            >
              Open VSX
            </a>
          </div>
        </div>
      </div>
    </footer>
  );
}

interface FooterLink {
  label: string;
  href: string;
  external?: boolean;
}

function FooterColumn({ title, links }: { title: string; links: FooterLink[] }) {
  return (
    <div>
      <h4 className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">
        {title}
      </h4>
      <ul className="mt-4 space-y-2 text-sm">
        {links.map((link) => (
          <li key={link.href}>
            {link.external ? (
              <a
                href={link.href}
                target="_blank"
                rel="noopener noreferrer"
                className="text-fg-muted transition-colors hover:text-fg"
              >
                {link.label}
              </a>
            ) : (
              <Link
                href={link.href}
                className="text-fg-muted transition-colors hover:text-fg"
              >
                {link.label}
              </Link>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
