// Store website for aid agent marketplace — fetches index from GitHub, renders agent catalog.
// Exports: Cloudflare Worker default handler.
// Deps: none (fetch-based).

const STORE_REPO = "https://raw.githubusercontent.com/agent-tools-org/aid-agents/main";
const SITE_URL = "https://store.agent-tools.org";
const REPO_URL = "https://github.com/agent-tools-org/aid-agents";
const AID_URL = "https://github.com/agent-tools-org/ai-dispatch";

interface AgentEntry {
  id: string;
  display_name: string;
  description: string;
  version: string;
  command: string;
  scripts?: string[];
}

interface AgentIndex {
  agents: AgentEntry[];
}

async function fetchIndex(): Promise<AgentIndex> {
  const res = await fetch(`${STORE_REPO}/index.json`);
  if (!res.ok) throw new Error(`Failed to fetch index: ${res.status}`);
  return res.json();
}

async function fetchToml(id: string): Promise<string> {
  const res = await fetch(`${STORE_REPO}/agents/${id}.toml`);
  if (!res.ok) throw new Error(`Agent not found: ${id}`);
  return res.text();
}

function baseHeaders(contentType: string) {
  const h = new Headers();
  h.set("Cache-Control", "public, max-age=300");
  h.set("Content-Type", contentType);
  return h;
}

function respond(body: string, contentType: string, status = 200) {
  return new Response(body, { status, headers: baseHeaders(contentType) });
}

function respondJSON(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), { status, headers: baseHeaders("application/json; charset=utf-8") });
}

function capabilityBar(score: number): string {
  const pct = score * 10;
  const color = score >= 7 ? "#22d3ee" : score >= 4 ? "#38bdf8" : "#475569";
  return `<div style="display:flex;align-items:center;gap:6px;">
    <div style="width:60px;height:6px;background:#1e293b;border-radius:3px;overflow:hidden;">
      <div style="width:${pct}%;height:100%;background:${color};border-radius:3px;"></div>
    </div>
    <span style="font-size:0.75rem;color:#94a3b8;">${score}</span>
  </div>`;
}

function agentCard(agent: AgentEntry, toml: string): string {
  const capLines = toml.match(/^\w+\s*=\s*\d+$/gm) || [];
  const caps = capLines.map(line => {
    const [key, val] = line.split("=").map(s => s.trim());
    return { name: key, score: parseInt(val) };
  }).filter(c => c.score > 0).sort((a, b) => b.score - a.score);

  const capsHtml = caps.length > 0
    ? caps.map(c => `<div style="display:flex;justify-content:space-between;align-items:center;"><span style="font-size:0.8rem;color:#cbd5e1;">${c.name}</span>${capabilityBar(c.score)}</div>`).join("")
    : `<span style="font-size:0.8rem;color:#64748b;">No capability scores</span>`;

  const scriptsTag = agent.scripts && agent.scripts.length > 0
    ? `<span style="font-size:0.7rem;padding:2px 8px;background:#1e3a5f;color:#7dd3fc;border-radius:4px;">+ scripts</span>`
    : "";

  return `<div style="background:#0f172a;border:1px solid rgba(226,232,240,.12);border-radius:12px;padding:1.25rem;display:flex;flex-direction:column;gap:0.75rem;transition:border-color .2s;" onmouseover="this.style.borderColor='rgba(34,211,238,.3)'" onmouseout="this.style.borderColor='rgba(226,232,240,.12)'">
    <div style="display:flex;justify-content:space-between;align-items:flex-start;">
      <div>
        <h3 style="margin:0;font-size:1.1rem;color:#f1f5f9;">${agent.display_name}</h3>
        <span style="font-size:0.75rem;color:#64748b;">${agent.id} v${agent.version}</span>
      </div>
      <div style="display:flex;gap:4px;">
        <span style="font-size:0.7rem;padding:2px 8px;background:#1e293b;color:#94a3b8;border-radius:4px;">${agent.command}</span>
        ${scriptsTag}
      </div>
    </div>
    <p style="margin:0;font-size:0.85rem;color:#94a3b8;line-height:1.4;">${agent.description}</p>
    <div style="display:flex;flex-direction:column;gap:4px;">${capsHtml}</div>
    <div style="margin-top:auto;display:flex;gap:8px;flex-wrap:wrap;">
      <code style="font-size:0.75rem;background:#040b16;padding:4px 10px;border-radius:6px;color:#22d3ee;flex:1;">aid store install ${agent.id}</code>
      <a href="/agent/${agent.id}" style="font-size:0.75rem;color:#7dd3fc;text-decoration:none;padding:4px 10px;">View TOML</a>
    </div>
  </div>`;
}

