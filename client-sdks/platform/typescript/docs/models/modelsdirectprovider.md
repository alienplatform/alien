# ModelsDirectProvider

## Example Usage

```typescript
import { ModelsDirectProvider } from "@alienplatform/platform-api/models";

let value: ModelsDirectProvider = {
  provider: "databricks",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  availableProviderModelIds: [
    "<value 1>",
  ],
  verifiedAt: new Date("2026-05-21T19:55:41.464Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.ModelsProvider](../models/modelsprovider.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |