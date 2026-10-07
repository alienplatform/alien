# ExternalBindingAi

External AI provider binding (BYO-key OpenAI/Anthropic, etc.)

## Example Usage

```typescript
import { ExternalBindingAi } from "@alienplatform/manager-api/models";

let value: ExternalBindingAi = {
  apiKey: "<value>",
  provider: "<value>",
  type: "ai",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `apiKey`                                                                                                             | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `provider`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The external AI provider name (e.g., "openai", "anthropic")                                                          |
| `type`                                                                                                               | [models.TypeAi](../models/typeai.md)                                                                                 | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |