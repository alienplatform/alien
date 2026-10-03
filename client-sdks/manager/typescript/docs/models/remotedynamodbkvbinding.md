# RemoteDynamodbKvBinding

Concrete DynamoDB KV topology returned to remote clients.

## Example Usage

```typescript
import { RemoteDynamodbKvBinding } from "@alienplatform/manager-api/models";

let value: RemoteDynamodbKvBinding = {
  region: "<value>",
  tableName: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `region`                                           | *string*                                           | :heavy_check_mark:                                 | AWS region of the table.                           |
| `tableName`                                        | *string*                                           | :heavy_check_mark:                                 | DynamoDB table authorized by the credential lease. |