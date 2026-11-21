# ConfigStatementsResponse

## Example Usage

```typescript
import { ConfigStatementsResponse } from "@alienplatform/platform-api/models";

let value: ConfigStatementsResponse = {
  items: [
    {
      payload: "<value>",
      signatures: [
        {
          signer: "<value>",
          sig: "<value>",
        },
      ],
    },
  ],
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `items`                                                                 | [models.SignedStatement](../models/signedstatement.md)[]                | :heavy_check_mark:                                                      | Every stored config statement, lowest sequence first, exactly as stored |