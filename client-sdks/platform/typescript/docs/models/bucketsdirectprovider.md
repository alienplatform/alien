# BucketsDirectProvider

## Example Usage

```typescript
import { BucketsDirectProvider } from "@alienplatform/platform-api/models";

let value: BucketsDirectProvider = {
  provider: "openai",
  providerEndpoint: null,
  keyFingerprint: "<value>",
  credentialStatus: "unknown",
  credentialCheckedAt: null,
  catalogObservedAt: new Date("2025-02-14T15:28:31.832Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.BucketsProvider](../models/bucketsprovider.md)                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.BucketsCredentialStatus](../models/bucketscredentialstatus.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |