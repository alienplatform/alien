# AzureCredentialsWorkloadIdentity

Azure AD Workload Identity (federated identity)

## Example Usage

```typescript
import { AzureCredentialsWorkloadIdentity } from "@alienplatform/manager-api/models";

let value: AzureCredentialsWorkloadIdentity = {
  authorityHost: "<value>",
  clientId: "<id>",
  federatedTokenFile: "<value>",
  tenantId: "<id>",
  type: "workloadIdentity",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `authorityHost`                                      | *string*                                             | :heavy_check_mark:                                   | The authority host URL                               |
| `clientId`                                           | *string*                                             | :heavy_check_mark:                                   | The client ID of the managed identity or application |
| `federatedTokenFile`                                 | *string*                                             | :heavy_check_mark:                                   | Path to the federated token file                     |
| `tenantId`                                           | *string*                                             | :heavy_check_mark:                                   | The tenant ID for authentication                     |
| `type`                                               | *"workloadIdentity"*                                 | :heavy_check_mark:                                   | N/A                                                  |