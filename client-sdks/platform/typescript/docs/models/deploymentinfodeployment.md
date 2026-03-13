# DeploymentInfoDeployment

Deployment details (present when using a deployment-scoped token)

## Example Usage

```typescript
import { DeploymentInfoDeployment } from "@aliendotdev/platform-api/models";

let value: DeploymentInfoDeployment = {
  name: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `platform`                                                           | [models.DeploymentInfoPlatform](../models/deploymentinfoplatform.md) | :heavy_check_mark:                                                   | Represents the target cloud platform.                                |