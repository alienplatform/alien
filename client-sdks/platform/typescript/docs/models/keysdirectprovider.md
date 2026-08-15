# KeysDirectProvider

## Example Usage

```typescript
import { KeysDirectProvider } from "@alienplatform/platform-api/models";

let value: KeysDirectProvider = {
  provider: "databricks",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  availableProviderModelIds: [],
  verifiedAt: new Date("2026-05-28T23:54:17.474Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.KeysProvider](../models/keysprovider.md)                                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |