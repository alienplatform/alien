# ExtendReleaseInfoGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { ExtendReleaseInfoGcp } from "@alienplatform/platform-api/models";

let value: ExtendReleaseInfoGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `binding`                                                                      | [models.ExtendReleaseInfoGcpBinding](../models/extendreleaseinfogcpbinding.md) | :heavy_check_mark:                                                             | Generic binding configuration for permissions                                  |
| `grant`                                                                        | [models.ExtendReleaseInfoGcpGrant](../models/extendreleaseinfogcpgrant.md)     | :heavy_check_mark:                                                             | Grant permissions for a specific cloud platform                                |