# SyncListResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncListResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: SyncListResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `federationTrust`                                                                                              | *models.SyncListResponseFederationTrustUnion*                                                                  | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `serviceAccountEmail`                                                                                          | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Linked source account metadata; never a direct target trust grant.                                             |
| `platform`                                                                                                     | [models.SyncListResponseManagementConfigPlatformGcp](../models/synclistresponsemanagementconfigplatformgcp.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |