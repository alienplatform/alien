FROM oven/bun:1.2-alpine

WORKDIR /app
COPY package.json bun.lock* ./
RUN bun install --production
COPY src ./src

ENV NODE_ENV=production
CMD ["bun", "src/index.ts"]
