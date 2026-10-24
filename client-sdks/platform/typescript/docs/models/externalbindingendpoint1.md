# ExternalBindingEndpoint1

## Example Usage

```typescript
import { ExternalBindingEndpoint1 } from "@alienplatform/platform-api/models";

let value: ExternalBindingEndpoint1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.EndpointSecretRef1](../models/endpointsecretref1.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |