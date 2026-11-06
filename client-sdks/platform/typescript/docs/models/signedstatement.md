# SignedStatement

## Example Usage

```typescript
import { SignedStatement } from "@alienplatform/platform-api/models";

let value: SignedStatement = {
  payload: "<value>",
  signatures: [],
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `payload`                                                      | *string*                                                       | :heavy_check_mark:                                             | Base64 of the statement's JSON bytes, exactly as signed        |
| `signatures`                                                   | [models.StatementSignature](../models/statementsignature.md)[] | :heavy_check_mark:                                             | Signatures over the payload bytes                              |