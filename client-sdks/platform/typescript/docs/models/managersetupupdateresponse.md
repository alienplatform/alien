# ManagerSetupUpdateResponse

## Example Usage

```typescript
import { ManagerSetupUpdateResponse } from "@alienplatform/platform-api/models";

let value: ManagerSetupUpdateResponse = {
  setupMethod: "cloudformation",
  setupToken: "<value>",
  expiresAt: "1761863911108",
  templateUrl: "https://insignificant-pinstripe.net/",
  stackId: "<id>",
  region: "<value>",
  availableSetupUpdates: [
    {
      inputId: "<id>",
      label: "<value>",
      description: "plus grok about serialize stiffen hm railway mmm whereas",
    },
  ],
  deploymentRegions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                                                                                                                                             | Type                                                                                                                                                              | Required                                                                                                                                                          | Description                                                                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `setupMethod`                                                                                                                                                     | [models.ManagerSetupUpdateResponseSetupMethod](../models/managersetupupdateresponsesetupmethod.md)                                                                | :heavy_check_mark:                                                                                                                                                | N/A                                                                                                                                                               |
| `setupToken`                                                                                                                                                      | *string*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                | A fresh setup token. Set the stack's Token parameter to it.                                                                                                       |
| `expiresAt`                                                                                                                                                       | *string*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                | When the setup token expires                                                                                                                                      |
| `templateUrl`                                                                                                                                                     | *string*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                | The newest setup template                                                                                                                                         |
| `stackId`                                                                                                                                                         | *string*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                | The instance's CloudFormation stack ARN                                                                                                                           |
| `stackName`                                                                                                                                                       | *string*                                                                                                                                                          | :heavy_minus_sign:                                                                                                                                                | The instance's CloudFormation stack name                                                                                                                          |
| `region`                                                                                                                                                          | *string*                                                                                                                                                          | :heavy_check_mark:                                                                                                                                                | Region of the stack                                                                                                                                               |
| `availableSetupUpdates`                                                                                                                                           | [models.AvailableSetupUpdate](../models/availablesetupupdate.md)[]                                                                                                | :heavy_check_mark:                                                                                                                                                | N/A                                                                                                                                                               |
| `deploymentRegions`                                                                                                                                               | *string*[]                                                                                                                                                        | :heavy_check_mark:                                                                                                                                                | Regions the instance's registry will copy images to. Enter them, comma separated, in the stack's deployment regions parameter. Empty leaves replication as it is. |