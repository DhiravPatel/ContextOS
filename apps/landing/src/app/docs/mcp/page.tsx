import type { Metadata } from "next";
import { DocsPage } from "@/components/docs/DocsPage";

export const metadata: Metadata = { title: "MCP server" };

const TOOLS = [
  {
    name: "optimize",
    description:
      "Run the full pipeline (skeletonise → dedup → compress → rank → budget) on a batch of chunks. Input: chunks + optional query + max_tokens. Output: optimised chunks plus before/after token counts.",
  },
  {
    name: "build_graph",
    description:
      "Full repository reindex. Respects .gitignore via the ignore crate. Typically 5–15 s for a mid-size repo, once per install.",
  },
  {
    name: "update_graph",
    description:
      "Incremental refresh for a list of changed files. Hash-keyed — unchanged files are skipped.",
  },
  {
    name: "impact_radius",
    description:
      "Given a list of changed files, return the symbols (and files) affected via reverse BFS over calls/imports/inherits edges. The workhorse for change-review prompts.",
  },
  {
    name: "skeleton",
    description:
      "Signature-only projection of a single source file — declarations only, bodies collapsed. Use when Claude needs to know a symbol exists but not how it's implemented.",
  },
  {
    name: "graph_stats",
    description:
      "Node / edge / file counts in the current graph. Useful for debugging and sanity checks.",
  },
];

export default function Mcp() {
  return (
    <DocsPage
      kicker="Integration"
      title="MCP server"
      lede="ContextOS exposes its engine as a local JSON-RPC 2.0 server over stdio, following the Model Context Protocol. Claude Code calls it automatically on every AI turn."
      pathname="/docs/mcp"
    >
      <h2>Wire-up</h2>
      <p>
        On first activation, the extension writes two files in your project
        root:
      </p>
      <pre>
        <code>{`.mcp.json
{
  "mcpServers": {
    "contextos": {
      "type": "stdio",
      "command": "/path/to/contextos",
      "args": ["serve", "--root", "/path/to/repo"]
    }
  }
}

.claude/settings.local.json
{
  "enabledMcpjsonServers": ["contextos"]
}`}</code>
      </pre>
      <p>
        Claude Code reads both at launch, starts the <code>contextos serve</code>{" "}
        process, and keeps its stdio pipes open for the lifetime of the session.
      </p>

      <h2>The six tools</h2>
      <ul className="!list-none !p-0">
        {TOOLS.map((tool) => (
          <li
            key={tool.name}
            className="!my-3 rounded-xl border border-line bg-bg-elevated/60 px-5 py-4"
          >
            <code className="!bg-transparent !p-0 !text-base font-semibold text-accent">
              {tool.name}
            </code>
            <p className="!mt-2 text-sm text-fg-muted">{tool.description}</p>
          </li>
        ))}
      </ul>

      <h2>Verifying Claude Code picked it up</h2>
      <p>
        In a Claude Code session inside the project, run <code>/mcp</code>. You
        should see a block like:
      </p>
      <pre>
        <code>{`contextos
  transport: stdio
  tools: 6 (optimize, build_graph, update_graph, impact_radius, skeleton, graph_stats)`}</code>
      </pre>
      <p>
        If Claude Code doesn't show the server, the most common cause is that
        it was started before <code>.mcp.json</code> existed. Quit and relaunch.
      </p>

      <h2>Troubleshooting</h2>
      <ul>
        <li>
          <strong>
            <code>ENOENT: spawn contextos</code>
          </strong>{" "}
          — the binary path in <code>.mcp.json</code> is stale (after an
          extension update). Run <strong>ContextOS: Reconfigure</strong> to
          rewrite it.
        </li>
        <li>
          <strong>Tools list is empty</strong> — check VS Code → Output →
          ContextOS for engine errors. The MCP server logs everything through
          stderr.
        </li>
        <li>
          <strong>Graph is missing nodes</strong> — the graph builder skips
          languages it can't parse. Supported today: Rust, TypeScript,
          JavaScript, Python. Others are coming.
        </li>
      </ul>

      <h2>Running the server standalone</h2>
      <p>
        Useful for CI jobs or integrating with other MCP-speaking clients
        (Cursor, Windsurf, custom harnesses).
      </p>
      <pre>
        <code>{`contextos serve --root /path/to/repo`}</code>
      </pre>
      <p>
        The server reads JSON-RPC frames from stdin and writes responses to
        stdout, one object per line.
      </p>
    </DocsPage>
  );
}
