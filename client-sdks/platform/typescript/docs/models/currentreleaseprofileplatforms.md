# CurrentReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { CurrentReleaseProfilePlatforms } from "@alienplatform/platform-api/models";

let value: CurrentReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `aws`                                                                          | [models.CurrentReleaseProfileAw](../models/currentreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                             | AWS permission configurations                                                  |
| `azure`                                                                        | [models.CurrentReleaseProfileAzure](../models/currentreleaseprofileazure.md)[] | :heavy_minus_sign:                                                             | Azure permission configurations                                                |
| `gcp`                                                                          | [models.CurrentReleaseProfileGcp](../models/currentreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                             | GCP permission configurations                                                  |