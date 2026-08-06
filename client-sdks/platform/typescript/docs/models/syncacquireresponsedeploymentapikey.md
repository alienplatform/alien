# SyncAcquireResponseDeploymentApiKey

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentApiKey } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentApiKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                      | [models.SyncAcquireResponseDeploymentApiKeySecretRef](../models/syncacquireresponsedeploymentapikeysecretref.md) | :heavy_check_mark:                                                                                               | Reference to a Kubernetes Secret                                                                                 |