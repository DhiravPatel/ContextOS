"use client";

import type { HTMLAttributes, MouseEvent } from "react";
import { useRef } from "react";
import { cn } from "@/lib/utils";

interface GlowCardProps extends HTMLAttributes<HTMLDivElement> {
  className?: string;
  children: React.ReactNode;
}

/**
 * Card with a soft gradient spotlight that tracks the cursor. Pure CSS
 * custom-properties — no re-renders per mouse move.
 * Always a `div`; wrap with `<li>` etc. at the call site if semantics demand.
 */
export function GlowCard({ className, children, ...rest }: GlowCardProps) {
  const ref = useRef<HTMLDivElement>(null);

  const handleMove = (event: MouseEvent<HTMLDivElement>) => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * 100;
    const y = ((event.clientY - rect.top) / rect.height) * 100;
    el.style.setProperty("--spot-x", `${x}%`);
    el.style.setProperty("--spot-y", `${y}%`);
  };

  return (
    <div
      ref={ref}
      onMouseMove={handleMove}
      className={cn(
        "group relative h-full overflow-hidden rounded-2xl border border-line bg-bg-elevated/70 p-6 backdrop-blur-sm",
        "transition-colors hover:border-line-strong",
        className,
      )}
      style={{
        backgroundImage:
          "radial-gradient(360px circle at var(--spot-x, 50%) var(--spot-y, -20%), rgba(52,211,153,0.10), transparent 60%)",
      }}
      {...rest}
    >
      <div className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100">
        <div
          className="absolute inset-0 rounded-2xl"
          style={{
            background:
              "radial-gradient(220px circle at var(--spot-x, 50%) var(--spot-y, 50%), rgba(34,211,238,0.14), transparent 60%)",
          }}
        />
      </div>
      <div className="relative">{children}</div>
    </div>
  );
}
