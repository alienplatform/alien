# ExtendReleaseInfoAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { ExtendReleaseInfoAzure } from "@aliendotdev/platform-api/models";

let value: ExtendReleaseInfoAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `binding`                                                                          | [models.ExtendReleaseInfoAzureBinding](../models/extendreleaseinfoazurebinding.md) | :heavy_check_mark:                                                                 | Generic binding configuration for permissions                                      |
| `grant`                                                                            | [models.ExtendReleaseInfoAzureGrant](../models/extendreleaseinfoazuregrant.md)     | :heavy_check_mark:                                                                 | Grant permissions for a specific cloud platform                                    |