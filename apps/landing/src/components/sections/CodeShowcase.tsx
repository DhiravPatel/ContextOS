import { Terminal } from "lucide-react";

const INSTALL_LINE = "code --install-extension DhiravPatel.contextos-vscode";

const BUILD_LINES = [
  { prompt: true, text: "contextos build --root ." },
  { prompt: false, text: "build: scanned=342 reparsed=342 nodes=1820 edges=4117" },
  { prompt: true, text: "git diff --name-only | contextos impact --root ." },
  { prompt: false, text: "src/auth.ts" },
  { prompt: false, text: "src/login.controller.ts" },
  { prompt: false, text: "src/middleware/session.ts" },
  { prompt: false, text: "src/routes/auth.test.ts" },
  { prompt: false, text: "# 4 files impacted (was 342)" },
];

export function CodeShowcase() {
  return (
    <section className="container-tight py-24 md:py-32">
      <div className="grid items-center gap-10 md:grid-cols-2 md:gap-16">
        <div>
          <p className="text-xs font-semibold uppercase tracking-widest text-accent">
            Zero-touch
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight md:text-4xl">
            Install once. Forget it exists.
          </h2>
          <p className="mt-4 text-base leading-relaxed text-fg-muted">
            ContextOS auto-detects Claude Code and wires itself up as an MCP
            server on first run. Your AI assistant silently routes every request
            through the optimization pipeline — no chat commands, no pasted
            prompts, no config editing.
          </p>
          <ul className="mt-6 space-y-3 text-sm text-fg-muted">
            {[
              "One-time consent dialog, remembered across workspaces",
              "Auto-starts watch mode — graph stays fresh as you edit",
              "Removes itself cleanly from any project with one command",
              "All state lives inside the project, nothing in your home dir",
            ].map((item) => (
              <li key={item} className="flex items-start gap-3">
                <span
                  aria-hidden
                  className="mt-1.5 inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-accent"
                />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>

        <div className="soft-border overflow-hidden p-0">
          <div className="flex items-center gap-2 border-b border-line bg-bg px-4 py-3">
            <div className="flex gap-1.5">
              <span className="h-2.5 w-2.5 rounded-full bg-[#ff5f57]" />
              <span className="h-2.5 w-2.5 rounded-full bg-[#febc2e]" />
              <span className="h-2.5 w-2.5 rounded-full bg-[#28c840]" />
            </div>
            <div className="ml-3 flex items-center gap-2 text-xs text-fg-subtle">
              <Terminal size={12} />
              contextos — zsh
            </div>
          </div>
          <pre className="m-0 overflow-x-auto px-5 py-4 font-mono text-sm leading-relaxed">
            <code>
              <span className="text-accent">$</span>{" "}
              <span className="text-fg">{INSTALL_LINE}</span>
              {"\n\n"}
              {BUILD_LINES.map((line, i) => (
                <span key={i}>
                  {line.prompt && <span className="text-accent">$ </span>}
                  <span
                    className={line.prompt ? "text-fg" : "text-fg-muted"}
                  >
                    {line.text}
                  </span>
                  {"\n"}
                </span>
              ))}
            </code>
          </pre>
        </div>
      </div>
    </section>
  );
}
