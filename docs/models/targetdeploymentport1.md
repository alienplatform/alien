# TargetDeploymentPort1

## Example Usage

```typescript
import { TargetDeploymentPort1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPort1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.TargetDeploymentPortSecretRef1](../models/targetdeploymentportsecretref1.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |