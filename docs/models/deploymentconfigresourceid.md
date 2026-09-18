# DeploymentConfigResourceId

## Example Usage

```typescript
import { DeploymentConfigResourceId } from "@alienplatform/platform-api/models";

let value: DeploymentConfigResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.DeploymentConfigResourceIdSecretRef](../models/deploymentconfigresourceidsecretref.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |