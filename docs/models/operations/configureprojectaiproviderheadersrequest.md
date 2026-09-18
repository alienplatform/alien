# ConfigureProjectAiProviderHeadersRequest

## Example Usage

```typescript
import { ConfigureProjectAiProviderHeadersRequest } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectAiProviderHeadersRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `idOrName`                                                    | *string*                                                      | :heavy_check_mark:                                            | Project ID or name.                                           |
| `aiProviderHeaders`                                           | [models.AIProviderHeaders](../../models/aiproviderheaders.md) | :heavy_minus_sign:                                            | N/A                                                           |