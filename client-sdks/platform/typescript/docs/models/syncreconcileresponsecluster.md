# SyncReconcileResponseCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { SyncReconcileResponseCluster } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `cloud`                                                                              | *models.SyncReconcileResponseCloudUnion*                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `namespace`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | Namespace where the Alien chart and application resources run.                       |
| `ownership`                                                                          | [models.SyncReconcileResponseOwnership](../models/syncreconcileresponseownership.md) | :heavy_check_mark:                                                                   | Ownership model for the Kubernetes cluster.                                          |