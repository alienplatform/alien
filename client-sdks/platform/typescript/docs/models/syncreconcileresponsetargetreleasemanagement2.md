# SyncReconcileResponseTargetReleaseManagement2

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseManagement2 = {
  override: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileResponseTargetReleaseOverrideUnion*[]>                                                        | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |