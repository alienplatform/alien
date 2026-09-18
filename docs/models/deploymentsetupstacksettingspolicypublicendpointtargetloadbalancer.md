# DeploymentSetupStackSettingsPolicyPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyPublicEndpointTargetLoadBalancer =
  {
    cnameTarget: "<value>",
    mode: "loadBalancer",
  };
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                                                | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | DNS name or URL for the external load balancer.                                                                              |
| `mode`                                                                                                                       | [models.DeploymentSetupStackSettingsPolicyModeLoadBalancer](../models/deploymentsetupstacksettingspolicymodeloadbalancer.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |