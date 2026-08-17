# DeploymentConfigContainerName

## Example Usage

```typescript
import { DeploymentConfigContainerName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.DeploymentConfigContainerNameSecretRef](../models/deploymentconfigcontainernamesecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |