# ManagerRetryResponseSetup

## Example Usage

```typescript
import { ManagerRetryResponseSetup } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseSetup = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  setupStatus: "pending",
  setupToken: "<value>",
  setupTokenId: "<id>",
  deploymentLink: "<value>",
  setupConfig: {
    metadata: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [],
    },
    environmentVariables: [
      {
        name: "<value>",
        type: "secret",
        targetResources: null,
      },
    ],
  },
  setup: {
    method: "terraform",
    deploymentPortalUrl: "https://confused-majority.net/",
    managerUrl: "https://deadly-cruelty.org/",
    providerSource: "<value>",
    moduleSource: "<value>",
    moduleInputs: {},
    mainTf: "<value>",
    tfvars: "<value>",
    commands: "<value>",
    stackSettings: {},
  },
  mode: "setup",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            | Example                                                                                |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `managerId`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    | mgr_enxscjrqiiu2lrc672hwwuc5                                                           |
| `setupStatus`                                                                          | *"pending"*                                                                            | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `setupToken`                                                                           | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `setupTokenId`                                                                         | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `deploymentLink`                                                                       | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `setupConfig`                                                                          | [models.ManagerRetryResponseSetupConfig](../models/managerretryresponsesetupconfig.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `setup`                                                                                | *models.ManagerRetryResponseSetupUnion*                                                | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |
| `mode`                                                                                 | [models.ModeSetup](../models/modesetup.md)                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |                                                                                        |