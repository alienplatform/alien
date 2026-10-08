# DeploymentDeployerSecret

The state of one deployer secret slot, reported with the deployment.

## Example Usage

```typescript
import { DeploymentDeployerSecret } from "@alienplatform/platform-api/models";

let value: DeploymentDeployerSecret = {
  inputId: "<id>",
  label: "<value>",
  location: {
    cliCommand: "<value>",
    name: "<value>",
    store: "gcp-secret-manager",
  },
  required: false,
  status: "missing",
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `inputId`                                                                                                                                | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Stack input id.                                                                                                                          |
| `label`                                                                                                                                  | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | The input's label.                                                                                                                       |
| `location`                                                                                                                               | [models.DeploymentLocation](../models/deploymentlocation.md)                                                                             | :heavy_check_mark:                                                                                                                       | Where a deployer writes a vault-native secret.                                                                                           |
| `message`                                                                                                                                | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Why the slot is invalid.                                                                                                                 |
| `required`                                                                                                                               | *boolean*                                                                                                                                | :heavy_check_mark:                                                                                                                       | Whether the workload cannot start without it.                                                                                            |
| `status`                                                                                                                                 | [models.DeploymentRuntimeMetadataStatus](../models/deploymentruntimemetadatastatus.md)                                                   | :heavy_check_mark:                                                                                                                       | Whether a deployer secret slot holds a usable value. Alien learns this from<br/>metadata only and never reads the value.                 |
| `version`                                                                                                                                | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | The secret store's version of the present value (never the value or a<br/>hash of it). A new version reaches workloads with the next update. |
