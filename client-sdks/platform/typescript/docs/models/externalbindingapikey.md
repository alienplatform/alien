# ExternalBindingApiKey

## Example Usage

```typescript
import { ExternalBindingApiKey } from "@alienplatform/platform-api/models";

let value: ExternalBindingApiKey = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretRef`                                            | [models.ApiKeySecretRef](../models/apikeysecretref.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |