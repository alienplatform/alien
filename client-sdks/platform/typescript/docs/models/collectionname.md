# CollectionName

## Example Usage

```typescript
import { CollectionName } from "@alienplatform/platform-api/models";

let value: CollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `secretRef`                                                            | [models.CollectionNameSecretRef](../models/collectionnamesecretref.md) | :heavy_check_mark:                                                     | Reference to a Kubernetes Secret                                       |