# OverrideReleaseInfoAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { OverrideReleaseInfoAzure } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `binding`                                                                              | [models.OverrideReleaseInfoAzureBinding](../models/overridereleaseinfoazurebinding.md) | :heavy_check_mark:                                                                     | Generic binding configuration for permissions                                          |
| `grant`                                                                                | [models.OverrideReleaseInfoAzureGrant](../models/overridereleaseinfoazuregrant.md)     | :heavy_check_mark:                                                                     | Grant permissions for a specific cloud platform                                        |