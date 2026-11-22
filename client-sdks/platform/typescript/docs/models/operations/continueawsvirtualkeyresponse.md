# ContinueAwsVirtualKeyResponse

Updated AWS setup facts

## Example Usage

```typescript
import { ContinueAwsVirtualKeyResponse } from "@alienplatform/platform-api/models/operations";

let value: ContinueAwsVirtualKeyResponse = {
  id: "vkey_0p9nwqgtftugsoey99lqyqq",
  projectId: "<id>",
  status: "failed",
  externalKeyId: "<id>",
  awsAccountId: "<id>",
  awsRegion: "<value>",
  customKeyStoreId: "<id>",
  kmsKeyArn: "<value>",
  alias: "<value>",
  description: "amount option provided last veto",
  deletionWindowDays: 357589,
  tags: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  observedAt: new Date("2025-11-07T06:41:17.981Z"),
  canaryPassedAt: new Date("2026-04-18T17:37:48.210Z"),
  deletionScheduledAt: new Date("2025-12-03T03:03:06.265Z"),
  finalDeletionObservedAt: new Date("2024-06-25T00:28:36.672Z"),
  createdAt: new Date("2026-02-07T14:29:31.605Z"),
  updatedAt: new Date("2026-12-25T13:39:40.933Z"),
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      | Example                                                                                          |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `id`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | Unique identifier for the aws virtual key.                                                       | vkey_0p9nwqgtftugsoey99lqyqq                                                                     |
| `projectId`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `status`                                                                                         | [operations.ContinueAwsVirtualKeyStatus](../../models/operations/continueawsvirtualkeystatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `externalKeyId`                                                                                  | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `awsAccountId`                                                                                   | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `awsRegion`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `customKeyStoreId`                                                                               | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `kmsKeyArn`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `alias`                                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `description`                                                                                    | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `deletionWindowDays`                                                                             | *number*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `tags`                                                                                           | Record<string, *string*>                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `observedAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `canaryPassedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `deletionScheduledAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `finalDeletionObservedAt`                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `createdAt`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |
| `updatedAt`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | N/A                                                                                              |                                                                                                  |