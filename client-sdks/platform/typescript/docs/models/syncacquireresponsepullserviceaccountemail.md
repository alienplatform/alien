# SyncAcquireResponsePullServiceAccountEmail

## Example Usage

```typescript
import { SyncAcquireResponsePullServiceAccountEmail } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                                    | [models.SyncAcquireResponsePullServiceAccountEmailSecretRef](../models/syncacquireresponsepullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                             | Reference to a Kubernetes Secret                                                                                               |