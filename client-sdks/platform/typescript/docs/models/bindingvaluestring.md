# BindingValueString

## Example Usage

```typescript
import { BindingValueString } from "@alienplatform/platform-api/models";

let value: BindingValueString = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.BindingValueStringSecretRef](../models/bindingvaluestringsecretref.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |