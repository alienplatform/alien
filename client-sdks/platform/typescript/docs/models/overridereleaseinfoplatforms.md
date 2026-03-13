# OverrideReleaseInfoPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { OverrideReleaseInfoPlatforms } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoPlatforms = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `aws`                                                                      | [models.OverrideReleaseInfoAw](../models/overridereleaseinfoaw.md)[]       | :heavy_minus_sign:                                                         | AWS permission configurations                                              |
| `azure`                                                                    | [models.OverrideReleaseInfoAzure](../models/overridereleaseinfoazure.md)[] | :heavy_minus_sign:                                                         | Azure permission configurations                                            |
| `gcp`                                                                      | [models.OverrideReleaseInfoGcp](../models/overridereleaseinfogcp.md)[]     | :heavy_minus_sign:                                                         | GCP permission configurations                                              |