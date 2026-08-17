# TargetDeploymentPort5

## Example Usage

```typescript
import { TargetDeploymentPort5 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPort5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentPortSecretRef5](../models/targetdeploymentportsecretref5.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |