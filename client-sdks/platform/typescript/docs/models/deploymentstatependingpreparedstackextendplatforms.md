# DeploymentStatePendingPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.DeploymentStatePendingPreparedStackExtendAw](../models/deploymentstatependingpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.DeploymentStatePendingPreparedStackExtendAzure](../models/deploymentstatependingpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.DeploymentStatePendingPreparedStackExtendGcp](../models/deploymentstatependingpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |