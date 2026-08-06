# DirectAnthropicBinding

## Example Usage

```typescript
import { DirectAnthropicBinding } from "@alienplatform/platform-api/models";

let value: DirectAnthropicBinding = {
  id: "<id>",
  provider: "anthropic",
  keyFingerprint: "<value>",
  availableProviderModelIds: [
    "<value 1>",
    "<value 2>",
  ],
  verifiedAt: new Date("2025-11-29T03:04:43.967Z"),
  updatedAt: new Date("2026-01-22T18:40:32.068Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | [models.DirectAnthropicBindingProvider](../models/directanthropicbindingprovider.md)          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |