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
  company: "Shields Group",
  acquisitionSource: "event-or-community",
  acquisitionSourceDetail: "<value>",
  useCases: "<value>",
  profileSetupCompletedAt: null,
  profileSetupVersion: null,
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
| `company`                                                                                                           | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | Company name collected during profile setup.                                                                        |
| `acquisitionSource`                                                                                                 | [models.UserProfileAcquisitionSource](../models/userprofileacquisitionsource.md)                                    | :heavy_check_mark:                                                                                                  | How the user heard about Alien.                                                                                     |
| `acquisitionSourceDetail`                                                                                           | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | Additional acquisition source detail when the source is other.                                                      |
| `useCases`                                                                                                          | *string*                                                                                                            | :heavy_check_mark:                                                                                                  | What the user is hoping to use Alien for.                                                                           |
| `profileSetupCompletedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                       | :heavy_check_mark:                                                                                                  | When the user completed the required profile setup dialog.                                                          |
| `profileSetupVersion`                                                                                               | *number*                                                                                                            | :heavy_check_mark:                                                                                                  | Version of the required profile setup dialog the user completed.                                                    |