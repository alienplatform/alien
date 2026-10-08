# ListAwsVirtualKeysResponse

AWS Virtual Keys in a project

## Example Usage

```typescript
import { ListAwsVirtualKeysResponse } from "@alienplatform/platform-api/models/operations";

let value: ListAwsVirtualKeysResponse = {
  virtualKeys: [
    {
      id: "vkey_0p9nwqgtftugsoey99lqyqq",
      projectId: "<id>",
      status: "action-required",
      externalKeyId: "<id>",
      awsAccountId: "<id>",
      awsRegion: "<value>",
      customKeyStoreId: "<id>",
      kmsKeyArn: "<value>",
      alias: "<value>",
      description: "behind shameful equatorial sarong unnecessarily since why",
      deletionWindowDays: 740873,
      tags: {
        "key": "<value>",
      },
      observedAt: new Date("2026-10-05T15:24:43.591Z"),
      canaryPassedAt: new Date("2024-03-07T15:19:39.583Z"),
      deletionScheduledAt: new Date("2024-04-17T14:47:31.545Z"),
      finalDeletionObservedAt: new Date("2025-12-28T17:26:04.668Z"),
      createdAt: new Date("2025-08-18T03:41:24.316Z"),
      updatedAt: new Date("2025-03-04T07:51:50.330Z"),
    },
  ],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `virtualKeys`                                                                                        | [operations.ListAwsVirtualKeysVirtualKey](../../models/operations/listawsvirtualkeysvirtualkey.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |