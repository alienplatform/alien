# ResumeDeploymentInstallationResponse

## Example Usage

```typescript
import { ResumeDeploymentInstallationResponse } from "@alienplatform/platform-api/models";

let value: ResumeDeploymentInstallationResponse = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  token: "<value>",
  installation: {
    method: "cloudformation",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    | Example                                                                                                        |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                 | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Unique identifier for the deployment.                                                                          | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                   |
| `token`                                                                                                        | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `runtimeEncryptionKey`                                                                                         | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `managementExternalId`                                                                                         | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | CloudFormation and Terraform installs: the ExternalId the saved launch URL or module configuration leaves out. |                                                                                                                |
| `installation`                                                                                                 | [models.DeploymentInstallation](../models/deploymentinstallation.md)                                           | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |