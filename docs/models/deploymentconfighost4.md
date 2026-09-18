# DeploymentConfigHost4

## Example Usage

```typescript
import { DeploymentConfigHost4 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHost4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigHostSecretRef4](../models/deploymentconfighostsecretref4.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |