# SyncReconcileResponseExternalBindingsAi

External AI provider binding configuration (BYO-key).

The operator-supplied secret rides inside the binding via
`BindingValue<String>`, so it is a literal on cloud platforms and gains
Kubernetes SecretRef resolution for free (`extract_binding_secrets` walks
the binding JSON for `secretRef`).

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsAi } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsAi = {
  provider: "<value>",
  type: "ai",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `apiKey`                                                                                                             | *models.SyncReconcileResponseApiKeyUnion*                                                                            | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `provider`                                                                                                           | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The external AI provider name (e.g., "openai", "anthropic")                                                          |
| `type`                                                                                                               | [models.TargetTypeAi](../models/targettypeai.md)                                                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |