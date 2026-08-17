# DeploymentConfigDatabase3

## Example Usage

```typescript
import { DeploymentConfigDatabase3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDatabase3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.DeploymentConfigDatabaseSecretRef3](../models/deploymentconfigdatabasesecretref3.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |