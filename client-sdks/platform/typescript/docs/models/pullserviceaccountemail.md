# PullServiceAccountEmail

## Example Usage

```typescript
import { PullServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: PullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `secretRef`                                                                              | [models.PullServiceAccountEmailSecretRef](../models/pullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                       | Reference to a Kubernetes Secret                                                         |
