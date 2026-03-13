# OverrideReleaseInfoAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { OverrideReleaseInfoAw } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `binding`                                                                        | [models.OverrideReleaseInfoAwBinding](../models/overridereleaseinfoawbinding.md) | :heavy_check_mark:                                                               | Generic binding configuration for permissions                                    |
| `grant`                                                                          | [models.OverrideReleaseInfoAwGrant](../models/overridereleaseinfoawgrant.md)     | :heavy_check_mark:                                                               | Grant permissions for a specific cloud platform                                  |