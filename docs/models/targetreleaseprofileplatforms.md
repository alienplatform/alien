# TargetReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetReleaseProfilePlatforms } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `aws`                                                                        | [models.TargetReleaseProfileAw](../models/targetreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                           | AWS permission configurations                                                |
| `azure`                                                                      | [models.TargetReleaseProfileAzure](../models/targetreleaseprofileazure.md)[] | :heavy_minus_sign:                                                           | Azure permission configurations                                              |
| `gcp`                                                                        | [models.TargetReleaseProfileGcp](../models/targetreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                           | GCP permission configurations                                                |