# TargetDeploymentExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { TargetDeploymentExtendPlatforms } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExtendPlatforms = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `aws`                                                                      | [models.TargetDeploymentExtendAw](../models/targetdeploymentextendaw.md)[] | :heavy_minus_sign:                                                         | AWS permission configurations                                              |
| `azure`                                                                    | [models.ExtendReleaseInfoAzure](../models/extendreleaseinfoazure.md)[]     | :heavy_minus_sign:                                                         | Azure permission configurations                                            |
| `gcp`                                                                      | [models.ExtendReleaseInfoGcp](../models/extendreleaseinfogcp.md)[]         | :heavy_minus_sign:                                                         | GCP permission configurations                                              |