# DeploymentDetailResponsePendingPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                    | [models.DeploymentDetailResponsePendingPreparedStackExtendAw](../models/deploymentdetailresponsependingpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                                       | AWS permission configurations                                                                                                            |
| `azure`                                                                                                                                  | [models.DeploymentDetailResponsePendingPreparedStackExtendAzure](../models/deploymentdetailresponsependingpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                                       | Azure permission configurations                                                                                                          |
| `gcp`                                                                                                                                    | [models.DeploymentDetailResponsePendingPreparedStackExtendGcp](../models/deploymentdetailresponsependingpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                                       | GCP permission configurations                                                                                                            |
