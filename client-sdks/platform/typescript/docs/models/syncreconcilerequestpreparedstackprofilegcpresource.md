# SyncReconcileRequestPreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `condition`                                                             | *models.SyncReconcileRequestPreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                      | N/A                                                                     |
| `scope`                                                                 | *string*                                                                | :heavy_check_mark:                                                      | Scope (project/resource level)                                          |