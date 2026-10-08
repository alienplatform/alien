# CreateSetupRegistrationOperationRequestPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value:
  CreateSetupRegistrationOperationRequestPublicEndpointTargetLoadBalancer = {
    cnameTarget: "<value>",
    mode: "loadBalancer",
  };
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                                                          | *string*                                                                                                                               | :heavy_check_mark:                                                                                                                     | DNS name or URL for the external load balancer.                                                                                        |
| `mode`                                                                                                                                 | [models.CreateSetupRegistrationOperationRequestModeLoadBalancer](../models/createsetupregistrationoperationrequestmodeloadbalancer.md) | :heavy_check_mark:                                                                                                                     | N/A                                                                                                                                    |