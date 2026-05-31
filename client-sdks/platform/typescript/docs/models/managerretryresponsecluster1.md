# ManagerRetryResponseCluster1

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { ManagerRetryResponseCluster1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCluster1 = {
  ownership: "managed",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `cloud`                                                                              | *models.ManagerRetryResponseCloudUnion1*                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `namespace`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | Namespace where the Alien chart and application resources run.                       |
| `ownership`                                                                          | [models.ManagerRetryResponseOwnership1](../models/managerretryresponseownership1.md) | :heavy_check_mark:                                                                   | Ownership model for the Kubernetes cluster.                                          |