# ReleaseDeploymentItemEnvironmentInfoAzure

Azure-specific environment information

## Example Usage

```typescript
import { ReleaseDeploymentItemEnvironmentInfoAzure } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `location`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Azure location/region                                                                        |
| `subscriptionId`                                                                             | *string*                                                                                     | :heavy_check_mark:                                                                           | Azure subscription ID                                                                        |
| `tenantId`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Azure tenant ID                                                                              |
| `platform`                                                                                   | [models.ReleaseDeploymentItemPlatformAzure](../models/releasedeploymentitemplatformazure.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
