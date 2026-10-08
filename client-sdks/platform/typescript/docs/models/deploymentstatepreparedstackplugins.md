# DeploymentStatePreparedStackPlugins

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { DeploymentStatePreparedStackPlugins } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackPlugins = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.DeploymentStatePreparedStackPluginsApprovalUnion*>                                         | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.DeploymentStatePreparedStackPluginsSettingsUnion*>                                         | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |