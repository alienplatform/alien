# PasswordSecretName

## Example Usage

```typescript
import { PasswordSecretName } from "@alienplatform/platform-api/models";

let value: PasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.PasswordSecretNameSecretRef](../models/passwordsecretnamesecretref.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |
