# AzureCredentialsServicePrincipal

Service principal with client secret

## Example Usage

```typescript
import { AzureCredentialsServicePrincipal } from "@alienplatform/manager-api/models";

let value: AzureCredentialsServicePrincipal = {
  clientId: "<id>",
  clientSecret: "<value>",
  type: "servicePrincipal",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `clientId`                     | *string*                       | :heavy_check_mark:             | The client ID (application ID) |
| `clientSecret`                 | *string*                       | :heavy_check_mark:             | The client secret              |
| `type`                         | *"servicePrincipal"*           | :heavy_check_mark:             | N/A                            |