# SyncReconcileResponseCurrentReleaseManagement2

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseManagement2 = {
  override: {
    "key": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileResponseCurrentReleaseOverrideUnion*[]>                                                       | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |