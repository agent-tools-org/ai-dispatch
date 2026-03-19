# aid-sandbox: Container image for sandboxed agent execution.
# Includes Node.js and npm-based AI CLI agents.
# Build: container build -t aid-sandbox:latest .

FROM ubuntu:latest

# System dependencies
RUN apt-get update -qq && \
    apt-get install -y -qq --no-install-recommends \
    curl ca-certificates git && \
    rm -rf /var/lib/apt/lists/*

# Install Node.js 22.x via NodeSource
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && \
    apt-get install -y -qq nodejs && \
    rm -rf /var/lib/apt/lists/*

# Install AI CLI agents (node-based)
RUN npm install -g --no-fund --no-audit \
    @openai/codex \
    @kilocode/cli \
    codebuff

# Gemini CLI
RUN npm install -g --no-fund --no-audit @google/gemini-cli

# Verify installations
RUN node --version && npm --version && \
    which codex && which kilo && which codebuff && which gemini

WORKDIR /work
