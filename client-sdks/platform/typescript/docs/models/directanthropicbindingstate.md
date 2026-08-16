# DirectAnthropicBindingState

## Example Usage

```typescript
import { DirectAnthropicBindingState } from "@alienplatform/platform-api/models";

let value: DirectAnthropicBindingState = {
  binding: {
    id: "<id>",
    provider: "anthropic",
    keyFingerprint: "<value>",
    availableProviderModelIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    verifiedAt: new Date("2025-11-28T13:09:12.725Z"),
    updatedAt: new Date("2025-04-24T16:17:24.576Z"),
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `binding`                                                            | [models.DirectAnthropicBinding](../models/directanthropicbinding.md) | :heavy_check_mark:                                                   | N/A                                                                  |