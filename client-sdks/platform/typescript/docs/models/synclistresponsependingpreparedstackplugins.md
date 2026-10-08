# SyncListResponsePendingPreparedStackPlugins

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackPlugins } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackPlugins = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.SyncListResponsePendingPreparedStackPluginsApprovalUnion*>                                 | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.SyncListResponsePendingPreparedStackPluginsSettingsUnion*>                                 | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |