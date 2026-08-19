# DeploymentConfigStaticIp

## Example Usage

```typescript
import { DeploymentConfigStaticIp } from "@alienplatform/platform-api/models";

let value: DeploymentConfigStaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.DeploymentConfigStaticIpSecretRef](../models/deploymentconfigstaticipsecretref.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |