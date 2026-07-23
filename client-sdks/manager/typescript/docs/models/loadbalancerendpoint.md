# LoadBalancerEndpoint

Load balancer endpoint information for DNS management.
This is optional metadata used by the DNS controller to create domain mappings.

## Example Usage

```typescript
import { LoadBalancerEndpoint } from "@alienplatform/manager-api/models";

let value: LoadBalancerEndpoint = {
  dnsName: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `dnsName`                                                                       | *string*                                                                        | :heavy_check_mark:                                                              | The DNS name of the load balancer endpoint (e.g., ALB DNS, API Gateway domain). |
| `hostedZoneId`                                                                  | *string*                                                                        | :heavy_minus_sign:                                                              | AWS Route53 hosted zone ID (for ALIAS records). Only set on AWS.                |