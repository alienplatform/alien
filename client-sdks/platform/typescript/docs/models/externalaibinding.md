# ExternalAIBinding

## Example Usage

```typescript
import { ExternalAIBinding } from "@alienplatform/platform-api/models";

let value: ExternalAIBinding = {
  id: "<id>",
  provider: "databricks",
  providerEndpoint: "<value>",
  providerClientId: null,
  keyFingerprint: "<value>",
  availableProviderModelIds: [
    "<value 1>",
    "<value 2>",
  ],
  requiredModelCoverage: [],
  verifiedAt: new Date("2025-12-12T16:37:42.381Z"),
  updatedAt: new Date("2024-10-12T00:27:34.829Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | [models.ExternalAIBindingProvider](../models/externalaibindingprovider.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerClientId`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `requiredModelCoverage`                                                                       | [models.RequiredModelCoverage](../models/requiredmodelcoverage.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |