# ImportSourceCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { ImportSourceCluster } from "@alienplatform/platform-api/models";

let value: ImportSourceCluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `cloud`                                                            | *models.ImportSourceCloudUnion*                                    | :heavy_minus_sign:                                                 | N/A                                                                |
| `namespace`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | Namespace where the Alien chart and application resources run.     |
| `ownership`                                                        | [models.ImportSourceOwnership](../models/importsourceownership.md) | :heavy_check_mark:                                                 | Ownership model for the Kubernetes cluster.                        |