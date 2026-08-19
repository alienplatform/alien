# TargetDeploymentNamespace1

## Example Usage

```typescript
import { TargetDeploymentNamespace1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentNamespace1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentNamespaceSecretRef1](../models/targetdeploymentnamespacesecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |