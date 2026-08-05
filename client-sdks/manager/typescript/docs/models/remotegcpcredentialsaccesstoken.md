# RemoteGcpCredentialsAccessToken

Short-lived OAuth access token. Its expiry is the response `expiresAt`.

## Example Usage

```typescript
import { RemoteGcpCredentialsAccessToken } from "@alienplatform/manager-api/models";

let value: RemoteGcpCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `token`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | OAuth bearer token.                                                      |
| `type`                                                                   | [models.RemoteGcpCredentialsType](../models/remotegcpcredentialstype.md) | :heavy_check_mark:                                                       | N/A                                                                      |