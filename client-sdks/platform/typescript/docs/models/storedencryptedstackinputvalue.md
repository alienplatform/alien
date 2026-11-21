# StoredEncryptedStackInputValue

## Example Usage

```typescript
import { StoredEncryptedStackInputValue } from "@alienplatform/platform-api/models";

let value: StoredEncryptedStackInputValue = {
  value: "<value>",
  kind: "integer",
  secret: false,
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `value`                                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | Encrypted JSON-encoded input value.                                                          |
| `kind`                                                                                       | [models.StoredEncryptedStackInputValueKind](../models/storedencryptedstackinputvaluekind.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `secret`                                                                                     | *boolean*                                                                                    | :heavy_check_mark:                                                                           | Whether the original input is secret.                                                        |
| `held`                                                                                       | [models.StoredEncryptedStackInputValueHeld](../models/storedencryptedstackinputvalueheld.md) | :heavy_minus_sign:                                                                           | Present when the manager stores the answer.                                                  |