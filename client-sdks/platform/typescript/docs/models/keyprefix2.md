# KeyPrefix2

## Example Usage

```typescript
import { KeyPrefix2 } from "@alienplatform/platform-api/models";

let value: KeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.KeyPrefixSecretRef2](../models/keyprefixsecretref2.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |
