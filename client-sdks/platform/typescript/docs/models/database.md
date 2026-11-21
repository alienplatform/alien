# Database

## Example Usage

```typescript
import { Database } from "@alienplatform/platform-api/models";

let value: Database = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.DatabaseSecretRef](../models/databasesecretref.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |