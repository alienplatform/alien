# TargetReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `aws`                                                                      | [models.TargetReleaseExtendAw](../models/targetreleaseextendaw.md)[]       | :heavy_minus_sign:                                                         | AWS permission configurations                                              |
| `azure`                                                                    | [models.TargetReleaseExtendAzure](../models/targetreleaseextendazure.md)[] | :heavy_minus_sign:                                                         | Azure permission configurations                                            |
| `gcp`                                                                      | [models.TargetReleaseExtendGcp](../models/targetreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                         | GCP permission configurations                                              |