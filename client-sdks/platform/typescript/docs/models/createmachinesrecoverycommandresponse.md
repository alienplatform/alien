# CreateMachinesRecoveryCommandResponse

## Example Usage

```typescript
import { CreateMachinesRecoveryCommandResponse } from "@alienplatform/platform-api/models";

let value: CreateMachinesRecoveryCommandResponse = {
  joinToken: "<value>",
  controlPlaneUrl: "https://paltry-mainstream.org/",
  clusterId: "<id>",
  expiresAt: "1762214091174",
  cliInstallScriptUrl: "https://feline-mozzarella.org",
  cliCommandName: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `joinToken`                                                                   | *string*                                                                      | :heavy_check_mark:                                                            | One-use recovery token for the join command. It works once, for this machine. |
| `controlPlaneUrl`                                                             | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `clusterId`                                                                   | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `expiresAt`                                                                   | *string*                                                                      | :heavy_check_mark:                                                            | When the recovery token stops working (ISO 8601).                             |
| `cliInstallScriptUrl`                                                         | *string*                                                                      | :heavy_check_mark:                                                            | Deploy CLI install script URL, or null when no ready CLI package exists.      |
| `cliCommandName`                                                              | *string*                                                                      | :heavy_check_mark:                                                            | CLI command name to use in join instructions.                                 |