# PreparedDeploymentStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { PreparedDeploymentStackSetup } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackSetup = {
  cloudformation: [],
  helm: [
    "gcp",
  ],
  importers: [],
  terraform: [],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                     | [models.PreparedDeploymentStackCloudformation](../models/prepareddeploymentstackcloudformation.md)[] | :heavy_check_mark:                                                                                   | Platforms with a CloudFormation emitter.                                                             |
| `helm`                                                                                               | [models.PreparedDeploymentStackHelm](../models/prepareddeploymentstackhelm.md)[]                     | :heavy_check_mark:                                                                                   | Platforms with a Helm emitter.                                                                       |
| `importers`                                                                                          | [models.PreparedDeploymentStackImporter](../models/prepareddeploymentstackimporter.md)[]             | :heavy_check_mark:                                                                                   | Platforms with an importer, which reads a resource a setup template created.                         |
| `terraform`                                                                                          | [models.PreparedDeploymentStackTerraform](../models/prepareddeploymentstackterraform.md)[]           | :heavy_check_mark:                                                                                   | Platforms with a Terraform emitter.                                                                  |