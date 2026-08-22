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
  credentialRevision: 619831,
  credentialStatus: "pending",
  credentialCheckedAt: new Date("2024-10-12T00:27:34.829Z"),
  catalogProviderModelIds: [
    "<value 1>",
  ],
  catalogObservedAt: new Date("2026-11-13T09:43:23.877Z"),
  requiredModelCoverage: [
    {
      publicModelId: "<id>",
      available: false,
      accessStatus: "verified",
      accessObservedAt: new Date("2024-07-16T18:19:22.272Z"),
    },
  ],
  updatedAt: new Date("2024-05-20T20:48:47.498Z"),
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
| `credentialRevision`                                                                          | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.ExternalAIBindingCredentialStatus](../models/externalaibindingcredentialstatus.md)    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogProviderModelIds`                                                                     | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `requiredModelCoverage`                                                                       | [models.RequiredModelCoverage](../models/requiredmodelcoverage.md)[]                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |