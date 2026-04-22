"use client";

import { Menu, X } from "lucide-react";
import { useEffect, useState } from "react";
import { DocsSidebar } from "./DocsSidebar";

/**
 * Floating "menu" toggle for small screens. On lg+ the docked sidebar is
 * always visible; this drawer replaces it on smaller viewports.
 */
export function DocsMobileDrawer() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    document.body.style.overflow = open ? "hidden" : "";
    return () => {
      document.body.style.overflow = "";
    };
  }, [open]);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="Open documentation navigation"
        className="inline-flex h-10 w-full items-center justify-between rounded-lg border border-line bg-bg-elevated px-4 text-sm text-fg-muted lg:hidden"
      >
        <span className="inline-flex items-center gap-2">
          <Menu size={14} /> Docs navigation
        </span>
      </button>

      {open && (
        <div className="fixed inset-0 z-50 lg:hidden" role="dialog" aria-modal>
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setOpen(false)}
          />
          <aside className="absolute left-0 top-0 h-full w-[80%] max-w-xs overflow-y-auto border-r border-line bg-bg p-4">
            <div className="mb-4 flex items-center justify-between">
              <span className="text-sm font-semibold">Docs</span>
              <button
                type="button"
                onClick={() => setOpen(false)}
                aria-label="Close navigation"
                className="rounded-md p-1.5 text-fg-muted hover:bg-bg-muted hover:text-fg"
              >
                <X size={16} />
              </button>
            </div>
            <DocsSidebar onNavigate={() => setOpen(false)} />
          </aside>
        </div>
      )}
    </>
  );
}
