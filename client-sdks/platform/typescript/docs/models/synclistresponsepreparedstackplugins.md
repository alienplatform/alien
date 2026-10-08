# SyncListResponsePreparedStackPlugins

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { SyncListResponsePreparedStackPlugins } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackPlugins = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.SyncListResponsePreparedStackPluginsApprovalUnion*>                                        | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.SyncListResponsePreparedStackPluginsSettingsUnion*>                                        | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |