import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: "class",
  content: [
    "./src/app/**/*.{ts,tsx}",
    "./src/components/**/*.{ts,tsx}",
  ],
  theme: {
    container: {
      center: true,
      padding: {
        DEFAULT: "1rem",
        sm: "1.5rem",
        lg: "2rem",
      },
      screens: {
        "2xl": "1200px",
      },
    },
    extend: {
      fontFamily: {
        sans: ["var(--font-geist-sans)", "ui-sans-serif", "system-ui"],
        mono: ["var(--font-geist-mono)", "ui-monospace", "SFMono-Regular"],
      },
      colors: {
        bg: {
          DEFAULT: "#07080b",
          elevated: "#0d1015",
          muted: "#15181f",
        },
        line: {
          DEFAULT: "#1d2028",
          strong: "#2a2f3a",
        },
        fg: {
          DEFAULT: "#e7e9ee",
          muted: "#9aa1ae",
          subtle: "#6a7180",
        },
        accent: {
          DEFAULT: "#34d399",
          strong: "#10b981",
          cyan: "#22d3ee",
        },
      },
      backgroundImage: {
        "grid-fade":
          "radial-gradient(ellipse at top, rgba(52,211,153,0.12), transparent 60%)",
        "mesh":
          "radial-gradient(circle at 20% 10%, rgba(52,211,153,0.10), transparent 40%), radial-gradient(circle at 80% 0%, rgba(34,211,238,0.08), transparent 40%)",
      },
      animation: {
        "pulse-slow": "pulse 3.2s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "fade-up": "fade-up 0.6s ease-out forwards",
        marquee: "marquee 28s linear infinite",
      },
      keyframes: {
        "fade-up": {
          "0%": { opacity: "0", transform: "translateY(10px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        marquee: {
          "0%": { transform: "translateX(0)" },
          "100%": { transform: "translateX(-50%)" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
