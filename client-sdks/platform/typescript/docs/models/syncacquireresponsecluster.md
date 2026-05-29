# SyncAcquireResponseCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { SyncAcquireResponseCluster } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cloud`                                                                          | *models.SyncAcquireResponseCloudUnion*                                           | :heavy_minus_sign:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | Namespace where the Alien chart and application resources run.                   |
| `ownership`                                                                      | [models.SyncAcquireResponseOwnership](../models/syncacquireresponseownership.md) | :heavy_check_mark:                                                               | Ownership model for the Kubernetes cluster.                                      |