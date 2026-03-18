# SyncReconcileRequestCurrentReleaseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.SyncReconcileRequestCurrentReleaseExtendConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |