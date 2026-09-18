# TargetDeploymentHost3

## Example Usage

```typescript
import { TargetDeploymentHost3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHost3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentHostSecretRef3](../models/targetdeploymenthostsecretref3.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |