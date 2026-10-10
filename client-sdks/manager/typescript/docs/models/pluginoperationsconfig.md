# PluginOperationsConfig

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { PluginOperationsConfig } from "@alienplatform/manager-api/models";

let value: PluginOperationsConfig = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.OperationApprovalUnion*>                                                                   | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.OperationSettingValueUnion*>                                                               | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |
