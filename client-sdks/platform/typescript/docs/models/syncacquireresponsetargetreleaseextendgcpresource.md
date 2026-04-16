# SyncAcquireResponseTargetReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `condition`                                                           | *models.SyncAcquireResponseTargetReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                                    | N/A                                                                   |
| `scope`                                                               | *string*                                                              | :heavy_check_mark:                                                    | Scope (project/resource level)                                        |