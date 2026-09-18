# RotateMachinesJoinTokenResponse

## Example Usage

```typescript
import { RotateMachinesJoinTokenResponse } from "@alienplatform/platform-api/models";

let value: RotateMachinesJoinTokenResponse = {
  joinToken: "<value>",
  controlPlaneUrl: "https://untried-harp.net",
  clusterId: "<id>",
  token: {
    id: "<id>",
    createdAt: "1728144387221",
    createdBy: "<value>",
    joinCount: 196349,
  },
  cliInstallScriptUrl: null,
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