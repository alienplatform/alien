# CreateManagerResponse

## Example Usage

```typescript
import { CreateManagerResponse } from "@alienplatform/platform-api/models";

let value: CreateManagerResponse = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  setupStatus: "pending",
  setupToken: "<value>",
  setupTokenId: "<id>",
  deploymentLink: "<value>",
  setupConfig: {
    metadata: {
      "key": "<value>",
      "key1": "<value>",
    },
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [
        "google-oauth",
      ],
    },
    environmentVariables: [],
  },
  setup: {
    method: "terraform",
    deploymentPortalUrl: "https://quixotic-sport.net/",
    managerUrl: "https://unsightly-trolley.com",
    providerSource: "<value>",
    moduleSource: "<value>",
    moduleInputs: {
      "key": "<value>",
      "key1": "<value>",
    },
    mainTf: "<value>",
    tfvars: "<value>",
    commands: "<value>",
    stackSettings: {},
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              | Example                                                                                  |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `managerId`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      | mgr_enxscjrqiiu2lrc672hwwuc5                                                             |
| `setupStatus`                                                                            | [models.CreateManagerResponseSetupStatus](../models/createmanagerresponsesetupstatus.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `setupToken`                                                                             | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `setupTokenId`                                                                           | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `deploymentLink`                                                                         | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `setupConfig`                                                                            | [models.SetupConfig](../models/setupconfig.md)                                           | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `setup`                                                                                  | *models.Setup*                                                                           | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |