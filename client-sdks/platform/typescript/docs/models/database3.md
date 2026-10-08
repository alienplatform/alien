# Database3

## Example Usage

```typescript
import { Database3 } from "@alienplatform/platform-api/models";

let value: Database3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef3](../models/databasesecretref3.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |