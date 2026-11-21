# ServerCaCertificates2

## Example Usage

```typescript
import { ServerCaCertificates2 } from "@alienplatform/platform-api/models";

let value: ServerCaCertificates2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `secretRef`                                                                          | [models.ServerCaCertificatesSecretRef2](../models/servercacertificatessecretref2.md) | :heavy_check_mark:                                                                   | Reference to a Kubernetes Secret                                                     |