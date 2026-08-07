# ExternalAIBinding

## Example Usage

```typescript
import { ExternalAIBinding } from "@alienplatform/platform-api/models";

let value: ExternalAIBinding = {
  id: "<id>",
  provider: "anthropic",
  keyFingerprint: "<value>",
  availableProviderModelIds: [],
  verifiedAt: new Date("2024-03-11T10:44:02.422Z"),
  updatedAt: new Date("2025-11-10T08:02:13.738Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | [models.ExternalAIBindingProvider](../models/externalaibindingprovider.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availableProviderModelIds`                                                                   | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |