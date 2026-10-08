# Database2

## Example Usage

```typescript
import { Database2 } from "@alienplatform/platform-api/models";

let value: Database2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef2](../models/databasesecretref2.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |