# PersistImportedDeploymentRequestPreparedStackSetup

The platforms each setup path supports for one resource type.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackSetup } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackSetup = {
  cloudformation: [
    "local",
  ],
  helm: [],
  importers: [
    "aws",
  ],
  terraform: [],
};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cloudformation`                                                                                                                                 | [models.PersistImportedDeploymentRequestPreparedStackCloudformation](../models/persistimporteddeploymentrequestpreparedstackcloudformation.md)[] | :heavy_check_mark:                                                                                                                               | Platforms with a CloudFormation emitter.                                                                                                         |
| `helm`                                                                                                                                           | [models.PersistImportedDeploymentRequestPreparedStackHelm](../models/persistimporteddeploymentrequestpreparedstackhelm.md)[]                     | :heavy_check_mark:                                                                                                                               | Platforms with a Helm emitter.                                                                                                                   |
| `importers`                                                                                                                                      | [models.PersistImportedDeploymentRequestPreparedStackImporter](../models/persistimporteddeploymentrequestpreparedstackimporter.md)[]             | :heavy_check_mark:                                                                                                                               | Platforms with an importer, which reads a resource a setup template created.                                                                     |
| `terraform`                                                                                                                                      | [models.PersistImportedDeploymentRequestPreparedStackTerraform](../models/persistimporteddeploymentrequestpreparedstackterraform.md)[]           | :heavy_check_mark:                                                                                                                               | Platforms with a Terraform emitter.                                                                                                              |