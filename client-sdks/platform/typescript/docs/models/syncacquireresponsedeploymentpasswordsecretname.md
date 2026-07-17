# SyncAcquireResponseDeploymentPasswordSecretName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPasswordSecretName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                              | [models.SyncAcquireResponseDeploymentPasswordSecretNameSecretRef](../models/syncacquireresponsedeploymentpasswordsecretnamesecretref.md) | :heavy_check_mark:                                                                                                                       | Reference to a Kubernetes Secret                                                                                                         |