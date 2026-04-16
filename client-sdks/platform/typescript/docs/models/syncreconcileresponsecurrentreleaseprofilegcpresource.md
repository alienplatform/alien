# SyncReconcileResponseCurrentReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `condition`                                                               | *models.SyncReconcileResponseCurrentReleaseProfileResourceConditionUnion* | :heavy_minus_sign:                                                        | N/A                                                                       |
| `scope`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | Scope (project/resource level)                                            |