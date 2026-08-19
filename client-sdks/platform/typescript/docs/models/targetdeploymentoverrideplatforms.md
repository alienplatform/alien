# TargetDeploymentOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetDeploymentOverridePlatforms } from "@alienplatform/platform-api/models";

let value: TargetDeploymentOverridePlatforms = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `aws`                                                                          | [models.TargetDeploymentOverrideAw](../models/targetdeploymentoverrideaw.md)[] | :heavy_minus_sign:                                                             | AWS permission configurations                                                  |
| `azure`                                                                        | [models.OverrideReleaseInfoAzure](../models/overridereleaseinfoazure.md)[]     | :heavy_minus_sign:                                                             | Azure permission configurations                                                |
| `gcp`                                                                          | [models.OverrideReleaseInfoGcp](../models/overridereleaseinfogcp.md)[]         | :heavy_minus_sign:                                                             | GCP permission configurations                                                  |