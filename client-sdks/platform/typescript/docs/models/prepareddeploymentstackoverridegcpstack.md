# PreparedDeploymentStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `condition`                                            | *models.PreparedDeploymentStackOverrideConditionUnion* | :heavy_minus_sign:                                     | N/A                                                    |
| `scope`                                                | *string*                                               | :heavy_check_mark:                                     | Scope (project/resource level)                         |