# DeploymentProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentProfileGcpResource } from "@aliendotdev/platform-api/models";

let value: DeploymentProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `condition`                    | *any*                          | :heavy_minus_sign:             | N/A                            |
| `scope`                        | *string*                       | :heavy_check_mark:             | Scope (project/resource level) |