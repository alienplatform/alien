// The control plane's view of one deployment: the commands the runner and the connector serve.
// Everything goes through the deployment's command channel, so the control plane needs no
// network path into the customer's cloud.

import { CommandsClient } from "@alienplatform/commands"

export interface OpenResult {
  sandboxId: string
  created: boolean
  resumed: boolean
}
export interface EventsResult {
  events: RunnerEvent[]
  running: boolean
  sessionId: string | null
}
export type RunnerEvent = { seq: number; kind: string } & Record<string, unknown>
export interface PendingWrite {
  id: string
  threadId: string
  user: string
  method: string
  path: string
  body: unknown
  requestedAt: string
}

export interface DataPlane {
  open(threadId: string, sandboxId?: string): Promise<OpenResult>
  issueToken(threadId: string, user: string): Promise<{ token: string; expiresAt: number }>
  bindTurn(sandboxId: string, token: string): Promise<{ bound: boolean; reason?: string }>
  turn(input: { threadId: string; sandboxId: string; sessionId?: string; user: string; text: string }): Promise<void>
  events(threadId: string, after: number, waitMs: number): Promise<EventsResult>
  /** Ends the turn's access: the connector forgets the thread's token and the sandbox's rules are removed. */
  endTurn(threadId: string, sandboxId: string): Promise<void>
  /** Ends every process the user's turns left running in the sandbox. */
  endProcesses(sandboxId: string, user: string): Promise<{ ended: number }>
  exec(
    sandboxId: string,
    command: string,
    user: string,
  ): Promise<{ exitCode: number | null; stdout: string; stderr: string }>
  pause(threadId: string, sandboxId: string): Promise<{ paused: boolean; reason?: string }>
  approvals(threadId?: string): Promise<PendingWrite[]>
  decide(input: { id: string; threadId: string; approve: boolean; by: string }): Promise<Decided>
}

export interface Decided {
  decided: boolean
  reason?: string
}

export function commandsDataPlane(config: { managerUrl: string; deploymentId: string; token: string }): DataPlane {
  const client = new CommandsClient({ ...config, timeoutMs: 120_000 })
  const runner = client.target("runner")
  const connector = client.target("connector")
  return {
    open: (threadId, sandboxId) => runner.invoke<OpenResult>("open", { threadId, sandboxId }),
    issueToken: (threadId, user) => connector.invoke("issue-token", { threadId, user }),
    bindTurn: (sandboxId, token) => runner.invoke("bind-turn", { sandboxId, token }),
    turn: async input => {
      await runner.invoke("turn", input)
    },
    events: (threadId, after, waitMs) => runner.invoke<EventsResult>("events", { threadId, after, waitMs }),
    endTurn: async (threadId, sandboxId) => {
      await Promise.all([
        connector.invoke("revoke-token", { threadId }),
        runner.invoke("unbind-turn", { sandboxId }),
      ])
    },
    endProcesses: (sandboxId, user) => runner.invoke("end-processes", { sandboxId, user }),
    exec: (sandboxId, command, user) => runner.invoke("exec", { sandboxId, command, user }),
    pause: (threadId, sandboxId) => runner.invoke("pause", { threadId, sandboxId }),
    approvals: async threadId =>
      (await connector.invoke<{ pending: PendingWrite[] }>("approvals", { threadId })).pending,
    decide: input => connector.invoke<Decided>("decide", input),
  }
}
