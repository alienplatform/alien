# AgentSessionEvent


## Supported Types

### `models.AgentSessionStatusEvent`

```typescript
const value: models.AgentSessionStatusEvent = {
  seq: 4210.33,
  createdAt: "1710340330705",
  type: "status",
  payload: {
    status: "<value>",
  },
};
```

### `models.AgentSessionStepEvent`

```typescript
const value: models.AgentSessionStepEvent = {
  seq: 1186.78,
  createdAt: "1731419010075",
  type: "step",
  payload: {
    stepId: "<id>",
    title: "<value>",
    status: "in_progress",
  },
};
```

### `models.AgentSessionToolCallEvent`

```typescript
const value: models.AgentSessionToolCallEvent = {
  seq: 400.77,
  createdAt: "1718464824179",
  type: "tool_call",
  payload: {
    toolCallId: "<id>",
    toolName: "<value>",
  },
};
```

### `models.AgentSessionToolResultEvent`

```typescript
const value: models.AgentSessionToolResultEvent = {
  seq: 4758.06,
  createdAt: "1735429468512",
  type: "tool_result",
  payload: {
    toolCallId: "<id>",
    toolName: "<value>",
    ok: true,
  },
};
```

### `models.AgentSessionMarkdownEvent`

```typescript
const value: models.AgentSessionMarkdownEvent = {
  seq: 4742.82,
  createdAt: "1714119157487",
  type: "markdown",
  payload: {
    text: "<value>",
  },
};
```

### `models.AgentSessionApprovalRequestedEvent`

```typescript
const value: models.AgentSessionApprovalRequestedEvent = {
  seq: 8489.57,
  createdAt: "1727488365811",
  type: "approval_requested",
  payload: {
    approvalId: "<id>",
    toolCallId: "<id>",
    toolName: "<value>",
  },
};
```

### `models.AgentSessionApprovalGrantedEvent`

```typescript
const value: models.AgentSessionApprovalGrantedEvent = {
  seq: 2155.04,
  createdAt: "1722031798553",
  type: "approval_granted",
  payload: {
    approvalId: "<id>",
    approvedByUserId: "<id>",
    approvedByName: "<value>",
    source: "dashboard",
  },
};
```

### `models.AgentSessionRestartedEvent`

```typescript
const value: models.AgentSessionRestartedEvent = {
  seq: 9503.12,
  createdAt: "1734099609945",
  type: "session_restarted",
  payload: {
    reason: "<value>",
  },
};
```

### `models.AgentSessionEventsTruncatedEvent`

```typescript
const value: models.AgentSessionEventsTruncatedEvent = {
  seq: 1033.37,
  createdAt: "1730528196150",
  type: "events_truncated",
  payload: {
    limit: 5615.57,
  },
};
```
