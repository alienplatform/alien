# SyncListResponsePendingPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `condition`                                                                      | *models.SyncListResponsePendingPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scope`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Scope (project/resource level)                                                   |