# NetworkSettingsUseDefault

Use the cloud provider's default VPC/network.

Designed for fast dev/test provisioning. No isolated VPC is created, so there
is nothing to wait for or clean up. VMs receive ephemeral public IPs for internet
access — no NAT gateway is provisioned.

- **AWS**: Discovers the account's default VPC. Subnets are public with auto-assigned IPs.
- **GCP**: Discovers the project's `default` network and regional subnet. Instance
  templates include an `AccessConfig` to assign an ephemeral external IP.
- **Azure**: Azure has no default VNet, so one is created along with a NAT Gateway.
  VMs stay private and use NAT for egress.

Not recommended for production. Use `Create` instead.

## Example Usage

```typescript
import { NetworkSettingsUseDefault } from "@alienplatform/manager-api/models";

let value: NetworkSettingsUseDefault = {
  type: "use-default",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `type`             | *"use-default"*    | :heavy_check_mark: | N/A                |