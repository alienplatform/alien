# DeploymentOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `condition`                    | *any*                          | :heavy_minus_sign:             | N/A                            |
| `scope`                        | *string*                       | :heavy_check_mark:             | Scope (project/resource level) |