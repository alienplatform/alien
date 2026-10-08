# SyncListResponsePendingPreparedStackCustom

A published custom plugin at an exact version.

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackCustom } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackCustom = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.SyncListResponsePendingPreparedStackCustomApprovalUnion*>                                  | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.SyncListResponsePendingPreparedStackCustomSettingsUnion*>                                  | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |
| `name`                                                                                                            | *string*                                                                                                          | :heavy_check_mark:                                                                                                | Plugin name as published.                                                                                         |
| `version`                                                                                                         | *string*                                                                                                          | :heavy_check_mark:                                                                                                | Exact published version.                                                                                          |