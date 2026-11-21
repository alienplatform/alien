# SyncListResponsePendingPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackSetup = {
  cloudformation: [],
  helm: [],
  importers: [],
  terraform: [
    "machines",
  ],
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `cloudformation`                                                                                                               | [models.SyncListResponsePendingPreparedStackCloudformation](../models/synclistresponsependingpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                             | Platforms with a CloudFormation emitter.                                                                                       |
| `helm`                                                                                                                         | [models.SyncListResponsePendingPreparedStackHelm](../models/synclistresponsependingpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                             | Platforms with a Helm emitter.                                                                                                 |
| `importers`                                                                                                                    | [models.SyncListResponsePendingPreparedStackImporter](../models/synclistresponsependingpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                             | Platforms with an importer, which reads a resource a setup template created.                                                   |
| `terraform`                                                                                                                    | [models.SyncListResponsePendingPreparedStackTerraform](../models/synclistresponsependingpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                             | Platforms with a Terraform emitter.                                                                                            |