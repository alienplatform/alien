# DeploymentDetailResponsePreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackSetup = {
  cloudformation: [
    "gcp",
  ],
  helm: [],
  importers: [
    "test",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                                                 | [models.DeploymentDetailResponsePreparedStackCloudformation](../models/deploymentdetailresponsepreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                               | Platforms with a CloudFormation emitter.                                                                                         |
| `helm`                                                                                                                           | [models.DeploymentDetailResponsePreparedStackHelm](../models/deploymentdetailresponsepreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                               | Platforms with a Helm emitter.                                                                                                   |
| `importers`                                                                                                                      | [models.DeploymentDetailResponsePreparedStackImporter](../models/deploymentdetailresponsepreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                               | Platforms with an importer, which reads a resource a setup template created.                                                     |
| `terraform`                                                                                                                      | [models.DeploymentDetailResponsePreparedStackTerraform](../models/deploymentdetailresponsepreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                               | Platforms with a Terraform emitter.                                                                                              |