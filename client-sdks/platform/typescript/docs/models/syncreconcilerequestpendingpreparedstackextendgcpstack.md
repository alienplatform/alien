# SyncReconcileRequestPendingPreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `condition`                                                 | *models.PendingPreparedStackExtendStateStackConditionUnion* | :heavy_minus_sign:                                          | N/A                                                         |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | Scope (project/resource level)                              |
