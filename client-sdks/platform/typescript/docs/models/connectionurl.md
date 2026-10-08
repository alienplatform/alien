# ConnectionUrl

## Example Usage

```typescript
import { ConnectionUrl } from "@alienplatform/platform-api/models";

let value: ConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `secretRef`                                                          | [models.ConnectionUrlSecretRef](../models/connectionurlsecretref.md) | :heavy_check_mark:                                                   | Reference to a Kubernetes Secret                                     |
