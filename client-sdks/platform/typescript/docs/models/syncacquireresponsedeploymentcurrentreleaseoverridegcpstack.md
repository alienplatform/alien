# SyncAcquireResponseDeploymentCurrentReleaseOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `condition`                                                                | *models.SyncAcquireResponseDeploymentCurrentReleaseOverrideConditionUnion* | :heavy_minus_sign:                                                         | N/A                                                                        |
| `scope`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | Scope (project/resource level)                                             |