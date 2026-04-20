# Deployment & End-to-End Flow

Two audiences live in this document:

- **Maintainers** — how to cut a release, build per-platform binaries, and publish the extension to the VS Code Marketplace + Open VSX.
- **End users** — what installing the extension actually does, what they need to do once, and how tokens get reduced after that (automatic vs. manual modes).

---

## Part A — Maintainer: publish a release

### A1. Tag & version

ContextOS ships three artefacts from one repo:

| Artefact | Version source | Where it lives |
|---|---|---|
| `contextos` CLI binary | `Cargo.toml` workspace version | GitHub Release assets, Homebrew, `cargo install` |
| VS Code extension (`.vsix`) | `apps/extension/package.json` | VS Code Marketplace + Open VSX |
| Docker image | git tag | `ghcr.io/contextos/contextos:<tag>` |

All three should carry the same version. Bump together:

```bash
# Bump workspace (Rust)
sed -i '' 's/^version = "0.2.0"/version = "0.2.1"/' Cargo.toml

# Bump extension (TS)
(cd apps/extension && npm version 0.2.1 --no-git-tag-version)

git add Cargo.toml apps/extension/package.json apps/extension/package-lock.json
git commit -m "release: v0.2.1"
git tag v0.2.1
git push origin main --tags
```

### A2. Build per-platform CLI binaries

The extension ships with the CLI bundled inside `apps/extension/bin/<platform>/contextos`. Build for every platform you want to support:

```bash
# macOS (Apple Silicon + Intel universal binary)
cargo build --release --target aarch64-apple-darwin   --bin contextos
cargo build --release --target x86_64-apple-darwin    --bin contextos
lipo -create \
  target/aarch64-apple-darwin/release/contextos \
  target/x86_64-apple-darwin/release/contextos \
  -output apps/extension/bin/darwin/contextos

# Linux x86_64 (MUSL for portability)
cargo build --release --target x86_64-unknown-linux-musl --bin contextos
cp target/x86_64-unknown-linux-musl/release/contextos apps/extension/bin/linux/contextos

# Linux aarch64
cargo build --release --target aarch64-unknown-linux-musl --bin contextos
cp target/aarch64-unknown-linux-musl/release/contextos apps/extension/bin/linux-arm64/contextos

# Windows x86_64
cargo build --release --target x86_64-pc-windows-msvc --bin contextos
cp target/x86_64-pc-windows-msvc/release/contextos.exe apps/extension/bin/win32/contextos.exe
```

In CI, use `cross` or a matrix of GitHub-hosted runners so you don't need local toolchains.

### A3. Package the extension

```bash
cd apps/extension
npm ci --no-audit --no-fund
npm run compile          # tsc → out/
npx vsce package --no-dependencies   # produces contextos-vscode-0.2.1.vsix
```

Validate the bundle size — the `.vsix` should be well under 50 MB. If it's larger, check `.vscodeignore` is excluding `node_modules/**` and unneeded platform binaries.

> **Recommended: per-platform VSIX.** Bundling all four CLIs in one `.vsix` bloats the download 4×. VS Code supports per-platform extensions:
>
> ```bash
> npx vsce package --target darwin-x64,darwin-arm64,linux-x64,linux-arm64,win32-x64
> ```
>
> Each upload includes only its matching `bin/<platform>/` folder.

### A4. Publish

You need two accounts:

- **VS Code Marketplace** — create a publisher at <https://marketplace.visualstudio.com/manage>, then a PAT under `Marketplace > Manage` with scope `Marketplace (Manage)`.
- **Open VSX** (Cursor, VSCodium, Windsurf use this) — token from <https://open-vsx.org/user-settings/tokens>.

```bash
# VS Code Marketplace
export VSCE_PAT="..."
cd apps/extension
npx vsce publish                     # uses the version in package.json

# Open VSX
export OVSX_PAT="..."
npx ovsx publish contextos-vscode-0.2.1.vsix
```

### A5. Release the CLI separately

Some users want the CLI without the extension (CI pipelines, Claude Code MCP users, Neovim users).

```bash
# GitHub release (attach the target/*/release/contextos binaries)
gh release create v0.2.1 \
  --title "v0.2.1" \
  --notes-file CHANGELOG.md \
  target/aarch64-apple-darwin/release/contextos#contextos-darwin-arm64 \
  target/x86_64-apple-darwin/release/contextos#contextos-darwin-x64 \
  target/x86_64-unknown-linux-musl/release/contextos#contextos-linux-x64 \
  target/aarch64-unknown-linux-musl/release/contextos#contextos-linux-arm64 \
  target/x86_64-pc-windows-msvc/release/contextos.exe#contextos-win32-x64.exe

# Crates.io (optional)
cd apps/cli && cargo publish
```

