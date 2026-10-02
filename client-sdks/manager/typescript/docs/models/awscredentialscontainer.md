# AwsCredentialsContainer

Container credentials endpoint: ECS task roles and EKS Pod Identity.

## Example Usage

```typescript
import { AwsCredentialsContainer } from "@alienplatform/manager-api/models";

let value: AwsCredentialsContainer = {
  endpoint: "<value>",
  type: "container",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `authorizationToken`                                                                                 | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Authorization header value for the endpoint                                                          |
| `authorizationTokenFile`                                                                             | *string*                                                                                             | :heavy_minus_sign:                                                                                   | File holding the authorization header value, re-read on each<br/>refresh because the platform rotates it |
| `endpoint`                                                                                           | *string*                                                                                             | :heavy_check_mark:                                                                                   | Credentials endpoint URL                                                                             |
| `type`                                                                                               | *"container"*                                                                                        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |