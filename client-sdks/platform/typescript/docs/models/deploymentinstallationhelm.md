# DeploymentInstallationHelm

## Example Usage

```typescript
import { DeploymentInstallationHelm } from "@alienplatform/platform-api/models";

let value: DeploymentInstallationHelm = {
  command: "<value>",
  managerUrl: "https://authorized-molasses.net/",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `command`                                                                                                                  | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `upgradeCommand`                                                                                                           | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |
| `managerUrl`                                                                                                               | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `values`                                                                                                                   | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |
| `takesEnrollmentCode`                                                                                                      | *boolean*                                                                                                                  | :heavy_minus_sign:                                                                                                         | The chart takes a one-use operator enrollment code. Ask the manager's helm-values route for one; older charts get a token. |