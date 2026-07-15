# AwsCredentialsProfile

AWS profile credentials loaded via the AWS CLI.

## Example Usage

```typescript
import { AwsCredentialsProfile } from "@alienplatform/manager-api/models";

let value: AwsCredentialsProfile = {
  name: "<value>",
  type: "profile",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `name`             | *string*           | :heavy_check_mark: | AWS profile name   |
| `type`             | *"profile"*        | :heavy_check_mark: | N/A                |