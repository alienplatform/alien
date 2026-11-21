# TargetReleasePermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetReleasePermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSetPlatforms = {};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `aws`                                                                                    | [models.TargetReleasePermissionSetAw](../models/targetreleasepermissionsetaw.md)[]       | :heavy_minus_sign:                                                                       | AWS permission configurations                                                            |
| `azure`                                                                                  | [models.TargetReleasePermissionSetAzure](../models/targetreleasepermissionsetazure.md)[] | :heavy_minus_sign:                                                                       | Azure permission configurations                                                          |
| `gcp`                                                                                    | [models.TargetReleasePermissionSetGcp](../models/targetreleasepermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                       | GCP permission configurations                                                            |