# SyncReconcileRequestCurrentReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `condition`                                                      | *models.SyncReconcileRequestCurrentReleaseProfileConditionUnion* | :heavy_minus_sign:                                               | N/A                                                              |
| `scope`                                                          | *string*                                                         | :heavy_check_mark:                                               | Scope (project/resource level)                                   |