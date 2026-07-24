# SyncReconcileRequestPendingPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `condition`                                                  | *models.PendingPreparedStackProfileStateStackConditionUnion* | :heavy_minus_sign:                                           | N/A                                                          |
| `scope`                                                      | *string*                                                     | :heavy_check_mark:                                           | Scope (project/resource level)                               |
