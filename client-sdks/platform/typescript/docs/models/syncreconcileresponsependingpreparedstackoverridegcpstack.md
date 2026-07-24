# SyncReconcileResponsePendingPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `condition`                                                                   | *models.SyncReconcileResponsePendingPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                            | N/A                                                                           |
| `scope`                                                                       | *string*                                                                      | :heavy_check_mark:                                                            | Scope (project/resource level)                                                |
