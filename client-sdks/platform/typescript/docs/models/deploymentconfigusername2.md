# DeploymentConfigUsername2

## Example Usage

```typescript
import { DeploymentConfigUsername2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigUsernameSecretRef2](../models/deploymentconfigusernamesecretref2.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |