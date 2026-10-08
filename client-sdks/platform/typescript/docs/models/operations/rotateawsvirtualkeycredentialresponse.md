# RotateAwsVirtualKeyCredentialResponse

The resumable XKS proxy credential rotation bundle

## Example Usage

```typescript
import { RotateAwsVirtualKeyCredentialResponse } from "@alienplatform/platform-api/models/operations";

let value: RotateAwsVirtualKeyCredentialResponse = {
  virtualKey: {
    id: "vkey_0p9nwqgtftugsoey99lqyqq",
    projectId: "<id>",
    status: "provisioning",
    externalKeyId: "<id>",
    awsAccountId: "<id>",
    awsRegion: "<value>",
    customKeyStoreId: "<id>",
    kmsKeyArn: "<value>",
    alias: "<value>",
    description: "reorganisation great live egg dispose know",
    deletionWindowDays: 330882,
    tags: {},
    observedAt: new Date("2026-10-14T12:23:05.729Z"),
    canaryPassedAt: new Date("2025-02-17T07:44:26.021Z"),
    deletionScheduledAt: new Date("2026-07-01T13:21:44.277Z"),
    finalDeletionObservedAt: new Date("2025-09-12T03:32:45.896Z"),
    createdAt: new Date("2026-10-11T00:05:14.784Z"),
    updatedAt: new Date("2025-02-08T15:01:17.984Z"),
  },
  setup: {
    proxyUriEndpoint: "https://liquid-armchair.org/",
    proxyUriPath: "<value>",
    accessKeyId: "<id>",
    secretAccessKey: "<value>",
    externalKeyId: "<id>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `virtualKey`                                                                                                             | [operations.RotateAwsVirtualKeyCredentialVirtualKey](../../models/operations/rotateawsvirtualkeycredentialvirtualkey.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `setup`                                                                                                                  | [operations.RotateAwsVirtualKeyCredentialSetup](../../models/operations/rotateawsvirtualkeycredentialsetup.md)           | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |