# DeploymentSetupProgressRequest

## Example Usage

```typescript
import { DeploymentSetupProgressRequest } from "@alienplatform/platform-api/models";

let value: DeploymentSetupProgressRequest = {
  event: "stack-rolled-back",
  cloudFormation: {
    stackId: "<id>",
    requestId: "<id>",
    logicalResourceId: "<id>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `event`                                                                                                          | [models.EventEnum](../models/eventenum.md)                                                                       | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `cloudFormation`                                                                                                 | [models.DeploymentSetupProgressRequestCloudFormation](../models/deploymentsetupprogressrequestcloudformation.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |