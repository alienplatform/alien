# DeploymentPreparedStackPlugins

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { DeploymentPreparedStackPlugins } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackPlugins = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.DeploymentPreparedStackPluginsApprovalUnion*>                                              | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.DeploymentPreparedStackPluginsSettingsUnion*>                                              | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |