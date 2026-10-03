# ListTokensResponse

## Example Usage

```typescript
import { ListTokensResponse } from "@alienplatform/manager-api/models";

let value: ListTokensResponse = {
  items: [
    {
      createdAt: "1730912071503",
      id: "<id>",
      keyPrefix: "<value>",
      tokenType: "<value>",
    },
  ],
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `items`                                              | [models.TokenResponse](../models/tokenresponse.md)[] | :heavy_check_mark:                                   | N/A                                                  |