# SyncReconcileResponse

State reconciliation result with optional target

## Example Usage

```typescript
import { SyncReconcileResponse } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponse = {
  success: false,
  current: {
    platform: "aws",
    protocolVersion: 626709,
    status: "initial-setup",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `success`                                                | *boolean*                                                | :heavy_check_mark:                                       | Whether the state was reconciled                         |
| `current`                                                | [models.DeploymentState](../models/deploymentstate.md)   | :heavy_check_mark:                                       | N/A                                                      |
| `target`                                                 | [models.TargetDeployment](../models/targetdeployment.md) | :heavy_minus_sign:                                       | Target deployment if update is needed                    |