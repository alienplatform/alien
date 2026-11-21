# DeploymentPendingPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentPendingPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackSetup = {
  cloudformation: [
    "azure",
  ],
  helm: [],
  importers: [
    "gcp",
  ],
  terraform: [
    "machines",
  ],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `cloudformation`                                                                                                   | [models.DeploymentPendingPreparedStackCloudformation](../models/deploymentpendingpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                 | Platforms with a CloudFormation emitter.                                                                           |
| `helm`                                                                                                             | [models.DeploymentPendingPreparedStackHelm](../models/deploymentpendingpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                 | Platforms with a Helm emitter.                                                                                     |
| `importers`                                                                                                        | [models.DeploymentPendingPreparedStackImporter](../models/deploymentpendingpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                 | Platforms with an importer, which reads a resource a setup template created.                                       |
| `terraform`                                                                                                        | [models.DeploymentPendingPreparedStackTerraform](../models/deploymentpendingpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                 | Platforms with a Terraform emitter.                                                                                |