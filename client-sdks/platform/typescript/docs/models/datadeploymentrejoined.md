# DataDeploymentRejoined

## Example Usage

```typescript
import { DataDeploymentRejoined } from "@alienplatform/platform-api/models";

let value: DataDeploymentRejoined = {
  deploymentGroupId: "<id>",
  deploymentId: "<id>",
  type: "DeploymentRejoined",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `deploymentGroupId`                                   | *string*                                              | :heavy_check_mark:                                    | ID of the deployment group that authorized the rejoin |
| `deploymentId`                                        | *string*                                              | :heavy_check_mark:                                    | ID of the deployment whose agent rejoined             |
| `type`                                                | *"DeploymentRejoined"*                                | :heavy_check_mark:                                    | N/A                                                   |