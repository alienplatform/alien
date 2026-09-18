# DeploymentConfigPort3

## Example Usage

```typescript
import { DeploymentConfigPort3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.DeploymentConfigPortSecretRef3](../models/deploymentconfigportsecretref3.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |