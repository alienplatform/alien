# DatabaseId

## Example Usage

```typescript
import { DatabaseId } from "@alienplatform/platform-api/models";

let value: DatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.DatabaseIdSecretRef](../models/databaseidsecretref.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |