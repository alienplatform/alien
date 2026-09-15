# Metadata

The complete canonical metadata.json from the uploaded bundle.

## Example Usage

```typescript
import { Metadata } from "@alienplatform/platform-api/models";

let value: Metadata = {
  name: "<value>",
  version: "<value>",
  binaries: {
    arm64: "<value>",
  },
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `name`                                           | *string*                                         | :heavy_check_mark:                               | N/A                                              |
| `version`                                        | *string*                                         | :heavy_check_mark:                               | N/A                                              |
| `tier`                                           | [models.MetadataTier](../models/metadatatier.md) | :heavy_minus_sign:                               | N/A                                              |
| `binaries`                                       | *models.Binaries*                                | :heavy_check_mark:                               | N/A                                              |
| `operations`                                     | [models.Operation](../models/operation.md)[]     | :heavy_minus_sign:                               | N/A                                              |