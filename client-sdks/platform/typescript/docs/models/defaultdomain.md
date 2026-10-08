# DefaultDomain

## Example Usage

```typescript
import { DefaultDomain } from "@alienplatform/platform-api/models";

let value: DefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `secretRef`                                                          | [models.DefaultDomainSecretRef](../models/defaultdomainsecretref.md) | :heavy_check_mark:                                                   | Reference to a Kubernetes Secret                                     |
