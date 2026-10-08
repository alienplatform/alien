# PreparedDeploymentStackCustom

A published custom plugin at an exact version.

## Example Usage

```typescript
import { PreparedDeploymentStackCustom } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackCustom = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `approval`                                                                                                        | Record<string, *models.PreparedDeploymentStackCustomApprovalUnion*>                                               | :heavy_minus_sign:                                                                                                | Approval rule per operation: an operation name, or `*` for all of them.<br/>Operations no rule matches need approval. |
| `settings`                                                                                                        | Record<string, *models.PreparedDeploymentStackCustomSettingsUnion*>                                               | :heavy_minus_sign:                                                                                                | Values for the settings the plugin's manifest declares.                                                           |
| `name`                                                                                                            | *string*                                                                                                          | :heavy_check_mark:                                                                                                | Plugin name as published.                                                                                         |
| `version`                                                                                                         | *string*                                                                                                          | :heavy_check_mark:                                                                                                | Exact published version.                                                                                          |