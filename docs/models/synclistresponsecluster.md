# SyncListResponseCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { SyncListResponseCluster } from "@alienplatform/platform-api/models";

let value: SyncListResponseCluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cloud`                                                                    | *models.SyncListResponseCloudUnion*                                        | :heavy_minus_sign:                                                         | N/A                                                                        |
| `namespace`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | Namespace where the Alien chart and application resources run.             |
| `ownership`                                                                | [models.SyncListResponseOwnership](../models/synclistresponseownership.md) | :heavy_check_mark:                                                         | Ownership model for the Kubernetes cluster.                                |