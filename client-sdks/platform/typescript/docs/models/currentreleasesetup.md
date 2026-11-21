# CurrentReleaseSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { CurrentReleaseSetup } from "@alienplatform/platform-api/models";

let value: CurrentReleaseSetup = {
  cloudformation: [],
  helm: [],
  importers: [
    "aws",
  ],
  terraform: [
    "machines",
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `cloudformation`                                                                   | [models.CurrentReleaseCloudformation](../models/currentreleasecloudformation.md)[] | :heavy_check_mark:                                                                 | Platforms with a CloudFormation emitter.                                           |
| `helm`                                                                             | [models.CurrentReleaseHelm](../models/currentreleasehelm.md)[]                     | :heavy_check_mark:                                                                 | Platforms with a Helm emitter.                                                     |
| `importers`                                                                        | [models.CurrentReleaseImporter](../models/currentreleaseimporter.md)[]             | :heavy_check_mark:                                                                 | Platforms with an importer, which reads a resource a setup template created.       |
| `terraform`                                                                        | [models.CurrentReleaseTerraform](../models/currentreleaseterraform.md)[]           | :heavy_check_mark:                                                                 | Platforms with a Terraform emitter.                                                |