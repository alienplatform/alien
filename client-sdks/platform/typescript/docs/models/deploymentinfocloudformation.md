# DeploymentInfoCloudformation

## Example Usage

```typescript
import { DeploymentInfoCloudformation } from "@aliendotdev/platform-api/models";

let value: DeploymentInfoCloudformation = {
  status: "ready",
  launchUrl: "https://miserly-bourgeoisie.name",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `status`                                                           | [models.CloudformationStatus](../models/cloudformationstatus.md)   | :heavy_check_mark:                                                 | Status of a package build                                          |
| `version`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `outputs`                                                          | [models.CloudformationOutputs](../models/cloudformationoutputs.md) | :heavy_minus_sign:                                                 | Outputs from a CloudFormation package build                        |
| `error`                                                            | *any*                                                              | :heavy_minus_sign:                                                 | N/A                                                                |
| `launchUrl`                                                        | *string*                                                           | :heavy_check_mark:                                                 | CloudFormation launch URL                                          |