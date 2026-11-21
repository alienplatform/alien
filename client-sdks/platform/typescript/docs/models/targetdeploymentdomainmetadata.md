# TargetDeploymentDomainMetadata

Domain metadata for auto-managed public resources (no private keys).

## Example Usage

```typescript
import { TargetDeploymentDomainMetadata } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDomainMetadata = {
  baseDomain: "<value>",
  hostedZoneId: "<id>",
  publicSubdomain: "<value>",
  resources: {
    "key": {
      certificateId: "<id>",
      certificateStatus: "pending",
      dnsStatus: "active",
      fqdn: "<value>",
    },
  },
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `baseDomain`                                                           | *string*                                                               | :heavy_check_mark:                                                     | Base domain for auto-generated domains (e.g., "vpc.direct").           |
| `hostedZoneId`                                                         | *string*                                                               | :heavy_check_mark:                                                     | Hosted zone ID for DNS records.                                        |
| `publicSubdomain`                                                      | *string*                                                               | :heavy_check_mark:                                                     | Deployment public subdomain (e.g., "k8f2j3").                          |
| `resources`                                                            | Record<string, [models.ConfigResources](../models/configresources.md)> | :heavy_check_mark:                                                     | Metadata per resource ID.                                              |