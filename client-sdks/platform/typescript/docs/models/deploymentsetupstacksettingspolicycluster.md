# DeploymentSetupStackSettingsPolicyCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyCluster } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `cloud`                                                                                                        | *models.DeploymentSetupStackSettingsPolicyCloudUnion*                                                          | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `namespace`                                                                                                    | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | Namespace where the Alien chart and application resources run.                                                 |
| `ownership`                                                                                                    | [models.DeploymentSetupStackSettingsPolicyOwnership](../models/deploymentsetupstacksettingspolicyownership.md) | :heavy_check_mark:                                                                                             | Ownership model for the Kubernetes cluster.                                                                    |