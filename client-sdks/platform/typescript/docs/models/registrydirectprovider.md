# RegistryDirectProvider

## Example Usage

```typescript
import { RegistryDirectProvider } from "@alienplatform/platform-api/models";

let value: RegistryDirectProvider = {
  provider: "openai",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  credentialStatus: "valid",
  credentialCheckedAt: new Date("2025-01-04T20:57:47.612Z"),
  catalogObservedAt: new Date("2024-05-10T01:32:07.078Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.RegistryProvider](../models/registryprovider.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.RegistryCredentialStatus](../models/registrycredentialstatus.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |