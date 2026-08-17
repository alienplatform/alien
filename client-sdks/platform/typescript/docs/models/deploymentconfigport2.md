# DeploymentConfigPort2

## Example Usage

```typescript
import { DeploymentConfigPort2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigPortSecretRef2](../models/deploymentconfigportsecretref2.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |