# SyncReconcileResponsePreparedStackManagement2

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackManagement2 = {
  override: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileResponsePreparedStackOverrideUnion*[]>                                                        | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |