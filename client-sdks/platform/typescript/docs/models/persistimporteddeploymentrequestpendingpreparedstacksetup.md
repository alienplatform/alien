# PersistImportedDeploymentRequestPendingPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackSetup = {
  cloudformation: [
    "kubernetes",
  ],
  helm: [
    "gcp",
  ],
  importers: [],
  terraform: [],
};
```

## Fields

| Field                                                                                                                                                          | Type                                                                                                                                                           | Required                                                                                                                                                       | Description                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cloudformation`                                                                                                                                               | [models.PersistImportedDeploymentRequestPendingPreparedStackCloudformation](../models/persistimporteddeploymentrequestpendingpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                                                             | Platforms with a CloudFormation emitter.                                                                                                                       |
| `helm`                                                                                                                                                         | [models.PersistImportedDeploymentRequestPendingPreparedStackHelm](../models/persistimporteddeploymentrequestpendingpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                                                             | Platforms with a Helm emitter.                                                                                                                                 |
| `importers`                                                                                                                                                    | [models.PersistImportedDeploymentRequestPendingPreparedStackImporter](../models/persistimporteddeploymentrequestpendingpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                                                             | Platforms with an importer, which reads a resource a setup template created.                                                                                   |
| `terraform`                                                                                                                                                    | [models.PersistImportedDeploymentRequestPendingPreparedStackTerraform](../models/persistimporteddeploymentrequestpendingpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                                                             | Platforms with a Terraform emitter.                                                                                                                            |