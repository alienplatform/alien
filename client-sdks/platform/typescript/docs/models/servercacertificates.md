# ServerCaCertificates

## Example Usage

```typescript
import { ServerCaCertificates } from "@alienplatform/platform-api/models";

let value: ServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `secretRef`                                                                        | [models.ServerCaCertificatesSecretRef](../models/servercacertificatessecretref.md) | :heavy_check_mark:                                                                 | Reference to a Kubernetes Secret                                                   |