# ExternalBindingEndpoint2

## Example Usage

```typescript
import { ExternalBindingEndpoint2 } from "@alienplatform/platform-api/models";

let value: ExternalBindingEndpoint2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.EndpointSecretRef2](../models/endpointsecretref2.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |