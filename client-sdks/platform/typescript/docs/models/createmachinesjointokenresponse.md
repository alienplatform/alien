# CreateMachinesJoinTokenResponse

## Example Usage

```typescript
import { CreateMachinesJoinTokenResponse } from "@alienplatform/platform-api/models";

let value: CreateMachinesJoinTokenResponse = {
  joinToken: "<value>",
  controlPlaneUrl: "https://sugary-doing.net/",
  clusterId: "<id>",
  token: {
    id: "<id>",
    createdAt: "1728144387221",
    createdBy: "<value>",
    joinCount: 196349,
  },
  cliInstallScriptUrl: "https://grubby-affect.info/",
  cliCommandName: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `joinToken`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `controlPlaneUrl`                                                        | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `clusterId`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `token`                                                                  | [models.MachinesJoinTokenSummary](../models/machinesjointokensummary.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `cliInstallScriptUrl`                                                    | *string*                                                                 | :heavy_check_mark:                                                       | Deploy CLI install script URL, or null when no ready CLI package exists. |
| `cliCommandName`                                                         | *string*                                                                 | :heavy_check_mark:                                                       | CLI command name to use in join instructions.                            |