# DeploymentDetailResponsePendingPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackSetup = {
  cloudformation: [],
  helm: [],
  importers: [],
  terraform: [],
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                                                               | [models.DeploymentDetailResponsePendingPreparedStackCloudformation](../models/deploymentdetailresponsependingpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                                             | Platforms with a CloudFormation emitter.                                                                                                       |
| `helm`                                                                                                                                         | [models.DeploymentDetailResponsePendingPreparedStackHelm](../models/deploymentdetailresponsependingpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                                             | Platforms with a Helm emitter.                                                                                                                 |
| `importers`                                                                                                                                    | [models.DeploymentDetailResponsePendingPreparedStackImporter](../models/deploymentdetailresponsependingpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                                             | Platforms with an importer, which reads a resource a setup template created.                                                                   |
| `terraform`                                                                                                                                    | [models.DeploymentDetailResponsePendingPreparedStackTerraform](../models/deploymentdetailresponsependingpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                                             | Platforms with a Terraform emitter.                                                                                                            |