# PersistImportedDeploymentRequestPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPermissions } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPermissions = {
  profiles: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.PersistImportedDeploymentRequestManagementUnion*                                                                           | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.PersistImportedDeploymentRequestProfileUnion*[]>>                                            | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |