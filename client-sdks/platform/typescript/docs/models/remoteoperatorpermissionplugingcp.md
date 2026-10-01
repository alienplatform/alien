# RemoteOperatorPermissionPluginGcp

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginGcp } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginGcp = {
  permissions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  scope: "projects/${projectName}",
  reason: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `permissions`                                                                                  | *string*[]                                                                                     | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `scope`                                                                                        | [models.RemoteOperatorPermissionPluginScope](../models/remoteoperatorpermissionpluginscope.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `reason`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |