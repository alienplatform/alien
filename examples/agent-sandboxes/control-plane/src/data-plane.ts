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
  /** Changes when the runner restarts: its event numbers start again. */
  generation: string
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
  turn(input: {
    threadId: string
    sandboxId: string
    sessionId?: string
    text: string
  }): Promise<void>
  events(threadId: string, after: number, waitMs: number): Promise<EventsResult>
  /** Ends the turn's access: the connector forgets the thread's token and the sandbox's rules are removed. */
  endTurn(threadId: string, sandboxId: string): Promise<void>
  /** Kills the running turn's agent process; rejects if the runner cannot be reached. */
  stopTurn(threadId: string): Promise<{ stopped: boolean; running: boolean }>
  /**
   * Ends every process in the sandbox that is not the sandbox's own, then lists again. Rejects
   * if anything is left or the listing fails.
   */
  endProcesses(sandboxId: string): Promise<{ ended: number }>
  exec(
    sandboxId: string,
    command: string,
  ): Promise<{ exitCode: number | null; stdout: string; stderr: string }>
  pause(threadId: string, sandboxId: string): Promise<{ paused: boolean; reason?: string }>
  approvals(threadId?: string): Promise<PendingWrite[]>
  decide(input: { id: string; threadId: string; approve: boolean; by: string }): Promise<Decided>
}

export interface Decided {
  decided: boolean
  reason?: string
}

export function commandsDataPlane(config: {
  managerUrl: string
  deploymentId: string
  token: string
}): DataPlane {
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
    events: (threadId, after, waitMs) =>
      runner.invoke<EventsResult>("events", { threadId, after, waitMs }),
    endTurn: async (threadId, sandboxId) => {
      await Promise.all([
        connector.invoke("revoke-token", { threadId }),
        runner.invoke("unbind-turn", { sandboxId }),
      ])
    },
    stopTurn: threadId => runner.invoke("stop-turn", { threadId }),
    endProcesses: sandboxId => runner.invoke("end-processes", { sandboxId }),
    exec: (sandboxId, command) => runner.invoke("exec", { sandboxId, command }),
    pause: (threadId, sandboxId) => runner.invoke("pause", { threadId, sandboxId }),
    approvals: async threadId =>
      (await connector.invoke<{ pending: PendingWrite[] }>("approvals", { threadId })).pending,
    decide: input => connector.invoke<Decided>("decide", input),
  }
}
