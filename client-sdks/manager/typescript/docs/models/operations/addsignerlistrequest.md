# AddSignerListRequest

## Example Usage

```typescript
import { AddSignerListRequest } from "@alienplatform/manager-api/models/operations";

let value: AddSignerListRequest = {
  project: "<value>",
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
| `project`                                                   | *string*                                                    | :heavy_check_mark:                                          | Project id                                                  |
| `statementRequest`                                          | [models.StatementRequest](../../models/statementrequest.md) | :heavy_check_mark:                                          | N/A                                                         |
