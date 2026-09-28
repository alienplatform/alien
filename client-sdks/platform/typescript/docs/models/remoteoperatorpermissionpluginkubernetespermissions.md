# RemoteOperatorPermissionPluginKubernetesPermissions

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginKubernetesPermissions } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginKubernetesPermissions = {
  rules: [
    {
      apiGroup: "<value>",
      resource: "<value>",
      verbs: [
        "<value 1>",
        "<value 2>",
        "<value 3>",
      ],
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `rules`                                                                                        | [models.RemoteOperatorPermissionPluginRule](../models/remoteoperatorpermissionpluginrule.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |