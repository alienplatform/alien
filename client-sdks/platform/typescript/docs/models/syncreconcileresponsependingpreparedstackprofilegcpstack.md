# SyncReconcileResponsePendingPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `condition`                                                                  | *models.SyncReconcileResponsePendingPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                           | N/A                                                                          |
| `scope`                                                                      | *string*                                                                     | :heavy_check_mark:                                                           | Scope (project/resource level)                                               |
