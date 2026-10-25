# ClusterEndpoint2

## Example Usage

```typescript
import { ClusterEndpoint2 } from "@alienplatform/platform-api/models";

let value: ClusterEndpoint2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `secretRef`                                                                | [models.ClusterEndpointSecretRef2](../models/clusterendpointsecretref2.md) | :heavy_check_mark:                                                         | Reference to a Kubernetes Secret                                           |