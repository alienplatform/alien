# SyncReconcileResponseTargetReleaseOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `condition`                                                       | *models.SyncReconcileResponseTargetReleaseOverrideConditionUnion* | :heavy_minus_sign:                                                | N/A                                                               |
| `scope`                                                           | *string*                                                          | :heavy_check_mark:                                                | Scope (project/resource level)                                    |