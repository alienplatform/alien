# DataDir2

## Example Usage

```typescript
import { DataDir2 } from "@alienplatform/platform-api/models";

let value: DataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.DataDirSecretRef2](../models/datadirsecretref2.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |