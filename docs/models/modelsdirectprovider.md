# ModelsDirectProvider

## Example Usage

```typescript
import { ModelsDirectProvider } from "@alienplatform/platform-api/models";

let value: ModelsDirectProvider = {
  provider: "databricks",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  credentialStatus: "valid",
  credentialCheckedAt: new Date("2026-08-29T20:03:48.935Z"),
  catalogObservedAt: new Date("2024-09-19T05:45:23.154Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.ModelsProvider](../models/modelsprovider.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.ModelsCredentialStatus](../models/modelscredentialstatus.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |