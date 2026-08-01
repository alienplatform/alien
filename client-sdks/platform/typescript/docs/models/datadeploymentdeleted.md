# DataDeploymentDeleted

## Example Usage

```typescript
import { DataDeploymentDeleted } from "@alienplatform/platform-api/models";

let value: DataDeploymentDeleted = {
  deploymentId: "<id>",
  type: "DeploymentDeleted",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `deploymentId`                        | *string*                              | :heavy_check_mark:                    | ID of the deployment that was deleted |
| `type`                                | *"DeploymentDeleted"*                 | :heavy_check_mark:                    | N/A                                   |