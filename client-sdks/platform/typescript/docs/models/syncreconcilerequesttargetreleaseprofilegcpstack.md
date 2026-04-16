# SyncReconcileRequestTargetReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.SyncReconcileRequestTargetReleaseProfileConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |