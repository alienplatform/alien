# AcceptRemoteOperatorPermissionsRequestBody

## Example Usage

```typescript
import { AcceptRemoteOperatorPermissionsRequestBody } from "@alienplatform/platform-api/models/operations";

let value: AcceptRemoteOperatorPermissionsRequestBody = {
  reviewedPermissions: [],
};
```

## Fields

| Field                                                                                     | Type                                                                                      | Required                                                                                  | Description                                                                               |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `reviewedPermissions`                                                                     | [models.RemoteOperatorPermissionPlugin](../../models/remoteoperatorpermissionplugin.md)[] | :heavy_check_mark:                                                                        | The enabled operations' permission declarations that the owner reviewed and re-applied.   |