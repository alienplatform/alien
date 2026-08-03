# PublicEndpointOutput

Runtime-resolved public endpoint metadata.

## Example Usage

```typescript
import { PublicEndpointOutput } from "@alienplatform/manager-api/models";

let value: PublicEndpointOutput = {
  host: "elastic-collaboration.info",
  port: 780477,
  protocol: "tcp",
  url: "https://functional-fuel.name/",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `host`                                                           | *string*                                                         | :heavy_check_mark:                                               | Hostname for this endpoint.                                      |
| `loadBalancerEndpoint`                                           | [models.LoadBalancerEndpoint](../models/loadbalancerendpoint.md) | :heavy_minus_sign:                                               | N/A                                                              |
| `port`                                                           | *number*                                                         | :heavy_check_mark:                                               | Public connection port.                                          |
| `protocol`                                                       | [models.ExposeProtocol](../models/exposeprotocol.md)             | :heavy_check_mark:                                               | Protocol for public workload endpoints.                          |
| `url`                                                            | *string*                                                         | :heavy_check_mark:                                               | Base URL for this endpoint.                                      |
| `wildcardHost`                                                   | *string*                                                         | :heavy_minus_sign:                                               | Wildcard hostname routed to this endpoint, when configured.      |