# ManagerRetryResponseCluster2

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { ManagerRetryResponseCluster2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCluster2 = {
  ownership: "managed",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `cloud`                                                                              | *models.ManagerRetryResponseCloudUnion2*                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `namespace`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | Namespace where the Alien chart and application resources run.                       |
| `ownership`                                                                          | [models.ManagerRetryResponseOwnership2](../models/managerretryresponseownership2.md) | :heavy_check_mark:                                                                   | Ownership model for the Kubernetes cluster.                                          |