async function buildHomePage(): Promise<string> {
  const index = await fetchIndex();
  const tomls = await Promise.all(
    index.agents.map(async (a) => {
      try { return await fetchToml(a.id); } catch { return ""; }
    })
  );
  const cards = index.agents.map((a, i) => agentCard(a, tomls[i])).join("");

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>aid Agent Store</title>
  <meta name="description" content="Community agent definitions for aid — the multi-AI CLI orchestrator. Browse, preview, and install agents.">
  <meta property="og:title" content="aid Agent Store">
  <meta property="og:description" content="Community agent definitions for aid. Browse, preview, and install with one command.">
  <meta property="og:url" content="${SITE_URL}">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <link rel="alternate" type="text/plain" href="/llms.txt">
  <style>
    *{box-sizing:border-box;}
    body{margin:0;font-family:"IBM Plex Mono",SFMono-Regular,Menlo,monospace;background:#05070a;color:#e2e8f0;min-height:100vh;}
    a{color:#7dd3fc;text-decoration:none;}
    a:hover{text-decoration:underline;}
    header{padding:2rem 4vw;border-bottom:1px solid rgba(226,232,240,.1);}
    main{padding:1.5rem 4vw 3rem;}
    footer{padding:1.5rem 4vw;border-top:1px solid rgba(226,232,240,.1);font-size:0.85rem;color:#64748b;}
    .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(340px,1fr));gap:1rem;}
    .hero-install{background:#0c111d;border:1px solid rgba(226,232,240,.1);border-radius:8px;padding:0.75rem 1rem;font-size:0.85rem;display:flex;align-items:center;gap:0.5rem;margin-top:1rem;max-width:500px;}
    .hero-install code{color:#22d3ee;}
    @media(max-width:600px){.grid{grid-template-columns:1fr;}}
  </style>
</head>
<body>
  <header>
    <p style="font-size:.8rem;letter-spacing:.2rem;text-transform:uppercase;color:#64748b;">store.agent-tools.org</p>
    <h1 style="margin:0.25rem 0;font-size:1.8rem;">aid Agent Store</h1>
    <p style="color:#94a3b8;max-width:600px;">Community agent definitions for <a href="${AID_URL}">aid</a> — the multi-AI CLI orchestrator. Browse, preview, and install with one command.</p>
    <div class="hero-install">
      <span style="color:#64748b;">$</span>
      <code>aid store install community/&lt;agent&gt;</code>
    </div>
  </header>
  <main>
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:1rem;flex-wrap:wrap;gap:0.5rem;">
      <h2 style="margin:0;font-size:1.1rem;color:#cbd5e1;">${index.agents.length} agents available</h2>
      <a href="${REPO_URL}" style="font-size:0.8rem;color:#64748b;">Contribute on GitHub</a>
    </div>
    <div class="grid">${cards}</div>
  </main>
  <footer>
    <div style="display:flex;flex-wrap:wrap;gap:1.5rem;align-items:center;">
      <a href="${AID_URL}">ai-dispatch</a>
      <a href="https://aid.agent-tools.org">aid docs</a>
      <a href="${REPO_URL}">store repo</a>
      <a href="/llms.txt">/llms.txt</a>
      <a href="/api/agents">/api/agents</a>
    </div>
  </footer>
</body>
</html>`;
}

async function buildAgentPage(id: string): Promise<string> {
  const index = await fetchIndex();
  const agent = index.agents.find(a => a.id === id);
  if (!agent) throw new Error("Not found");
  const toml = await fetchToml(id);

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>${agent.display_name} — aid Agent Store</title>
  <meta name="description" content="${agent.description}">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>
    *{box-sizing:border-box;}
    body{margin:0;font-family:"IBM Plex Mono",SFMono-Regular,Menlo,monospace;background:#05070a;color:#e2e8f0;min-height:100vh;}
    a{color:#7dd3fc;text-decoration:none;}
    a:hover{text-decoration:underline;}
    header{padding:2rem 4vw;border-bottom:1px solid rgba(226,232,240,.1);}
    main{padding:1.5rem 4vw 3rem;max-width:800px;}
    footer{padding:1.5rem 4vw;border-top:1px solid rgba(226,232,240,.1);font-size:0.85rem;color:#64748b;}
    pre{background:#0c111d;border:1px solid rgba(226,232,240,.1);border-radius:8px;padding:1rem;overflow-x:auto;font-size:0.85rem;line-height:1.5;}
    .badge{font-size:0.75rem;padding:3px 10px;border-radius:4px;display:inline-block;}
  </style>
</head>
<body>
  <header>
    <a href="/" style="font-size:.8rem;color:#64748b;">&larr; back to store</a>
    <h1 style="margin:0.5rem 0;font-size:1.6rem;">${agent.display_name}</h1>
    <p style="color:#94a3b8;">${agent.description}</p>
    <div style="display:flex;gap:8px;margin-top:0.5rem;flex-wrap:wrap;">
      <span class="badge" style="background:#1e293b;color:#94a3b8;">v${agent.version}</span>
      <span class="badge" style="background:#1e293b;color:#94a3b8;">${agent.command}</span>
      <span class="badge" style="background:#0c4a6e;color:#7dd3fc;">${agent.id}</span>
      ${agent.scripts?.map(s => `<span class="badge" style="background:#1e3a5f;color:#7dd3fc;">script: ${s}</span>`).join("") || ""}
    </div>
  </header>
  <main>
    <h2 style="font-size:1rem;color:#cbd5e1;">Install</h2>
    <pre><code style="color:#22d3ee;">aid store install ${agent.id}</code></pre>

    <h2 style="font-size:1rem;color:#cbd5e1;">Agent Definition (TOML)</h2>
    <pre><code>${escapeHtml(toml)}</code></pre>

    <h2 style="font-size:1rem;color:#cbd5e1;">Source</h2>
    <p><a href="${REPO_URL}/blob/main/agents/${agent.id}.toml">View on GitHub</a></p>
  </main>
  <footer>
    <div style="display:flex;flex-wrap:wrap;gap:1.5rem;">
      <a href="/">Store</a>
      <a href="${AID_URL}">ai-dispatch</a>
      <a href="${REPO_URL}">store repo</a>
    </div>
  </footer>
</body>
</html>`;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function buildLLMSText(index: AgentIndex): string {
  const lines = [
    `Name: aid Agent Store`,
    `Description: Community agent definitions for aid — the multi-AI CLI orchestrator.`,
    `Homepage: ${SITE_URL}`,
    `Repository: ${REPO_URL}`,
    ``,
    `## Agents`,
  ];
  for (const a of index.agents) {
    lines.push(`- ${a.id}: ${a.display_name} — ${a.description} (v${a.version}, command: ${a.command})`);
  }
  lines.push(``);
  lines.push(`## Install`);
  lines.push(`aid store install <publisher/name>`);
  lines.push(``);
  lines.push(`## Contribute`);
  lines.push(`Add agent TOMLs to ${REPO_URL}`);
  return lines.join("\n");
}

async function handleRequest(request: Request): Promise<Response> {
  if (request.method !== "GET") {
    const h = baseHeaders("text/plain; charset=utf-8");
    h.set("Allow", "GET");
    return new Response("Method not allowed", { status: 405, headers: h });
  }

  const url = new URL(request.url);
  const path = url.pathname;

  try {
    if (path === "/") {
      return respond(await buildHomePage(), "text/html; charset=utf-8");
    }

    if (path.startsWith("/agent/")) {
      const id = path.slice(7); // strip "/agent/"
      return respond(await buildAgentPage(id), "text/html; charset=utf-8");
    }

    if (path === "/api/agents") {
      const index = await fetchIndex();
      return respondJSON(index);
    }

    if (path === "/llms.txt") {
      const index = await fetchIndex();
      return respond(buildLLMSText(index), "text/plain; charset=utf-8");
    }

    if (path === "/robots.txt") {
      return respond("User-agent: *\nAllow: /\n", "text/plain; charset=utf-8");
    }

    return respond("Not found", "text/plain; charset=utf-8", 404);
  } catch (e) {
    const msg = e instanceof Error ? e.message : "Internal error";
    return respond(msg, "text/plain; charset=utf-8", 500);
  }
}

export default {
  async fetch(request: Request) {
    return handleRequest(request);
  }
};
