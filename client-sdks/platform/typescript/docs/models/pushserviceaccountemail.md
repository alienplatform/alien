# PushServiceAccountEmail

## Example Usage

```typescript
import { PushServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: PushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `secretRef`                                                                              | [models.PushServiceAccountEmailSecretRef](../models/pushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                       | Reference to a Kubernetes Secret                                                         |
