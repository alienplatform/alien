# DeploymentCredentialRotationStatus

## Example Usage

```typescript
import { DeploymentCredentialRotationStatus } from "@alienplatform/platform-api/models";

let value: DeploymentCredentialRotationStatus = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  revision: 796490,
  rotation: {
    id: "<id>",
    revision: 477003,
    status: "cancelled",
    expiresAt: "1745023978745",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      | Example                                                                          |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `deploymentId`                                                                   | *string*                                                                         | :heavy_check_mark:                                                               | Unique identifier for the deployment.                                            | dep_0c29fq4a2yjb7kx3smwdgxlc                                                     |
| `revision`                                                                       | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |                                                                                  |
| `rotation`                                                                       | [models.DeploymentCredentialRotation](../models/deploymentcredentialrotation.md) | :heavy_check_mark:                                                               | N/A                                                                              |                                                                                  |