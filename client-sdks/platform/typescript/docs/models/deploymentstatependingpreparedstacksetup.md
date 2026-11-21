# DeploymentStatePendingPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackSetup = {
  cloudformation: [],
  helm: [
    "machines",
  ],
  importers: [
    "kubernetes",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                                             | [models.DeploymentStatePendingPreparedStackCloudformation](../models/deploymentstatependingpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                           | Platforms with a CloudFormation emitter.                                                                                     |
| `helm`                                                                                                                       | [models.DeploymentStatePendingPreparedStackHelm](../models/deploymentstatependingpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                           | Platforms with a Helm emitter.                                                                                               |
| `importers`                                                                                                                  | [models.DeploymentStatePendingPreparedStackImporter](../models/deploymentstatependingpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                           | Platforms with an importer, which reads a resource a setup template created.                                                 |
| `terraform`                                                                                                                  | [models.DeploymentStatePendingPreparedStackTerraform](../models/deploymentstatependingpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                           | Platforms with a Terraform emitter.                                                                                          |