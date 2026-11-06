# DataDir

## Example Usage

```typescript
import { DataDir } from "@alienplatform/platform-api/models";

let value: DataDir = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `secretRef`                                              | [models.DataDirSecretRef](../models/datadirsecretref.md) | :heavy_check_mark:                                       | Reference to a Kubernetes Secret                         |