# SyncAcquireResponsePushServiceAccountEmail

## Example Usage

```typescript
import { SyncAcquireResponsePushServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                                    | [models.SyncAcquireResponsePushServiceAccountEmailSecretRef](../models/syncacquireresponsepushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                             | Reference to a Kubernetes Secret                                                                                               |