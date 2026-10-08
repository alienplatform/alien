# Database6

## Example Usage

```typescript
import { Database6 } from "@alienplatform/platform-api/models";

let value: Database6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef6](../models/databasesecretref6.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
