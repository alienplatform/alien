# DeploymentConfigPort4

## Example Usage

```typescript
import { DeploymentConfigPort4 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPort4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigPortSecretRef4](../models/deploymentconfigportsecretref4.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |