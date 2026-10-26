# Database7

## Example Usage

```typescript
import { Database7 } from "@alienplatform/platform-api/models";

let value: Database7 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef7](../models/databasesecretref7.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |