# SyncReconcileRequestPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `condition`                                                          | *models.SyncReconcileRequestPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                   | N/A                                                                  |
| `scope`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Scope (project/resource level)                                       |