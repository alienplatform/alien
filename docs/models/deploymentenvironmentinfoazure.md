# DeploymentEnvironmentInfoAzure

Azure-specific environment information

## Example Usage

```typescript
import { DeploymentEnvironmentInfoAzure } from "@alienplatform/platform-api/models";

let value: DeploymentEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `location`                                                             | *string*                                                               | :heavy_check_mark:                                                     | Azure location/region                                                  |
| `subscriptionId`                                                       | *string*                                                               | :heavy_check_mark:                                                     | Azure subscription ID                                                  |
| `tenantId`                                                             | *string*                                                               | :heavy_check_mark:                                                     | Azure tenant ID                                                        |
| `platform`                                                             | [models.DeploymentPlatformAzure](../models/deploymentplatformazure.md) | :heavy_check_mark:                                                     | N/A                                                                    |