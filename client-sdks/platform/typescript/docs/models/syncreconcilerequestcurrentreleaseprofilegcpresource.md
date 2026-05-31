# SyncReconcileRequestCurrentReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `condition`                                               | *models.CurrentReleaseProfileStateResourceConditionUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `scope`                                                   | *string*                                                  | :heavy_check_mark:                                        | Scope (project/resource level)                            |