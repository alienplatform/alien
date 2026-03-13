# ProfileReleaseInfoGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { ProfileReleaseInfoGcp } from "@aliendotdev/platform-api/models";

let value: ProfileReleaseInfoGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `binding`                                                                        | [models.ProfileReleaseInfoGcpBinding](../models/profilereleaseinfogcpbinding.md) | :heavy_check_mark:                                                               | Generic binding configuration for permissions                                    |
| `grant`                                                                          | [models.ProfileReleaseInfoGcpGrant](../models/profilereleaseinfogcpgrant.md)     | :heavy_check_mark:                                                               | Grant permissions for a specific cloud platform                                  |