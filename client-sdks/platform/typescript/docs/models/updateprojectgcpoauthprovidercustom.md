# UpdateProjectGcpOAuthProviderCustom

## Example Usage

```typescript
import { UpdateProjectGcpOAuthProviderCustom } from "@alienplatform/platform-api/models";

let value: UpdateProjectGcpOAuthProviderCustom = {
  mode: "custom",
  clientId: "1234567890-abc123.apps.googleusercontent.com",
  clientSecret: "GOCSPX-example",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            | Example                                                                |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `mode`                                                                 | *"custom"*                                                             | :heavy_check_mark:                                                     | N/A                                                                    |                                                                        |
| `clientId`                                                             | *string*                                                               | :heavy_check_mark:                                                     | Google OAuth web client ID.                                            | 1234567890-abc123.apps.googleusercontent.com                           |
| `clientSecret`                                                         | *string*                                                               | :heavy_minus_sign:                                                     | Google OAuth web client secret. Write-only; never returned by the API. | GOCSPX-example                                                         |