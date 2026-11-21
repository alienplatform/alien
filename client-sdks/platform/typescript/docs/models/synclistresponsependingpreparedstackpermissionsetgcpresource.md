# SyncListResponsePendingPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `condition`                                                                         | *models.SyncListResponsePendingPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                                  | N/A                                                                                 |
| `scope`                                                                             | *string*                                                                            | :heavy_check_mark:                                                                  | Scope (project/resource level)                                                      |