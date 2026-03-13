# OverrideReleaseInfoGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { OverrideReleaseInfoGcp } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `binding`                                                                          | [models.OverrideReleaseInfoGcpBinding](../models/overridereleaseinfogcpbinding.md) | :heavy_check_mark:                                                                 | Generic binding configuration for permissions                                      |
| `grant`                                                                            | [models.OverrideReleaseInfoGcpGrant](../models/overridereleaseinfogcpgrant.md)     | :heavy_check_mark:                                                                 | Grant permissions for a specific cloud platform                                    |