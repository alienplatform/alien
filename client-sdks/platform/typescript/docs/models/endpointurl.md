# EndpointUrl

## Example Usage

```typescript
import { EndpointUrl } from "@alienplatform/platform-api/models";

let value: EndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.EndpointUrlSecretRef](../models/endpointurlsecretref.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |