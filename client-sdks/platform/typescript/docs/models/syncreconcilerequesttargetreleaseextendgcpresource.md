# SyncReconcileRequestTargetReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `condition`                                                            | *models.SyncReconcileRequestTargetReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |
| `scope`                                                                | *string*                                                               | :heavy_check_mark:                                                     | Scope (project/resource level)                                         |