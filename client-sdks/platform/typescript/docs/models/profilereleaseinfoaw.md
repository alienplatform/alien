# ProfileReleaseInfoAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { ProfileReleaseInfoAw } from "@aliendotdev/platform-api/models";

let value: ProfileReleaseInfoAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `binding`                                                                      | [models.ProfileReleaseInfoAwBinding](../models/profilereleaseinfoawbinding.md) | :heavy_check_mark:                                                             | Generic binding configuration for permissions                                  |
| `grant`                                                                        | [models.ProfileReleaseInfoAwGrant](../models/profilereleaseinfoawgrant.md)     | :heavy_check_mark:                                                             | Grant permissions for a specific cloud platform                                |