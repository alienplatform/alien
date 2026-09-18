# DeploymentCredentialRotation

## Example Usage

```typescript
import { DeploymentCredentialRotation } from "@alienplatform/platform-api/models";

let value: DeploymentCredentialRotation = {
  id: "<id>",
  revision: 239715,
  status: "completed",
  expiresAt: "1753287885720",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `revision`                                                                                           | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `status`                                                                                             | [models.DeploymentCredentialRotationStatusEnum](../models/deploymentcredentialrotationstatusenum.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `expiresAt`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |