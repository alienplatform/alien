# Database5

## Example Usage

```typescript
import { Database5 } from "@alienplatform/platform-api/models";

let value: Database5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.DatabaseSecretRef5](../models/databasesecretref5.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
