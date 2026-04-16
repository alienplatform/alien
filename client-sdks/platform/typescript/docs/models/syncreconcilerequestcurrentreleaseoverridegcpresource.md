# SyncReconcileRequestCurrentReleaseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `condition`                                                               | *models.SyncReconcileRequestCurrentReleaseOverrideResourceConditionUnion* | :heavy_minus_sign:                                                        | N/A                                                                       |
| `scope`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | Scope (project/resource level)                                            |