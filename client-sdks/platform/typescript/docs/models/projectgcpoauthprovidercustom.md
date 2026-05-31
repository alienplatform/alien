# ProjectGcpOAuthProviderCustom

## Example Usage

```typescript
import { ProjectGcpOAuthProviderCustom } from "@alienplatform/platform-api/models";

let value: ProjectGcpOAuthProviderCustom = {
  mode: "custom",
  clientId: "1234567890-abc123.apps.googleusercontent.com",
  hasClientSecret: false,
  redirectUris: [
    "https://rowdy-catalyst.biz/",
    "https://inborn-gymnast.name/",
  ],
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  | Example                                                                      |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `mode`                                                                       | *"custom"*                                                                   | :heavy_check_mark:                                                           | N/A                                                                          |                                                                              |
| `clientId`                                                                   | *string*                                                                     | :heavy_check_mark:                                                           | Google OAuth web client ID.                                                  | 1234567890-abc123.apps.googleusercontent.com                                 |
| `hasClientSecret`                                                            | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |                                                                              |
| `redirectUris`                                                               | *string*[]                                                                   | :heavy_check_mark:                                                           | Authorized redirect URIs that must be configured on the Google OAuth client. |                                                                              |