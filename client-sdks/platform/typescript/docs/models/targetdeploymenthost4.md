# TargetDeploymentHost4

## Example Usage

```typescript
import { TargetDeploymentHost4 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHost4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentHostSecretRef4](../models/targetdeploymenthostsecretref4.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |