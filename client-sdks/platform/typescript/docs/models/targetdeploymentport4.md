# TargetDeploymentPort4

## Example Usage

```typescript
import { TargetDeploymentPort4 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPort4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentPortSecretRef4](../models/targetdeploymentportsecretref4.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |