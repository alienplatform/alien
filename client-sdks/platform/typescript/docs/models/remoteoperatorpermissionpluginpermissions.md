# RemoteOperatorPermissionPluginPermissions

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginPermissions } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginPermissions = {
  aws: [
    {
      effect: "Allow",
      actions: [
        "<value 1>",
      ],
      resources: [
        "<value 1>",
        "<value 2>",
      ],
      condition: {},
      reason: "<value>",
    },
  ],
  gcp: [],
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `aws`                                                                                        | [models.RemoteOperatorPermissionPluginAw](../models/remoteoperatorpermissionpluginaw.md)[]   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `gcp`                                                                                        | [models.RemoteOperatorPermissionPluginGcp](../models/remoteoperatorpermissionplugingcp.md)[] | :heavy_check_mark:                                                                           | N/A                                                                                          |