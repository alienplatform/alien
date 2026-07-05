# SyncAcquireResponseClusterEndpoint

## Example Usage

```typescript
import { SyncAcquireResponseClusterEndpoint } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseClusterEndpointSecretRef](../models/syncacquireresponseclusterendpointsecretref.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |