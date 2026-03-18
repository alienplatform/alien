# SyncReconcileResponsePreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncReconcileResponsePreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |