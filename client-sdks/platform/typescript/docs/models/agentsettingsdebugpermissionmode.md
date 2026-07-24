# AgentSettingsDebugPermissionMode

Workspace-level policy for ai-agent debug commands. `auto` runs `alien_debug` tool calls without asking; `ask` halts each session before every debug command and waits for a human approval from dashboard or Slack.

## Example Usage

```typescript
import { AgentSettingsDebugPermissionMode } from "@alienplatform/platform-api/models";

let value: AgentSettingsDebugPermissionMode = "auto";
```

## Values

```typescript
"auto" | "ask"
```
