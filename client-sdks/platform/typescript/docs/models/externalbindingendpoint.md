# ExternalBindingEndpoint

## Example Usage

```typescript
import { ExternalBindingEndpoint } from "@alienplatform/platform-api/models";

let value: ExternalBindingEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.EndpointSecretRef](../models/endpointsecretref.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |
