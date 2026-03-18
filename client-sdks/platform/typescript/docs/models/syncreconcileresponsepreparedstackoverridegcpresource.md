# SyncReconcileResponsePreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `condition`                                                               | *models.SyncReconcileResponsePreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                        | N/A                                                                       |
| `scope`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | Scope (project/resource level)                                            |