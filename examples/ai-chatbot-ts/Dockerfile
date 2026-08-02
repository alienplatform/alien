# The bindings addon ships glibc-only prebuilds built against glibc 2.39, so the
# base must be glibc >= 2.39 in both stages: alpine (musl) cannot install them and
# bookworm (2.36) cannot load them.
FROM node:22-trixie-slim AS build
WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm install
COPY . .
RUN npm run build

FROM node:22-trixie-slim
WORKDIR /app
ENV NODE_ENV=production
# .next/static and public are not part of the standalone output and must be
# copied alongside it for server.js to serve them.
COPY --from=build --chown=node:node /app/.next/standalone ./
COPY --from=build --chown=node:node /app/.next/static ./.next/static
COPY --from=build --chown=node:node /app/public ./public
# The base image's unprivileged account, so a compromised server is not root.
USER node
ENV HOSTNAME=0.0.0.0
ENV PORT=3000
EXPOSE 3000
CMD ["node", "server.js"]
