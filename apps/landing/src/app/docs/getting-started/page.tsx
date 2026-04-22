import type { Metadata } from "next";
import { DocsPage } from "@/components/docs/DocsPage";

export const metadata: Metadata = { title: "Getting started" };

export default function GettingStarted() {
  return (
    <DocsPage
      kicker="Install"
      title="Getting started"
      lede="Three steps — install the extension, click Enable once, use your AI as usual."
      pathname="/docs/getting-started"
    >
      <h2>1. Install the extension</h2>
      <p>
        ContextOS ships as a VS Code extension with the Rust engine bundled
        inside. No separate install, no Docker, no background daemon to manage.
      </p>
      <ul>
        <li>
          <strong>VS Code:</strong> <code>⌘⇧X</code> →{" "}
          <em>search "ContextOS"</em> → <em>Install</em>.
        </li>
        <li>
          <strong>Cursor / VSCodium / Windsurf:</strong> install the same
          extension from Open VSX.
        </li>
      </ul>

      <h2>2. Open any project</h2>
      <p>
        On first activation per workspace you'll see a single consent dialog:
      </p>
      <blockquote>
        ContextOS will auto-configure Claude Code for this project (writes{" "}
        <code>.mcp.json</code>) and keep a local code graph in{" "}
        <code>.contextos/</code>. Continue?
      </blockquote>
      <p>
        Click <strong>Enable</strong>. ContextOS writes its MCP config into the
        project, indexes the codebase (typically 5–15 s for a mid-sized repo),
        and spawns a background watcher so the graph stays fresh as you edit.
      </p>

      <h2>3. Open Claude Code</h2>
      <p>
        Quit Claude Code completely and relaunch it inside the same project.
        Claude Code reads the fresh <code>.mcp.json</code> and connects to the
        ContextOS MCP server. Verify with <code>/mcp</code> in the chat — you
        should see <code>contextos</code> listed with 6 tools available.
      </p>

      <p>
        From here on, every AI request in that project transparently routes
        through the optimization pipeline. You don't have to invoke anything.
      </p>

      <h2>What gets created</h2>
      <ul>
        <li>
          <code>.mcp.json</code> — declares the MCP server for Claude Code.
        </li>
        <li>
          <code>.claude/settings.local.json</code> — opts Claude Code into it.
        </li>
        <li>
          <code>.contextos/graph.db</code> — SQLite graph of your codebase.
        </li>
      </ul>
      <p>
        All three live inside the project. Add <code>.contextos/</code> to{" "}
        <code>.gitignore</code> if you don't want the graph checked in (tracked
        graphs are fine; they just bloat the diff).
      </p>

      <h2>Removing ContextOS from a project</h2>
      <p>
        Run <strong>ContextOS: Remove from this Project</strong> from the VS
        Code command palette. It deletes only our entries from{" "}
        <code>.mcp.json</code> and <code>settings.local.json</code>, leaving any
        other MCP servers you've configured untouched.
      </p>
    </DocsPage>
  );
}
