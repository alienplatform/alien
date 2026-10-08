# DeploymentCredentialRotationEvent

## Example Usage

```typescript
import { DeploymentCredentialRotationEvent } from "@alienplatform/platform-api/models";

let value: DeploymentCredentialRotationEvent = {
  type: "DeploymentCredentialRotation",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  rotationId: "<id>",
  revision: 519446,
  status: "cancelled",
  previousKeyId: "<id>",
  candidateKeyId: "<id>",
  actor: {
    kind: "serviceAccount",
    id: "<id>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            | Example                                                                                                |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `type`                                                                                                 | *"DeploymentCredentialRotation"*                                                                       | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `deploymentId`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | Unique identifier for the deployment.                                                                  | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                           |
| `rotationId`                                                                                           | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `revision`                                                                                             | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `status`                                                                                               | [models.DeploymentCredentialRotationEventStatus](../models/deploymentcredentialrotationeventstatus.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `previousKeyId`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `candidateKeyId`                                                                                       | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `actor`                                                                                                | [models.DeploymentCredentialRotationEventActor](../models/deploymentcredentialrotationeventactor.md)   | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |