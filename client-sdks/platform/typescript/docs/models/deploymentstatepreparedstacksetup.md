# DeploymentStatePreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentStatePreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackSetup = {
  cloudformation: [
    "kubernetes",
  ],
  helm: [
    "test",
  ],
  importers: [
    "aws",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                               | [models.DeploymentStatePreparedStackCloudformation](../models/deploymentstatepreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                             | Platforms with a CloudFormation emitter.                                                                       |
| `helm`                                                                                                         | [models.DeploymentStatePreparedStackHelm](../models/deploymentstatepreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                             | Platforms with a Helm emitter.                                                                                 |
| `importers`                                                                                                    | [models.DeploymentStatePreparedStackImporter](../models/deploymentstatepreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                             | Platforms with an importer, which reads a resource a setup template created.                                   |
| `terraform`                                                                                                    | [models.DeploymentStatePreparedStackTerraform](../models/deploymentstatepreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                             | Platforms with a Terraform emitter.                                                                            |