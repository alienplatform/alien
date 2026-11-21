# ClusterEndpoint1

## Example Usage

```typescript
import { ClusterEndpoint1 } from "@alienplatform/platform-api/models";

let value: ClusterEndpoint1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `secretRef`                                                                | [models.ClusterEndpointSecretRef1](../models/clusterendpointsecretref1.md) | :heavy_check_mark:                                                         | Reference to a Kubernetes Secret                                           |