// aid-website — Cloudflare Worker for aid.agent-tools.org
// Serves: HTML landing page, /llms.txt, /llms-full.txt, /install.sh, /api/*

const VERSION = "8.6.0";
const SITE_URL = "https://aid.agent-tools.org";
const REPO_URL = "https://github.com/agent-tools-org/ai-dispatch";
const META_DESCRIPTION =
  "Multi-AI CLI team orchestrator that dispatches work to gemini, codex, opencode, cursor, kilo, codebuff, auto, and custom agents defined via ~/.aid/agents/.";

const JSON_LD_DATA = JSON.stringify({
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "ai-dispatch (aid)",
  description: META_DESCRIPTION,
  applicationCategory: "DeveloperTool",
  operatingSystem: "Cross-platform",
  softwareVersion: VERSION,
  url: SITE_URL,
  developer: { name: "agent-tools-org", url: REPO_URL },
  genre: "Productivity",
});

interface Command {
  name: string;
  purpose: string;
  example: string;
}

const COMMANDS: Command[] = [
  { name: "run", purpose: "Dispatch task to an agent with optional bg, verify, worktree, on-done, retry, context, skill, --id, and --scope flags.", example: 'aid run codex "Document the MCP server workflow" --dir . --worktree docs/mcp-readme --verify auto --id doc-task' },
  { name: "batch", purpose: "Run a TOML batch file with DAG dependency scheduling.", example: "aid batch tasks.toml --parallel --wait" },
  { name: "watch", purpose: "Follow live progress in text, quiet, or TUI mode.", example: "aid watch --tui" },
  { name: "board", purpose: "List tracked tasks with filters and zombie detection.", example: "aid board --today" },
  { name: "show", purpose: "Inspect task summary, diff, output, log, or AI explanation.", example: "aid show t-1234 --diff" },
  { name: "usage", purpose: "View task history, per-agent analytics, and budget windows.", example: "aid usage --agent codex --period 7d" },
  { name: "retry", purpose: "Re-dispatch a failed task with feedback, optionally switching agent or dir.", example: 'aid retry t-1234 -f "Fix the compilation error" --agent opencode' },
  { name: "respond", purpose: "Send interactive input to a running background task.", example: 'aid respond t-1234 "Please rerun with logging enabled"' },
  { name: "stop", purpose: "Gracefully stop a running task (SIGTERM + 5s wait + SIGKILL).", example: "aid stop t-1234" },
  { name: "kill", purpose: "Force-kill a running task immediately (SIGKILL).", example: "aid kill t-1234" },
  { name: "steer", purpose: "Inject guidance into a running PTY task mid-flight.", example: 'aid steer t-1234 "Switch to approach B instead"' },
  { name: "benchmark", purpose: "Compare the same task across multiple agents.", example: 'aid benchmark --agents codex,cursor "Implement new parsing"' },
  { name: "output", purpose: "Show task output directly without additional metadata.", example: "aid output t-1234" },
  { name: "ask", purpose: "Quick research or exploration task.", example: 'aid ask "How does the retry flow work in this repo?"' },
  { name: "mcp", purpose: "Start the stdio MCP server for Claude Code or other MCP clients.", example: "aid mcp" },
  { name: "merge", purpose: "Mark done tasks as merged or perform bulk workgroup merges.", example: "aid merge --group wg-a3f1" },
  { name: "clean", purpose: "Remove old tasks, orphaned worktrees, and logs.", example: "aid clean --days 30" },
  { name: "agent", purpose: "Manage custom agent definitions: list, show, add, remove, fork.", example: "aid agent fork codex --as codex-fast" },
  { name: "config", purpose: "Inspect agent profiles, skills, pricing, prompt token budget.", example: "aid config prompt-budget" },
  { name: "worktree", purpose: "Manage worktree lifecycle (create/list/remove).", example: "aid worktree create --dir feat/parser" },
  { name: "group", purpose: "Workgroup CRUD with shared context and constraints. Supports --id for custom group IDs.", example: 'aid group create dispatch --context "Docs only" --id my-wg' },
  { name: "init", purpose: "Initialize default skills and templates for a fresh project.", example: "aid init" },
  { name: "store", purpose: "Browse, install (with version pinning), and update community agent/skill packages.", example: "aid store install community/aider@1.0.0" },
  { name: "upgrade", purpose: "Upgrade aid to latest version from crates.io (checks for running tasks).", example: "aid upgrade" },
  { name: "memory", purpose: "Manage project-scoped agent memory (discoveries, conventions, lessons, facts).", example: 'aid memory add discovery "Auth uses bcrypt not argon2"' },
  { name: "finding", purpose: "Post or list workgroup findings for shared investigation evidence.", example: 'aid finding add wg-abc1 "gamma can be zero in tricrypto"' },
  { name: "tree", purpose: "Show retry chain as an ASCII tree.", example: "aid tree t-1234" },
  { name: "summary", purpose: "Summarize workgroup results with milestones, findings, and costs.", example: "aid summary wg-abc1" },
  { name: "export", purpose: "Export a task with full context in markdown or JSON.", example: "aid export t-1234 --format json" },
  { name: "query", purpose: "Fast LLM query via OpenRouter (no agent startup). Free and auto tiers.", example: 'aid query "question", aid query --auto "question"' },
  { name: "setup", purpose: "Interactive configuration wizard. Detects agents, sets API keys.", example: "aid setup" },
  { name: "broadcast", purpose: "Send a message to a workgroup's broadcast channel.", example: 'aid broadcast wg-abc1 "update"' },
  { name: "team", purpose: "Manage teams with knowledge context, rules, and agent preferences.", example: "aid team list, aid team show dev, aid team create dev" },
  { name: "project", purpose: "Initialize and manage per-repo project profiles (.aid/project.toml) with built-in presets.", example: "aid project init, aid project show" },
];

const AGENT_CATEGORIES = ["Research", "Simple Edit", "Complex Impl", "Frontend", "Debugging", "Testing", "Refactoring", "Documentation"];

const AGENT_MATRIX: Record<string, Record<string, number>> = {
  gemini:   { Research: 9, "Simple Edit": 2, "Complex Impl": 3, Frontend: 2, Debugging: 5, Testing: 3, Refactoring: 3, Documentation: 6 },
  codex:    { Research: 1, "Simple Edit": 4, "Complex Impl": 9, Frontend: 4, Debugging: 7, Testing: 7, Refactoring: 8, Documentation: 3 },
  opencode: { Research: 1, "Simple Edit": 8, "Complex Impl": 3, Frontend: 2, Debugging: 4, Testing: 4, Refactoring: 4, Documentation: 5 },
  kilo:     { Research: 1, "Simple Edit": 7, "Complex Impl": 2, Frontend: 2, Debugging: 3, Testing: 3, Refactoring: 3, Documentation: 4 },
  cursor:   { Research: 2, "Simple Edit": 4, "Complex Impl": 7, Frontend: 9, Debugging: 5, Testing: 5, Refactoring: 6, Documentation: 4 },
  codebuff: { Research: 2, "Simple Edit": 5, "Complex Impl": 8, Frontend: 7, Debugging: 6, Testing: 6, Refactoring: 7, Documentation: 4 },
};

const MCP_TOOLS = ["aid_run", "aid_board", "aid_show", "aid_retry", "aid_usage", "aid_ask"];

async function fetchReadme(): Promise<string> {
  try {
    const res = await fetch("https://raw.githubusercontent.com/agent-tools-org/ai-dispatch/main/README.md");
    if (!res.ok) return buildLLMSText() + "\n\n(Full README unavailable — see https://github.com/agent-tools-org/ai-dispatch)";
    return res.text();
  } catch {
    return buildLLMSText() + "\n\n(Full README unavailable — see https://github.com/agent-tools-org/ai-dispatch)";
  }
}

function baseHeaders(contentType: string): Headers {
  const headers = new Headers();
  headers.set("Cache-Control", "public, max-age=3600");
  headers.set("Content-Type", contentType);
  return headers;
}

function respondText(text: string, contentType: string, status = 200): Response {
  return new Response(text, { status, headers: baseHeaders(contentType) });
}

function respondJSON(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), { status, headers: baseHeaders("application/json; charset=utf-8") });
}

