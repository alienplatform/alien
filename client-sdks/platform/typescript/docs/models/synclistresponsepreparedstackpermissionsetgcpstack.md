# SyncListResponsePreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponsePreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `condition`                                                               | *models.SyncListResponsePreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                        | N/A                                                                       |
| `scope`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | Scope (project/resource level)                                            |