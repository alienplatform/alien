# TargetReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetReleaseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `aws`                                                                          | [models.TargetReleaseOverrideAw](../models/targetreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                             | AWS permission configurations                                                  |
| `azure`                                                                        | [models.TargetReleaseOverrideAzure](../models/targetreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                             | Azure permission configurations                                                |
| `gcp`                                                                          | [models.TargetReleaseOverrideGcp](../models/targetreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                             | GCP permission configurations                                                  |