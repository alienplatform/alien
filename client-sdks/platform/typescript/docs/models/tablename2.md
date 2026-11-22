# TableName2

## Example Usage

```typescript
import { TableName2 } from "@alienplatform/platform-api/models";

let value: TableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.TableNameSecretRef2](../models/tablenamesecretref2.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |