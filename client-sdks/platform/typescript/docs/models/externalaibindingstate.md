# ExternalAIBindingState

## Example Usage

```typescript
import { ExternalAIBindingState } from "@alienplatform/platform-api/models";

let value: ExternalAIBindingState = {
  binding: {
    id: "<id>",
    provider: "openai",
    keyFingerprint: "<value>",
    availableProviderModelIds: [
      "<value 1>",
    ],
    verifiedAt: new Date("2026-12-05T06:43:16.603Z"),
    updatedAt: new Date("2026-11-15T00:57:37.176Z"),
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `binding`                                                  | [models.ExternalAIBinding](../models/externalaibinding.md) | :heavy_check_mark:                                         | N/A                                                        |