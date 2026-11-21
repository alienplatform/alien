# DeploymentPreparationResponse

## Example Usage

```typescript
import { DeploymentPreparationResponse } from "@alienplatform/platform-api/models";

let value: DeploymentPreparationResponse = {
  installer: {
    name: "<value>",
    platform: "azure",
    method: "helm",
    configuration: "<value>",
    command: "<value>",
  },
  token: "<value>",
  encryptionKey: "<value>",
  reference: "6d40b24d-de0c-46af-8049-ef0b5e79284d",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                | Example                                    |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `installer`                                | [models.Installer](../models/installer.md) | :heavy_check_mark:                         | N/A                                        |                                            |
| `token`                                    | *string*                                   | :heavy_check_mark:                         | N/A                                        |                                            |
| `encryptionKey`                            | *string*                                   | :heavy_check_mark:                         | N/A                                        |                                            |
| `reference`                                | *string*                                   | :heavy_check_mark:                         | N/A                                        |                                            |
| `deploymentId`                             | *string*                                   | :heavy_check_mark:                         | Unique identifier for the deployment.      | dep_0c29fq4a2yjb7kx3smwdgxlc               |