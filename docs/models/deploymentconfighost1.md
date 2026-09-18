# DeploymentConfigHost1

## Example Usage

```typescript
import { DeploymentConfigHost1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHost1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigHostSecretRef1](../models/deploymentconfighostsecretref1.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |