# SyncAcquireResponseDeploymentNamespace2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentNamespace2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentNamespaceSecretRef2](../models/syncacquireresponsedeploymentnamespacesecretref2.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |