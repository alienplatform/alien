# RegistryDirectProvider

## Example Usage

```typescript
import { RegistryDirectProvider } from "@alienplatform/platform-api/models";

let value: RegistryDirectProvider = {
  provider: "openai",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  availableProviderModelIds: [
    "<value 1>",
  ],
  verifiedAt: new Date("2026-04-17T01:52:37.799Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.RegistryProvider](../models/registryprovider.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |