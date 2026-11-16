# AddConfigStatementRequest

## Example Usage

```typescript
import { AddConfigStatementRequest } from "@alienplatform/manager-api/models/operations";

let value: AddConfigStatementRequest = {
  id: "<id>",
  statementRequest: {
    statement: {
      payload: "<value>",
      signatures: [
        {
          sig: "<value>",
          signer: "<value>",
        },
      ],
    },
  },
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `id`                                                        | *string*                                                    | :heavy_check_mark:                                          | Deployment id                                               |
| `statementRequest`                                          | [models.StatementRequest](../../models/statementrequest.md) | :heavy_check_mark:                                          | N/A                                                         |
