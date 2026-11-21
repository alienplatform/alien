# ConfigStatementResponse

## Example Usage

```typescript
import { ConfigStatementResponse } from "@alienplatform/platform-api/models";

let value: ConfigStatementResponse = {
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