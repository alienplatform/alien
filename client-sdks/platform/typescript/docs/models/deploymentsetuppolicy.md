# DeploymentSetupPolicy

## Example Usage

```typescript
import { DeploymentSetupPolicy } from "@alienplatform/platform-api/models";

let value: DeploymentSetupPolicy = {
  allowedPlatforms: [
    "test",
  ],
  allowedSetupMethods: [
    "cloudformation",
  ],
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `allowedPlatforms`                                                                           | [models.AllowedPlatform](../models/allowedplatform.md)[]                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `allowedKubernetesBasePlatforms`                                                             | [models.AllowedKubernetesBasePlatform](../models/allowedkubernetesbaseplatform.md)[]         | :heavy_minus_sign:                                                                           | Kubernetes base environments the recipient may target.                                       |
| `allowedKubernetesClusterSources`                                                            | [models.KubernetesClusterSource](../models/kubernetesclustersource.md)[]                     | :heavy_minus_sign:                                                                           | Whether recipients may create a cluster, use an existing cluster, or both.                   |
| `allowedSetupMethods`                                                                        | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[]                         | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `allowReleasePinning`                                                                        | *boolean*                                                                                    | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `stackSettings`                                                                              | [models.DeploymentSetupStackSettingsPolicy](../models/deploymentsetupstacksettingspolicy.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |