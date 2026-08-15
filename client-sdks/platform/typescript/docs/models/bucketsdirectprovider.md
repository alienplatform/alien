# BucketsDirectProvider

## Example Usage

```typescript
import { BucketsDirectProvider } from "@alienplatform/platform-api/models";

let value: BucketsDirectProvider = {
  provider: "openai",
  providerEndpoint: null,
  keyFingerprint: "<value>",
  availableProviderModelIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  verifiedAt: new Date("2024-03-23T02:09:43.731Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.BucketsProvider](../models/bucketsprovider.md)                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |