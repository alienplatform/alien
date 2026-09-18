# CurrentReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { CurrentReleasePermissions } from "@alienplatform/platform-api/models";

let value: CurrentReleasePermissions = {
  profiles: {
    "key": {
      "key": [
        {
          description: "across tenderly vivaciously who",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [],
    },
    "key1": {
      "key": [
        "<value>",
      ],
      "key1": [],
      "key2": [
        "<value>",
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.CurrentReleaseManagementUnion*                                                                                             | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.CurrentReleaseProfileUnion*[]>>                                                              | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |