# PasswordSecretArn2

## Example Usage

```typescript
import { PasswordSecretArn2 } from "@alienplatform/platform-api/models";

let value: PasswordSecretArn2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.PasswordSecretArnSecretRef2](../models/passwordsecretarnsecretref2.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |