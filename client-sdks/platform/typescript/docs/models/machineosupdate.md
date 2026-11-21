# MachineOsUpdate

Where the deployment's machines are in moving to the machine image its channel publishes. Null without a compute cluster or a channel.


## Supported Types

### `models.MachineOsUpdateRolling`

```typescript
const value: models.MachineOsUpdateRolling = {
  state: "rolling",
  imageVersion: "<value>",
  remaining: 289661,
};
```

### `models.MachineOsUpdateDone`

```typescript
const value: models.MachineOsUpdateDone = {
  state: "done",
  imageVersion: "<value>",
};
```

### `models.MachineOsUpdateHeld`

```typescript
const value: models.MachineOsUpdateHeld = {
  state: "held",
  imageVersion: "<value>",
  reason: "<value>",
};
```

### `string`

```typescript
const value: string = "<value>";
```
