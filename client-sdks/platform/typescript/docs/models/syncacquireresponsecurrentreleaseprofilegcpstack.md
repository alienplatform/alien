# SyncAcquireResponseCurrentReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.SyncAcquireResponseCurrentReleaseProfileConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |