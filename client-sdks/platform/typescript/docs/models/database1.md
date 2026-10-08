# Database1

## Example Usage

```typescript
import { Database1 } from "@alienplatform/platform-api/models";

let value: Database1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef1](../models/databasesecretref1.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
