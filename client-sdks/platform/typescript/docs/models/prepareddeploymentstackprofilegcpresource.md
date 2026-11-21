# PreparedDeploymentStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `condition`                                                   | *models.PreparedDeploymentStackProfileResourceConditionUnion* | :heavy_minus_sign:                                            | N/A                                                           |
| `scope`                                                       | *string*                                                      | :heavy_check_mark:                                            | Scope (project/resource level)                                |