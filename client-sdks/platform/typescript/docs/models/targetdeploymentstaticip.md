# TargetDeploymentStaticIp

## Example Usage

```typescript
import { TargetDeploymentStaticIp } from "@alienplatform/platform-api/models";

let value: TargetDeploymentStaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.TargetDeploymentStaticIpSecretRef](../models/targetdeploymentstaticipsecretref.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |