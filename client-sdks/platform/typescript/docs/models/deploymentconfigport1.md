# DeploymentConfigPort1

## Example Usage

```typescript
import { DeploymentConfigPort1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPort1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigPortSecretRef1](../models/deploymentconfigportsecretref1.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |