# Repository

## Example Usage

```typescript
import { Repository } from "@alienplatform/platform-api/models/operations";

let value: Repository = {
  createdAt: 4047.24,
  kmsKeyPresent: true,
  registryId: "<id>",
  repositoryArn: "<value>",
  repositoryName: "<value>",
  repositoryUri: "https://colorful-maestro.info",
};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `createdAt`          | *number*             | :heavy_check_mark:   | N/A                  |
| `encryptionType`     | *string*             | :heavy_minus_sign:   | N/A                  |
| `imageTagMutability` | *string*             | :heavy_minus_sign:   | N/A                  |
| `kmsKeyPresent`      | *boolean*            | :heavy_check_mark:   | N/A                  |
| `registryId`         | *string*             | :heavy_check_mark:   | N/A                  |
| `repositoryArn`      | *string*             | :heavy_check_mark:   | N/A                  |
| `repositoryName`     | *string*             | :heavy_check_mark:   | N/A                  |
| `repositoryUri`      | *string*             | :heavy_check_mark:   | N/A                  |
| `scanOnPush`         | *boolean*            | :heavy_minus_sign:   | N/A                  |