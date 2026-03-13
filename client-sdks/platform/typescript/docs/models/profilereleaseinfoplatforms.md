# ProfileReleaseInfoPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { ProfileReleaseInfoPlatforms } from "@aliendotdev/platform-api/models";

let value: ProfileReleaseInfoPlatforms = {};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `aws`                                                                    | [models.ProfileReleaseInfoAw](../models/profilereleaseinfoaw.md)[]       | :heavy_minus_sign:                                                       | AWS permission configurations                                            |
| `azure`                                                                  | [models.ProfileReleaseInfoAzure](../models/profilereleaseinfoazure.md)[] | :heavy_minus_sign:                                                       | Azure permission configurations                                          |
| `gcp`                                                                    | [models.ProfileReleaseInfoGcp](../models/profilereleaseinfogcp.md)[]     | :heavy_minus_sign:                                                       | GCP permission configurations                                            |