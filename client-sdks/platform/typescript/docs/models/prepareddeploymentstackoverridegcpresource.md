# PreparedDeploymentStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `condition`                                                    | *models.PreparedDeploymentStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                             | N/A                                                            |
| `scope`                                                        | *string*                                                       | :heavy_check_mark:                                             | Scope (project/resource level)                                 |