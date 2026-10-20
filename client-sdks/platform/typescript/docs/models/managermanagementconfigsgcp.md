# ManagerManagementConfigsGcp

## Example Usage

```typescript
import { ManagerManagementConfigsGcp } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigsGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `federationTrust`                                                                              | *models.ManagerManagementConfigsFederationTrustUnion*                                          | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `serviceAccountEmail`                                                                          | *string*                                                                                       | :heavy_check_mark:                                                                             | Linked source account metadata; never a direct target trust grant.                             |
| `platform`                                                                                     | [models.ManagerManagementConfigsPlatformGcp](../models/managermanagementconfigsplatformgcp.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |