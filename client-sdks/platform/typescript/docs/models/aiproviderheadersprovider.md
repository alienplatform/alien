# AIProviderHeadersProvider

## Example Usage

```typescript
import { AIProviderHeadersProvider } from "@alienplatform/platform-api/models";

let value: AIProviderHeadersProvider = {
  provider: "gcp-vertex",
  headers: [
    {
      name: "<value>",
      value: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `provider`                                                                         | [models.AIProviderHeadersProviderEnum](../models/aiproviderheadersproviderenum.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `headers`                                                                          | [models.Header](../models/header.md)[]                                             | :heavy_check_mark:                                                                 | N/A                                                                                |