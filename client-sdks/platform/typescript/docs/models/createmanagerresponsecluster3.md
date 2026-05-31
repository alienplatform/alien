# CreateManagerResponseCluster3

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { CreateManagerResponseCluster3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCluster3 = {
  ownership: "managed",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `cloud`                                                                                | *models.CreateManagerResponseCloudUnion3*                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | Namespace where the Alien chart and application resources run.                         |
| `ownership`                                                                            | [models.CreateManagerResponseOwnership3](../models/createmanagerresponseownership3.md) | :heavy_check_mark:                                                                     | Ownership model for the Kubernetes cluster.                                            |