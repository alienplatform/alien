# SyncAcquireResponseTargetReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `condition`                                                            | *models.SyncAcquireResponseTargetReleaseProfileResourceConditionUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |
| `scope`                                                                | *string*                                                               | :heavy_check_mark:                                                     | Scope (project/resource level)                                         |