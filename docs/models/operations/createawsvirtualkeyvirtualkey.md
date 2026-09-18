# CreateAwsVirtualKeyVirtualKey

## Example Usage

```typescript
import { CreateAwsVirtualKeyVirtualKey } from "@alienplatform/platform-api/models/operations";

let value: CreateAwsVirtualKeyVirtualKey = {
  id: "vkey_0p9nwqgtftugsoey99lqyqq",
  projectId: "<id>",
  status: "provisioning",
  externalKeyId: "<id>",
  awsAccountId: "<id>",
  awsRegion: "<value>",
  customKeyStoreId: "<id>",
  kmsKeyArn: "<value>",
  alias: "<value>",
  description: "instead er deceivingly",
  deletionWindowDays: 668701,
  tags: {
    "key": "<value>",
    "key1": "<value>",
  },
  observedAt: null,
  canaryPassedAt: new Date("2026-08-02T14:12:57.393Z"),
  deletionScheduledAt: new Date("2026-10-08T05:12:41.922Z"),
  finalDeletionObservedAt: null,
  createdAt: new Date("2026-05-15T21:44:06.660Z"),
  updatedAt: new Date("2026-04-07T19:01:17.142Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the aws virtual key.                                                    | vkey_0p9nwqgtftugsoey99lqyqq                                                                  |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [operations.CreateAwsVirtualKeyStatus](../../models/operations/createawsvirtualkeystatus.md)  | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
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