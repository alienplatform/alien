# ConfigStatementsResponse

Config statements stored for a deployment.

## Example Usage

```typescript
import { ConfigStatementsResponse } from "@alienplatform/manager-api/models";

let value: ConfigStatementsResponse = {
  items: [
    {
      payload: "<value>",
      signatures: [
        {
          sig: "<value>",
          signer: "<value>",
        },
      ],
    },
  ],
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `items`                                                  | [models.SignedStatement](../models/signedstatement.md)[] | :heavy_check_mark:                                       | Every stored statement, lowest sequence first.           |
