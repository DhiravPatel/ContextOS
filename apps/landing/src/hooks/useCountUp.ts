"use client";

import { useEffect, useRef, useState, type RefObject } from "react";

interface UseCountUpOptions {
  from?: number;
  to: number;
  durationMs?: number;
  /** Start the animation only when the element enters the viewport. */
  triggerOnView?: boolean;
}

/**
 * Counts up from `from` to `to` over `durationMs` once the hook is mounted
 * (or once the returned ref enters the viewport when `triggerOnView` is set).
 * Easing: cubic ease-out. SSR-safe (starts at `from` until mount).
 */
export function useCountUp<T extends HTMLElement>(
  options: UseCountUpOptions,
): { ref: RefObject<T>; value: number } {
  const { from = 0, to, durationMs = 1400, triggerOnView = true } = options;
  const ref = useRef<T>(null);
  const [value, setValue] = useState(from);

  useEffect(() => {
    const el = ref.current;

    const animate = () => {
      const start = performance.now();
      let frame = 0;
      const tick = (now: number) => {
        const elapsed = now - start;
        const t = Math.min(1, elapsed / durationMs);
        const eased = 1 - Math.pow(1 - t, 3); // cubic ease-out
        setValue(from + (to - from) * eased);
        if (t < 1) frame = requestAnimationFrame(tick);
      };
      frame = requestAnimationFrame(tick);
      return () => cancelAnimationFrame(frame);
    };

    if (!triggerOnView || !el || typeof IntersectionObserver === "undefined") {
      return animate();
    }

    let stop: (() => void) | undefined;
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        if (entry && entry.isIntersecting && !stop) {
          stop = animate();
          observer.disconnect();
        }
      },
      { threshold: 0.35 },
    );
    observer.observe(el);
    return () => {
      observer.disconnect();
      stop?.();
    };
  }, [from, to, durationMs, triggerOnView]);

  return { ref, value };
}
