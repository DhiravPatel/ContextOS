# ContextOS — Landing Page

Marketing site for ContextOS. Next.js 15 (App Router) + React 18 + TypeScript (strict) + Tailwind CSS v3 + lucide-react.

## Folder layout

```
apps/landing/
├── public/                         # static assets (favicon, OG image)
├── src/
│   ├── app/
│   │   ├── layout.tsx              # root HTML, metadata, fonts
│   │   ├── page.tsx                # landing page composition
│   │   ├── globals.css             # Tailwind entry + base styles
│   │   ├── sitemap.ts              # /sitemap.xml
│   │   └── robots.ts               # /robots.txt
│   ├── components/
│   │   ├── ui/                     # primitives (Button, Badge, GlowCard)
│   │   └── sections/               # one-file-per-section landing blocks
│   ├── hooks/
│   │   └── useCountUp.ts           # viewport-triggered count-up
│   └── lib/
│       ├── constants.ts            # site copy, URLs, feature data
│       └── utils.ts                # cn(), formatNumber(), formatPercent()
├── tailwind.config.ts
├── next.config.ts
├── tsconfig.json
└── package.json
```

## Local development

```bash
# from the monorepo root
npm install                      # installs workspace deps
npm run dev:landing              # starts Next.js dev server on :3000
```

Or from this folder:

```bash
cd apps/landing
npm install
npm run dev
```

## Type-checking & linting

```bash
npm run typecheck
npm run lint
```

## Production build

```bash
npm run build
npm run start                    # serves .next/standalone
```

## Deployment

**Vercel (recommended — zero config):**

1. Import the monorepo in Vercel.
2. Set **Root Directory** to `apps/landing`.
3. Framework preset auto-detected as Next.js.
4. Every push to `main` auto-deploys; PRs get preview URLs.

**Self-host:**

```bash
npm run build
PORT=3000 node .next/standalone/apps/landing/server.js
```

**Static export** (if you ever want to host as plain HTML):

Add `output: "export"` to `next.config.ts` and run `npm run build`. Output ends up in `out/`. Note: dynamic features (sitemap, robots) still work because they're generated at build time.

## Editing content

Most copy lives in [src/lib/constants.ts](src/lib/constants.ts):

- `SITE` — name, tagline, URLs, social handles
- `NAV_LINKS` — header navigation
- `HERO_METRICS` — numbers shown in the animated hero counter
- `FEATURES` — the 3 feature cards
- `PIPELINE_STAGES` — the 5 pipeline stages

Change values there and the components pick them up automatically. No hand-wired copy inside the components themselves.

## Before first deploy

Drop these files in `public/`:

- `favicon.ico` (32×32, multi-resolution ICO)
- `apple-touch-icon.png` (180×180)
- `og-image.png` (1200×630 — shown as social preview)

Also update the GitHub / Marketplace / Open VSX URLs in [src/lib/constants.ts](src/lib/constants.ts) once your publisher page is live.
