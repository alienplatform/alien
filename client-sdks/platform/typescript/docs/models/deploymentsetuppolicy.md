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

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `allowedPlatforms`                                                                                                             | [models.DeploymentSetupPolicyAllowedPlatform](../models/deploymentsetuppolicyallowedplatform.md)[]                             | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `allowedAIProviders`                                                                                                           | [models.DeploymentSetupPolicyAllowedAIProvider](../models/deploymentsetuppolicyallowedaiprovider.md)[]                         | :heavy_minus_sign:                                                                                                             | AI providers the recipient may connect. Omit to allow every provider supported by the Project.                                 |
| `allowedKubernetesBasePlatforms`                                                                                               | [models.DeploymentSetupPolicyAllowedKubernetesBasePlatform](../models/deploymentsetuppolicyallowedkubernetesbaseplatform.md)[] | :heavy_minus_sign:                                                                                                             | Kubernetes base environments the recipient may target.                                                                         |
| `allowedKubernetesClusterSources`                                                                                              | [models.KubernetesClusterSource](../models/kubernetesclustersource.md)[]                                                       | :heavy_minus_sign:                                                                                                             | Whether recipients may create a cluster, use an existing cluster, or both.                                                     |
| `allowedSetupMethods`                                                                                                          | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[]                                                           | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `allowReleasePinning`                                                                                                          | *boolean*                                                                                                                      | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
| `stackSettings`                                                                                                                | [models.DeploymentSetupStackSettingsPolicy](../models/deploymentsetupstacksettingspolicy.md)                                   | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |