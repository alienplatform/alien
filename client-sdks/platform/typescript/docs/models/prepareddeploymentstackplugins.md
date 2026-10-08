# PreparedDeploymentStackPlugins

Settings and approval rules for one plugin.

## Example Usage

```typescript
import { PreparedDeploymentStackPlugins } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackPlugins = {};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.PreparedDeploymentStackPluginsApprovalUnion*>                                              | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.PreparedDeploymentStackPluginsSettingsUnion*>                                              | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |