# ExtendReleaseInfoAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { ExtendReleaseInfoAw } from "@alienplatform/platform-api/models";

let value: ExtendReleaseInfoAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `binding`                                                                    | [models.ExtendReleaseInfoAwBinding](../models/extendreleaseinfoawbinding.md) | :heavy_check_mark:                                                           | Generic binding configuration for permissions                                |
| `grant`                                                                      | [models.ExtendReleaseInfoAwGrant](../models/extendreleaseinfoawgrant.md)     | :heavy_check_mark:                                                           | Grant permissions for a specific cloud platform                              |