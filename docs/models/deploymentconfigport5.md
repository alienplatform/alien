# DeploymentConfigPort5

## Example Usage

```typescript
import { DeploymentConfigPort5 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPort5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigPortSecretRef5](../models/deploymentconfigportsecretref5.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |