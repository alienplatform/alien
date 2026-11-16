# SignedStatement

A statement as it is stored and carried.

## Example Usage

```typescript
import { SignedStatement } from "@alienplatform/manager-api/models";

let value: SignedStatement = {
  payload: "<value>",
  signatures: [],
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `payload`                                    | *string*                                     | :heavy_check_mark:                           | Base64 of the payload bytes.                 |
| `signatures`                                 | [models.Signature](../models/signature.md)[] | :heavy_check_mark:                           | Signatures over the payload bytes.           |
