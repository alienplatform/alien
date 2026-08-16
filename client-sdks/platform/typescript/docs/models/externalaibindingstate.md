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
    availableProviderModelIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    requiredModelCoverage: [],
    verifiedAt: new Date("2026-10-17T13:05:17.905Z"),
    updatedAt: new Date("2025-09-22T19:00:03.377Z"),
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `binding`                                                  | [models.ExternalAIBinding](../models/externalaibinding.md) | :heavy_check_mark:                                         | N/A                                                        |