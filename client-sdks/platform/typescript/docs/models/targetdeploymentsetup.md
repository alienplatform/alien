# TargetDeploymentSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { TargetDeploymentSetup } from "@alienplatform/platform-api/models";

let value: TargetDeploymentSetup = {
  cloudformation: [
    "gcp",
  ],
  helm: [
    "kubernetes",
  ],
  importers: [
    "test",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `cloudformation`                                                                       | [models.TargetDeploymentCloudformation](../models/targetdeploymentcloudformation.md)[] | :heavy_check_mark:                                                                     | Platforms with a CloudFormation emitter.                                               |
| `helm`                                                                                 | [models.TargetDeploymentHelm](../models/targetdeploymenthelm.md)[]                     | :heavy_check_mark:                                                                     | Platforms with a Helm emitter.                                                         |
| `importers`                                                                            | [models.TargetDeploymentImporter](../models/targetdeploymentimporter.md)[]             | :heavy_check_mark:                                                                     | Platforms with an importer, which reads a resource a setup template created.           |
| `terraform`                                                                            | [models.TargetDeploymentTerraform](../models/targetdeploymentterraform.md)[]           | :heavy_check_mark:                                                                     | Platforms with a Terraform emitter.                                                    |