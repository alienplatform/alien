# RemoteOperatorState

## Example Usage

```typescript
import { RemoteOperatorState } from "@alienplatform/platform-api/models";

let value: RemoteOperatorState = {
  state: "connecting",
  stateReason: "<value>",
  actionNeeded: "<value>",
  operationsSync: {},
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `state`                                                                                | [models.DeploymentListItemResponseState](../models/deploymentlistitemresponsestate.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `stateReason`                                                                          | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `actionNeeded`                                                                         | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `operationsSync`                                                                       | [models.OperationsSync](../models/operationssync.md)                                   | :heavy_check_mark:                                                                     | N/A                                                                                    |