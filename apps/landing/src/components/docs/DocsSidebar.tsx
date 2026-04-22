"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { DOCS_NAV } from "@/lib/constants";
import { cn } from "@/lib/utils";

/**
 * Left-rail navigation shared across every /docs/* page.
 * Pulled into mobile via the `hideOnMobile` flag (caller renders two copies
 * — one in a drawer, one docked to the side on lg+).
 */
export function DocsSidebar({
  className,
  onNavigate,
}: {
  className?: string;
  onNavigate?: () => void;
}) {
  const pathname = usePathname();

  return (
    <nav className={cn("w-full", className)} aria-label="Documentation">
      {DOCS_NAV.map((group) => (
        <div key={group.group} className="mb-8 last:mb-0">
          <h4 className="px-3 text-xs font-semibold uppercase tracking-wider text-fg-subtle">
            {group.group}
          </h4>
          <ul className="mt-3 space-y-1">
            {group.items.map((item) => {
              const active = pathname === item.href;
              return (
                <li key={item.href}>
                  <Link
                    href={item.href}
                    onClick={onNavigate}
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "block rounded-lg px-3 py-1.5 text-sm transition-colors",
                      active
                        ? "bg-accent/10 text-accent"
                        : "text-fg-muted hover:bg-bg-muted hover:text-fg",
                    )}
                  >
                    {item.title}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </nav>
  );
}
