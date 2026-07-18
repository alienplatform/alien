# GcpCredentialsAccessToken

Use an already-minted OAuth2 access token.

## Example Usage

```typescript
import { GcpCredentialsAccessToken } from "@alienplatform/manager-api/models";

let value: GcpCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `token`            | *string*           | :heavy_check_mark: | N/A                |
| `type`             | *"accessToken"*    | :heavy_check_mark: | N/A                |