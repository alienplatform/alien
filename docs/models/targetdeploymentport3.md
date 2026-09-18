# TargetDeploymentPort3

## Example Usage

```typescript
import { TargetDeploymentPort3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentPortSecretRef3](../models/targetdeploymentportsecretref3.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |