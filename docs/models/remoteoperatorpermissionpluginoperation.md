# RemoteOperatorPermissionPluginOperation

## Example Usage

```typescript
import { RemoteOperatorPermissionPluginOperation } from "@alienplatform/platform-api/models";

let value: RemoteOperatorPermissionPluginOperation = {
  name: "<value>",
  permissions: {
    aws: [],
    gcp: [],
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                                         | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `permissions`                                                                                                                  | [models.RemoteOperatorPermissionPluginPermissions](../models/remoteoperatorpermissionpluginpermissions.md)                     | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `kubernetesPermissions`                                                                                                        | [models.RemoteOperatorPermissionPluginKubernetesPermissions](../models/remoteoperatorpermissionpluginkubernetespermissions.md) | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |