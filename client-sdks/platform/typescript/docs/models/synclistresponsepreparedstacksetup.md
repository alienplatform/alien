# SyncListResponsePreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { SyncListResponsePreparedStackSetup } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackSetup = {
  cloudformation: [
    "test",
  ],
  helm: [
    "aws",
  ],
  importers: [
    "test",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                                 | [models.SyncListResponsePreparedStackCloudformation](../models/synclistresponsepreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                               | Platforms with a CloudFormation emitter.                                                                         |
| `helm`                                                                                                           | [models.SyncListResponsePreparedStackHelm](../models/synclistresponsepreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                               | Platforms with a Helm emitter.                                                                                   |
| `importers`                                                                                                      | [models.SyncListResponsePreparedStackImporter](../models/synclistresponsepreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                               | Platforms with an importer, which reads a resource a setup template created.                                     |
| `terraform`                                                                                                      | [models.SyncListResponsePreparedStackTerraform](../models/synclistresponsepreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                               | Platforms with a Terraform emitter.                                                                              |