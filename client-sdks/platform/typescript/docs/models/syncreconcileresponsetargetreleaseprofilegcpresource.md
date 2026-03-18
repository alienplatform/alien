# SyncReconcileResponseTargetReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncReconcileResponseTargetReleaseProfileResourceConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |