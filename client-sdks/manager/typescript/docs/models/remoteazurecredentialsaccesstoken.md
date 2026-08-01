# RemoteAzureCredentialsAccessToken

OAuth bearer token for `https://storage.azure.com/.default`.

## Example Usage

```typescript
import { RemoteAzureCredentialsAccessToken } from "@alienplatform/manager-api/models";

let value: RemoteAzureCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `token`                                                                      | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `type`                                                                       | [models.RemoteAzureCredentialsType](../models/remoteazurecredentialstype.md) | :heavy_check_mark:                                                           | N/A                                                                          |
