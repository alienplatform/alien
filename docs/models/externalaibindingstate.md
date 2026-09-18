# ExternalAIBindingState

## Example Usage

```typescript
import { ExternalAIBindingState } from "@alienplatform/platform-api/models";

let value: ExternalAIBindingState = {
  binding: {
    id: "<id>",
    provider: "openai",
    providerEndpoint: "<value>",
    providerClientId: "<id>",
    keyFingerprint: "<value>",
    credentialRevision: 957154,
    credentialStatus: "pending",
    credentialCheckedAt: new Date("2025-09-22T19:00:03.377Z"),
    catalogProviderModelIds: [
      "<value 1>",
    ],
    catalogObservedAt: new Date("2024-11-19T15:50:33.963Z"),
    requiredModelCoverage: [],
    updatedAt: new Date("2024-01-04T17:07:00.104Z"),
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `binding`                                                  | [models.ExternalAIBinding](../models/externalaibinding.md) | :heavy_check_mark:                                         | N/A                                                        |