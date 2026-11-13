# RuntimePurpose

Runtime operations cannot administer deployments or mint credentials.


## Supported Types

### `models.RuntimePurposeTelemetry`

```typescript
const value: models.RuntimePurposeTelemetry = {
  type: "telemetry",
};
```

### `models.RuntimePurposeOperatorSync`

```typescript
const value: models.RuntimePurposeOperatorSync = {
  type: "operatorSync",
};
```

### `models.RuntimePurposeRegistryPull`

```typescript
const value: models.RuntimePurposeRegistryPull = {
  type: "registryPull",
};
```

### `models.RuntimePurposeOperatorTunnel`

```typescript
const value: models.RuntimePurposeOperatorTunnel = {
  type: "operatorTunnel",
};
```

### `models.RuntimePurposeOperatorOperations`

```typescript
const value: models.RuntimePurposeOperatorOperations = {
  type: "operatorOperations",
};
```

### `models.RuntimePurposeOperationRelay`

```typescript
const value: models.RuntimePurposeOperationRelay = {
  target: {
    resourceId: "<id>",
    resourceType: "daemon",
  },
  type: "operationRelay",
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
