# ExternalPasswordSecretRef

## Example Usage

```typescript
import { ExternalPasswordSecretRef } from "@alienplatform/manager-api/models";

let value: ExternalPasswordSecretRef = {
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
