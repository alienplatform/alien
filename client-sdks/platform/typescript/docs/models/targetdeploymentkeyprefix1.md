# TargetDeploymentKeyPrefix1

## Example Usage

```typescript
import { TargetDeploymentKeyPrefix1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentKeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentKeyPrefixSecretRef1](../models/targetdeploymentkeyprefixsecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |