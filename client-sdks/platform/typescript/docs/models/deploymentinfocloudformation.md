# DeploymentInfoCloudformation

## Example Usage

```typescript
import { DeploymentInfoCloudformation } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCloudformation = {
  status: "ready",
  mode: "outputs",
  launchUrl: "https://bulky-metal.net",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `status`                                                           | [models.CloudformationStatus](../models/cloudformationstatus.md)   | :heavy_check_mark:                                                 | Status of a package build                                          |
| `version`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `outputs`                                                          | [models.CloudformationOutputs](../models/cloudformationoutputs.md) | :heavy_minus_sign:                                                 | Outputs from a CloudFormation package build                        |
| `error`                                                            | *any*                                                              | :heavy_minus_sign:                                                 | N/A                                                                |
| `mode`                                                             | [models.DeploymentInfoMode](../models/deploymentinfomode.md)       | :heavy_check_mark:                                                 | N/A                                                                |
| `launchUrl`                                                        | *string*                                                           | :heavy_check_mark:                                                 | CloudFormation launch URL                                          |
| `outputsSchema`                                                    | *any*                                                              | :heavy_minus_sign:                                                 | N/A                                                                |