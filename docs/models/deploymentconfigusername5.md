# DeploymentConfigUsername5

## Example Usage

```typescript
import { DeploymentConfigUsername5 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigUsernameSecretRef5](../models/deploymentconfigusernamesecretref5.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |