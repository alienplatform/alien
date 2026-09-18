# DeploymentConfigUsername3

## Example Usage

```typescript
import { DeploymentConfigUsername3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigUsernameSecretRef3](../models/deploymentconfigusernamesecretref3.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |