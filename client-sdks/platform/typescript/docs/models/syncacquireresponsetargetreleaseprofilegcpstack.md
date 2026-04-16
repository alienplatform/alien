# SyncAcquireResponseTargetReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `condition`                                                    | *models.SyncAcquireResponseTargetReleaseProfileConditionUnion* | :heavy_minus_sign:                                             | N/A                                                            |
| `scope`                                                        | *string*                                                       | :heavy_check_mark:                                             | Scope (project/resource level)                                 |