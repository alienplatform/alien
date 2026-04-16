# SyncReconcileResponsePreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `condition`                                                          | *models.SyncReconcileResponsePreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                                   | N/A                                                                  |
| `scope`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Scope (project/resource level)                                       |