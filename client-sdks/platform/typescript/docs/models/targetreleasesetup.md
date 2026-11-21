# TargetReleaseSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { TargetReleaseSetup } from "@alienplatform/platform-api/models";

let value: TargetReleaseSetup = {
  cloudformation: [
    "azure",
  ],
  helm: [],
  importers: [
    "kubernetes",
  ],
  terraform: [
    "kubernetes",
  ],
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cloudformation`                                                                 | [models.TargetReleaseCloudformation](../models/targetreleasecloudformation.md)[] | :heavy_check_mark:                                                               | Platforms with a CloudFormation emitter.                                         |
| `helm`                                                                           | [models.TargetReleaseHelm](../models/targetreleasehelm.md)[]                     | :heavy_check_mark:                                                               | Platforms with a Helm emitter.                                                   |
| `importers`                                                                      | [models.TargetReleaseImporter](../models/targetreleaseimporter.md)[]             | :heavy_check_mark:                                                               | Platforms with an importer, which reads a resource a setup template created.     |
| `terraform`                                                                      | [models.TargetReleaseTerraform](../models/targetreleaseterraform.md)[]           | :heavy_check_mark:                                                               | Platforms with a Terraform emitter.                                              |