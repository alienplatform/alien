# PreparedDeploymentStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `condition`                                          | *models.PreparedDeploymentStackExtendConditionUnion* | :heavy_minus_sign:                                   | N/A                                                  |
| `scope`                                              | *string*                                             | :heavy_check_mark:                                   | Scope (project/resource level)                       |