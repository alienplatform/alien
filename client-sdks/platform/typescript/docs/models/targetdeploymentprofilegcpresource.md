# TargetDeploymentProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetDeploymentProfileGcpResource } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `condition`                                            | *models.TargetDeploymentProfileResourceConditionUnion* | :heavy_minus_sign:                                     | N/A                                                    |
| `scope`                                                | *string*                                               | :heavy_check_mark:                                     | Scope (project/resource level)                         |