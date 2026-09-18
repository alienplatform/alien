# DeploymentConfigHost3

## Example Usage

```typescript
import { DeploymentConfigHost3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHost3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigHostSecretRef3](../models/deploymentconfighostsecretref3.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |