# CreateTokenRequest

## Example Usage

```typescript
import { CreateTokenRequest } from "@alienplatform/manager-api/models";

let value: CreateTokenRequest = {
  type: "tunnel",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `deploymentGroupId`                                          | *string*                                                     | :heavy_minus_sign:                                           | Limit the token to one deployment group's deployments.       |
| `type`                                                       | [models.CreatableTokenType](../models/creatabletokentype.md) | :heavy_check_mark:                                           | Token kinds an admin can create through the API.             |