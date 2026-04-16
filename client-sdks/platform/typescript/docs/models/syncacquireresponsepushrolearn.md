# SyncAcquireResponsePushRoleArn

## Example Usage

```typescript
import { SyncAcquireResponsePushRoleArn } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponsePushRoleArnSecretRef](../models/syncacquireresponsepushrolearnsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |