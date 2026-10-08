# SyncListResponseEnvironmentInfoAzure

Azure-specific environment information

## Example Usage

```typescript
import { SyncListResponseEnvironmentInfoAzure } from "@alienplatform/platform-api/models";

let value: SyncListResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `location`                                                                                                       | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Azure location/region                                                                                            |
| `subscriptionId`                                                                                                 | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Azure subscription ID                                                                                            |
| `tenantId`                                                                                                       | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Azure tenant ID                                                                                                  |
| `platform`                                                                                                       | [models.SyncListResponseEnvironmentInfoPlatformAzure](../models/synclistresponseenvironmentinfoplatformazure.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |