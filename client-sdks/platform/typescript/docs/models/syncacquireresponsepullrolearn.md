# SyncAcquireResponsePullRoleArn

## Example Usage

```typescript
import { SyncAcquireResponsePullRoleArn } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponsePullRoleArnSecretRef](../models/syncacquireresponsepullrolearnsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |