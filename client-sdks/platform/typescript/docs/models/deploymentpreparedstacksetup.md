# DeploymentPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackSetup = {
  cloudformation: [],
  helm: [],
  importers: [
    "machines",
  ],
  terraform: [
    "aws",
  ],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                     | [models.DeploymentPreparedStackCloudformation](../models/deploymentpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                   | Platforms with a CloudFormation emitter.                                                             |
| `helm`                                                                                               | [models.DeploymentPreparedStackHelm](../models/deploymentpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                   | Platforms with a Helm emitter.                                                                       |
| `importers`                                                                                          | [models.DeploymentPreparedStackImporter](../models/deploymentpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                   | Platforms with an importer, which reads a resource a setup template created.                         |
| `terraform`                                                                                          | [models.DeploymentPreparedStackTerraform](../models/deploymentpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                   | Platforms with a Terraform emitter.                                                                  |