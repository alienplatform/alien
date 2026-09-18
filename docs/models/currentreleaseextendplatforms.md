# CurrentReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { CurrentReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: CurrentReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `aws`                                                                        | [models.CurrentReleaseExtendAw](../models/currentreleaseextendaw.md)[]       | :heavy_minus_sign:                                                           | AWS permission configurations                                                |
| `azure`                                                                      | [models.CurrentReleaseExtendAzure](../models/currentreleaseextendazure.md)[] | :heavy_minus_sign:                                                           | Azure permission configurations                                              |
| `gcp`                                                                        | [models.CurrentReleaseExtendGcp](../models/currentreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                           | GCP permission configurations                                                |