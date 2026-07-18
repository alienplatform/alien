# SyncAcquireResponseDeploymentTargetReleaseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `condition`                                                             | *models.SyncAcquireResponseDeploymentTargetReleaseExtendConditionUnion* | :heavy_minus_sign:                                                      | N/A                                                                     |
| `scope`                                                                 | *string*                                                                | :heavy_check_mark:                                                      | Scope (project/resource level)                                          |