# BindingValueVecString

A Kubernetes Secret reference (must come before Expression)

## Example Usage

```typescript
import { BindingValueVecString } from "@alienplatform/manager-api/models";

let value: BindingValueVecString = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretRef`                                            | [models.SecretReference](../models/secretreference.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |