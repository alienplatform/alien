# ListAwsVirtualKeysVirtualKey

## Example Usage

```typescript
import { ListAwsVirtualKeysVirtualKey } from "@alienplatform/platform-api/models/operations";

let value: ListAwsVirtualKeysVirtualKey = {
  id: "vkey_0p9nwqgtftugsoey99lqyqq",
  projectId: "<id>",
  status: "deleted",
  externalKeyId: "<id>",
  awsAccountId: "<id>",
  awsRegion: "<value>",
  customKeyStoreId: "<id>",
  kmsKeyArn: "<value>",
  alias: "<value>",
  description: "on overfeed pave edge mmm",
  deletionWindowDays: 625743,
  tags: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  observedAt: new Date("2026-12-27T22:05:00.783Z"),
  canaryPassedAt: new Date("2024-01-21T00:34:36.340Z"),
  deletionScheduledAt: new Date("2026-11-17T20:19:04.315Z"),
  finalDeletionObservedAt: new Date("2024-06-18T03:12:01.740Z"),
  createdAt: new Date("2026-03-17T01:34:42.173Z"),
  updatedAt: new Date("2024-07-18T17:10:40.514Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the aws virtual key.                                                    | vkey_0p9nwqgtftugsoey99lqyqq                                                                  |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [operations.ListAwsVirtualKeysStatus](../../models/operations/listawsvirtualkeysstatus.md)    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `externalKeyId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `awsAccountId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `awsRegion`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `customKeyStoreId`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `kmsKeyArn`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `alias`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deletionWindowDays`                                                                          | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `tags`                                                                                        | Record<string, *string*>                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `canaryPassedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deletionScheduledAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `finalDeletionObservedAt`                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |