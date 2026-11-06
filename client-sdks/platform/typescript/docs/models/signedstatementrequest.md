# SignedStatementRequest

## Example Usage

```typescript
import { SignedStatementRequest } from "@alienplatform/platform-api/models";

let value: SignedStatementRequest = {
  statement: {
    payload: "<value>",
    signatures: [],
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `statement`                                            | [models.SignedStatement](../models/signedstatement.md) | :heavy_check_mark:                                     | N/A                                                    |