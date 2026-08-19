# DeploymentConfigHost2

## Example Usage

```typescript
import { DeploymentConfigHost2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHost2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigHostSecretRef2](../models/deploymentconfighostsecretref2.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |