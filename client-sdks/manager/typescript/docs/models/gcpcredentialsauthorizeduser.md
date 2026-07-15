# GcpCredentialsAuthorizedUser

Use gcloud Application Default Credentials (authorized_user).
Exchanges refresh_token for an access_token via Google's OAuth2 endpoint.

## Example Usage

```typescript
import { GcpCredentialsAuthorizedUser } from "@alienplatform/manager-api/models";

let value: GcpCredentialsAuthorizedUser = {
  clientId: "<id>",
  clientSecret: "<value>",
  refreshToken: "<value>",
  type: "authorizedUser",
};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `clientId`           | *string*             | :heavy_check_mark:   | OAuth2 client ID     |
| `clientSecret`       | *string*             | :heavy_check_mark:   | OAuth2 client secret |
| `refreshToken`       | *string*             | :heavy_check_mark:   | OAuth2 refresh token |
| `type`               | *"authorizedUser"*   | :heavy_check_mark:   | N/A                  |