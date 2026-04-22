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
                      "group relative block rounded-md px-3 py-1.5 text-sm transition-colors",
                      // Left accent bar — only visible on the active item.
                      "before:absolute before:left-0 before:top-1/2 before:h-4 before:-translate-y-1/2 before:rounded-full before:bg-accent before:transition-all",
                      active
                        ? "font-medium text-accent before:w-[3px]"
                        : "text-fg-muted before:w-0 hover:text-fg",
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
