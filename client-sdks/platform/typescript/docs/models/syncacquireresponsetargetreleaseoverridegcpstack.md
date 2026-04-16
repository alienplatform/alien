# SyncAcquireResponseTargetReleaseOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.SyncAcquireResponseTargetReleaseOverrideConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |