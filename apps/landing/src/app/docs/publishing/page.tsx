import type { Metadata } from "next";
import { DocsPage } from "@/components/docs/DocsPage";

export const metadata: Metadata = { title: "Publishing" };

export default function Publishing() {
  return (
    <DocsPage
      kicker="Operations"
      title="Publishing ContextOS"
      lede="How to cut a release and ship to the VS Code Marketplace and Open VSX. Only the maintainers need this page."
      pathname="/docs/publishing"
    >
      <h2>Pre-flight checklist</h2>
      <ul>
        <li>
          <code>cargo test --workspace</code> green
        </li>
        <li>
          <code>npm --workspace apps/extension run compile</code> clean
        </li>
        <li>
          <code>CHANGELOG.md</code> has an entry for the new version
        </li>
        <li>
          Per-platform CLI binaries staged under{" "}
          <code>apps/extension/bin/&lt;platform&gt;/contextos</code>
        </li>
        <li>
          Smoke-tested the packaged <code>.vsix</code> in a fresh VS Code window
        </li>
        <li>
          Version bumped in <em>both</em> <code>Cargo.toml</code> and{" "}
          <code>apps/extension/package.json</code>
        </li>
      </ul>

      <h2>Build the Rust CLI for every platform</h2>
      <pre>
        <code>{`# Apple Silicon + Intel universal
cargo build --release --target aarch64-apple-darwin --bin contextos
cargo build --release --target x86_64-apple-darwin  --bin contextos
lipo -create \\
  target/aarch64-apple-darwin/release/contextos \\
  target/x86_64-apple-darwin/release/contextos \\
  -output apps/extension/bin/darwin/contextos

# Linux x86_64 (musl for portability)
cargo build --release --target x86_64-unknown-linux-musl --bin contextos
cp target/x86_64-unknown-linux-musl/release/contextos apps/extension/bin/linux/contextos

# Windows x86_64
cargo build --release --target x86_64-pc-windows-msvc --bin contextos
cp target/x86_64-pc-windows-msvc/release/contextos.exe apps/extension/bin/win32/contextos.exe`}</code>
      </pre>

      <h2>Package and smoke-test</h2>
      <pre>
        <code>{`cd apps/extension
npm ci --no-audit --no-fund
npm run compile
npx vsce package --no-dependencies       # → contextos-vscode-<version>.vsix
code --install-extension contextos-vscode-<version>.vsix`}</code>
      </pre>
      <p>
        Open a fresh project — the consent dialog should fire, watch should
        start, <code>.mcp.json</code> should appear.
      </p>

      <h2>Publish</h2>
      <pre>
        <code>{`export VSCE_PAT="..."                   # from dev.azure.com (Marketplace/Manage scope)
export OVSX_PAT="..."                   # from open-vsx.org

cd apps/extension
npx vsce publish --target darwin-arm64,darwin-x64,linux-x64,linux-arm64,win32-x64
npx ovsx publish *.vsix`}</code>
      </pre>
      <p>
        First publish propagates in 1–3 minutes. The Marketplace emails the
        publisher account on success / failure.
      </p>

      <h2>Verify</h2>
      <ul>
        <li>
          <code>
            https://marketplace.visualstudio.com/items?itemName=&lt;publisher&gt;.contextos-vscode
          </code>{" "}
          renders with the new version, icon, and README.
        </li>
        <li>Search "ContextOS" inside VS Code → extension appears within ~5 min.</li>
        <li>
          <code>
            https://open-vsx.org/extension/&lt;publisher&gt;/contextos-vscode
          </code>{" "}
          shows the matching version.
        </li>
      </ul>

      <h2>Semver</h2>
      <ul>
        <li>
          <strong>patch</strong> — bug fixes only.
        </li>
        <li>
          <strong>minor</strong> — new command / new setting / new MCP tool —
          backward compatible.
        </li>
        <li>
          <strong>major</strong> — breaking change (e.g.{" "}
          <code>.mcp.json</code> schema changes, removed command).
        </li>
      </ul>
      <p>
        Keep <code>Cargo.toml</code> (workspace version) in lockstep with{" "}
        <code>apps/extension/package.json</code> — the CLI version the extension
        spawns must match the extension's version so bundled-binary drift
        doesn't silently break things.
      </p>
    </DocsPage>
  );
}
