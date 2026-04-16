# SyncReconcileResponseImagePullCredentials

Image pull credentials for container registries

## Example Usage

```typescript
import { SyncReconcileResponseImagePullCredentials } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseImagePullCredentials = {
  password: "apVd_US99ovybFp",
  username: "Chaz58",
};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `password`                          | *string*                            | :heavy_check_mark:                  | Password for the container registry |
| `username`                          | *string*                            | :heavy_check_mark:                  | Username for the container registry |