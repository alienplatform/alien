# SyncReconcileRequestTargetReleaseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncReconcileRequestTargetReleaseOverrideResourceConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |