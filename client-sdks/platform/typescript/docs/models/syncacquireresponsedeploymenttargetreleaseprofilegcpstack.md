# SyncAcquireResponseDeploymentTargetReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncAcquireResponseDeploymentTargetReleaseProfileConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |