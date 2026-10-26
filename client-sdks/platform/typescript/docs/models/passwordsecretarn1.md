# PasswordSecretArn1

## Example Usage

```typescript
import { PasswordSecretArn1 } from "@alienplatform/platform-api/models";

let value: PasswordSecretArn1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.PasswordSecretArnSecretRef1](../models/passwordsecretarnsecretref1.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |