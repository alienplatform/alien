# ProjectGcpOAuthProviderAlienManaged

## Example Usage

```typescript
import { ProjectGcpOAuthProviderAlienManaged } from "@alienplatform/platform-api/models";

let value: ProjectGcpOAuthProviderAlienManaged = {
  mode: "alien-managed",
  redirectUris: [
    "https://strident-technologist.org/",
    "https://inferior-plastic.name/",
    "https://neat-planula.name/",
  ],
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `mode`                                                                       | *"alien-managed"*                                                            | :heavy_check_mark:                                                           | N/A                                                                          |
| `redirectUris`                                                               | *string*[]                                                                   | :heavy_check_mark:                                                           | Authorized redirect URIs that must be configured on the Google OAuth client. |