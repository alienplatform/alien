# UserProfile

## Example Usage

```typescript
import { UserProfile } from "@alienplatform/platform-api/models";

let value: UserProfile = {
  id: "<id>",
  email: "Anderson_Bauch@yahoo.com",
  name: "<value>",
  image: null,
  githubUsername: "<value>",
  cliConnected: false,
};
```

## Fields

| Field                                                                                                               | Type                                                                                                                | Required                                                                                                            | Description                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | Unique user identifier                                                                                              |
| `email`                                                                                                             | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | User's email address                                                                                                |
| `name`                                                                                                              | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | User's display name                                                                                                 |
| `image`                                                                                                             | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | User's avatar image URL                                                                                             |
| `githubUsername`                                                                                                    | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | Linked GitHub username                                                                                              |
| `cliConnected`                                                                                                      | *boolean*                                                                                                           | :heavy_check_mark:                                                                                                  | Whether this user has ever authenticated a request from the Alien CLI. Latched on first CLI request, never cleared. |