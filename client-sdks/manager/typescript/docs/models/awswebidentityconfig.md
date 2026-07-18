# AwsWebIdentityConfig

Configuration for AWS Web Identity Token authentication

## Example Usage

```typescript
import { AwsWebIdentityConfig } from "@alienplatform/manager-api/models";

let value: AwsWebIdentityConfig = {
  roleArn: "<value>",
  webIdentityTokenFile: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `durationSeconds`                                               | *number*                                                        | :heavy_minus_sign:                                              | Optional duration for the assumed role credentials (in seconds) |
| `roleArn`                                                       | *string*                                                        | :heavy_check_mark:                                              | The ARN of the role to assume                                   |
| `sessionName`                                                   | *string*                                                        | :heavy_minus_sign:                                              | Optional session name for the assumed role session              |
| `webIdentityTokenFile`                                          | *string*                                                        | :heavy_check_mark:                                              | The path to the web identity token file                         |