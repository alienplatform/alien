# SyncReconcileRequestEnvironmentInfoAzure

Azure-specific environment information

## Example Usage

```typescript
import { SyncReconcileRequestEnvironmentInfoAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `location`                                                                                 | *string*                                                                                   | :heavy_check_mark:                                                                         | Azure location/region                                                                      |
| `subscriptionId`                                                                           | *string*                                                                                   | :heavy_check_mark:                                                                         | Azure subscription ID                                                                      |
| `tenantId`                                                                                 | *string*                                                                                   | :heavy_check_mark:                                                                         | Azure tenant ID                                                                            |
| `platform`                                                                                 | [models.SyncReconcileRequestPlatformAzure](../models/syncreconcilerequestplatformazure.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |