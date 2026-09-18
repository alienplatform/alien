# CreateManagerResponseCluster1

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { CreateManagerResponseCluster1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCluster1 = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `cloud`                                                                                | *models.CreateManagerResponseCloudUnion1*                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | Namespace where the Alien chart and application resources run.                         |
| `ownership`                                                                            | [models.CreateManagerResponseOwnership1](../models/createmanagerresponseownership1.md) | :heavy_check_mark:                                                                     | Ownership model for the Kubernetes cluster.                                            |