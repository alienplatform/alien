# EncryptedStackInputValue

## Example Usage

```typescript
import { EncryptedStackInputValue } from "@alienplatform/platform-api/models";

let value: EncryptedStackInputValue = {
  value: "<value>",
  kind: "secret",
  secret: true,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `value`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Encrypted JSON-encoded input value.                                              |
| `kind`                                                                           | [models.EncryptedStackInputValueKind](../models/encryptedstackinputvaluekind.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `secret`                                                                         | *boolean*                                                                        | :heavy_check_mark:                                                               | Whether the original input is secret.                                            |