# CurrentReleasePermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { CurrentReleasePermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: CurrentReleasePermissionSetPlatforms = {};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `aws`                                                                                      | [models.CurrentReleasePermissionSetAw](../models/currentreleasepermissionsetaw.md)[]       | :heavy_minus_sign:                                                                         | AWS permission configurations                                                              |
| `azure`                                                                                    | [models.CurrentReleasePermissionSetAzure](../models/currentreleasepermissionsetazure.md)[] | :heavy_minus_sign:                                                                         | Azure permission configurations                                                            |
| `gcp`                                                                                      | [models.CurrentReleasePermissionSetGcp](../models/currentreleasepermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                         | GCP permission configurations                                                              |