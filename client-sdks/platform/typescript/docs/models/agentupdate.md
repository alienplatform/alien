# AgentUpdate

Outcome of the agent's in-flight self-update from the last /v1/sync.


## Supported Types

### `models.AgentUpdateInProgress`

```typescript
const value: models.AgentUpdateInProgress = {
  state: "inProgress",
  targetVersion: "<value>",
  attempt: 88597,
};
```

### `models.AgentUpdateFailed`

```typescript
const value: models.AgentUpdateFailed = {
  state: "failed",
  targetVersion: "<value>",
  phase: "spawn",
  message: "<value>",
  attempt: 757332,
};
```

### `any`

```typescript
const value: any = "<value>";
```

