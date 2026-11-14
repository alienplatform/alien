# StatementRequest

A body carrying one signed statement.

## Example Usage

```typescript
import { StatementRequest } from "@alienplatform/manager-api/models";

let value: StatementRequest = {
  statement: {
    payload: "<value>",
    signatures: [
      {
        sig: "<value>",
        signer: "<value>",
      },
    ],
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `statement`                                            | [models.SignedStatement](../models/signedstatement.md) | :heavy_check_mark:                                     | A statement as it is stored and carried.               |
