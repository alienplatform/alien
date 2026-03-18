# SyncAcquireResponseCurrentReleaseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncAcquireResponseCurrentReleaseOverrideResourceConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |