# AgentSyncRequest

## Example Usage

```typescript
import { AgentSyncRequest } from "@alienplatform/manager-api/models";

let value: AgentSyncRequest = {
  deploymentId: "<id>",
};
```

## Fields

| Field                                                                                                                                                                   | Type                                                                                                                                                                    | Required                                                                                                                                                                | Description                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `currentState`                                                                                                                                                          | *any*                                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                                      | Current deployment state as reported by the agent.<br/>When present, the manager updates the deployment record to reflect<br/>the agent's progress (status, stack_state, etc.). |
| `deploymentId`                                                                                                                                                          | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | N/A                                                                                                                                                                     |