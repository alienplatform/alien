# TargetDeploymentHost2

## Example Usage

```typescript
import { TargetDeploymentHost2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHost2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentHostSecretRef2](../models/targetdeploymenthostsecretref2.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |