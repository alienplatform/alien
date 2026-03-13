# GitHub Code Intelligence Dashboard

A beautiful control plane UI for the GitHub Code Intelligence agent. This dashboard shows code review analytics with data that comes from agents running in the customer's environment.

## Quick Start

### 1. Start the database

```bash
docker-compose up -d
```

### 2. Set up the database

```bash
pnpm install
pnpm db:push
pnpm db:seed
```

### 3. Start the agent (in a separate terminal)

```bash
cd ../remote-agent
alien dev
# → Dev server at http://localhost:9090
```

### 4. Start the dashboard

```bash
pnpm dev
# → Dashboard at http://localhost:3001
```

### 5. Sign in

Use the demo credentials:
- Email: `demo@example.com`
- Password: `demo1234`

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Control Plane (this app)                                     │
│                                                              │
│  1. GET /api/agents → List agents via Alien SDK              │
│  2. POST /api/integrations → Send creds to agent vault       │
│  3. Workflow syncs metrics periodically via ARC commands     │
│  4. Browser fetches PRs directly from agent (E2E encrypted)  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ alien dev (http://localhost:9090)                            │
│                                                              │
│  /api/agents/* → Agent management                            │
│  /v1/commands/* → Embedded command server                    │
│  /proxy/agent/* → Proxy to running function                  │
└─────────────────────────────────────────────────────────────┘
```

## Key Features

- **Privacy-first**: GitHub tokens are stored only in the agent's vault. Source code never touches the control plane.
- **Real-time metrics**: Workflow syncs metrics from agent every 5 seconds
- **E2E encryption**: Browser connects directly to agent for PR data
- **Beautiful UI**: Modern dashboard with shadcn/ui components and charts

## Environment Variables

See `.env.example` for all required environment variables.

| Variable | Description | Required |
|----------|-------------|----------|
| `ALIEN_API_URL` | Alien Platform API URL | Yes |
| `ALIEN_TOKEN` | Alien Platform API token | Yes |
| `ALIEN_WORKSPACE` | Workspace ID | Yes |
| `ALIEN_PROJECT` | Project ID | Yes |
| `DATABASE_URL` | PostgreSQL connection string | Yes |
| `BETTER_AUTH_SECRET` | Secret for better-auth | Yes |
| `BETTER_AUTH_URL` | Dashboard URL | Yes |
| `NEXT_PUBLIC_ALIEN_URL` | Agent public URL (for display) | No |

## Key Files

- `app/(dashboard)/page.tsx` - Main dashboard with metrics
- `app/(dashboard)/agents/page.tsx` - Agent deployment instructions
- `app/(dashboard)/integrations/page.tsx` - GitHub integrations management
- `lib/config.ts` - Environment configuration and SDK setup
- `lib/arc.ts` - ARC client for agent commands
- `lib/queries.ts` - React Query hooks for data fetching
- `workflows/sync-metrics.ts` - Periodic metric sync workflow

## Tech Stack

- **Framework**: Next.js 15
- **Data Fetching**: React Query (TanStack Query)
- **UI**: shadcn/ui + Tailwind CSS
- **Charts**: Recharts with shadcn chart components
- **Auth**: better-auth with email/password
- **Database**: PostgreSQL with Drizzle ORM
- **Workflows**: workflow.dev for durable execution
- **Type Safety**: TypeScript + ts-pattern
