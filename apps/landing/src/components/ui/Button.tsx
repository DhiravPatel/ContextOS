import Link from "next/link";
import * as React from "react";
import { cn } from "@/lib/utils";

type Variant = "primary" | "secondary" | "ghost";
type Size = "sm" | "md" | "lg";

interface BaseProps {
  variant?: Variant;
  size?: Size;
  className?: string;
  children: React.ReactNode;
}

type ButtonProps = BaseProps & React.ButtonHTMLAttributes<HTMLButtonElement>;
type LinkProps = BaseProps &
  Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "href"> & {
    href: string;
    external?: boolean;
  };

const variants: Record<Variant, string> = {
  primary:
    "bg-accent-strong text-bg hover:bg-accent shadow-[0_0_0_1px_rgba(52,211,153,0.4),0_10px_30px_-10px_rgba(52,211,153,0.55)]",
  secondary:
    "bg-bg-muted text-fg hover:bg-line border border-line-strong",
  ghost: "text-fg-muted hover:text-fg hover:bg-bg-muted",
};

const sizes: Record<Size, string> = {
  sm: "h-9 px-3 text-sm",
  md: "h-10 px-4 text-sm",
  lg: "h-11 px-5 text-base",
};

function baseClasses(variant: Variant, size: Size, className?: string): string {
  return cn(
    "inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors",
    "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
    "disabled:pointer-events-none disabled:opacity-50",
    variants[variant],
    sizes[size],
    className,
  );
}

export function Button({
  variant = "primary",
  size = "md",
  className,
  ...rest
}: ButtonProps) {
  return (
    <button className={baseClasses(variant, size, className)} {...rest} />
  );
}

export function LinkButton({
  variant = "primary",
  size = "md",
  className,
  href,
  external,
  children,
  ...rest
}: LinkProps) {
  const classes = baseClasses(variant, size, className);
  if (external) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        className={classes}
        {...rest}
      >
        {children}
      </a>
    );
  }
  return (
    <Link href={href} className={classes} {...rest}>
      {children}
    </Link>
  );
}
