# TargetDeploymentPort2

## Example Usage

```typescript
import { TargetDeploymentPort2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentPortSecretRef2](../models/targetdeploymentportsecretref2.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |