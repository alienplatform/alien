# RemoteOperatorPermissionPlugin

## Example Usage

```typescript
import { RemoteOperatorPermissionPlugin } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPlugin = {
  name: "<value>",
  operations: [
    {
      name: "<value>",
      permissions: {
        aws: [],
        gcp: [],
      },
    },
  ],
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `operations`                                                                                             | [models.RemoteOperatorPermissionPluginOperation](../models/remoteoperatorpermissionpluginoperation.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |