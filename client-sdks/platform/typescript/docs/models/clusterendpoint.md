# ClusterEndpoint

## Example Usage

```typescript
import { ClusterEndpoint } from "@alienplatform/platform-api/models";

let value: ClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `secretRef`                                                              | [models.ClusterEndpointSecretRef](../models/clusterendpointsecretref.md) | :heavy_check_mark:                                                       | Reference to a Kubernetes Secret                                         |