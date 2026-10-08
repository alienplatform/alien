# KeyPrefix1

## Example Usage

```typescript
import { KeyPrefix1 } from "@alienplatform/platform-api/models";

let value: KeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.KeyPrefixSecretRef1](../models/keyprefixsecretref1.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |