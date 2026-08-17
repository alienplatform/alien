# DeploymentConfigUsername1

## Example Usage

```typescript
import { DeploymentConfigUsername1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigUsernameSecretRef1](../models/deploymentconfigusernamesecretref1.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |