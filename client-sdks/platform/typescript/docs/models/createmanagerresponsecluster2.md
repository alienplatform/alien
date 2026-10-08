# CreateManagerResponseCluster2

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { CreateManagerResponseCluster2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCluster2 = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `cloud`                                                                                | *models.CreateManagerResponseCloudUnion2*                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | Namespace where the Alien chart and application resources run.                         |
| `ownership`                                                                            | [models.CreateManagerResponseOwnership2](../models/createmanagerresponseownership2.md) | :heavy_check_mark:                                                                     | Ownership model for the Kubernetes cluster.                                            |