Recommended install methods for users who skip the extension:

```bash
# Homebrew tap (maintain a simple formula)
brew install contextos/tap/contextos

# Cargo
cargo install contextos-cli

# Curl script (single binary)
curl -fsSL https://github.com/contextos/contextos/releases/latest/download/install.sh | sh
```

### A6. Release checklist

Before cutting the tag:

- [ ] `cargo test --workspace` is green
- [ ] `npm --workspace apps/extension run compile` is clean
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] `CHANGELOG.md` updated
- [ ] Per-platform CLI binaries are staged inside `apps/extension/bin/`
- [ ] `vsce package` under 50 MB
- [ ] Smoke-tested in a fresh VS Code extension host (`F5` from `apps/extension`)

---

## Part B — End user: install & use

### B1. Install the extension

From inside VS Code:

1. `⌘⇧X` → search **ContextOS**
2. Click **Install**
3. Reload window when prompted

That's it for the extension side. The CLI ships bundled inside the extension at `~/.vscode/extensions/contextos.contextos-vscode-<version>/bin/<platform>/contextos`, so the user doesn't need to install anything else.

### B2. First-run setup — fully automatic

The only supported AI tool in v0.2 is **Claude Code**. Everything below happens without the user lifting a finger beyond clicking **Enable** on a single consent dialog.

On first activation in any workspace, the extension:

1. **Asks for consent once** (globally): *"ContextOS will auto-configure Claude Code for this project (writes `.mcp.json`) and keep a local code graph in `.contextos/`. Continue?"* — buttons: **Enable / Not now / Never**.
2. On **Enable**, runs `contextos install --root <workspace>`, which writes:
   - `.mcp.json` in the project root — MCP server entry for Claude Code.
   - `.claude/settings.local.json` — adds `"contextos"` to `enabledMcpjsonServers`.
3. Runs `contextos build --root <workspace>` — indexes the repo (~10s for 500 files, ~1–2 min for 5k).
4. Spawns `contextos watch --root <workspace>` as a background child process — keeps the graph fresh on every save/create/delete.
5. Shows: *"ContextOS: wired into Claude Code for this project. Reload Claude Code to activate."*

The user opens Claude Code → the MCP server is detected → every AI request automatically calls ContextOS tools. Tokens drop silently.

**Consent is remembered globally.** Once the user clicks **Enable** once, every future workspace auto-installs without prompting. **Never** disables for all workspaces. **Not now** asks again next time.

**State files written:**
- `<workspace>/.mcp.json` — project-scoped MCP server list.
- `<workspace>/.claude/settings.local.json` — project-scoped Claude Code settings.
- `<workspace>/.contextos/graph.db` — SQLite code graph.

All three live inside the workspace; nothing touches the user's home directory or global VS Code state beyond the consent flag.

### B3. Commands (for when the user does want to intervene)

| Command | Purpose |
|---|---|
| `ContextOS: Reconfigure Claude Code for this Project` | Re-run install + build — useful after a binary upgrade |
| `ContextOS: Remove from this Project` | Calls `contextos uninstall`; deletes our entries from `.mcp.json` and `settings.local.json` (preserves any other servers) |
| `ContextOS: Optimize Current Context` (`⌘⌥O`) | Legacy manual path — collects context, prints reduced bundle to a new tab. Useful if the user wants to paste into a web-based LLM chat. |
| `ContextOS: Show Session Stats` | Cumulative tokens saved this session |

### B4. Everyday usage

Three ways to trigger reduction:

| Trigger | When | Result |
|---|---|---|
| Press `⌘⌥O` | You want to manually compose a prompt | Opens a new tab with the optimized bundle |
| AI assistant calls MCP tool | Automatic, every AI turn (Path 2) | AI receives reduced context directly |
| `git commit` / file save | Automatic (Path 3 with watch) | Graph stays fresh; no prompt changes |

Check savings anytime with **ContextOS: Show Session Stats** — shows cumulative tokens saved this session.

---

## Part C — End-to-end data flow

Walk through what happens on a single AI request, for the fully-automatic setup (extension + MCP + watch mode).

```
┌────────────────────────────────────────────────────────────────────┐
│  T-0   Developer edits `src/auth.ts`, saves file                    │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────────────────┐
│  T+20ms  `contextos watch` (background process)                     │
│          • notices file system event                                │
│          • SHA-256 of auth.ts → hash changed                        │
│          • reparses JUST auth.ts via Tree-sitter                    │
│          • upserts 12 nodes + 34 edges into graph.db                │
│          • all other files untouched (hash unchanged → skipped)     │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────────────────┐
│  T+2s   Developer asks Claude Code: "add 2FA to the login flow"     │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────────────────┐
│  T+2.1s  Claude Code calls MCP tool `impact_radius`                 │
│          params: { files: ["src/auth.ts"], depth: 2 }               │
│                                                                     │
│  ContextOS (MCP server) responds:                                   │
│     impacted_nodes: 18                                              │
│     impacted_files: [                                               │
│       "src/auth.ts",                                                │
│       "src/login.controller.ts",                                    │
│       "src/middleware/session.ts",                                  │
│       "src/routes/auth.test.ts"                                     │
│     ]                                                               │
│     (5 files instead of 312 in the repo)                            │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────────────────┐
│  T+2.3s  Claude Code calls MCP tool `optimize` with:                │
│          • full text of 4 impacted files                            │
│          • skeletons of 6 peripheral-but-referenced files           │
│          • query: "add 2FA to the login flow"                       │
│                                                                     │
│  Core engine pipeline runs:                                         │
│     • Skeletonise   → 6 periphery files become signatures only      │
│     • Dedup         → 3 near-duplicate helpers collapsed (MinHash)  │
│     • Compress      → comments + console.log stripped (AST)         │
│     • Rank          → BM25 + PageRank → login.controller first      │
│     • Budget        → fits into 4000-token cap                      │
│                                                                     │
│  Response:                                                          │
│     original_tokens: 38,400                                         │
│     final_tokens:      3,920                                        │
│     tokens_saved:     34,480   (−89.8%)                             │
│     elapsed_ms:          47                                         │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────────────────┐
│  T+2.4s  Claude Code sends 3,920 tokens (not 38,400) to the LLM    │
│          → faster response, lower cost, same answer quality         │
└────────────────────────────────────────────────────────────────────┘
```

Every number above is representative of an actual run on a mid-size TypeScript repo.

---

## Part D — Auto-update & watch mode details

Watch mode is a separate process. If you enable Path 3, the extension launches it as a child of the extension host. Lifecycle:

- **Starts** when the extension activates (workspace opens).
- **Debounces** filesystem events with a 200ms window, so a 50-file save batch is one graph update, not 50.
- **Skips** anything under `.git/`, `node_modules/`, `target/`, `.contextos/`, `*.swp`, and editor backup files.
- **Stops** cleanly when the workspace closes (`vscode.Disposable` chain).

Manual alternatives that don't need the extension:

```bash
# One-shot update from a git diff
git diff --name-only HEAD~1 | contextos update

# Post-commit hook
cat > .git/hooks/post-commit <<'EOF'
#!/usr/bin/env bash
git diff-tree --no-commit-id --name-only -r HEAD | contextos update
EOF
chmod +x .git/hooks/post-commit

# Continuous watch (shell, no extension)
contextos watch --root .
```

---

## Part E — Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `contextos: command not found` in MCP logs | Binary not on PATH | Use the full bundled path under `~/.vscode/extensions/contextos.contextos-vscode-*/bin/` |
| First build takes > 5 minutes | Massive repo (>50k files) | `echo 'generated/' >> .contextosignore`, then rebuild |
| `impact_radius` returns empty | Graph not built yet | Run `ContextOS: Build Graph` once, or just wait out first-run auto-build |
| `.contextos/graph.db locked` | Two watchers running | Kill stray `contextos watch` processes; only one per workspace |
| Extension can't spawn CLI (`ENOENT`) | Antivirus quarantined bundled binary | Whitelist the `.vscode/extensions/contextos.*/bin/` folder |
| Big token counts even after optimize | Query text too generic | Pass the user's *actual* prompt as `query`; relevance ranking needs the question |

---

## Part F — Security & trust model

- **Everything is local.** No code, hashes, prompts, or stats leave the user's machine. Verify with `lsof -p $(pgrep contextos)` — the only open sockets are stdio pipes.
- **Single binary boundary.** Only the Rust CLI ever reads bulk source bytes. The extension passes paths, not content.
- **SQLite lives inside the repo** (`.contextos/graph.db`), so it moves with the project and respects whatever backup / gitignore rules the user has.
- **No network.** The default build has zero network dependencies. Optional embedding backends (future) will be opt-in with explicit user consent.

---

## TL;DR for users

```
1. Install VS Code extension "ContextOS" from the Marketplace.
2. Open any project. Click "Enable" on the one-time consent dialog.
3. Open Claude Code in that project. That's it.
```

You never need to:

- Edit any config file yourself.
- Paste anything into a terminal.
- Run `contextos build`, `install`, `watch` by hand.
- Remember to do anything on project #2, #3, #4.

The extension handles all of it. Only Claude Code is supported in v0.2 — Cursor, Windsurf, Continue, and others will land in later releases.