function buildLLMSText(): string {
  const lines: string[] = [];
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
  lines.push(`Define custom agents in ~/.aid/agents/ so any CLI or workflow wrapper can join the orchestrator.`);
  lines.push(`Example: TOML config with id, command, prompt_mode, and capability scores.`);
  lines.push(``);
  lines.push(`## Agent Store`);
  lines.push(`Browse and install community agents from the GitHub-backed store (agent-tools-org/aid-agents).`);
  lines.push(`Commands: aid store browse [query], aid store show <publisher/name>, aid store install <publisher/name>`);
  lines.push(``);
  lines.push(`## Teams (v8.4)`);
  lines.push(`Teams provide knowledge context, behavioral rules, and soft agent preferences.`);
  lines.push(`Each team has preferred agents (scoring boost), capability overrides, always-injected rules, and a knowledge directory.`);
  lines.push(`Commands: aid team list, aid team show <name>, aid team create <name>, aid team delete <name>.`);
  lines.push(`Use --team on aid run/batch to inject team context.`);
  lines.push(``);
  lines.push(`## Project Profiles (v8.5)`);
  lines.push(`Per-repo configuration via .aid/project.toml with built-in presets (hobby/standard/production).`);
  lines.push(`Profiles expand into verify, budget, and rules defaults. Project settings act as CLI fallbacks.`);
  lines.push(`Commands: aid project init, aid project show.`);
  lines.push(`Profiles: hobby ($5/day, budget mode), standard (auto verify, $20/day, tests required), production (cargo test, $50/day, tests+no unwrap+cross-review).`);
  lines.push(``);
  lines.push(`## Smart Knowledge Injection (v8.5)`);
  lines.push(`Stop-word filtering (70+ words), relevance threshold ≥2 word overlap, cross-layer dedup (project-first, skip overlapping team entries), content truncation at 500 chars.`);
  lines.push(`Auto-stash on merge, VFAIL merge guard, space-separated --context/--scope/--skill args.`);
  lines.push(``);
  lines.push(`## Task Lifecycle Hooks`);
  lines.push(`Define shell hooks in ~/.aid/hooks.toml that run at before_run, after_complete, or on_fail.`);
  lines.push(``);
  lines.push(`## Skills`);
  lines.push(`Methodology files under ~/.aid/skills/ inject repeatable behavior per agent.`);
  lines.push(``);
  lines.push(`## Agent Memory (v5.4)`);
  lines.push(`Project-scoped persistent knowledge auto-injected into agent prompts.`);
  lines.push(`Types: discovery, convention, lesson (30-day TTL), fact.`);
  lines.push(``);
  lines.push(`## Live Task Control (v8.3)`);
  lines.push(`aid stop (SIGTERM + 5s + SIGKILL), aid kill (immediate SIGKILL), aid steer (mid-flight guidance injection).`);
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
  lines.push(`## Quick Start`);
  lines.push(`1. Install: \`curl -fsSL https://aid.agent-tools.org/install.sh | sh\` or \`cargo install ai-dispatch\`.`);
  lines.push(`2. Run \`aid setup\` to configure API keys and detect installed agents.`);
  lines.push(`3. Run \`aid project init\` to set up project profile (hobby/standard/production).`);
  lines.push(`4. Dispatch tasks with \`aid run\`, query with \`aid query\`, monitor with \`aid watch\`, inspect with \`aid show\`.`);
  lines.push(``);
  lines.push(`## MCP Integration`);
  lines.push(`Start with \`aid mcp\` to expose tools: ${MCP_TOOLS.join(", ")}.`);
  lines.push(``);
  lines.push(`## Documentation`);
  lines.push(`Full docs: ${SITE_URL}/llms-full.txt`);
  return lines.join("\n");
}

function buildHTML(): string {
  const commandsRows = COMMANDS.map(
    (cmd) => `<tr><td>${cmd.name}</td><td>${cmd.purpose}</td><td>${cmd.example}</td></tr>`
  ).join("");

  const agentHeader = AGENT_CATEGORIES.map((cat) => `<th>${cat}</th>`).join("");
  const agentRows = Object.entries(AGENT_MATRIX)
    .map(([agent, scores]) => {
      const cells = AGENT_CATEGORIES.map((cat) => {
        const s = scores[cat];
        let style = "";
        if (s >= 8) style = "background:rgba(6,182,212,0.25)";
        else if (s >= 6) style = "background:rgba(59,130,246,0.2)";
        else if (s >= 4) style = "background:rgba(148,163,184,0.08)";
        else style = "background:rgba(100,116,139,0.12);color:#64748b";
        return `<td style="${style}">${s}</td>`;
      }).join("");
      return `<tr><th>${agent}</th>${cells}</tr>`;
    })
    .join("");

  const installCmd = "curl -fsSL https://aid.agent-tools.org/install.sh | sh";

  const archDiagram = `┌─────────────────────────────────────┐
│           aid (CLI binary)          │
├──────┬──────┬──────┬───────┬────────┬───────────┤
│ run  │ watch│ show │ board │ usage  │ benchmark │  ← user-facing commands
├──────┴──────┴──────┴───────┴────────┤
│           Task Manager              │
│  ┌────────┐ ┌────────┐ ┌────────┐  │
│  │Classif.│ │ Watch  │ │ Store  │  │
│  │+ Agent │ │ Engine │ │(SQLite)│  │
│  │Registry│ │        │ │        │  │
│  └────┬───┘ └────┬───┘ └────┬───┘  │
│       │          │          │       │
├───────┴──────────┴──────────┴───────┤
│         Agent Adapters              │
│  ┌──────┐ ┌─────┐ ┌────────┐ ┌──────┐ ┌────┐ ┌───┐ ┌────────┐
│  │Gemini│ │Codex│ │OpenCode│ │Cursor│ │Kilo│ │Codebuff│
│  └──────┘ └─────┘ └────────┘ └──────┘ └────┘ └────────┘
└─────────────────────────────────────┘`;

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
  <style>
    html { scroll-behavior: smooth; }
    body { margin: 0; font-family: system-ui, -apple-system, sans-serif; background: #030711; color: #f1f5f9; min-height: 100vh; }
    body::before { content: ""; position: fixed; inset: 0; background: radial-gradient(ellipse 80% 50% at 50% -20%, rgba(59,130,246,0.15), transparent), radial-gradient(ellipse 60% 40% at 80% 60%, rgba(6,182,212,0.08), transparent); pointer-events: none; z-index: 0; }
    .wrap { position: relative; z-index: 1; }
    .nav { position: sticky; top: 0; z-index: 100; display: flex; align-items: center; gap: 2rem; padding: 0.75rem 4vw; background: rgba(15,23,42,0.6); border-bottom: 1px solid rgba(148,163,184,0.08); backdrop-filter: blur(12px); }
    .nav-logo { font-weight: 700; font-size: 1.25rem; color: #f1f5f9; text-decoration: none; background: linear-gradient(90deg, #3b82f6, #06b6d4); -webkit-background-clip: text; -webkit-text-fill-color: transparent; background-clip: text; }
    .nav-links { display: flex; gap: 1.5rem; flex-wrap: wrap; }
    .nav-links a { color: #94a3b8; text-decoration: none; font-size: 0.9rem; }
    .nav-links a:hover { color: #f1f5f9; }
    .hero { padding: 4rem 4vw 3rem; text-align: center; max-width: 720px; margin: 0 auto; }
    .hero-title { font-size: clamp(3rem, 10vw, 5rem); font-weight: 800; margin: 0; letter-spacing: -0.03em; background: linear-gradient(90deg, #3b82f6, #06b6d4); -webkit-background-clip: text; -webkit-text-fill-color: transparent; background-clip: text; }
    .hero-sub { font-size: 1.25rem; color: #94a3b8; margin: 0.5rem 0 0; font-weight: 500; }
    .hero-desc { color: #94a3b8; font-size: 1rem; line-height: 1.6; margin: 1rem 0 1.5rem; }
    .term-block { background: rgba(15,23,42,0.9); border: 1px solid rgba(148,163,184,0.08); border-radius: 12px; overflow: hidden; text-align: left; margin: 1.5rem 0; }
    .term-head { display: flex; align-items: center; gap: 6px; padding: 0.5rem 1rem; background: rgba(15,23,42,0.95); border-bottom: 1px solid rgba(148,163,184,0.08); }
    .term-dot { width: 10px; height: 10px; border-radius: 50%; }
    .term-dot.r { background: #ef4444; } .term-dot.y { background: #eab308; } .term-dot.g { background: #22c55e; }
    .term-body { padding: 1rem 1.25rem; font-family: "SF Mono", "Monaco", "Inconsolata", monospace; font-size: 0.9rem; color: #e2e8f0; display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; white-space: pre; }
    .term-copy { flex-shrink: 0; padding: 0.35rem 0.75rem; font-size: 0.8rem; background: rgba(59,130,246,0.2); color: #93c5fd; border: 1px solid rgba(59,130,246,0.3); border-radius: 6px; cursor: pointer; font-family: inherit; }
    .term-copy:hover { background: rgba(59,130,246,0.3); }
    .cta { display: flex; gap: 1rem; justify-content: center; flex-wrap: wrap; margin-top: 1.5rem; }
    .cta a { display: inline-block; padding: 0.6rem 1.25rem; border-radius: 8px; font-size: 0.95rem; font-weight: 500; text-decoration: none; }
    .cta-primary { background: linear-gradient(90deg, #3b82f6, #06b6d4); color: #fff; }
    .cta-primary:hover { opacity: 0.9; }
    .cta-secondary { background: rgba(15,23,42,0.6); color: #94a3b8; border: 1px solid rgba(148,163,184,0.2); }
    .cta-secondary:hover { color: #f1f5f9; border-color: rgba(148,163,184,0.4); }
    .stats { display: flex; flex-wrap: wrap; gap: 1rem; justify-content: center; padding: 1rem 4vw 2rem; color: #64748b; font-size: 0.9rem; }
    .stats span { padding: 0.25rem 0.6rem; background: rgba(15,23,42,0.6); border: 1px solid rgba(148,163,184,0.08); border-radius: 6px; }
    section { padding: 2.5rem 4vw; max-width: 1100px; margin: 0 auto; }
    .sec-title { font-size: 1.75rem; margin: 0 0 1.5rem; color: #f1f5f9; }
    .why-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; }
    .card { background: rgba(15,23,42,0.6); border: 1px solid rgba(148,163,184,0.08); border-radius: 12px; padding: 1.25rem; transition: transform 0.2s, border-color 0.2s; }
    .card:hover { transform: translateY(-2px); border-color: rgba(59,130,246,0.3); }
    .card h3 { margin: 0 0 0.5rem; font-size: 1rem; color: #f1f5f9; } .card p { margin: 0; font-size: 0.9rem; color: #94a3b8; line-height: 1.5; }
    .feat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 1rem; }
    .feat-card { background: rgba(15,23,42,0.6); border: 1px solid rgba(148,163,184,0.08); border-radius: 12px; padding: 1.25rem; transition: transform 0.2s, border-color 0.2s; }
    .feat-card:hover { transform: translateY(-2px); border-color: rgba(59,130,246,0.3); }
    .feat-card h3 { margin: 0 0 0.5rem; font-size: 1rem; color: #f1f5f9; }
    .feat-card .badge { font-size: 0.7rem; color: #64748b; margin-left: 0.35rem; }
    .feat-card p { margin: 0; font-size: 0.875rem; color: #94a3b8; line-height: 1.5; }
    .table-wrap { overflow-x: auto; border: 1px solid rgba(148,163,184,0.08); border-radius: 12px; background: rgba(15,23,42,0.6); }
    table { width: 100%; border-collapse: collapse; font-size: 0.9rem; }
    table th, table td { padding: 0.6rem 1rem; text-align: left; border-bottom: 1px solid rgba(148,163,184,0.08); }
    table th { background: rgba(15,23,42,0.8); color: #94a3b8; font-weight: 600; }
    table tbody tr:nth-child(even) { background: rgba(15,23,42,0.35); }
    table tbody tr:hover { background: rgba(59,130,246,0.06); }
    .arch-term .term-body { white-space: pre; font-size: 0.8rem; line-height: 1.4; }
    .steps { counter-reset: step; }
    .step { display: flex; gap: 1rem; margin-bottom: 1.5rem; align-items: flex-start; }
    .step-num { counter-increment: step; flex-shrink: 0; width: 1.75rem; height: 1.75rem; border-radius: 50%; background: linear-gradient(90deg, #3b82f6, #06b6d4); color: #fff; display: flex; align-items: center; justify-content: center; font-size: 0.9rem; font-weight: 600; }
    .step-num::before { content: counter(step); }
    .step-content { flex: 1; } .step-content p { margin: 0 0 0.5rem; color: #94a3b8; font-size: 0.9rem; } .step-content pre { margin: 0; }
    footer { padding: 1.5rem 4vw; border-top: 1px solid rgba(148,163,184,0.08); font-size: 0.9rem; color: #64748b; display: flex; flex-wrap: wrap; gap: 1rem; align-items: center; }
    footer a { color: #7dd3fc; text-decoration: none; } footer a:hover { text-decoration: underline; }
    code { font-family: "SF Mono", "Monaco", "Inconsolata", monospace; font-size: 0.85em; background: rgba(15,23,42,0.8); padding: 0.15em 0.4em; border-radius: 4px; color: #e2e8f0; }
  </style>
  <script type="application/ld+json">${JSON_LD_DATA}<\/script>
</head>
<body>
  <div class="wrap">
    <nav class="nav">
      <a href="#" class="nav-logo">aid</a>
      <div class="nav-links">
        <a href="#features">Features</a>
        <a href="#commands">Commands</a>
        <a href="#agents">Agents</a>
        <a href="${REPO_URL}" target="_blank" rel="noopener">GitHub</a>
      </div>
    </nav>
    <header class="hero">
      <h1 class="hero-title">aid</h1>
      <p class="hero-sub">Multi-AI CLI Team Orchestrator</p>
      <p class="hero-desc">${META_DESCRIPTION}</p>
      <div class="term-block">
        <div class="term-head"><span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span></div>
        <div class="term-body">
          <code class="term-code">${installCmd}</code>
          <button type="button" class="term-copy" data-copy="${installCmd.replace(/"/g, "&quot;")}">Copy</button>
        </div>
      </div>
      <div class="cta">
        <a href="#quick-start" class="cta-primary">Get Started</a>
        <a href="${REPO_URL}" class="cta-secondary" target="_blank" rel="noopener">GitHub</a>
      </div>
    </header>
    <div class="stats">
      <span>v${VERSION}</span>
      <span>${Object.keys(AGENT_MATRIX).length} agents</span>
      <span>${COMMANDS.length} commands</span>
      <span>MIT</span>
    </div>
    <section id="why">
      <h2 class="sec-title">Why aid?</h2>
      <div class="why-grid">
        <div class="card"><h3>Unified Interface</h3><p>One CLI for gemini, codex, opencode, cursor, kilo, codebuff, and custom agents. Same flags and workflow across all of them.</p></div>
        <div class="card"><h3>Progress Visibility</h3><p>Watch background tasks in real time with <code>aid watch</code> or the TUI. No more blind runs until completion.</p></div>
        <div class="card"><h3>Cost Tracking</h3><p>Per-agent usage, budgets, and spend windows. Set limits and run low-value work with <code>--budget</code>.</p></div>
        <div class="card"><h3>Git-Native Isolation</h3><p>Per-task worktrees, auto-merge, and escape detection. Parallel work without polluting the main branch.</p></div>
        <div class="card"><h3>Methodology Enforcement</h3><p>Skills inject repeatable behavior. Shared discipline for verification, testing, and prompts across agents.</p></div>
      </div>
    </section>
    <section id="features">
      <h2 class="sec-title">Features</h2>
      <div class="feat-grid">
        <div class="feat-card"><h3>Auto Selection</h3><p>Let aid pick the best agent for the task from capability scores and history.</p></div>
        <div class="feat-card"><h3>Batch DAG</h3><p>Run TOML batch files with dependency scheduling, parallel execution, and conditional chains.</p></div>
        <div class="feat-card"><h3>Agent Memory</h3><p>Project-scoped discoveries, conventions, and lessons auto-injected into agent prompts.</p></div>
        <div class="feat-card"><h3>Custom Agents</h3><p>Define agents in ~/.aid/agents/ so any CLI or wrapper can join the orchestrator.</p></div>
        <div class="feat-card"><h3>TUI Dashboard</h3><p><code>aid watch --tui</code> for live progress, stats view, and task timeline.</p></div>
        <div class="feat-card"><h3>Teams <span class="badge">v8.4</span></h3><p>Role-based agent groups with knowledge context, behavioral rules, and capability overrides via <code>--team</code>.</p></div>
        <div class="feat-card"><h3>Project Profiles <span class="badge">v8.5</span></h3><p>Per-repo <code>.aid/project.toml</code> with hobby/standard/production presets for verify, budget, and rules.</p></div>
        <div class="feat-card"><h3>Smart Knowledge Injection <span class="badge">v8.5</span></h3><p>Stop-word filtering, cross-layer dedup, and relevance scoring keep agent prompts lean and focused.</p></div>
        <div class="feat-card"><h3>Live Task Control <span class="badge">v8.3</span></h3><p><code>aid stop</code> / <code>aid kill</code> for termination, <code>aid steer</code> for mid-flight guidance injection.</p></div>
        <div class="feat-card"><h3>Best-of-N</h3><p>Dispatch the same task to N agents, run quality metrics, and keep the best result.</p></div>
        <div class="feat-card"><h3>Agent Store</h3><p>Browse and install community agents from the GitHub-backed store with version pinning.</p></div>
      </div>
    </section>
    <section id="commands">
      <h2 class="sec-title">Commands</h2>
      <div class="table-wrap">
        <table>
          <thead><tr><th>Command</th><th>Purpose</th><th>Example</th></tr></thead>
          <tbody>${commandsRows}</tbody>
        </table>
      </div>
    </section>
    <section id="agents">
      <h2 class="sec-title">Agent capability matrix</h2>
      <div class="table-wrap">
        <table>
          <thead><tr><th>Agent</th>${agentHeader}</tr></thead>
          <tbody>${agentRows}</tbody>
        </table>
      </div>
    </section>
    <section id="arch">
      <h2 class="sec-title">Architecture</h2>
      <div class="term-block arch-term">
        <div class="term-head"><span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span></div>
        <div class="term-body"><pre style="margin:0;background:transparent;color:inherit;padding:0;">${archDiagram}</pre></div>
      </div>
    </section>
    <section id="quick-start">
      <h2 class="sec-title">Quick Start</h2>
      <div class="steps">
        <div class="step"><span class="step-num"></span><div class="step-content"><p>Install Rust 1.85+ and ensure agent CLIs are on your PATH.</p><div class="term-block" style="margin-top:0.5rem;"><div class="term-head"><span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span></div><div class="term-body">curl -fsSL https://aid.agent-tools.org/install.sh | sh</div></div></div></div>
        <div class="step"><span class="step-num"></span><div class="step-content"><p>Run <code>aid setup</code> to configure API keys and detect installed agents.</p><div class="term-block" style="margin-top:0.5rem;"><div class="term-head"><span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span></div><div class="term-body">aid setup</div></div></div></div>
        <div class="step"><span class="step-num"></span><div class="step-content"><p>Set up your project profile and dispatch tasks.</p><div class="term-block" style="margin-top:0.5rem;"><div class="term-head"><span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span></div><div class="term-body">aid project init
aid run codex "Document the API"
aid watch --tui</div></div></div></div>
      </div>
    </section>
    <footer>
      <a href="/llms.txt">/llms.txt</a>
      <a href="/llms-full.txt">/llms-full.txt</a>
      <a href="${REPO_URL}" target="_blank" rel="noopener">GitHub</a>
      <span>MIT License</span>
    </footer>
  </div>
  <script>
    (function(){
      var btn = document.querySelector(".term-copy");
      if (btn) btn.addEventListener("click", function(){ var t = this.getAttribute("data-copy"); if (t) navigator.clipboard.writeText(t.replace(/&quot;/g, '"')).then(function(){ btn.textContent = "Copied"; setTimeout(function(){ btn.textContent = "Copy"; }, 1500); }); });
    })();
  <\/script>
</body>
</html>`;
}

function buildInstallScript(): string {
  return `#!/bin/sh
set -e

echo "Installing aid (ai-dispatch) v${VERSION}..."

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo not found. Install Rust first:"
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

RUST_VER=$(rustc --version | grep -oE '[0-9]+\\.[0-9]+' | head -1)
if [ "$(echo "$RUST_VER < 1.85" | bc)" = "1" ] 2>/dev/null; then
  echo "Warning: Rust 1.85+ recommended (you have $RUST_VER). Run: rustup update"
fi

cargo install ai-dispatch
echo ""
echo "Done! Run 'aid --version' to verify."
echo "Quick start: https://aid.agent-tools.org"
`;
}

function buildRobots(): string {
  return [
    "User-agent: *", "Allow: /", "",
    "User-agent: GPTBot", "Allow: /", "",
    "User-agent: ClaudeBot", "Allow: /", "",
    "User-agent: anthropic-ai", "Allow: /",
  ].join("\n");
}

function buildPluginManifest() {
  return {
    schema_version: "v1",
    name_for_model: "aid",
    name_for_human: "aid agent dispatcher",
    description_for_model: META_DESCRIPTION,
    description_for_human: "Multi-AI CLI team orchestrator built on ai-dispatch.",
    api: { type: "none" },
    logo_url:
      "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA2NCA2NCIgPjxyZWN0IHdpZHRoPSI2NCIgaGVpZ2h0PSI2NCIgZmlsbD0iIzBmMTcyYSIvPjx0ZXh0IHg9IjUwJSIgeT0iNTAlIiBmaWxsPSIjZTJlOGYwIiBmb250LXNpemU9IjI4IiBmb250LWZhbWlseT0ibW9ub3NwYWNlIiB0ZXh0LWFuY2hvcj0ibWlkZGxlIiBkb21pbmFudC1iYXNlbGluZT0iY2VudHJhbCI+YWlkPC90ZXh0Pjwvc3ZnPg==",
    legal_info_url: `${REPO_URL}/blob/main/LICENSE`,
  };
}

function notFound(): Response {
  return respondText("Not found", "text/plain; charset=utf-8", 404);
}

function methodNotAllowed(): Response {
  const headers = baseHeaders("text/plain; charset=utf-8");
  headers.set("Allow", "GET");
  return new Response("Method not allowed", { status: 405, headers });
}

async function handleRequest(request: Request): Promise<Response> {
  if (request.method !== "GET") return methodNotAllowed();
  const url = new URL(request.url);

  switch (url.pathname) {
    case "/":
      return respondText(buildHTML(), "text/html; charset=utf-8");
    case "/llms.txt":
      return respondText(buildLLMSText(), "text/plain; charset=utf-8");
    case "/llms-full.txt":
      return respondText(await fetchReadme(), "text/plain; charset=utf-8");
    case "/api/info":
      return respondJSON({
        name: "aid", version: VERSION, description: META_DESCRIPTION,
        repository: REPO_URL, license: "MIT",
        install: "curl -fsSL https://aid.agent-tools.org/install.sh | sh",
        agents: Object.keys(AGENT_MATRIX),
        commands: COMMANDS.map((cmd) => ({ name: cmd.name, purpose: cmd.purpose })),
      });
    case "/api/commands":
      return respondJSON(COMMANDS.map((cmd) => ({ name: cmd.name, purpose: cmd.purpose, example: cmd.example })));
    case "/api/agents":
      return respondJSON(AGENT_MATRIX);
    case "/install.sh":
      return respondText(buildInstallScript(), "text/plain; charset=utf-8");
    case "/robots.txt":
      return respondText(buildRobots(), "text/plain; charset=utf-8");
    case "/.well-known/ai-plugin.json":
      return respondJSON(buildPluginManifest());
    default:
      return notFound();
  }
}

export default {
  async fetch(request: Request): Promise<Response> {
    return handleRequest(request);
  },
};
