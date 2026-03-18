# SyncAcquireResponseTargetReleaseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `condition`                                                   | *models.SyncAcquireResponseTargetReleaseExtendConditionUnion* | :heavy_minus_sign:                                            | N/A                                                           |
| `scope`                                                       | *string*                                                      | :heavy_check_mark:                                            | Scope (project/resource level)                                |