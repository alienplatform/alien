# PersistImportedDeploymentRequestPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `cnameTarget`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | DNS name or URL for the external load balancer.                                                                          |
| `mode`                                                                                                                   | [models.PersistImportedDeploymentRequestModeLoadBalancer](../models/persistimporteddeploymentrequestmodeloadbalancer.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |