# Database4

## Example Usage

```typescript
import { Database4 } from "@alienplatform/platform-api/models";

let value: Database4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef4](../models/databasesecretref4.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |