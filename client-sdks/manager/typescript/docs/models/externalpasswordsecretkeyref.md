# ExternalPasswordSecretKeyRef

## Example Usage

```typescript
import { ExternalPasswordSecretKeyRef } from "@alienplatform/manager-api/models";

let value: ExternalPasswordSecretKeyRef = {
  secretKeyRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretKeyRef`                                         | [models.SecretReference](../models/secretreference.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |
