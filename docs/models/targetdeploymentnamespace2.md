# TargetDeploymentNamespace2

## Example Usage

```typescript
import { TargetDeploymentNamespace2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentNamespaceSecretRef2](../models/targetdeploymentnamespacesecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |