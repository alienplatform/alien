# SyncAcquireResponseDeploymentConnectionUrl

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentConnectionUrl } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                                    | [models.SyncAcquireResponseDeploymentConnectionUrlSecretRef](../models/syncacquireresponsedeploymentconnectionurlsecretref.md) | :heavy_check_mark:                                                                                                             | Reference to a Kubernetes Secret                                                                                               |