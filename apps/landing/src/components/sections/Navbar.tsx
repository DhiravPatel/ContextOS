"use client";

import Link from "next/link";
import { Menu, X } from "lucide-react";
import { useEffect, useState } from "react";
import { LinkButton } from "@/components/ui/Button";
import { NAV_LINKS, SITE } from "@/lib/constants";
import { cn } from "@/lib/utils";

export function Navbar() {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    if (open) document.body.style.overflow = "hidden";
    else document.body.style.overflow = "";
    return () => {
      document.body.style.overflow = "";
    };
  }, [open]);

  return (
    <header
      className={cn(
        "sticky top-0 z-50 w-full transition-all duration-200",
        scrolled
          ? "border-b border-line/80 bg-bg/85 backdrop-blur-md"
          : "bg-transparent",
      )}
    >
      <nav className="container-tight flex h-16 items-center justify-between">
        <Link
          href="/"
          className="group flex items-center gap-2 text-sm font-semibold tracking-tight"
          aria-label={SITE.name}
        >
          <span
            aria-hidden
            className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-line-strong bg-gradient-to-br from-accent to-accent-cyan text-bg"
          >
            <svg viewBox="0 0 20 20" width="14" height="14" fill="currentColor">
              <path d="M10 1 1 6l9 5 9-5-9-5Zm0 13L1 9v5l9 5 9-5V9l-9 5Z" />
            </svg>
          </span>
          <span className="text-fg">ContextOS</span>
        </Link>

        {/* Desktop nav */}
        <ul className="hidden items-center gap-8 md:flex">
          {NAV_LINKS.map((link) => (
            <li key={link.href}>
              <Link
                href={link.href}
                className="text-sm text-fg-muted transition-colors hover:text-fg"
              >
                {link.label}
              </Link>
            </li>
          ))}
        </ul>

        <div className="hidden items-center gap-3 md:flex">
          <LinkButton
            variant="ghost"
            size="sm"
            href={SITE.github}
            external
            aria-label="GitHub repository"
          >
            GitHub
          </LinkButton>
          <LinkButton
            variant="primary"
            size="sm"
            href={SITE.marketplace}
            external
          >
            Install
          </LinkButton>
        </div>

        {/* Mobile menu toggle */}
        <button
          type="button"
          aria-label={open ? "Close menu" : "Open menu"}
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-line text-fg md:hidden"
        >
          {open ? <X size={16} /> : <Menu size={16} />}
        </button>
      </nav>

      {/* Mobile panel */}
      {open && (
        <div className="border-t border-line bg-bg md:hidden">
          <ul className="container-tight flex flex-col gap-1 py-4">
            {NAV_LINKS.map((link) => (
              <li key={link.href}>
                <Link
                  href={link.href}
                  onClick={() => setOpen(false)}
                  className="block rounded-md px-3 py-2 text-sm text-fg-muted transition-colors hover:bg-bg-muted hover:text-fg"
                >
                  {link.label}
                </Link>
              </li>
            ))}
            <li className="mt-2 flex gap-2 px-3">
              <LinkButton
                variant="secondary"
                size="sm"
                href={SITE.github}
                external
                className="flex-1"
              >
                GitHub
              </LinkButton>
              <LinkButton
                variant="primary"
                size="sm"
                href={SITE.marketplace}
                external
                className="flex-1"
              >
                Install
              </LinkButton>
            </li>
          </ul>
        </div>
      )}
    </header>
  );
}
