# TableName1

## Example Usage

```typescript
import { TableName1 } from "@alienplatform/platform-api/models";

let value: TableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.TableNameSecretRef1](../models/tablenamesecretref1.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |
