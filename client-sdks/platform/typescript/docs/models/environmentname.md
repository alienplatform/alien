# EnvironmentName

## Example Usage

```typescript
import { EnvironmentName } from "@alienplatform/platform-api/models";

let value: EnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `secretRef`                                                              | [models.EnvironmentNameSecretRef](../models/environmentnamesecretref.md) | :heavy_check_mark:                                                       | Reference to a Kubernetes Secret                                         |
