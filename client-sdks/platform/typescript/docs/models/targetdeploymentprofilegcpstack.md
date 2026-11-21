# TargetDeploymentProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentProfileGcpStack } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `condition`                                    | *models.TargetDeploymentProfileConditionUnion* | :heavy_minus_sign:                             | N/A                                            |
| `scope`                                        | *string*                                       | :heavy_check_mark:                             | Scope (project/resource level)                 |