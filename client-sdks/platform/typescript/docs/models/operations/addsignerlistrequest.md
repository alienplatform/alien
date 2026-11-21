# AddSignerListRequest

## Example Usage

```typescript
import { AddSignerListRequest } from "@alienplatform/platform-api/models/operations";

let value: AddSignerListRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `idOrName`                                                              | *string*                                                                | :heavy_check_mark:                                                      | Project ID or name.                                                     |
| `signedStatementRequest`                                                | [models.SignedStatementRequest](../../models/signedstatementrequest.md) | :heavy_minus_sign:                                                      | N/A                                                                     |