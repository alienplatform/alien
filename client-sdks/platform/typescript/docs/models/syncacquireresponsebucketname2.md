# SyncAcquireResponseBucketName2

## Example Usage

```typescript
import { SyncAcquireResponseBucketName2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseBucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponseBucketNameSecretRef2](../models/syncacquireresponsebucketnamesecretref2.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |