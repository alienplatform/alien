# DeploymentConfigUsername4

## Example Usage

```typescript
import { DeploymentConfigUsername4 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigUsernameSecretRef4](../models/deploymentconfigusernamesecretref4.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |