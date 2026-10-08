# RemoteOperatorPermissionPluginAw

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginAw } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginAw = {
  effect: "Deny",
  actions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  resources: [
    "<value 1>",
  ],
  condition: {
    "key": {
      "key": "<value>",
      "key1": "<value>",
    },
  },
  reason: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `effect`                                                                                         | [models.RemoteOperatorPermissionPluginEffect](../models/remoteoperatorpermissionplugineffect.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `actions`                                                                                        | *string*[]                                                                                       | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `resources`                                                                                      | *string*[]                                                                                       | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `condition`                                                                                      | Record<string, Record<string, *string*>>                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `reason`                                                                                         | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |