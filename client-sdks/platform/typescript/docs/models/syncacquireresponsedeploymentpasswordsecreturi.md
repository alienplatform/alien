# SyncAcquireResponseDeploymentPasswordSecretUri

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPasswordSecretUri } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                            | [models.SyncAcquireResponseDeploymentPasswordSecretUriSecretRef](../models/syncacquireresponsedeploymentpasswordsecreturisecretref.md) | :heavy_check_mark:                                                                                                                     | Reference to a Kubernetes Secret                                                                                                       |