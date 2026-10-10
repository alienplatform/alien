# MoveDeploymentRequest

## Example Usage

```typescript
import { MoveDeploymentRequest } from "@alienplatform/platform-api/models";

let value: MoveDeploymentRequest = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 | Example                                     |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `deploymentGroupId`                         | *string*                                    | :heavy_check_mark:                          | Unique identifier for the deployment group. | dg_r27ict8c7vcgsumpj90ackf7b                |
| `expectedMembershipRevision`                | *number*                                    | :heavy_minus_sign:                          | N/A                                         |                                             |
| `dryRun`                                    | *boolean*                                   | :heavy_minus_sign:                          | N/A                                         |                                             |