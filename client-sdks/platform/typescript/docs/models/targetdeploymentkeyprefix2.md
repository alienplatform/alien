# TargetDeploymentKeyPrefix2

## Example Usage

```typescript
import { TargetDeploymentKeyPrefix2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentKeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentKeyPrefixSecretRef2](../models/targetdeploymentkeyprefixsecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |