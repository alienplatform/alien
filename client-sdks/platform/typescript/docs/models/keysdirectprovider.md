# KeysDirectProvider

## Example Usage

```typescript
import { KeysDirectProvider } from "@alienplatform/platform-api/models";

let value: KeysDirectProvider = {
  provider: "databricks",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  credentialStatus: "pending",
  credentialCheckedAt: new Date("2026-10-11T08:12:07.727Z"),
  catalogObservedAt: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.KeysProvider](../models/keysprovider.md)                                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.KeysCredentialStatus](../models/keyscredentialstatus.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |