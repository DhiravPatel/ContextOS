# Publishing ContextOS to the VS Code Marketplace

Concrete, copy-paste steps for publishing **this repo** to the VS Code Marketplace (and Open VSX for Cursor users). Follow top to bottom the first time; after the first release, you mostly re-run Steps 4–7.

---

## Step 0 — Prerequisites

Two things aren't set up on a fresh dev machine:

### 0.1 Rust toolchain

The extension bundles a native Rust CLI. You need Rust to build it.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version            # sanity check
```

### 0.2 Missing files in `apps/extension/`

Marketplace rejects any extension without both an **icon** and a **LICENSE**. Neither exists yet in this repo.

```bash
cd "/Users/dhiravpatel/Documents/Project 2/ContextOS"

# LICENSE (MIT — change if you use a different licence)
cat > apps/extension/LICENSE <<'EOF'
MIT License

Copyright (c) 2026 <Your Name>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND.
EOF

# Icon — put your 128×128 (or larger) PNG here:
mkdir -p apps/extension/media
# cp <path-to-your-logo.png> apps/extension/media/icon.png
```

---

## Step 1 — Create a Marketplace publisher (5 min, one-time)

1. Go to **<https://marketplace.visualstudio.com/manage>** and sign in with any Microsoft account (personal accounts work fine).
2. Click **Create publisher**.
3. Pick a publisher ID. This is **not** `contextos` unless you register that org — use your own handle, e.g. `grishma-shah` or your company name. The ID becomes part of your extension URL.
4. Copy the ID — you paste it into `package.json` in Step 3.

---

## Step 2 — Get the Azure DevOps PAT (5 min, one-time)

`vsce` authenticates via Azure DevOps, not the Marketplace site itself. This is the step where most first-time publishers get stuck.

1. Go to **<https://dev.azure.com/>** and sign in with the same Microsoft account you used in Step 1.
2. If prompted, create any organisation (the name doesn't matter — we never use it again).
3. Top-right avatar → **Personal Access Tokens** → **New Token**:
   - **Name:** `vsce`
   - **Organization:** **All accessible organizations** ← do not leave as the default
   - **Expiration:** 1 year
   - **Scopes:** click **Show all scopes** → scroll to **Marketplace** → tick **Manage**
4. Click **Create**, and copy the token immediately (you cannot view it again after closing the dialog).
5. Stash it:

```bash
export VSCE_PAT="paste-your-token-here"
# For persistence, append the line to ~/.zshrc
```

---

## Step 3 — Configure this repo's `package.json`

Edit [apps/extension/package.json](../apps/extension/package.json) and set these fields:

```json
{
  "publisher": "your-publisher-id-from-step-1",
  "icon": "media/icon.png",
  "license": "MIT",
  "repository": {
    "type": "git",
    "url": "https://github.com/<your-gh-user>/ContextOS"
  },
  "bugs": {
    "url": "https://github.com/<your-gh-user>/ContextOS/issues"
  },
  "homepage": "https://github.com/<your-gh-user>/ContextOS#readme"
}
```

All five fields must be present. Missing any of them causes `vsce package` to fail.

---

## Step 4 — Build the Rust CLI and stage it inside the extension

For a first publish, just your current platform (macOS) is fine.

```bash
cd "/Users/dhiravpatel/Documents/Project 2/ContextOS"

# Apple Silicon (M-series)
cargo build --release --target aarch64-apple-darwin --bin contextos

# Intel macs — skip if you only target M-series
rustup target add x86_64-apple-darwin
cargo build --release --target x86_64-apple-darwin --bin contextos

# Combine into a universal binary (works on both)
mkdir -p apps/extension/bin/darwin
lipo -create \
  target/aarch64-apple-darwin/release/contextos \
  target/x86_64-apple-darwin/release/contextos \
  -output apps/extension/bin/darwin/contextos
chmod +x apps/extension/bin/darwin/contextos

# Sanity check
./apps/extension/bin/darwin/contextos version
```

For Linux / Windows publishes later, repeat the build with the right `--target` triples and drop the binary under `apps/extension/bin/<platform>/contextos[.exe]`. See [DEPLOYMENT.md § A2](DEPLOYMENT.md#a2-build-per-platform-cli-binaries).

---

## Step 5 — Install `vsce` and package the extension

```bash
npm install -g @vscode/vsce

cd "/Users/dhiravpatel/Documents/Project 2/ContextOS/apps/extension"
npm ci --no-audit --no-fund
npm run compile                    # tsc → out/
vsce package --no-dependencies     # produces contextos-vscode-0.2.0.vsix
```

Inspect what got packaged:

```bash
ls -lh *.vsix                      # should be well under 20 MB for macOS-only
unzip -l contextos-vscode-0.2.0.vsix | head -30
```

If the size is huge, check `.vscodeignore` — node_modules should be excluded.

---

## Step 6 — Smoke-test the local `.vsix` before publishing

**Critical:** once a version is on the Marketplace, you cannot silently replace it — only supersede with a newer version. Always test the packaged `.vsix` locally first.

```bash
code --install-extension contextos-vscode-0.2.0.vsix
```

Open any project in VS Code and verify:

- [ ] First-run consent dialog appears: *"ContextOS will auto-configure Claude Code…"*
- [ ] After clicking **Enable**, a "setting up…" progress toast appears.
- [ ] View → Output → **ContextOS** channel shows `auto-install: …` log lines followed by `watch: pid=…`.
- [ ] `.mcp.json` and `.contextos/graph.db` exist in the project root.
- [ ] `ContextOS: Show Session Stats` command runs without error.
- [ ] Uninstall the extension (`code --uninstall-extension <publisher>.contextos-vscode`) cleanly.

If anything fails, fix it, bump the patch version (e.g. `0.2.0` → `0.2.1`) and re-package. Never ship a broken `.vsix`.

---

## Step 7 — Publish

```bash
cd "/Users/dhiravpatel/Documents/Project 2/ContextOS/apps/extension"
vsce publish                       # uses the version in package.json
```

First publish propagates in 1–3 minutes. You'll see a success line in the terminal, and Microsoft emails the publisher account to confirm.

---

## Step 8 — Verify it's live

Your Marketplace page is:

```
https://marketplace.visualstudio.com/items?itemName=<your-publisher-id>.contextos-vscode
```

Check:

- [ ] Icon renders correctly
- [ ] README renders (pulls from [apps/extension/README.md](../apps/extension/README.md))
- [ ] Commands and settings appear in the sidebar panels
- [ ] Search for "ContextOS" from inside VS Code → Extensions → your extension appears within ~5 min

---

## Step 9 — Publish to Open VSX (Cursor / VSCodium / Windsurf users)

Microsoft's Marketplace forbids non-Microsoft products from pulling from it. Cursor and the VSCodium family use **Open VSX** instead — a separate publish.

```bash
npm install -g ovsx

# Get a token from https://open-vsx.org/user-settings/tokens
export OVSX_PAT="..."

cd apps/extension
ovsx publish contextos-vscode-0.2.0.vsix
```

Verify at `https://open-vsx.org/extension/<your-publisher-id>/contextos-vscode`.

---

## Step 10 — Shipping a new version

For any subsequent release:

```bash
cd "/Users/dhiravpatel/Documents/Project 2/ContextOS"

# 1. Rebuild the CLI (otherwise the bundled binary is stale)
cargo build --release --target aarch64-apple-darwin --bin contextos
# (+ other targets as needed)

# 2. Bump + publish in one command
cd apps/extension
vsce publish patch           # 0.2.0 → 0.2.1 — auto-commits + tags + publishes
# or: vsce publish minor / vsce publish major
```

Don't forget to also publish to Open VSX (Step 9) so Cursor users get the update.

### Semver cheat-sheet

- `patch` — bug fixes only, no API change.
- `minor` — new command / new setting / new MCP tool — backward compatible.
- `major` — breaking change (e.g. `.mcp.json` schema changes, removed command).

Also keep `Cargo.toml` (workspace version) in lockstep with `apps/extension/package.json` so the CLI version and extension version match:

```bash
# In the repo root, before running `vsce publish <bump>`:
sed -i '' -E 's/^version = "[^"]+"/version = "0.2.1"/' Cargo.toml
```

---

## Pre-flight checklist

Copy this into a checklist before every publish:

- [ ] Rust installed (`rustc --version` works)
- [ ] `VSCE_PAT` exported in the shell
- [ ] `publisher` set in `apps/extension/package.json`
- [ ] `apps/extension/media/icon.png` exists (≥128×128)
- [ ] `apps/extension/LICENSE` exists
- [ ] `apps/extension/bin/darwin/contextos` exists and `./… version` works
- [ ] `cargo test --workspace` green
- [ ] `npm run compile` in `apps/extension` clean
- [ ] `CHANGELOG.md` entry written for this version
- [ ] `.vsix` packaged, size sane (<20 MB single-platform, <50 MB max)
- [ ] Smoke-tested the `.vsix` in a fresh VS Code window
- [ ] Version bumped in **both** `Cargo.toml` and `apps/extension/package.json`
- [ ] Git tag pushed

---

## TL;DR

```bash
# Once per machine
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
npm install -g @vscode/vsce ovsx
export VSCE_PAT="..."           # from dev.azure.com
export OVSX_PAT="..."           # from open-vsx.org

# Every release
cd "/Users/dhiravpatel/Documents/Project 2/ContextOS"
cargo build --release --target aarch64-apple-darwin --bin contextos
mkdir -p apps/extension/bin/darwin
cp target/aarch64-apple-darwin/release/contextos apps/extension/bin/darwin/contextos
chmod +x apps/extension/bin/darwin/contextos

cd apps/extension
npm ci && npm run compile
vsce package --no-dependencies
code --install-extension *.vsix      # smoke-test in fresh window
vsce publish                         # ship to Marketplace
ovsx publish *.vsix                  # ship to Open VSX
```

That's the entire flow.

---

## Troubleshooting

| Error | Fix |
|---|---|
| `ERROR: Make sure to edit the README.md` | The extension-folder `README.md` still has the scaffold template. Rewrite it for end users. |
| `ERROR: Missing publisher name` | Add `"publisher"` to `apps/extension/package.json`. |
| `ERROR: Icon file not found` | Path in `icon` field is wrong or the file doesn't exist. |
| `ERROR: Extension with name X already published` | Either a different publisher owns that name, or you have it under a different publisher ID. Either bump to a unique name or use your existing ID. |
| `ENOENT: spawn contextos` at extension runtime | The bundled CLI wasn't built for the user's platform. Rebuild for the target platform and re-publish with `vsce publish --target <platforms>`. |
| Silent fail after `vsce publish` | Check the Marketplace publisher email — rejection reasons arrive there, not in your terminal. |
| Extension doesn't appear in VS Code search for 10+ minutes | Marketplace indexing lag. Browse directly to `https://marketplace.visualstudio.com/items?itemName=…` to verify it's actually published. |
