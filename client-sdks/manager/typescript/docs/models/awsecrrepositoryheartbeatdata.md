# AwsEcrRepositoryHeartbeatData

## Example Usage

```typescript
import { AwsEcrRepositoryHeartbeatData } from "@alienplatform/manager-api/models";

let value: AwsEcrRepositoryHeartbeatData = {
  createdAt: 1029.54,
  kmsKeyPresent: false,
  registryId: "<id>",
  repositoryArn: "<value>",
  repositoryName: "<value>",
  repositoryUri: "https://descriptive-role.name",
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