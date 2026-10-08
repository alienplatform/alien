# SecretAccessKey

## Example Usage

```typescript
import { SecretAccessKey } from "@alienplatform/platform-api/models";

let value: SecretAccessKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `secretRef`                                                              | [models.SecretAccessKeySecretRef](../models/secretaccesskeysecretref.md) | :heavy_check_mark:                                                       | Reference to a Kubernetes Secret                                         |
