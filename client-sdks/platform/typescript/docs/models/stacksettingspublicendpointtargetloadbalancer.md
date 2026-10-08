# StackSettingsPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { StackSettingsPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: StackSettingsPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `cnameTarget`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | DNS name or URL for the external load balancer.                                    |
| `mode`                                                                             | [models.StackSettingsModeLoadBalancer](../models/stacksettingsmodeloadbalancer.md) | :heavy_check_mark:                                                                 | N/A                                                                                |