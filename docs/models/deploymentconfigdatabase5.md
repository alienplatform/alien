# DeploymentConfigDatabase5

## Example Usage

```typescript
import { DeploymentConfigDatabase5 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef5](../models/deploymentconfigdatabasesecretref5.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |