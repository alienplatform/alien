# SyncAcquireResponseDeploymentTargetReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `condition`                                                                     | *models.SyncAcquireResponseDeploymentTargetReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                                              | N/A                                                                             |
| `scope`                                                                         | *string*                                                                        | :heavy_check_mark:                                                              | Scope (project/resource level)                                                  |