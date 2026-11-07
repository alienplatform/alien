# RuntimePurpose

Runtime operations cannot administer deployments or mint credentials.


## Supported Types

### `models.RuntimePurposeTelemetry`

```typescript
const value: models.RuntimePurposeTelemetry = {
  type: "telemetry",
};
```

### `models.RuntimePurposeReceiveCommands`

```typescript
const value: models.RuntimePurposeReceiveCommands = {
  target: {
    resourceId: "<id>",
    resourceType: "daemon",
  },
  type: "receiveCommands",
};
```
