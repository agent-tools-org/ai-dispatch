# aid × EverMemOS — Multi-Agent Dev Team with Cloud Memory

![Rust](https://img.shields.io/badge/rust-2024-orange)
![EverMemOS](https://img.shields.io/badge/EverMemOS-Integration-blue)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**aid** is a multi-AI CLI team orchestrator that dispatches tasks to a fleet of AI coding agents (Codex, Gemini, OpenCode, Cursor, and more). This integration brings the power of **EverMemOS** to the developer workflow, adding a cloud-backed semantic memory layer that transforms a group of isolated AI tools into a cohesive, learning-capable engineering team.

## What it does

The EverMemOS integration provides `aid` agents with a "long-term brain" that persists across sessions, machines, and individual tasks.

1.  **Remember** — After every task, `aid` automatically extracts key learnings, bug patterns, and architectural decisions from the agent's output and stores them in EverMemOS.
2.  **Recall** — Before a new task begins, `aid` performs a semantic search against the EverMemOS cloud to retrieve relevant past memories and injects them directly into the agent's prompt.
3.  **Share** — Knowledge discovered by one agent (e.g., Agent A fixing a bug in the auth module) is immediately available to all other agents, even across different physical machines or different team members.
4.  **Evolve** — EverMemOS goes beyond simple storage by consolidating episodic task traces into stable, high-level knowledge over time, allowing your AI team to get "smarter" the more they work on your codebase.

## Architecture

```text
┌─────────────────────────────────────┐
│         aid orchestrator            │
│  (plan → dispatch → review → learn) │
└──────┬──────────────┬───────────────┘
       │              │
  ┌────▼────┐    ┌────▼────┐
  │ Dispatch │    │ Extract  │
  │ (inject  │    │ (task    │
  │  memory) │    │  result  │
  └────┬────┘    │  → mem)  │
       │         └────┬────┘
       │              │
  ┌────▼──────────────▼────┐
  │      EverMemOS API      │
  │  (store / search / evolve) │
  └────────────────────────┘
       │
  ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
  │  codex  │  │ gemini  │  │opencode │
  └─────────┘  └─────────┘  └─────────┘
```

## Key Features

-   **Dual Memory Layer**: Combines lightning-fast local SQLite storage for offline work with the EverMemOS cloud for deep semantic search and persistence.
-   **Automatic Injection**: No manual tagging required; relevant cloud memories are seamlessly injected into every agent dispatch based on task context.
-   **Cloud-Native CLI**: Manage your team's collective brain with simple commands: `aid memory cloud-status`, `aid memory cloud-search`, and `aid memory cloud-push`.
-   **Zero-Config Default**: `aid` works out of the box with local memory; enabling the cloud is as simple as adding a few lines to your configuration.

## Quick Start

### 1. Run EverMemOS Locally
Start the memory backend using Docker:
```bash
docker compose up -d  # Run from the EverMemOS repository
```

### 2. Configure aid
Add the `[evermemos]` section to your `~/.aid/config.toml`:
```toml
[evermemos]
enabled = true
base_url = "http://localhost:1995/api/v1"
user_id = "my-dev-team"
# api_key = "optional-for-cloud"
```

### 3. Verify Integration
Ensure `aid` can communicate with your cloud memory:
```bash
aid memory cloud-status
```

## Demo Scenario: Collective Learning

1.  **Discovery**: Agent A is tasked with fixing a complex bug in the retry logic. It discovers that a specific vendor API requires an exponential backoff capped at exactly 45 seconds. This discovery is automatically stored in EverMemOS.
2.  **Persistence**: The next day, a different developer on a different machine dispatches Agent B to implement a new feature in the same module.
3.  **Recall**: Before Agent B starts, `aid` searches EverMemOS, finds the 45-second cap discovery, and injects it into Agent B's prompt.
4.  **Result**: Agent B produces code that is already compliant with the vendor's specific requirements, avoiding a regression without the developer ever having to explain the pattern.

## Tech Stack

-   **aid**: High-performance Rust CLI orchestrator (v7.4).
-   **EverMemOS**: Advanced Python-based Memory OS utilizing Milvus (vector DB), Elasticsearch, MongoDB, and Redis.
-   **Integration**: Robust REST API communication via the `ureq` HTTP client for minimal overhead.

## Competition Track
**Track 2: Platform Plugins** — This project demonstrates the seamless integration of EverMemOS into a professional developer tool (an AI CLI orchestrator), providing immediate, tangible value to multi-agent engineering workflows.

## Links
-   **aid GitHub**: [https://github.com/agent-tools-org/ai-dispatch](https://github.com/agent-tools-org/ai-dispatch)
-   **EverMemOS**: [https://github.com/EverMind-AI/EverMemOS](https://github.com/EverMind-AI/EverMemOS)
-   **Competition**: AIMemory Genesis 2026
