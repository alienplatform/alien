# RestoreAwsVirtualKeyResponse

Virtual Key awaiting a successful AWS XKS canary after restore

## Example Usage

```typescript
import { RestoreAwsVirtualKeyResponse } from "@alienplatform/platform-api/models/operations";

let value: RestoreAwsVirtualKeyResponse = {
  id: "vkey_0p9nwqgtftugsoey99lqyqq",
  projectId: "<id>",
  status: "action-required",
  externalKeyId: "<id>",
  awsAccountId: "<id>",
  awsRegion: "<value>",
  customKeyStoreId: "<id>",
  kmsKeyArn: "<value>",
  alias: "<value>",
  description: "provided sell square rationalise now",
  deletionWindowDays: 450636,
  tags: {
    "key": "<value>",
    "key1": "<value>",
  },
  observedAt: new Date("2026-03-05T17:49:17.838Z"),
  canaryPassedAt: new Date("2026-04-06T08:11:58.624Z"),
  deletionScheduledAt: new Date("2026-08-25T02:41:36.889Z"),
  finalDeletionObservedAt: new Date("2026-03-07T19:00:50.088Z"),
  createdAt: new Date("2025-04-10T14:08:00.332Z"),
  updatedAt: new Date("2026-04-17T07:33:13.454Z"),
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the aws virtual key.                                                     | vkey_0p9nwqgtftugsoey99lqyqq                                                                   |
| `projectId`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `status`                                                                                       | [operations.RestoreAwsVirtualKeyStatus](../../models/operations/restoreawsvirtualkeystatus.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `externalKeyId`                                                                                | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `awsAccountId`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `awsRegion`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `customKeyStoreId`                                                                             | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `kmsKeyArn`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `alias`                                                                                        | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `description`                                                                                  | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `deletionWindowDays`                                                                           | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `tags`                                                                                         | Record<string, *string*>                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `observedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `canaryPassedAt`                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `deletionScheduledAt`                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `finalDeletionObservedAt`                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `createdAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `updatedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |