const VERSION = "5.0.1";
const SITE_URL = "https://aid.agent-tools.org";
const REPO_URL = "https://github.com/sunoj/ai-dispatch";
const META_DESCRIPTION = "Multi-AI CLI team orchestrator that dispatches work to gemini, codex, opencode, cursor, kilo, ob1, codebuff, auto, and custom agents defined via ~/.aid/agents/.";
const JSON_LD_DATA = JSON.stringify({
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "ai-dispatch (aid)",
  description: META_DESCRIPTION,
  applicationCategory: "DeveloperTool",
  operatingSystem: "Cross-platform",
  softwareVersion: VERSION,
  url: SITE_URL,
  developer: { name: "sunoj", url: REPO_URL },
  genre: "Productivity"
});
const COMMANDS = [
  { name: "run", purpose: "Dispatch task to an agent with optional bg, verify, worktree, on-done, retry, context, and skill flags.", example: "aid run codex \"Document the MCP server workflow\" --dir . --worktree docs/mcp-readme --verify auto" },
  { name: "batch", purpose: "Run a TOML batch file with DAG dependency scheduling.", example: "aid batch tasks.toml --parallel --wait" },
  { name: "watch", purpose: "Follow live progress in text, quiet, or TUI mode.", example: "aid watch --tui" },
  { name: "board", purpose: "List tracked tasks with filters and zombie detection.", example: "aid board --today" },
  { name: "show", purpose: "Inspect task summary, diff, output, log, or AI explanation.", example: "aid show t-1234 --diff" },
  { name: "usage", purpose: "View task history usage plus configured budget windows.", example: "aid usage --today" },
  { name: "retry", purpose: "Re-dispatch a failed task with feedback.", example: "aid retry t-1234 --feedback \"Tighten the configuration example\"" },
  { name: "respond", purpose: "Send interactive input to a running background task.", example: "aid respond t-1234 \"Please rerun with logging enabled\"" },
  { name: "benchmark", purpose: "Compare the same task across multiple agents.", example: "aid benchmark --agents codex,cursor \"Implement new parsing\"" },
  { name: "output", purpose: "Show task output directly without additional metadata.", example: "aid output t-1234" },
  { name: "ask", purpose: "Quick research or exploration task.", example: "aid ask \"How does the retry flow work in this repo?\"" },
  { name: "mcp", purpose: "Start the stdio MCP server for Claude Code or other MCP clients.", example: "aid mcp" },
  { name: "merge", purpose: "Mark done tasks as merged or perform bulk workgroup merges.", example: "aid merge --group wg-a3f1" },
  { name: "clean", purpose: "Remove old tasks, orphaned worktrees, and logs.", example: "aid clean --days 30" },
  { name: "config", purpose: "Inspect agent profiles, skills, pricing, and webhook settings.", example: "aid config agents" },
  { name: "worktree", purpose: "Manage worktree lifecycle (create/list/remove).", example: "aid worktree create --dir feat/parser" },
  { name: "group", purpose: "Workgroup CRUD with shared context and constraints.", example: "aid group create dispatch --context \"Docs only, cite sources\"" },
  { name: "init", purpose: "Initialize default skills and templates for a fresh project.", example: "aid init" }
];
const AGENT_CATEGORIES = ["Research", "Simple Edit", "Complex Impl", "Frontend", "Debugging", "Testing", "Refactoring", "Documentation"] as const;
const AGENT_MATRIX: Record<string, Record<string, number>> = {
  gemini: { Research: 9, "Simple Edit": 2, "Complex Impl": 3, Frontend: 2, Debugging: 5, Testing: 3, Refactoring: 3, Documentation: 6 },
  codex: { Research: 1, "Simple Edit": 4, "Complex Impl": 9, Frontend: 4, Debugging: 7, Testing: 7, Refactoring: 8, Documentation: 3 },
  opencode: { Research: 1, "Simple Edit": 8, "Complex Impl": 3, Frontend: 2, Debugging: 4, Testing: 4, Refactoring: 4, Documentation: 5 },
  kilo: { Research: 1, "Simple Edit": 7, "Complex Impl": 2, Frontend: 2, Debugging: 3, Testing: 3, Refactoring: 3, Documentation: 4 },
  cursor: { Research: 2, "Simple Edit": 4, "Complex Impl": 7, Frontend: 9, Debugging: 5, Testing: 5, Refactoring: 6, Documentation: 4 },
  ob1: { Research: 5, "Simple Edit": 3, "Complex Impl": 5, Frontend: 3, Debugging: 4, Testing: 4, Refactoring: 4, Documentation: 3 },
  codebuff: { Research: 2, "Simple Edit": 5, "Complex Impl": 8, Frontend: 7, Debugging: 6, Testing: 6, Refactoring: 7, Documentation: 4 }
};
const MCP_TOOLS = ["aid_run", "aid_board", "aid_show", "aid_retry", "aid_usage", "aid_ask"];
async function fetchReadme(): Promise<string> {
  try {
    const res = await fetch("https://raw.githubusercontent.com/sunoj/ai-dispatch/main/README.md");
    if (!res.ok) return buildLLMSText() + "\n\n(Full README unavailable — see https://github.com/sunoj/ai-dispatch)";
    return res.text();
  } catch {
    return buildLLMSText() + "\n\n(Full README unavailable — see https://github.com/sunoj/ai-dispatch)";
  }
}
function baseHeaders(contentType: string) {
  const headers = new Headers();
  headers.set("Cache-Control", "public, max-age=3600");
  headers.set("Content-Type", contentType);
  return headers;
}
function respondText(text: string, contentType: string, status = 200) {
  return new Response(text, { status, headers: baseHeaders(contentType) });
}
function respondJSON(payload: unknown, status = 200) {
  return new Response(JSON.stringify(payload), { status, headers: baseHeaders("application/json; charset=utf-8") });
}
function buildLLMSText() {
  const lines = [] as string[];
  lines.push(`Name: ai-dispatch (aid)`);
  lines.push(`Description: ${META_DESCRIPTION}`);
  lines.push(`Homepage: ${SITE_URL}`);
  lines.push(`Version: ${VERSION}`);
  lines.push(``);
  lines.push(`## Why aid?`);
  lines.push(`- Multiple AI CLIs use different flags, output formats, and conventions, making coordination fragile.`);
  lines.push(`- Background progress visibility is missing without a centralized watcher.`);
  lines.push(`- Cost tracking across agents is hard, so budgets and spend drift without enforcement.`);
  lines.push(`- Manual worktree juggling slows parallel task execution.`);
  lines.push(`- Methodology and testing discipline drift when every agent improvises its own process.`);
  lines.push(``);
  lines.push(`## Custom Agents`);
  lines.push(`v5.0 lets you define custom agents in ~/.aid/agents/ so any CLI or workflow wrapper can join the orchestrator.`);
  lines.push(`Example definition:`);
  lines.push(`\`\`\`toml`);
  lines.push(`[agent]`);
  lines.push(`name = "ops-bot"`);
  lines.push(`command = "ops-agent --task ${AID_TASK_ID}"`);
  lines.push(`timeout = "45m"`);
  lines.push(``);
  lines.push(`[agent.capabilities]`);
  lines.push(`Research = 6`);
  lines.push(`Frontend = 4`);
  lines.push(`\`\`\``);
  lines.push(``);
  lines.push(`## Skills`);
  lines.push(`Methodology files under ~/.aid/skills/ (code-scout, implementer, researcher, test-writer, debugger, etc.) inject repeatable behavior per agent and can be extended with --skill or disabled with --no-skill.`);
  lines.push(``);
  lines.push(`## Workgroups`);
  lines.push(`Shared context containers created via aid group create keep prompts, constraints, and notes in sync across multiple tasks.`);
  lines.push(`Use --group or set AID_GROUP so watch, board, run, and merge commands automatically stay scoped to the same workspace.`);
  lines.push(``);
  lines.push(`## Worktree Management`);
  lines.push(`aid auto-creates per-task worktrees when you pass --worktree, or manage them explicitly with aid worktree create/list/remove.`);
  lines.push(`aid merge auto-merges the worktree branch, runs pre-merge verification, auto-commits uncommitted work, and flags escape detection.`);
  lines.push(``);
  lines.push(`## TUI`);
  lines.push(`aid watch --tui opens a dashboard; press s to open the stats view and a to toggle between today-only and all-time task data.`);
  lines.push(``);
  lines.push(`## Reliability`);
  lines.push(`- Zombie detection, max-duration enforcement, and auto-commit protect background worktrees.`);
  lines.push(`- Shared \`CARGO_TARGET_DIR\` avoids redundant Rust builds when tasks run in parallel.`);
  lines.push(`- Worktree escape detection warns if an agent mutates the main repo.`);
  lines.push(`- SQLite busy_timeout, fallback agent suggestions, and retry worktrees keep parallel dispatches healthy.`);
  lines.push(``);
  lines.push(`## Best Practices`);
  lines.push(`1. Plan the work.`);
  lines.push(`2. Dispatch specialized agents.`);
  lines.push(`3. Monitor progress with watch/board and review artifacts.`);
  lines.push(`4. Iterate with retry or follow-up tasks until verification passes.`);
  lines.push(`Cost tips: use aid ask or gemini for research, prefer opencode for single-file edits, reuse workgroups, set budgets, and run low-value work with --budget.`);
  lines.push(``);
  lines.push(`## Quick Start`);
  lines.push(`1. Install Rust 1.85+ and the CLI agents (gemini, codex, opencode, cursor, kilo, ob1, codebuff, auto).`);
  lines.push(`2. Run \`cargo install --path .\`, then \`aid config agents\` and \`aid config skills\`.`);
  lines.push(`3. Optionally append \`claude-prompt.md\` to your CLAUDE.md and set \`AID_HOME\` for sandboxed runs.`);
  lines.push(`4. Dispatch tasks with \`aid run\`, monitor with \`aid watch\`, inspect with \`aid show\`, and retry via \`aid retry\`.`);
  lines.push(``);
  lines.push(`## Commands`);
  COMMANDS.forEach((cmd) => {
    lines.push(`- ${cmd.name}: ${cmd.purpose}`);
  });
  lines.push(``);
  lines.push(`## Agent Capability Matrix (0-10)`);
  Object.entries(AGENT_MATRIX).forEach(([agent, scores]) => {
    const summary = AGENT_CATEGORIES.map((cat) => `${cat}=${scores[cat]}`).join(", ");
    lines.push(`- ${agent}: ${summary}`);
  });
  lines.push(``);
  lines.push(`## Installation`);
  lines.push(`1. Install Rust 1.85 or later (Edition 2024).`);
  lines.push(`2. Ensure the agent CLIs are on your PATH (gemini, codex, opencode, cursor, kilo, ob1, codebuff).`);
  lines.push(`3. Run \`cargo install --path .\` and use \`aid config agents\` / \`aid config skills\`.`);
  lines.push(``);
  lines.push(`## Architecture`);
  lines.push(`\`\`\`text`);
  lines.push(`┌─────────────────────────────────────┐`);
  lines.push(`│           aid (CLI binary)          │`);
  lines.push(`├──────┬──────┬──────┬───────┬────────┬───────────┤`);
  lines.push(`│ run  │ watch│ show │ board │ usage  │ benchmark │  ← user-facing commands`);
  lines.push(`├──────┴──────┴──────┴───────┴────────┤`);
  lines.push(`│           Task Manager              │`);
  lines.push(`│  ┌────────┐ ┌────────┐ ┌────────┐  │`);
  lines.push(`│  │Classif.│ │ Watch  │ │ Store  │  │`);
  lines.push(`│  │+ Agent │ │ Engine │ │(SQLite)│  │`);
  lines.push(`│  │Registry│ │        │ │        │  │`);
  lines.push(`│  └────┬───┘ └────┬───┘ └────┬───┘  │`);
  lines.push(`│       │          │          │       │`);
  lines.push(`├───────┴──────────┴──────────┴───────┤`);
  lines.push(`│         Agent Adapters              │`);
  lines.push(`│  ┌──────┐ ┌─────┐ ┌────────┐ ┌──────┐ ┌────┐ ┌───┐ ┌────────┐`);
  lines.push(`│  │Gemini│ │Codex│ │OpenCode│ │Cursor│ │Kilo│ │OB1│ │Codebuff│`);
  lines.push(`│  └──────┘ └─────┘ └────────┘ └──────┘ └────┘ └───┘ └────────┘`);
  lines.push(`└─────────────────────────────────────┘`);
  lines.push(`\`\`\``);
  lines.push(``);
  lines.push(`## MCP Integration`);
  lines.push(`Start the server with \`aid mcp\` to expose MCP tools: ${MCP_TOOLS.join(", ")}.`);
  lines.push(`Configure Claude Code by registering a stdio MCP server that runs \`aid mcp\` or \`cargo run -- mcp\`.`);
  lines.push(`Once connected, call \`aid_run\`, \`aid_board\`, \`aid_show\`, \`aid_retry\`, \`aid_usage\`, and \`aid_ask\` directly.`);
  lines.push(``);
  lines.push(`## Documentation`);
  lines.push(`Full docs: ${SITE_URL}/llms-full.txt`);
  return lines.join("\n");
}
function buildHTML() {
  const commandsRows = COMMANDS.map((cmd) => `<tr><td>${cmd.name}</td><td>${cmd.purpose}</td><td>${cmd.example}</td></tr>`).join("");
  const agentHeader = AGENT_CATEGORIES.map((cat) => `<th>${cat}</th>`).join("");
  const agentRows = Object.entries(AGENT_MATRIX)
    .map(([agent, scores]) => {
      const cells = AGENT_CATEGORIES.map((cat) => `<td>${scores[cat]}</td>`).join("");
      return `<tr><th>${agent}</th>${cells}</tr>`;
    })
    .join("");
  const quickList = [
    "Install Rust 1.85+ and agent CLIs (gemini, codex, opencode, cursor, kilo, ob1, codebuff, auto).",
    "Run `cargo install --path .` then `aid config agents` and `aid config skills`.",
    "Use `aid run` for tasks, `aid watch` for progress, `aid board` to inspect the queue, `aid show` to review artifacts, and `aid retry` to iterate.",
    "For MCP workflows, start `aid mcp` and call MCP tools from another client."
  ].map((item) => `<li>${item}</li>`).join("");
  const mcpList = MCP_TOOLS.map((tool) => `<li>${tool}</li>`).join("");
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>aid | ai-dispatch</title>
  <meta name="description" content="${META_DESCRIPTION}">
  <meta property="og:title" content="aid / ai-dispatch">
  <meta property="og:description" content="${META_DESCRIPTION}">
  <link rel="alternate" type="text/plain" href="/llms.txt">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>body{margin:0;font-family:"IBM Plex Mono",SFMono-Regular,Menlo,monospace;background:#05070a;color:#e2e8f0;min-height:100vh;}header,section,footer{padding:1.5rem 4vw;}header{border-bottom:1px solid rgba(226,232,240,.2);}h1{margin:0;font-size:2em;}p{margin:0.4rem 0;}main{display:flex;flex-direction:column;gap:1.2rem;}section{background:#0c111d;border-radius:12px;border:1px solid rgba(226,232,240,.1);}table{width:100%;border-collapse:collapse;font-size:0.95rem;}th,td{border:1px solid rgba(226,232,240,.15);padding:0.5rem;text-align:left;}th{background:#11182b;}ul{margin:0;padding-left:1.5rem;}footer{border-top:1px solid rgba(226,232,240,.2);font-size:0.9rem;display:flex;flex-wrap:wrap;gap:1rem;}</style>
  <script type="application/ld+json">${JSON_LD_DATA}</script>
</head>
<body>
  <header>
    <p style="font-size:.85rem;letter-spacing:.2rem;text-transform:uppercase;color:#94a3b8;">aid.agent-tools.org</p>
    <h1>aid (ai-dispatch)</h1>
    <p>${META_DESCRIPTION}</p>
    <p style="font-size:.9rem;color:#94a3b8;">Version ${VERSION} · <a href="${REPO_URL}" style="color:#7dd3fc;text-decoration:none;">${REPO_URL}</a></p>
  </header>
  <main>
    <section>
      <h2>What it is</h2>
      <p>Multi-AI CLI orchestrator that dispatches work to gemini, codex, opencode, cursor, kilo, ob1, codebuff, auto, and custom agents defined via ~/.aid/agents/ while tracking progress, enforcing methodology, and tracking cost.</p>
    </section>
    <section>
      <h2>Why aid?</h2>
      <ul>
        <li>Managing multiple AI CLIs is chaotic because every tool has different flags, output formats, and calling conventions.</li>
        <li>No unified progress visibility leaves background work blind until completion.</li>
        <li>Cost tracking across agents is manual, so budgets and tokens drift.</li>
        <li>Manual worktree juggling slows parallel tasks.</li>
        <li>Without shared methodology, prompt discipline and testing drift between agents.</li>
      </ul>
    </section>
    <section>
      <h2>Custom Agents</h2>
      <p>v5.0 lets you define custom agents via TOML files under ~/.aid/agents/, so wrappers and legacy CLIs can join the orchestrator.</p>
      <pre style="background:#040b16;padding:1rem;border-radius:8px;margin:0;"><code>[agent]&#10;name = "ops-bot"&#10;command = "ops-agent --task ${AID_TASK_ID}"&#10;timeout = "45m"&#10;&#10;[agent.capabilities]&#10;Research = 6&#10;Frontend = 4</code></pre>
    </section>
    <section>
      <h2>Skills</h2>
      <p>Methodology files under ~/.aid/skills/ (code-scout, implementer, researcher, test-writer, debugger, etc.) inject repeatable behavior per agent; use --skill to add extras or --no-skill to opt out.</p>
    </section>
    <section>
      <h2>Workgroups</h2>
      <p>Workgroups share context and constraints. Create one with <code>aid group create</code> and scope tasks via <code>--group</code> or the <code>AID_GROUP</code> environment variable.</p>
    </section>
    <section>
      <h2>Worktree Management</h2>
      <p>aid auto-creates per-task worktrees when you pass <code>--worktree</code>, and <code>aid worktree create/list/remove</code> manage them explicitly. <code>aid merge</code> auto-merges branches, auto-commits leftovers, and warns when escape detection triggers.</p>
    </section>
    <section>
      <h2>TUI</h2>
      <p><code>aid watch --tui</code> opens the dashboard; press <code>s</code> to view stats and <code>a</code> to toggle today-only vs all-time timelines.</p>
    </section>
    <section>
      <h2>Reliability</h2>
      <ul>
        <li>Zombie detection, max-duration enforcement, and auto-commit keep background worktrees healthy.</li>
        <li>Zombie recovery preserves artifacts before failing a task.</li>
        <li>Shared <code>CARGO_TARGET_DIR</code> avoids redundant Rust compilations.</li>
        <li>Worktree escape detection warns if an agent mutates the main repo.</li>
        <li>Auto merge on <code>aid merge</code> plus SQLite busy_timeout and the fallback agent chain keep concurrency stable.</li>
      </ul>
    </section>
    <section>
      <h2>Best Practices</h2>
      <p>Follow the orchestrator pattern: plan the work, dispatch specialized agents, monitor progress, review artifacts, and iterate with retries or follow-up tasks.</p>
      <ul>
        <li>Use <code>aid ask</code> or <code>gemini</code> for research and low-cost exploration.</li>
        <li>Prefer <code>opencode</code> for straightforward single-file edits.</li>
        <li>Reuse workgroups for shared context and budgets.</li>
        <li>Set [[usage.budget]] entries and run low-value work with <code>--budget</code>.</li>
      </ul>
    </section>
    <section>
      <h2>Install</h2>
      <p>Rust 1.85+ is required. Install the binary with <code>cargo install --path .</code> and bootstrap agent/skill configs with <code>aid config agents</code> and <code>aid config skills</code>.</p>
    </section>
    <section>
      <h2>Quick start</h2>
      <ul>${quickList}</ul>
    </section>
    <section>
      <h2>Commands</h2>
      <div style="overflow-x:auto;">
        <table>
          <thead>
            <tr><th>Command</th><th>Purpose</th><th>Example</th></tr>
          </thead>
          <tbody>${commandsRows}</tbody>
        </table>
      </div>
    </section>
    <section>
      <h2>Agent capability matrix</h2>
      <div style="overflow-x:auto;">
        <table>
          <thead>
            <tr><th>Agent</th>${agentHeader}</tr>
          </thead>
          <tbody>${agentRows}</tbody>
        </table>
      </div>
    </section>
    <section>
      <h2>Architecture</h2>
      <pre style="background:#040b16;padding:1rem;border-radius:8px;margin:0;font-family:inherit;"><code>┌─────────────────────────────────────┐&#10;│           aid (CLI binary)          │&#10;├──────┬──────┬──────┬───────┬────────┬───────────┤&#10;│ run  │ watch│ show │ board │ usage  │ benchmark │  ← user-facing commands&#10;├──────┴──────┴──────┴───────┴────────┤&#10;│           Task Manager              │&#10;│  ┌────────┐ ┌────────┐ ┌────────┐  │&#10;│  │Classif.│ │ Watch  │ │ Store  │  │&#10;│  │+ Agent │ │ Engine │ │(SQLite)│  │&#10;│  │Registry│ │        │ │        │  │&#10;│  └────┬───┘ └────┬───┘ └────┬───┘  │&#10;│       │          │          │       │&#10;├───────┴──────────┴──────────┴───────┤&#10;│         Agent Adapters              │&#10;│  ┌──────┐ ┌─────┐ ┌────────┐ ┌──────┐ ┌────┐ ┌───┐ ┌────────┐&#10;│  │Gemini│ │Codex│ │OpenCode│ │Cursor│ │Kilo│ │OB1│ │Codebuff│&#10;│  └──────┘ └─────┘ └────────┘ └──────┘ └────┘ └───┘ └────────┘&#10;└─────────────────────────────────────┘</code></pre>
    </section>
    <section>
      <h2>MCP integration</h2>
      <p>Start <code>aid mcp</code> to expose the following stdio MCP tools:</p>
      <ul>${mcpList}</ul>
      <p>Register the server in your Claude Code MCP config to call <code>aid_run</code>, <code>aid_board</code>, <code>aid_show</code>, <code>aid_retry</code>, <code>aid_usage</code>, and <code>aid_ask</code> without shell parsing.</p>
    </section>
  </main>
  <footer>
    <a href="/llms.txt" style="color:#7dd3fc;text-decoration:none;">/llms.txt</a>
    <a href="/llms-full.txt" style="color:#7dd3fc;text-decoration:none;">/llms-full.txt</a>
    <span>Full docs: <a href="${SITE_URL}/llms-full.txt" style="color:#7dd3fc;text-decoration:none;">${SITE_URL}/llms-full.txt</a></span>
  </footer>
</body>
</html>`;
}
function notFound() {
  return respondText("Not found", "text/plain; charset=utf-8", 404);
}
function methodNotAllowed() {
  const headers = baseHeaders("text/plain; charset=utf-8");
  headers.set("Allow", "GET");
  return new Response("Method not allowed", { status: 405, headers });
}
async function handleLLMSFull(request: Request) {
  const text = await fetchReadme();
  return respondText(text, "text/plain; charset=utf-8");
}
function buildRobots() {
  return [
    "User-agent: *",
    "Allow: /",
    "",
    "User-agent: GPTBot",
    "Allow: /",
    "",
    "User-agent: ClaudeBot",
    "Allow: /",
    "",
    "User-agent: anthropic-ai",
    "Allow: /"].join("\n");
}
function buildPluginManifest() {
  return {
    schema_version: "v1",
    name_for_model: "aid",
    name_for_human: "aid agent dispatcher",
    description_for_model: META_DESCRIPTION,
    description_for_human: "Multi-AI CLI team orchestrator built on ai-dispatch.",
    api: { type: "none" },
    logo_url: "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA2NCA2NCIgPjxyZWN0IHdpZHRoPSI2NCIgaGVpZ2h0PSI2NCIgZmlsbD0iIzBmMTcyYSIvPjx0ZXh0IHg9IjUwJSIgeT0iNTAlIiBmaWxsPSIjZTJlOGYwIiBmb250LXNpemU9IjI4IiBmb250LWZhbWlseT0ibW9ub3NwYWNlIiB0ZXh0LWFuY2hvcj0ibWlkZGxlIiBkb21pbmFudC1iYXNlbGluZT0iY2VudHJhbCI+YWlkPC90ZXh0Pjwvc3ZnPg==",
    legal_info_url: `${REPO_URL}/blob/main/LICENSE`
  };
}
async function handleRequest(request: Request) {
  if (request.method !== "GET") return methodNotAllowed();
  const url = new URL(request.url);
  switch (url.pathname) {
    case "/":
      return respondText(buildHTML(), "text/html; charset=utf-8");
    case "/llms.txt":
      return respondText(buildLLMSText(), "text/plain; charset=utf-8");
    case "/llms-full.txt":
      return handleLLMSFull(request);
    case "/api/info":
      return respondJSON({
        name: "aid",
        version: VERSION,
        description: META_DESCRIPTION,
        repository: REPO_URL,
        license: "MIT",
        install: "cargo install --path .",
        agents: Object.keys(AGENT_MATRIX),
        commands: COMMANDS.map((cmd) => ({ name: cmd.name, purpose: cmd.purpose }))
      });
    case "/api/commands":
      return respondJSON(COMMANDS.map((cmd) => ({ name: cmd.name, purpose: cmd.purpose, example: cmd.example })));
    case "/api/agents":
      return respondJSON(AGENT_MATRIX);
    case "/robots.txt":
      return respondText(buildRobots(), "text/plain; charset=utf-8");
    case "/.well-known/ai-plugin.json":
      return respondJSON(buildPluginManifest());
    default:
      return notFound();
  }
}
export default {
  async fetch(request: Request) {
    return handleRequest(request);
  }
};
