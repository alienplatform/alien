# ReleaseDeploymentItemDeploymentGroup

Deployment group this deployment belongs to

## Example Usage

```typescript
import { ReleaseDeploymentItemDeploymentGroup } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemDeploymentGroup = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  name: "prod-us-east-1",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 | Example                                     |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `id`                                        | *string*                                    | :heavy_check_mark:                          | Unique identifier for the deployment group. | dg_r27ict8c7vcgsumpj90ackf7b                |
| `name`                                      | *string*                                    | :heavy_check_mark:                          | Deployment group name.                      | prod-us-east-1                              |
