# AzureCredentialsVMManagedIdentity

Azure VM IMDS managed identity.

## Example Usage

```typescript
import { AzureCredentialsVMManagedIdentity } from "@alienplatform/manager-api/models";

let value: AzureCredentialsVMManagedIdentity = {
  clientId: "<id>",
  type: "vmManagedIdentity",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `clientId`                                          | *string*                                            | :heavy_check_mark:                                  | The client ID of the user-assigned managed identity |
| `identityEndpoint`                                  | *string*                                            | :heavy_minus_sign:                                  | Optional IMDS endpoint override                     |
| `type`                                              | *"vmManagedIdentity"*                               | :heavy_check_mark:                                  | N/A                                                 |