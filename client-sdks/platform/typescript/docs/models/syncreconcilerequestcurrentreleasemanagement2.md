# SyncReconcileRequestCurrentReleaseManagement2

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseManagement2 = {
  override: {
    "key": [],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileRequestCurrentReleaseOverrideUnion*[]>                                                        | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |