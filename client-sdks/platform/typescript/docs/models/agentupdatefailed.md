# AgentUpdateFailed

## Example Usage

```typescript
import { AgentUpdateFailed } from "@alienplatform/platform-api/models";

let value: AgentUpdateFailed = {
  state: "failed",
  targetVersion: "<value>",
  phase: "spawn",
  message: "<value>",
  attempt: 757332,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `state`                                                                    | [models.StateFailed](../models/statefailed.md)                             | :heavy_check_mark:                                                         | N/A                                                                        |
| `targetVersion`                                                            | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `phase`                                                                    | [models.SyncReconcileRequestPhase](../models/syncreconcilerequestphase.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `message`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `attempt`                                                                  | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |