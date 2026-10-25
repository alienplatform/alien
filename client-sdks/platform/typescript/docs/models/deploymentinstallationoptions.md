# DeploymentInstallationOptions

Build and save installation instructions on the server. Requires setupHandoff=await-registration.

## Example Usage

```typescript
import { DeploymentInstallationOptions } from "@alienplatform/platform-api/models";

let value: DeploymentInstallationOptions = {};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `region`                                                 | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `publicEndpoints`                                        | Record<string, Record<string, *string*>>                 | :heavy_minus_sign:                                       | N/A                                                      |
| `customerKeyConnection`                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `gcpProjectId`                                           | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `stackName`                                              | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `clusterSource`                                          | [models.ClusterSource](../models/clustersource.md)       | :heavy_minus_sign:                                       | N/A                                                      |
| `chartInstallMode`                                       | [models.ChartInstallMode](../models/chartinstallmode.md) | :heavy_minus_sign:                                       | N/A                                                      |
| `namespace`                                              | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |