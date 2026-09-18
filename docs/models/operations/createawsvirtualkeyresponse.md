# CreateAwsVirtualKeyResponse

Created or resumed Virtual Key

## Example Usage

```typescript
import { CreateAwsVirtualKeyResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateAwsVirtualKeyResponse = {
  virtualKey: {
    id: "vkey_0p9nwqgtftugsoey99lqyqq",
    projectId: "<id>",
    status: "ready",
    externalKeyId: "<id>",
    awsAccountId: "<id>",
    awsRegion: "<value>",
    customKeyStoreId: "<id>",
    kmsKeyArn: "<value>",
    alias: "<value>",
    description: "drat confirm aboard after thigh",
    deletionWindowDays: 158206,
    tags: {
      "key": "<value>",
    },
    observedAt: new Date("2024-04-21T01:17:14.876Z"),
    canaryPassedAt: new Date("2026-03-05T20:02:13.242Z"),
    deletionScheduledAt: new Date("2024-12-03T10:50:36.022Z"),
    finalDeletionObservedAt: new Date("2024-11-02T19:54:50.258Z"),
    createdAt: new Date("2026-07-13T11:47:49.577Z"),
    updatedAt: new Date("2025-03-28T18:57:53.378Z"),
  },
  setup: {
    proxyUriEndpoint: "https://enchanting-bidet.com/",
    proxyUriPath: "<value>",
    accessKeyId: "<id>",
    secretAccessKey: "<value>",
    externalKeyId: "<id>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `virtualKey`                                                                                         | [operations.CreateAwsVirtualKeyVirtualKey](../../models/operations/createawsvirtualkeyvirtualkey.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `setup`                                                                                              | [operations.CreateAwsVirtualKeySetup](../../models/operations/createawsvirtualkeysetup.md)           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |