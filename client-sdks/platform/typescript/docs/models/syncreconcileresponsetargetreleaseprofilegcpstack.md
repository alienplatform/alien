# SyncReconcileResponseTargetReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `condition`                                                      | *models.SyncReconcileResponseTargetReleaseProfileConditionUnion* | :heavy_minus_sign:                                               | N/A                                                              |
| `scope`                                                          | *string*                                                         | :heavy_check_mark:                                               | Scope (project/resource level)                                   |