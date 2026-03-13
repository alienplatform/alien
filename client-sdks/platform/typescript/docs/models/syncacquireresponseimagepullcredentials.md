# SyncAcquireResponseImagePullCredentials

Image pull credentials for container registries

## Example Usage

```typescript
import { SyncAcquireResponseImagePullCredentials } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseImagePullCredentials = {
  password: "2bazurwG7aW1oYx",
  username: "Gilda.Koss",
};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `password`                          | *string*                            | :heavy_check_mark:                  | Password for the container registry |
| `username`                          | *string*                            | :heavy_check_mark:                  | Username for the container registry |