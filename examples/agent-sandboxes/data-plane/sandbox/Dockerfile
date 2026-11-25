# The agent's sandbox: Claude Code, curl, and the `orders` and `scanner` tools. No credentials are baked in.
FROM public.ecr.aws/docker/library/node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl procps \
 && rm -rf /var/lib/apt/lists/*
ARG CLAUDE_CODE_VERSION=latest
RUN npm install -g @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION} && npm cache clean --force
COPY orders scanner /usr/local/bin/
COPY CLAUDE.md /opt/agent/CLAUDE.md
RUN chmod 755 /usr/local/bin/orders /usr/local/bin/scanner
USER node
