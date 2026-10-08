# DataDir3

## Example Usage

```typescript
import { DataDir3 } from "@alienplatform/platform-api/models";

let value: DataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.DataDirSecretRef3](../models/datadirsecretref3.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |
