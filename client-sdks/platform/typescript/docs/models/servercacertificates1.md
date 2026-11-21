# ServerCaCertificates1

## Example Usage

```typescript
import { ServerCaCertificates1 } from "@alienplatform/platform-api/models";

let value: ServerCaCertificates1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.ServerCaCertificatesSecretRef1](../models/servercacertificatessecretref1.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |